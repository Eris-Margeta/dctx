#!/usr/bin/env python3
"""Offline PL-RELC format checks, implementing CONTRACT.md sections 2-7.
Uses only Python stdlib and the adjacent closed schemas/profile/build recipe.
REL-T and release-tool examples use this; it never signs or grants promotion.
Self-test fixtures are synthetic, in memory, and cannot establish native evidence.
"""

import argparse
import copy
import hashlib
import json
from pathlib import Path
import re
import sys

BASE = Path(__file__).resolve().parent
KEYWORDS = set(('$schema $id $comment $defs $ref type properties required '
                'additionalProperties const enum allOf if then items minItems '
                'maxItems uniqueItems minimum maximum minLength maxLength pattern').split())
EVIDENCE = 'native reproducibility crypto lifecycle privacy git image_binding native_inputs'.split()


class Refusal(ValueError):
    """An explicit malformed-format or inconsistent-binding refusal."""


def require(condition, reason):
    if not condition:
        raise Refusal(reason)


def encode(value):
    return (json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True,
                       allow_nan=False) + '\n').encode('utf-8')


def sha(data):
    return hashlib.sha256(data).hexdigest()


def pairs(items):
    result = {}
    for key, value in items:
        require(key not in result, 'duplicate-json-key: ' + key)
        result[key] = value
    return result


def decode(data):
    def nonfinite(value):
        raise Refusal('nonfinite-json-number: ' + value)
    require(len(data) <= 8 * 1024 * 1024, 'document-too-large')
    return json.loads(data.decode('utf-8'), object_pairs_hook=pairs, parse_constant=nonfinite)


def read(path):
    with Path(path).open('rb') as stream:
        data = stream.read(8 * 1024 * 1024 + 1)
    return decode(data), data


def audit_schema(schema, root):
    require(not set(schema) - KEYWORDS, 'unsupported-schema-keyword')
    if schema.get('type') == 'object':
        require(schema.get('additionalProperties') is False, 'schema-object-not-closed')
    if '$ref' in schema:
        require(schema['$ref'].startswith('#/$defs/'), 'nonlocal-schema-reference')
        require(schema['$ref'][8:] in root.get('$defs', {}), 'missing-schema-definition')
    for key in ('properties', '$defs'):
        for child in schema.get(key, {}).values():
            audit_schema(child, root)
    for key in ('items', 'if', 'then'):
        if key in schema:
            audit_schema(schema[key], root)
    for child in schema.get('allOf', []):
        audit_schema(child, root)


def validate(value, schema, root=None, path='$'):
    """Interpret only the audited keywords used here; no remote resolution."""
    root = schema if root is None else root
    if '$ref' in schema:
        validate(value, root['$defs'][schema['$ref'][8:]], root, path)
    kind = schema.get('type')
    types = {'object': dict, 'array': list, 'string': str, 'integer': int, 'boolean': bool}
    if kind:
        require(kind in types and type(value) is types[kind], path + ': type')
    if 'const' in schema:
        require(encode(value) == encode(schema['const']), path + ': const')
    if 'enum' in schema:
        require(any(encode(value) == encode(item) for item in schema['enum']), path + ': enum')
    if isinstance(value, dict):
        properties = schema.get('properties', {})
        require(set(schema.get('required', [])) <= set(value), path + ': missing-field')
        if schema.get('additionalProperties') is False:
            require(set(value) <= set(properties), path + ': unknown-field')
        for key in value.keys() & properties.keys():
            validate(value[key], properties[key], root, path + '.' + key)
    if isinstance(value, list):
        require(schema.get('minItems', 0) <= len(value) <= schema.get('maxItems', len(value)), path + ': array-length')
        if schema.get('uniqueItems'):
            require(len({encode(item) for item in value}) == len(value), path + ': duplicate-item')
        if 'items' in schema:
            for index, item in enumerate(value):
                validate(item, schema['items'], root, path + '[' + str(index) + ']')
    if isinstance(value, str):
        require(schema.get('minLength', 0) <= len(value) <= schema.get('maxLength', len(value)), path + ': string-length')
        if 'pattern' in schema:
            require(re.fullmatch(schema['pattern'], value) is not None, path + ': pattern')
    if type(value) is int:
        require(schema.get('minimum', value) <= value <= schema.get('maximum', value), path + ': integer-range')
    for child in schema.get('allOf', []):
        validate(value, child, root, path)
    if 'if' in schema:
        try:
            validate(value, schema['if'], root, path)
        except Refusal:
            pass
        else:
            validate(value, schema.get('then', {}), root, path)


