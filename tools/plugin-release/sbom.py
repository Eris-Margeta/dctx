#!/usr/bin/env python3
"""Deterministic Cargo/native inventory, CONTRACT.md sections 5–7.
Pure conversion consumes Cargo metadata plus explicit dctx_release inputs.
The CLI and contract loader are the IO boundary; no Cargo, Git or network.
Used by the release driver and adjacent unittest controls.
"""

import argparse
import copy
import importlib.util
import json
from pathlib import Path
import re
import sys

sys.dont_write_bytecode = True
CONTRACT_DIR = Path(__file__).resolve().parents[2] / 'docs/design/plugin-release'
spec = importlib.util.spec_from_file_location('release_contract', CONTRACT_DIR / 'validate_contract.py')
contract_validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(contract_validator)
CONTRACT = contract_validator.load_contract()
Refusal = contract_validator.Refusal
require = contract_validator.require
encode = contract_validator.encode


def properties(values):
    return [{'name': key, 'value': value} for key, value in sorted(values.items())]


def digest(value):
    require(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value),
            'missing-or-invalid-sha256')
    return value


def target_matches(expression, target, target_features=()):
    """Return True/False/None for matching, excluded or unresolved Cargo cfg."""
    if expression is None:
        return True
    if not expression.startswith('cfg('):
        return expression == target
    arch, vendor, os_name, *environment = target.split('-')
    os_name = {'darwin': 'macos'}.get(os_name, os_name)
    family = 'windows' if os_name == 'windows' else 'unix'
    facts = {'target_arch': arch, 'target_vendor': vendor, 'target_os': os_name,
             'target_env': environment[0] if environment else '', 'target_abi': '',
             'target_family': family, 'target_pointer_width': '64',
             'target_endian': 'little', 'panic': 'unwind',
             'target_has_atomic': {'8', '16', '32', '64', 'ptr'},
             'target_feature': set(target_features)}
    tokens = re.findall(r'"[^"\\]*"|\w+|[(),=]|\S', expression)
    position = 0

    def take(expected=None):
        nonlocal position
        require(position < len(tokens), 'malformed-target-cfg')
        token = tokens[position]
        position += 1
        require(expected is None or token == expected, 'malformed-target-cfg')
        return token

    def parse():
        name = take()
        require(re.fullmatch(r'[A-Za-z_]\w*', name), 'malformed-target-cfg')
        if name in ('cfg', 'all', 'any', 'not'):
            take('(')
            values = []
            while position < len(tokens) and tokens[position] != ')':
                values.append(parse())
                if position >= len(tokens) or tokens[position] != ',':
                    break
                take(',')
            take(')')
            if name in ('cfg', 'not'):
                require(len(values) == 1, 'malformed-target-cfg')
                return values[0] if name == 'cfg' or values[0] is None else not values[0]
            if name == 'all':
                return False if False in values else None if None in values else True
            return True if True in values else None if None in values else False
        if position < len(tokens) and tokens[position] == '=':
            take('=')
            value = take()
            require(re.fullmatch(r'"[^"\\]*"', value), 'malformed-target-cfg')
            if name not in facts:
                return None
            actual = facts[name]
            return value[1:-1] in actual if isinstance(actual, set) else actual == value[1:-1]
        return {'unix': family == 'unix', 'windows': family == 'windows',
                'debug_assertions': False, 'test': False}.get(name)

    result = parse()
    require(position == len(tokens), 'malformed-target-cfg')
    return result


def component(identity, name, version, purl, sha256, license_text, role, extra):
    require(isinstance(license_text, str) and license_text.strip() == license_text
            and license_text and not re.search(r'\b(UNKNOWN|NOASSERTION|NONE)\b', license_text),
            'missing-or-unresolved-license')
    return {'type': 'application' if role in ('cli', 'standalone-git-plugin') else 'library',
            'bom-ref': identity, 'name': name, 'version': version, 'purl': purl,
            'hashes': [{'alg': 'SHA-256', 'content': digest(sha256)}],
            'licenses': [{'expression': license_text}],
            'properties': properties({'dctx:role': role, 'dctx:license-source':
                                     'native-inventory' if role == 'native-library' else 'cargo-metadata',
                                     **extra})}


