"""Offline release inventory controls for CONTRACT.md sections 5–7.
Handwritten four-package metadata exercises normal/build/target closure.
The kernel alone enables the Cargo adapter with DCTX_RELEASE_REAL=1.
All ordinary tests are stdlib-only and keep their outputs in memory.
"""

import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest

sys.dont_write_bytecode = True
BASE = Path(__file__).resolve().parent
sys.path.insert(0, str(BASE))
import sbom
import provenance

spec = importlib.util.spec_from_file_location('sbom_contract_test',
                                             sbom.CONTRACT_DIR / 'validate_contract.py')
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)


def fixture():
    return json.loads((BASE / 'fixtures/metadata_small.json').read_text())


def workspace_metadata(root):
    """Retry metadata once online; restore only the lock bytes we observed."""
    lock = root / 'Cargo.lock'
    before = cargo_bytes = lock.read_bytes()
    try:
        for flags in (['--offline'], []):
            try:
                completed = subprocess.run(
                    ['cargo', 'metadata', '--format-version', '1', *flags],
                    cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=300)
            finally:
                cargo_bytes = lock.read_bytes()
            if completed.returncode == 0:
                return completed.stdout, before, cargo_bytes
        tail = '\n'.join(completed.stderr.decode(errors='replace').splitlines()[-20:])
        raise AssertionError(f'cargo metadata exited {completed.returncode}:\n{tail}')
    finally:
        if cargo_bytes != before and lock.read_bytes() == cargo_bytes:
            lock.write_bytes(before)


def test_inventory(metadata, lock_bytes, contract_bytes):
    """Explicit stand-ins for an offline smoke run; never release evidence."""
    lock_hash = hashlib.sha256(lock_bytes).hexdigest()
    contract_hash = hashlib.sha256(contract_bytes).hexdigest()
    supplied = os.environ.get('DCTX_RELEASE_INVENTORY')
    inventory = validator.decode(supplied.encode()) if supplied else {}
    inventory.setdefault('target', 'aarch64-apple-darwin')
    inventory.setdefault('profile', 'default')
    inventory.setdefault('source_revision', os.environ.get('DCTX_RELEASE_REVISION', 'unversioned'))
    inventory.setdefault('standalone_package', 'dctx-plugin-git')
    inventory.setdefault('artifacts', {
        'cli': os.environ.get('DCTX_RELEASE_CLI_SHA256', lock_hash),
        'standalone-git-plugin': os.environ.get('DCTX_RELEASE_PLUGIN_SHA256', contract_hash)})
    inventory.setdefault('source_hashes', {
        p['id']: contract_hash for p in metadata['packages'] if not p.get('source')})
    inventory.setdefault('native_libraries', [])
    inventory.setdefault('native_note', 'none declared in test run')
    inventory['test_run'] = True
    return inventory