def load_contract():
    release, _ = read(BASE / 'release.schema.json')
    provenance, _ = read(BASE / 'provenance.schema.json')
    profile, _ = read(BASE / 'sbom-profile.json')
    inputs, raw = read(BASE / 'build-inputs.json')
    for schema in (release, provenance, profile['schema']):
        audit_schema(schema, schema)
    approved = provenance['properties']['predicate']['properties']['buildDefinition']['properties']['externalParameters']['properties']['toolchain']['const']
    require(inputs['toolchain'] == approved, 'toolchain-contract-drift')
    require(set(inputs['toolchain']['components']) == set(inputs['component_versions']), 'component-contract-drift')
    require(set(inputs['component_versions'].values()) == {'1.97.1'}, 'component-version-drift')
    targets = {item['triple'] for item in inputs['targets'].values()}
    require(targets == set(release['$defs']['target']['enum']), 'target-contract-drift')
    eligible = {item['triple'] for item in inputs['targets'].values() if item['policy'] == 'eligible'}
    require(eligible == set(inputs['rust_std_targets']) == {'x86_64-unknown-linux-gnu', 'aarch64-apple-darwin'}, 'promotion-target-drift')
    return release, provenance, profile, inputs, sha(raw)


def safe_file(name):
    stem = name.split('.')[0].upper()
    require(stem not in {'CON', 'PRN', 'AUX', 'NUL', *('COM' + str(i) for i in range(1, 10)),
                         *('LPT' + str(i) for i in range(1, 10))}, 'reserved-file-name')


def check_release(doc, contract):
    validate(doc, contract[0])
    require(doc['build_inputs_sha256'] == contract[4], 'build-inputs-digest-mismatch')
    qualification = doc['qualification']
    for field in ('target', 'profile', 'source_revision', 'build_inputs_sha256'):
        require(qualification[field] == doc[field], 'evidence-binding: ' + field)
    for role in ('cli', 'plugin'):
        require(qualification[role + '_sha256'] == doc['artifacts'][role]['sha256'], 'evidence-artifact-binding')
    links = list(doc['artifacts'].values()) + list(qualification['records'].values())
    require(len({item['file'].casefold() for item in links}) == len(links), 'duplicate-file-name')
    for item in links:
        safe_file(item['file'])
        require(item['file'].casefold() not in {'release.json', 'release.json.sig', 'index.json', 'index.json.sig', 'plugin.json.sig'}, 'circular-or-signature-artifact')
    for role, name in {'manifest': 'plugin.json', 'checksums': 'SHA256SUMS', 'sbom': 'sbom.cdx.json', 'provenance': 'provenance.json'}.items():
        require(doc['artifacts'][role]['file'] == name, 'metadata-file-name: ' + role)
    if qualification['state'] == 'candidate':
        require(qualification['reason'].strip(), 'empty-candidate-reason')


def check_provenance(doc, contract, release=None):
    validate(doc, contract[1])
    definition = doc['predicate']['buildDefinition']
    materials = pairs((item['uri'], item['digest']['sha256']) for item in definition['resolvedDependencies'])
    require({'urn:dctx:build-inputs', 'urn:dctx:source', 'urn:dctx:lock:workspace', 'urn:dctx:native-inputs'} <= materials.keys(), 'missing-build-material')
    require(materials['urn:dctx:build-inputs'] == contract[4], 'provenance-build-inputs-mismatch')
    subjects = pairs((item['name'], item['digest']['sha256']) for item in doc['subject'])
    require({'plugin.json', 'SHA256SUMS', 'sbom.cdx.json'} <= subjects.keys(), 'missing-metadata-subject')
    require(len({name.casefold() for name in subjects}) == len(subjects), 'case-colliding-subject')
    for name in subjects:
        safe_file(name)
    require(not {'provenance.json', 'release.json'} & subjects.keys(), 'self-referential-subject')
    if release:
        for key in ('target', 'profile', 'source_revision'):
            require(definition['externalParameters'][key] == release[key], 'provenance-binding: ' + key)
        expected = {item['file']: item['sha256'] for role, item in release['artifacts'].items() if role != 'provenance'}
        require(subjects == expected, 'provenance-subject-mismatch')
        native = release['qualification']['records'].get('native_inputs')
        if native:
            require(materials['urn:dctx:native-inputs'] == native['sha256'], 'native-inputs-material-mismatch')