def cargo_metadata_to_cyclonedx(metadata: dict, root_package: str, target: str) -> dict:
    """Return a closed release SBOM; refuse incomplete caller inventory.

    See README for metadata['metadata']['dctx_release']. Inputs are never mutated.
    Normal and build edges form the closure; dev edges are excluded.
    """
    require(target in CONTRACT[0]['$defs']['target']['enum'], 'unknown-target')
    release = (metadata.get('metadata') or {}).get('dctx_release')
    require(isinstance(release, dict), 'missing-release-inventory')
    test_run = release.get('test_run') is True
    require(release.get('target') == target, 'inventory-target-mismatch')
    packages = contract_validator.pairs((p['id'], p) for p in metadata['packages'])
    nodes = contract_validator.pairs((n['id'], n) for n in metadata['resolve']['nodes'])

    def select(label):
        matches = [p['id'] for p in packages.values() if label in (p['id'], p['name'])]
        require(len(matches) == 1, 'missing-or-ambiguous-package: ' + label)
        return matches[0]

    root = select(root_package)
    standalone = select(release['standalone_package'])
    require(packages[root]['name'] == 'dctx-cli', 'root-must-be-cli')
    require(packages[standalone]['name'] == 'dctx-plugin-git', 'standalone-package-name')
    edges, pending, unresolved = {}, [root, standalone], {}
    while pending:
        identity = pending.pop()
        if identity in edges:
            continue
        require(identity in nodes and identity in packages, 'missing-resolve-package')
        node = nodes[identity]
        require('features' in node and 'deps' in node, 'missing-feature-or-edge-closure')
        deps = set()
        for dep in node['deps']:
            require('dep_kinds' in dep and dep['dep_kinds'], 'missing-dependency-kind')
            for kind in dep['dep_kinds']:
                require(kind['kind'] in (None, 'normal', 'build', 'dev'), 'unknown-dependency-kind')
                if kind['kind'] == 'dev':
                    continue
                match = target_matches(kind['target'], target, release.get('target_features', ()))
                if match is not False:
                    deps.add(dep['pkg'])
                if match is None:
                    unresolved.setdefault(dep['pkg'], set()).add(kind['target'])
        edges[identity] = deps
        pending.extend(deps - edges.keys())
    workspace = set(metadata['workspace_members'])
    refs = {key: ('dctx/cli' if key == root else 'dctx/git/standalone' if key == standalone
                  else ('urn:dctx:first-party:' if key in workspace else 'pkg:cargo/')
                  + packages[key]['name'] + '@' + packages[key]['version']) for key in edges}
    components = []
    for key in edges:
        package, node = packages[key], nodes[key]
        source_hash = release.get('source_hashes', {}).get(key)
        if (package.get('source') or '').startswith('registry+'):
            source_hash = digest(package.get('checksum'))
        role = 'cli' if key == root else 'standalone-git-plugin' if key == standalone else 'rust-crate'
        sha256 = release['artifacts'][role] if role != 'rust-crate' else source_hash
        features = sorted(set(node['features']))
        if key == root and not test_run:
            require(features == sorted(CONTRACT[3]['profiles'][release['profile']]['cli_features']),
                    'cli-feature-profile')
        license_text = package.get('license')
        if not license_text and package.get('license_file'):
            reviewed = release.get('license_files', {}).get(key, {})
            license_text = reviewed.get('expression', '')
            require(license_text.startswith('LicenseRef-'), 'unreviewed-license-file')
            digest(reviewed.get('sha256'))
        extra = {'dctx:features': json.dumps(features, separators=(',', ':'))}
        if key in unresolved:
            extra['cdx:cargo:cfg-unresolved'] = '\n'.join(sorted(unresolved[key]))
        components.append(component(refs[key], package['name'], package['version'],
                                    'pkg:cargo/' + package['name'] + '@' + package['version'],
                                    sha256, license_text, role,
                                    extra))
    graph = {refs[key]: {refs[dep] for dep in deps} for key, deps in edges.items()}
    if release['profile'] == 'default':
        worker = copy.deepcopy(next(c for c in components if c['bom-ref'] == 'dctx/git/standalone'))
        worker.update({'bom-ref': 'dctx/git/bundled', 'type': 'library',
                       'hashes': [{'alg': 'SHA-256', 'content': release['artifacts']['cli']}]})
        props = {p['name']: p['value'] for p in worker['properties']}
        props.update({'dctx:role': 'bundled-git-worker', 'dctx:containing-component': 'dctx/cli'})
        worker['properties'] = properties(props)
        components.append(worker)
        graph['dctx/git/bundled'] = set(graph['dctx/git/standalone'])
        graph['dctx/cli'].add('dctx/git/bundled')
    require(isinstance(release.get('native_libraries'), list), 'missing-native-library-inventory')
    require(release['native_libraries'] or (test_run and release.get('native_note') ==
            'none declared in test run'), 'missing-native-library-inventory')
    for native in release['native_libraries']:
        components.append(component(native['ref'], native['name'], native['version'], native['purl'],
                                    native['sha256'], native['license'], 'native-library',
                                    {'dctx:linkage': native['linkage']}))
        require(native['ref'] not in graph, 'duplicate-component-ref')
        graph[native['ref']] = set(native['depends_on'])
    for native in release['native_libraries']:
        for user in native['used_by']:
            require(user in graph, 'unknown-native-consumer')
            graph[user].add(native['ref'])
    document = {'bomFormat': 'CycloneDX', 'specVersion': '1.5', 'version': 1,
                'components': sorted(components, key=lambda c: (c['purl'], c['bom-ref'])),
                'dependencies': [{'ref': key, 'dependsOn': sorted(value)}
                                 for key, value in sorted(graph.items())],
                'properties': properties({'dctx:target': target, 'dctx:profile': release['profile'],
                                          'dctx:source-revision': release['source_revision']})}
    if unresolved:
        document['properties'] = properties({**contract_validator.properties(document['properties']),
            'cdx:cargo:cfg-unresolved-count': str(len(unresolved))})
    if test_run:
        # Test inventories lack production source/native/profile evidence.
        contract_validator.validate(document, CONTRACT[2]['schema'])
    else:
        contract_validator.check_sbom(document, CONTRACT)
    return document


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('metadata', 'root', 'target', 'out'):
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--epoch', type=int, help='Validate supplied epoch; profile forbids timestamps')
    args = parser.parse_args(argv)
    try:
        require(args.epoch is None or 0 <= args.epoch <= 9007199254740991, 'invalid-epoch')
        metadata, _ = contract_validator.read(args.metadata)
        document = cargo_metadata_to_cyclonedx(metadata, args.root, args.target)
        Path(args.out).write_bytes(encode(document))
    except (ValueError, KeyError, TypeError, OSError) as error:
        parser.exit(2, 'sbom: ' + str(error) + '\n')


if __name__ == '__main__':
    main()
