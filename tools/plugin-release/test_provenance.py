"""Statement controls for frozen CONTRACT.md sections 5–7.
Uses synthetic caller digests with the actual approved recipe bytes.
Exercises closed validation, canonical encoding and refusal bindings.
No builders, Cargo, Git, network or disk output are involved.
"""

import copy
import hashlib
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import provenance
from sbom import CONTRACT, CONTRACT_DIR, contract_validator as validator, encode


def inputs():
    return dict(subjects={'dctx': '1' * 64, 'dctx-plugin-git': '2' * 64,
                          'plugin.json': '3' * 64, 'SHA256SUMS': '4' * 64,
                          'sbom.cdx.json': '5' * 64},
                builder_id='urn:dctx:builder:native-mac', toolchain=copy.deepcopy(CONTRACT[3]['toolchain']),
                target='aarch64-apple-darwin', profile='default', source_revision='a' * 40,
                source_date_epoch=1234567890,
                build_inputs_bytes=(CONTRACT_DIR / 'build-inputs.json').read_bytes(),
                cargo_lock_sha256='6' * 64, source_sha256='7' * 64,
                native_inputs_sha256='8' * 64)


class ProvenanceTests(unittest.TestCase):
    def test_exact_statement(self):
        args = inputs()
        doc = provenance.build_provenance(**args)
        expected = {
            '_type': 'https://in-toto.io/Statement/v1',
            'predicateType': 'https://slsa.dev/provenance/v1',
            'subject': [{'name': name, 'digest': {'sha256': value}}
                        for name, value in sorted(args['subjects'].items())],
            'predicate': {
                'buildDefinition': {
                    'buildType': 'urn:dctx:release:build:v1',
                    'externalParameters': {key: args[key] for key in
                                           ('target', 'profile', 'source_revision', 'toolchain')},
                    'internalParameters': {'source_date_epoch': 1234567890},
                    'resolvedDependencies': [
                        {'uri': uri, 'digest': {'sha256': value}} for uri, value in sorted({
                            'urn:dctx:build-inputs': hashlib.sha256(args['build_inputs_bytes']).hexdigest(),
                            'urn:dctx:build-inputs:canonical': hashlib.sha256(encode(CONTRACT[3])).hexdigest(),
                            'urn:dctx:lock:workspace': '6' * 64,
                            'urn:dctx:native-inputs': '8' * 64,
                            'urn:dctx:source': '7' * 64}.items())]},
                'runDetails': {'builder': {'id': args['builder_id']}}}}
        self.assertEqual(doc, expected)
        validator.check_provenance(doc, CONTRACT)
        args['toolchain']['channel'] = 'changed-after-call'
        self.assertEqual(doc['predicate']['buildDefinition']['externalParameters']['toolchain'],
                         CONTRACT[3]['toolchain'])

    def test_determinism_and_material_order(self):
        args = inputs()
        args['materials'] = [{'uri': 'pkg:cargo/z@1', 'digest': {'sha256': 'a' * 64}},
                             {'uri': 'pkg:cargo/a@1', 'digest': {'sha256': 'b' * 64}}]
        original = copy.deepcopy(args)
        first = encode(provenance.build_provenance(**args))
        self.assertEqual(args, original)
        args['subjects'] = dict(reversed(list(args['subjects'].items())))
        args['materials'].reverse()
        self.assertEqual(first, encode(provenance.build_provenance(**args)))
        self.assertTrue(first.endswith(bytes([10])))

    def test_refusals(self):
        mutations = [
            {'target': 'unknown'}, {'source_revision': 'HEAD'}, {'source_date_epoch': True},
            {'source_date_epoch': -1}, {'source_date_epoch': 1.5},
            {'builder_id': 'unapproved-builder'}, {'cargo_lock_sha256': 'A' * 64},
            {'toolchain': {}}, {'subjects': {'plugin.json': '1' * 64}},
            {'build_inputs_bytes': encode(CONTRACT[3])},
            {'materials': [{'uri': 'urn:dctx:source', 'digest': {'sha256': '1' * 64}}]},
            {'materials': [{'uri': 'pkg:cargo/x@1', 'digest': {'sha256': '1' * 64}, 'extra': 1}]},
        ]
        for mutation in mutations:
            with self.subTest(mutation=list(mutation)):
                args = inputs()
                args.update(mutation)
                with self.assertRaises(validator.Refusal):
                    provenance.build_provenance(**args)
        for bad in ('provenance.json', 'release.json', 'CON.txt', 'PLUGIN.JSON', '../x'):
            args = inputs()
            args['subjects'][bad] = args['subjects'].pop('dctx')
            with self.subTest(subject=bad), self.assertRaises(validator.Refusal):
                provenance.build_provenance(**args)

    def test_closed_fields(self):
        for location in ('top', 'predicate', 'digest', 'parameters'):
            doc = provenance.build_provenance(**inputs())
            objects = {'top': doc, 'predicate': doc['predicate'],
                       'digest': doc['subject'][0]['digest'],
                       'parameters': doc['predicate']['buildDefinition']['externalParameters']}
            objects[location]['extra'] = True
            with self.subTest(location=location), self.assertRaises(validator.Refusal):
                validator.check_provenance(doc, CONTRACT)

    def test_non_promotable_shape_checks(self):
        args = inputs()
        args.update(source_revision='unversioned', test_only=True)
        doc = provenance.build_provenance(**args)
        provenance.check_test_shape(doc)
        with self.assertRaises(validator.Refusal):
            validator.check_provenance(doc, CONTRACT)
        for location, key, value in [
            ('top', 'extra', True), ('parameters', 'extra', True),
            ('parameters', 'target', 'unknown'), ('parameters', 'toolchain', {}),
            ('parameters', 'source_revision', ''), ('digest', 'sha256', 'bad'),
        ]:
            changed = copy.deepcopy(doc)
            locations = {'top': changed, 'digest': changed['subject'][0]['digest'],
                         'parameters': changed['predicate']['buildDefinition']['externalParameters']}
            locations[location][key] = value
            with self.subTest(location=location, key=key), self.assertRaises(validator.Refusal):
                provenance.check_test_shape(changed)


if __name__ == '__main__':
    unittest.main()