def properties(items):
    return pairs((item['name'], item['value']) for item in items)


def check_sbom(doc, contract, release=None, provenance=None):
    profile = contract[2]
    validate(doc, profile['schema'])
    fields = profile['required_fields']
    context = properties(doc['properties'])
    required_context = set(fields['document_properties'])
    require(required_context <= set(context) <= required_context | {'cdx:cargo:cfg-unresolved-count'}, 'sbom-context-fields')
    if 'cdx:cargo:cfg-unresolved-count' in context:
        require(re.fullmatch(r'[0-9]+', context['cdx:cargo:cfg-unresolved-count']), 'sbom-cfg-unresolved-count')
    require(context['dctx:target'] in contract[0]['$defs']['target']['enum'], 'sbom-target')
    require(context['dctx:profile'] in ('default', 'strict'), 'sbom-profile')
    require(re.fullmatch(r'([0-9a-f]{40}|[0-9a-f]{64})', context['dctx:source-revision']), 'sbom-source-revision')
    nodes = pairs((item['bom-ref'], item) for item in doc['components'])
    roles = {}
    for component in nodes.values():
        props = properties(component['properties'])
        role = props.get('dctx:role')
        require(role in fields['roles'], 'unknown-sbom-role')
        required = set(fields['component_properties'] + fields['role_properties'].get(role, []))
        require(required <= set(props) <= required | {'cdx:cargo:cfg-unresolved'}, 'sbom-component-properties')
        if 'cdx:cargo:cfg-unresolved' in props:
            require(isinstance(props['cdx:cargo:cfg-unresolved'], str) and props['cdx:cargo:cfg-unresolved'], 'sbom-cfg-unresolved')
        roles.setdefault(role, []).append(component)
        native = role == 'native-library'
        require(props['dctx:license-source'] == ('native-inventory' if native else 'cargo-metadata'), 'license-source')
        license_text = component['licenses'][0]['expression']
        require(license_text.strip() == license_text and license_text and not re.search(r'\b(UNKNOWN|NOASSERTION|NONE)\b', license_text), 'unresolved-license')
        if native:
            require(props['dctx:linkage'] in ('static', 'dynamic', 'system'), 'native-linkage')
            require(component['type'] in ('library', 'framework'), 'native-component-type')
        else:
            require(component['purl'].startswith('pkg:cargo/'), 'cargo-purl')
            features = decode(props['dctx:features'].encode('utf-8'))
            require(isinstance(features, list) and all(isinstance(f, str) and f for f in features), 'feature-list')
            require(features == sorted(set(features)), 'feature-order-or-duplicate')
    require(roles.get('native-library'), 'missing-native-library-inventory')
    identities = {'cli': ('dctx/cli', 'application'), 'standalone-git-plugin': ('dctx/git/standalone', 'application')}
    if context['dctx:profile'] == 'default':
        identities['bundled-git-worker'] = ('dctx/git/bundled', 'library')
    else:
        require('bundled-git-worker' not in roles, 'bundled-worker-in-strict')
    for role, (identity, kind) in identities.items():
        require(len(roles.get(role, [])) == 1, 'missing-or-duplicate-role: ' + role)
        require(roles[role][0]['bom-ref'] == identity and roles[role][0]['type'] == kind, 'component-identity')
        name = 'dctx-cli' if role == 'cli' else 'dctx-plugin-git'
        require(nodes[identity]['name'] == name, 'component-name')
        require(nodes[identity]['purl'] == 'pkg:cargo/' + name + '@' + nodes[identity]['version'], 'component-purl')
    cli_features = decode(properties(nodes['dctx/cli']['properties'])['dctx:features'].encode())
    require(cli_features == sorted(contract[3]['profiles'][context['dctx:profile']]['cli_features']), 'cli-feature-profile')
    if 'bundled-git-worker' in identities:
        worker = nodes['dctx/git/bundled']
        require(properties(worker['properties'])['dctx:containing-component'] == 'dctx/cli', 'worker-container')
        require(worker['hashes'] == nodes['dctx/cli']['hashes'], 'worker-containing-digest')
    edges = pairs((item['ref'], item['dependsOn']) for item in doc['dependencies'])
    require(edges.keys() == nodes.keys(), 'missing-dependency-node')
    require(all(set(deps) <= nodes.keys() for deps in edges.values()), 'dangling-dependency')
    if 'bundled-git-worker' in identities:
        require('dctx/git/bundled' in edges['dctx/cli'], 'missing-worker-edge')
    if release:
        for prop, key in [('dctx:target', 'target'), ('dctx:profile', 'profile'), ('dctx:source-revision', 'source_revision')]:
            require(context[prop] == release[key], 'sbom-context-mismatch')
        for identity, role, version in [('dctx/cli', 'cli', release['version']), ('dctx/git/standalone', 'plugin', release['plugin']['version'])]:
            require(nodes[identity]['hashes'][0]['content'] == release['artifacts'][role]['sha256'], 'sbom-artifact-digest')
            require(nodes[identity]['version'] == version, 'sbom-artifact-version')
        if 'dctx/git/bundled' in nodes:
            require(nodes['dctx/git/bundled']['version'] == release['plugin']['version'], 'worker-version')
    if provenance:
        parameters = provenance['predicate']['buildDefinition']['externalParameters']
        require({key: context[key] for key in required_context} == {'dctx:target': parameters['target'], 'dctx:profile': parameters['profile'],
                            'dctx:source-revision': parameters['source_revision']}, 'sbom-provenance-context')
        materials = {item['uri']: item['digest']['sha256'] for item in provenance['predicate']['buildDefinition']['resolvedDependencies']}
        for component in roles.get('native-library', []) + roles.get('rust-crate', []):
            require(materials.get(component['purl']) == component['hashes'][0]['content'], 'sbom-material-mismatch')