class SbomTests(unittest.TestCase):
    def setUp(self):
        self.metadata = fixture()
        self.target = 'aarch64-apple-darwin'

    def generate(self):
        return sbom.cargo_metadata_to_cyclonedx(self.metadata, 'dctx-cli', self.target)

    def test_exact_structure_and_closed_contract(self):
        doc = self.generate()
        validator.check_sbom(doc, sbom.CONTRACT)
        self.assertEqual(set(doc), {'bomFormat', 'specVersion', 'version', 'components',
                                    'dependencies', 'properties'})
        self.assertEqual((doc['bomFormat'], doc['specVersion'], doc['version']),
                         ('CycloneDX', '1.5', 1))
        self.assertEqual(doc['properties'], [
            {'name': 'dctx:profile', 'value': 'default'},
            {'name': 'dctx:source-revision', 'value': 'b' * 40},
            {'name': 'dctx:target', 'value': self.target}])
        components = {c['bom-ref']: c for c in doc['components']}
        self.assertEqual(len(components), 6)
        expected = {
            'type': 'library', 'bom-ref': 'pkg:cargo/serde@1.0.0', 'name': 'serde',
            'version': '1.0.0', 'purl': 'pkg:cargo/serde@1.0.0',
            'hashes': [{'alg': 'SHA-256', 'content': 'a' * 64}],
            'licenses': [{'expression': 'MIT OR Apache-2.0'}],
            'properties': [{'name': 'dctx:features', 'value': '[]'},
                           {'name': 'dctx:license-source', 'value': 'cargo-metadata'},
                           {'name': 'dctx:role', 'value': 'rust-crate'}]}
        self.assertEqual(components['pkg:cargo/serde@1.0.0'], expected)
        self.assertEqual(doc['dependencies'], [
            {'ref': 'dctx/cli', 'dependsOn': ['dctx/git/bundled', 'native/system',
             'pkg:cargo/serde@1.0.0', 'urn:dctx:first-party:build-helper@0.1.0']},
            {'ref': 'dctx/git/bundled', 'dependsOn': ['pkg:cargo/serde@1.0.0']},
            {'ref': 'dctx/git/standalone', 'dependsOn': ['native/system', 'pkg:cargo/serde@1.0.0']},
            {'ref': 'native/system', 'dependsOn': []},
            {'ref': 'pkg:cargo/serde@1.0.0', 'dependsOn': []},
            {'ref': 'urn:dctx:first-party:build-helper@0.1.0', 'dependsOn': []}])
        self.assertEqual(components['dctx/git/bundled']['hashes'], components['dctx/cli']['hashes'])
        doc['components'][0]['scope'] = 'required'
        with self.assertRaises(validator.Refusal):
            validator.check_sbom(doc, sbom.CONTRACT)

    def test_deterministic_without_mutating_input(self):
        original = copy.deepcopy(self.metadata)
        first = sbom.encode(self.generate())
        self.metadata['packages'].reverse()
        self.metadata['resolve']['nodes'].reverse()
        self.assertEqual(first, sbom.encode(self.generate()))
        self.metadata['packages'].reverse()
        self.metadata['resolve']['nodes'].reverse()
        self.assertEqual(self.metadata, original)
        self.assertNotIn(b'timestamp', first)
        self.assertNotIn(b'/fixture/', first)

    def test_target_and_build_only_edges(self):
        build_ref = 'urn:dctx:first-party:build-helper@0.1.0'
        self.assertIn(build_ref, {c['bom-ref'] for c in self.generate()['components']})
        self.metadata['resolve']['nodes'][0]['deps'][1]['dep_kinds'][0]['kind'] = 'dev'
        self.assertNotIn(build_ref, {c['bom-ref'] for c in self.generate()['components']})
        self.metadata['resolve']['nodes'][0]['deps'][1]['dep_kinds'][0]['kind'] = 'build'
        self.target = 'x86_64-pc-windows-msvc'
        self.metadata['metadata']['dctx_release']['target'] = self.target
        self.assertNotIn(build_ref, {c['bom-ref'] for c in self.generate()['components']})

    def test_refusals(self):
        changes = [
            lambda m: m['packages'][2].update(checksum=None),
            lambda m: m['packages'][2].update(license=None),
            lambda m: m['packages'][2].update(license='NOASSERTION'),
            lambda m: m['metadata']['dctx_release'].update(native_libraries=[]),
            lambda m: m['metadata']['dctx_release'].update(source_hashes={}),
            lambda m: m['metadata']['dctx_release'].update(source_revision='HEAD'),
            lambda m: m['resolve']['nodes'][0]['deps'][0].update(pkg='missing'),
            lambda m: m['resolve']['nodes'][0]['deps'][0].update(dep_kinds=[]),
            lambda m: m['resolve']['nodes'][0].pop('features'),
            lambda m: m.update(metadata={}),
        ]
        for change in changes:
            with self.subTest(change=changes.index(change)):
                self.metadata = fixture()
                change(self.metadata)
                with self.assertRaises((sbom.Refusal, KeyError)):
                    self.generate()
        with self.assertRaisesRegex(sbom.Refusal, 'unknown-target'):
            sbom.cargo_metadata_to_cyclonedx(fixture(), 'dctx-cli', 'unknown-target')

    def test_strict_profile(self):
        self.metadata['metadata']['dctx_release']['profile'] = 'strict'
        self.metadata['resolve']['nodes'][0]['features'] = ['strict']
        doc = self.generate()
        validator.check_sbom(doc, sbom.CONTRACT)
        self.assertNotIn('dctx/git/bundled', {c['bom-ref'] for c in doc['components']})

    def test_cfg_predicates(self):
        for expression, expected in [
            (None, True), ('aarch64-apple-darwin', True),
            ('cfg(any(windows, target_os = "macos"))', True),
            ('cfg(not(unix))', False), ('cfg(all())', True), ('cfg(any())', False),
            ('cfg(target_has_atomic = "ptr")', True),
            ('cfg(target_has_atomic = "128")', False), ('cfg(target_abi = "")', True),
        ]:
            self.assertEqual(sbom.target_matches(expression, self.target), expected)
        for target in ('x86_64-unknown-linux-gnu', self.target):
            for atom in ('unix', 'panic = "unwind"', 'target_pointer_width = "64"',
                         'target_endian = "little"', 'target_has_atomic = "ptr"'):
                self.assertIs(sbom.target_matches('cfg(' + atom + ')', target), True)
            for atom in ('windows', 'debug_assertions', 'test', 'target_feature = "neon"'):
                self.assertIs(sbom.target_matches('cfg(' + atom + ')', target), False)
        self.assertTrue(sbom.target_matches('cfg(target_feature = "neon")', self.target, ['neon']))
        for expression in ('cfg(feature = "x")', 'cfg(custom_atom)', 'cfg(not(custom_atom))'):
            self.assertIsNone(sbom.target_matches(expression, self.target))
        for expression in ('cfg(not())', 'cfg(unix', 'cfg(unix)junk', 'cfg(all(unix, any(windows))'):
            with self.assertRaises(sbom.Refusal):
                sbom.target_matches(expression, self.target)

    def test_unknown_cfg_inclusion_and_contract(self):
        cfg = 'cfg(custom_atom = "enabled")'
        self.metadata['resolve']['nodes'][0]['deps'][1]['dep_kinds'][0]['target'] = cfg
        doc = self.generate()
        helper = next(c for c in doc['components'] if c['name'] == 'build-helper')
        self.assertEqual(validator.properties(helper['properties'])['cdx:cargo:cfg-unresolved'], cfg)
        self.assertEqual(validator.properties(doc['properties'])['cdx:cargo:cfg-unresolved-count'], '1')
        validator.check_sbom(doc, sbom.CONTRACT)
        for value in ('', '-1', '1.0', '１２'):
            changed = copy.deepcopy(doc)
            changed['properties'][0]['value'] = value
            with self.assertRaises(validator.Refusal):
                validator.check_sbom(changed, sbom.CONTRACT)
        helper['properties'][0]['name'] = 'cdx:cargo:unknown'
        with self.assertRaises(validator.Refusal):
            validator.check_sbom(doc, sbom.CONTRACT)

    def test_self_contained_test_inventory(self):
        from unittest.mock import patch
        with patch.dict(os.environ, {}, clear=True):
            inventory = test_inventory(self.metadata, b'lock', b'contract')
        self.assertEqual(inventory['source_revision'], 'unversioned')
        self.assertEqual(inventory['native_libraries'], [])
        self.assertEqual(inventory['native_note'], 'none declared in test run')
        self.assertEqual(inventory['artifacts']['cli'], hashlib.sha256(b'lock').hexdigest())
        self.metadata['metadata']['dctx_release'] = inventory
        doc = self.generate()
        validator.validate(doc, sbom.CONTRACT[2]['schema'])
        with self.assertRaises(validator.Refusal):
            validator.check_sbom(doc, sbom.CONTRACT)
        inventory.pop('test_run')
        with self.assertRaises(sbom.Refusal):
            self.generate()

    def test_metadata_retry_and_lock_restore(self):
        from unittest.mock import MagicMock, patch
        for codes in ((0,), (101, 0), (101, 101)):
            with self.subTest(codes=codes):
                root, lock = MagicMock(), MagicMock()
                root.__truediv__.return_value = lock
                lock.read_bytes.side_effect = [b'original', *([b'cargo'] * len(codes)), b'cargo']
                results = [subprocess.CompletedProcess([], code, b'{}',
                           '\n'.join(str(n) for n in range(25)).encode()) for code in codes]
                with patch.object(subprocess, 'run', side_effect=results) as run:
                    if codes[-1]:
                        with self.assertRaisesRegex(AssertionError, 'exited 101:\n5\n') as error:
                            workspace_metadata(root)
                        self.assertTrue(str(error.exception).endswith('\n24'))
                    else:
                        self.assertEqual(workspace_metadata(root), (b'{}', b'original', b'cargo'))
                self.assertEqual(run.call_count, len(codes))
                self.assertEqual(run.call_args_list[0].args[0][-1], '--offline')
                if len(codes) == 2:
                    self.assertNotIn('--offline', run.call_args_list[1].args[0])
                lock.write_bytes.assert_called_once_with(b'original')
        lock.read_bytes.side_effect = [b'original', b'cargo', b'concurrent']
        lock.write_bytes.reset_mock()
        with patch.object(subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'{}')):
            workspace_metadata(root)
        lock.write_bytes.assert_not_called()

    @unittest.skipUnless(os.environ.get('DCTX_RELEASE_REAL') == '1', 'kernel-only real Cargo check')
    def test_real_workspace(self):
        root = BASE.parents[1]
        output, before, resolved_lock = workspace_metadata(root)
        metadata = validator.decode(output)
        contract_bytes = (sbom.CONTRACT_DIR / 'CONTRACT.md').read_bytes()
        inventory = test_inventory(metadata, before, contract_bytes)
        metadata['metadata'] = dict(metadata.get('metadata') or {})
        metadata['metadata']['dctx_release'] = inventory
        # Cargo metadata omits checksums; use the lock matching its resolution.
        import tomllib
        locked = tomllib.loads(resolved_lock.decode())['package']
        checksums = {(p['name'], p['version'], p.get('source')): p.get('checksum') for p in locked}
        for package in metadata['packages']:
            if (package.get('source') or '').startswith('registry+'):
                package['checksum'] = checksums.get((package['name'], package['version'], package['source']))
        doc = sbom.cargo_metadata_to_cyclonedx(metadata, 'dctx-cli', inventory['target'])
        validator.validate(doc, sbom.CONTRACT[2]['schema'])
        nodes = {c['bom-ref']: c for c in doc['components']}
        edges = {row['ref']: row['dependsOn'] for row in doc['dependencies']}
        self.assertEqual(len(nodes), len(doc['components']))
        self.assertEqual(len(edges), len(doc['dependencies']))
        self.assertEqual(nodes.keys(), edges.keys())
        self.assertTrue(all(set(deps) <= nodes.keys() for deps in edges.values()))
        self.assertEqual(sbom.encode(doc), sbom.encode(
            sbom.cargo_metadata_to_cyclonedx(metadata, 'dctx-cli', inventory['target'])))
        lock_hash = hashlib.sha256(before).hexdigest()
        contract_hash = hashlib.sha256(contract_bytes).hexdigest()
        subjects = {'Cargo.lock': lock_hash, 'CONTRACT.md': contract_hash,
                    'plugin.json': inventory['artifacts']['cli'],
                    'SHA256SUMS': inventory['artifacts']['standalone-git-plugin'],
                    'sbom.cdx.json': hashlib.sha256(sbom.encode(doc)).hexdigest()}
        args = dict(subjects=subjects, builder_id='urn:dctx:builder:non-promotable-test',
                    toolchain=sbom.CONTRACT[3]['toolchain'], target=inventory['target'],
                    profile=inventory['profile'], source_revision=inventory['source_revision'],
                    source_date_epoch=0, build_inputs_bytes=(sbom.CONTRACT_DIR /
                    'build-inputs.json').read_bytes(), cargo_lock_sha256=lock_hash,
                    source_sha256=contract_hash,
                    native_inputs_sha256=hashlib.sha256(sbom.encode({
                        'libraries': inventory['native_libraries'],
                        'note': inventory['native_note']})).hexdigest(), test_only=True)
        statement = provenance.build_provenance(**args)
        provenance.check_test_shape(statement)
        self.assertEqual(sbom.encode(statement), sbom.encode(provenance.build_provenance(**args)))
        self.assertEqual({s['name']: s['digest']['sha256'] for s in statement['subject']}, subjects)
        context = validator.properties(doc['properties'])
        parameters = statement['predicate']['buildDefinition']['externalParameters']
        for field in ('target', 'profile'):
            self.assertEqual(context['dctx:' + field], parameters[field])
        self.assertEqual(context['dctx:source-revision'], parameters['source_revision'])
        materials = {m['uri']: m['digest']['sha256'] for m in
                     statement['predicate']['buildDefinition']['resolvedDependencies']}
        self.assertEqual(materials['urn:dctx:lock:workspace'], lock_hash)
        self.assertEqual(materials['urn:dctx:build-inputs'], sbom.CONTRACT[4])


if __name__ == '__main__':
    unittest.main()