def check_documents(contract, release=None, provenance=None, sbom=None, raw=None):
    if release is not None:
        check_release(release, contract)
    if provenance is not None:
        check_provenance(provenance, contract, release)
    if sbom is not None:
        check_sbom(sbom, contract, release, provenance)
    if release and raw:
        for role, data in raw.items():
            link = release['artifacts'][role]
            require(link['sha256'] == sha(data) and link['bytes'] == len(data), 'exact-document-digest: ' + role)


def check_toolchain(text, contract):
    try:
        import tomllib
    except ImportError as error:
        raise Refusal('--toolchain requires Python 3.11+ stdlib tomllib') from error
    doc = tomllib.loads(text)
    require(set(doc) == {'toolchain'}, 'toolchain-top-fields')
    toolchain = doc['toolchain']
    require(set(toolchain) == {'channel', 'profile', 'components', 'targets'}, 'toolchain-fields')
    inputs = contract[3]
    require(toolchain['channel'] == inputs['toolchain']['channel'], 'toolchain-exact-channel')
    require(toolchain['profile'] == inputs['toolchain_profile'], 'toolchain-profile')
    for key, expected in [('components', inputs['toolchain']['components']), ('targets', inputs['rust_std_targets'])]:
        require(isinstance(toolchain[key], list) and sorted(toolchain[key]) == sorted(expected), 'toolchain-' + key)


def fixture(contract, target='x86_64-unknown-linux-gnu', profile='default'):
    """Synthetic format control; hashes do not identify real build/evidence files."""
    def link(name, data=None):
        data = ('SYNTHETIC ' + name).encode() if data is None else data
        return {'file': name, 'bytes': len(data), 'sha256': sha(data)}
    source = 'a' * 40
    artifacts = {role: link(name) for role, name in [('cli', 'dctx'), ('plugin', 'dctx-plugin-git'), ('manifest', 'plugin.json'), ('checksums', 'SHA256SUMS')]}
    records = {name: link(name + '-evidence.json') for name in EVIDENCE}
    def component(identity, role, name, version, digest, extra):
        props = {'dctx:role': role, 'dctx:license-source': 'native-inventory' if role == 'native-library' else 'cargo-metadata', **extra}
        return {'type': 'application' if role in ('cli', 'standalone-git-plugin') else 'library', 'bom-ref': identity,
                'name': name, 'version': version, 'purl': 'pkg:' + ('generic/' if role == 'native-library' else 'cargo/') + name + '@' + version,
                'hashes': [{'alg': 'SHA-256', 'content': digest}], 'licenses': [{'expression': 'Apache-2.0'}],
                'properties': [{'name': key, 'value': value} for key, value in sorted(props.items())]}
    components = [component('dctx/cli', 'cli', 'dctx-cli', '0.25.0', artifacts['cli']['sha256'], {'dctx:features': '["dctx/plugin-runtime","git-worker"]'}),
                  component('dctx/git/standalone', 'standalone-git-plugin', 'dctx-plugin-git', '1.0.0', artifacts['plugin']['sha256'], {'dctx:features': '[]'}),
                  component('dctx/git/bundled', 'bundled-git-worker', 'dctx-plugin-git', '1.0.0', artifacts['cli']['sha256'], {'dctx:features': '[]', 'dctx:containing-component': 'dctx/cli'}),
                  component('test/native', 'native-library', 'test-system-library', '1.0.0', sha(b'test-native'), {'dctx:linkage': 'system'}),
                  component('test/crate', 'rust-crate', 'test-crate', '1.0.0', sha(b'test-crate'), {'dctx:features': '[]'})]
    if profile == 'strict':
        components = [c for c in components if c['bom-ref'] != 'dctx/git/bundled']
        next(p for p in components[0]['properties'] if p['name'] == 'dctx:features')['value'] = '["strict"]'
    cli_dependencies = ['test/native', 'test/crate'] + (['dctx/git/bundled'] if profile == 'default' else [])
    sbom = {'bomFormat': 'CycloneDX', 'specVersion': '1.5', 'version': 1, 'components': components,
            'dependencies': [{'ref': c['bom-ref'], 'dependsOn': cli_dependencies if c['bom-ref'] == 'dctx/cli' else []} for c in components],
            'properties': [{'name': key, 'value': value} for key, value in [('dctx:target', target), ('dctx:profile', profile), ('dctx:source-revision', source)]]}
    artifacts['sbom'] = link('sbom.cdx.json', encode(sbom))
    materials = {'urn:dctx:build-inputs': contract[4], 'urn:dctx:source': sha(b'test-source'), 'urn:dctx:lock:workspace': sha(b'test-lock'), 'urn:dctx:native-inputs': records['native_inputs']['sha256']}
    materials.update({c['purl']: c['hashes'][0]['content'] for c in components[-2:]})
    provenance = {'_type': 'https://in-toto.io/Statement/v1', 'predicateType': 'https://slsa.dev/provenance/v1',
                  'subject': [{'name': a['file'], 'digest': {'sha256': a['sha256']}} for a in artifacts.values()],
                  'predicate': {'buildDefinition': {'buildType': 'urn:dctx:release:build:v1',
                      'externalParameters': {'target': target, 'profile': profile, 'source_revision': source, 'toolchain': copy.deepcopy(contract[3]['toolchain'])},
                      'internalParameters': {'source_date_epoch': 1792281600},
                      'resolvedDependencies': [{'uri': uri, 'digest': {'sha256': digest}} for uri, digest in sorted(materials.items())]},
                      'runDetails': {'builder': {'id': 'urn:dctx:builder:synthetic-control'}}}}
    artifacts['provenance'] = link('provenance.json', encode(provenance))
    release = {'schema': 1, 'kind': 'dctx-release', 'version': '0.25.0', 'plugin': {'id': 'dctx/git', 'version': '1.0.0', 'release_sequence': 1},
               'target': target, 'profile': profile, 'source_revision': source, 'build_inputs_sha256': contract[4], 'artifacts': artifacts,
               'bundled_git_component': 'dctx/git/bundled' if profile == 'default' else None, 'authentication': {'algorithm': 'Ed25519', 'domain': 'dctx-plugin-index-v1', 'key_fingerprint': sha(b'test-key'), 'index_sha256': sha(b'test-index'), 'index_sequence': 1, 'trust': 'test'},
               'qualification': {'state': 'candidate', 'reason': 'Synthetic format fixture; not native evidence.', 'native': False, 'target': target, 'profile': profile, 'source_revision': source,
                   'build_inputs_sha256': contract[4], 'cli_sha256': artifacts['cli']['sha256'], 'plugin_sha256': artifacts['plugin']['sha256'], 'provider': 'unqualified', 'records': records}}
    return release, provenance, sbom


def self_test(contract):
    release, provenance, sbom = fixture(contract)
    positives = 0
    for target in contract[3]['rust_std_targets']:
        for profile in ('default', 'strict'):
            control, prov, bom = fixture(contract, target, profile)
            check_documents(contract, control, prov, bom, {'provenance': encode(prov), 'sbom': encode(bom)})
            control['authentication']['trust'] = 'production'
            control['qualification'].update(state='promotable', reason='', native=True, provider='qualified')
            check_release(control, contract)  # Shape only, not real native proof.
            positives += 2
    pending = copy.deepcopy(control)
    pending['profile'] = pending['qualification']['profile'] = 'default'
    pending['bundled_git_component'] = 'dctx/git/bundled'
    pending['qualification']['provider'] = 'bundled-git-pending-confinement'
    check_release(pending, contract)
    positives += 1
    promoted = copy.deepcopy(release)
    promoted['authentication']['trust'] = 'production'
    promoted['qualification'].update(state='promotable', reason='', native=True, provider='qualified')
    check_release(promoted, contract)  # Shape only: no signature or native proof exists.
    positives += 1
    unresolved = copy.deepcopy(sbom)
    unresolved['components'][0]['properties'].append({'name': 'cdx:cargo:cfg-unresolved', 'value': 'cfg(custom_atom)'})
    unresolved['properties'].append({'name': 'cdx:cargo:cfg-unresolved-count', 'value': '1'})
    check_sbom(unresolved, contract, release, provenance)
    positives += 1
    cases = [
        ('unknown SBOM property', sbom, ('components', 0, 'properties', 0, 'name'), 'cdx:cargo:other'),
        ('missing digest', release, ('artifacts', 'cli', 'sha256'), None),
        ('unknown field', release, ('artifacts', 'cli', 'extra'), 1),
        ('unknown top field', release, ('extra',), 1),
        ('wrong domain', release, ('authentication', 'domain'), 'dctx-plugin-manifest-v1'),
        ('missing native evidence', promoted, ('qualification', 'records', 'native'), None),
        ('cross-build evidence', promoted, ('qualification', 'native'), False),
        ('TEST promotion', promoted, ('authentication', 'trust'), 'test'),
        ('wrong evidence digest', release, ('qualification', 'cli_sha256'), 'b' * 64),
        ('wrong evidence profile', release, ('qualification', 'profile'), 'strict'),
        ('boolean integer', release, ('plugin', 'release_sequence'), True),
        ('path traversal', release, ('artifacts', 'cli', 'file'), '../dctx'),
        ('device name', release, ('artifacts', 'cli', 'file'), 'NUL.exe'),
        ('empty reason', release, ('qualification', 'reason'), ' '),
        ('wrong provenance subject', provenance, ('subject', 0, 'digest', 'sha256'), 'b' * 64),
        ('missing provenance digest', provenance, ('subject', 0, 'digest', 'sha256'), None),
        ('missing builder', provenance, ('predicate', 'runDetails', 'builder'), None),
        ('missing materials', provenance, ('predicate', 'buildDefinition', 'resolvedDependencies'), []),
        ('unknown provenance field', provenance, ('predicate', 'extra'), 1),
        ('floating toolchain', provenance, ('predicate', 'buildDefinition', 'externalParameters', 'toolchain', 'channel'), 'stable'),
        ('missing epoch', provenance, ('predicate', 'buildDefinition', 'internalParameters', 'source_date_epoch'), None),
        ('missing license', sbom, ('components', 0, 'licenses'), []),
        ('unresolved license', sbom, ('components', 0, 'licenses', 0, 'expression'), 'NOASSERTION'),
        ('missing native inventory', sbom, ('components',), [c for c in sbom['components'] if c['bom-ref'] != 'test/native']),
        ('missing bundled worker', sbom, ('components',), [c for c in sbom['components'] if c['bom-ref'] != 'dctx/git/bundled']),
        ('wrong bundled digest', sbom, ('components', 2, 'hashes', 0, 'content'), 'b' * 64),
        ('unknown SBOM field', sbom, ('components', 0, 'extra'), 1),
        ('dangling dependency', sbom, ('dependencies', 0, 'dependsOn'), ['absent']),
        ('missing feature closure', sbom, ('components', 0, 'properties'), [{'name': 'dctx:role', 'value': 'cli'}, {'name': 'dctx:license-source', 'value': 'cargo-metadata'}])
    ]
    for target in ('x86_64-apple-darwin', 'aarch64-unknown-linux-gnu', 'x86_64-pc-windows-msvc', 'x86_64-unknown-linux-musl'):
        cases.append(('unsupported target promoted', promoted, ('target',), target))
    for label, original, path, value in cases:
        changed = copy.deepcopy(original)
        parent = changed
        for key in path[:-1]:
            parent = parent[key]
        if value is None:
            del parent[path[-1]]
        else:
            parent[path[-1]] = value
        if label == 'unsupported target promoted':
            changed['qualification']['target'] = value  # Avoid a vacuous binding-mismatch refusal.
        try:
            if original is provenance:
                check_provenance(changed, contract, release)
            elif original is sbom:
                check_sbom(changed, contract, release, provenance)
            else:
                check_release(changed, contract)
        except Refusal:
            continue
        raise Refusal('self-test accepted: ' + label)
    for data in (b'{"schema":1,"schema":1}', b'{"nested":{"x":1,"x":2}}', b'{"x":NaN}'):
        try:
            decode(data)
        except Refusal:
            continue
        raise Refusal('self-test accepted duplicate/nonfinite JSON')
    try:
        check_documents(contract, release, raw={'sbom': encode(sbom) + b' '})
    except Refusal:
        pass
    else:
        raise Refusal('self-test accepted changed sidecar bytes')
    negatives = len(cases) + 4
    if sys.version_info >= (3, 11):
        inputs = contract[3]
        toolchain = '[toolchain]\nchannel = "1.97.1"\nprofile = "minimal"\ncomponents = ' + json.dumps(inputs['toolchain']['components']) + '\ntargets = ' + json.dumps(inputs['rust_std_targets']) + '\n'
        check_toolchain(toolchain, contract)
        positives += 1
        for bad in (toolchain.replace('1.97.1', 'stable'), toolchain + 'extra = true\n', toolchain.replace('"clippy", ', '')):
            try:
                check_toolchain(bad, contract)
            except Refusal:
                negatives += 1
                continue
            raise Refusal('self-test accepted changed toolchain')
    print('self-test: ' + str(positives) + ' positive controls; ' + str(negatives) + ' negative controls passed; format checks only')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--self-test', action='store_true')
    for name in ('release', 'provenance', 'sbom', 'toolchain'):
        parser.add_argument('--' + name, type=Path)
    args = parser.parse_args()
    supplied = any(getattr(args, name) for name in ('release', 'provenance', 'sbom', 'toolchain'))
    require(args.self_test != supplied, 'choose --self-test or document paths')
    contract = load_contract()
    if args.self_test:
        self_test(contract)
        return
    documents, raw = {}, {}
    for name in ('release', 'provenance', 'sbom'):
        path = getattr(args, name)
        if path:
            documents[name], data = read(path)
            if name != 'release':
                raw[name] = data
    check_documents(contract, **documents, raw=raw)
    if args.toolchain:
        check_toolchain(args.toolchain.read_text(encoding='utf-8'), contract)
    print('valid contract shape and supplied bindings; signatures/native evidence not verified')


if __name__ == '__main__':
    try:
        main()
    except (Refusal, OSError, UnicodeError, ValueError, KeyError, TypeError, RecursionError) as error:
        print('refused: ' + str(error), file=sys.stderr)
        sys.exit(1)
