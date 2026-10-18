#!/usr/bin/env python3
"""Pure in-toto/SLSA release statements, CONTRACT.md sections 5–7.
Consumes caller digests, revision labels, epoch and approved recipe bytes.
Uses stdlib and sbom's contract adapter; never invokes Git or a builder.
The CLI only reads supplied files and writes canonical JSON.
"""

import argparse
import copy
import hashlib
from pathlib import Path
import sys

sys.dont_write_bytecode = True
if __package__ in (None, ''):
    sys.path.insert(0, str(Path(__file__).resolve().parent))
from sbom import CONTRACT, CONTRACT_DIR, contract_validator, digest, encode, require


def check_test_shape(document):
    """Check closed statement structure, excluding the revision evidence field.

    The frozen schema has no test profile. A literal unversioned label is kept
    visible; it is never replaced with an invented production revision.
    """
    excluded = ('predicate', 'buildDefinition', 'externalParameters', 'source_revision')

    def visit(value, schema, path=()):
        if path == excluded:
            require(isinstance(value, str) and value.strip(), 'missing-test-revision-label')
        elif schema.get('type') == 'object':
            require(type(value) is dict, 'test-shape-object')
            require(set(schema.get('required', [])) <= value.keys(), 'test-shape-missing-field')
            require(value.keys() <= schema['properties'].keys(), 'test-shape-unknown-field')
            for key, child in value.items():
                visit(child, schema['properties'][key], path + (key,))
        else:
            contract_validator.validate(value, schema, CONTRACT[1])

    visit(document, CONTRACT[1])


def build_provenance(*, subjects, builder_id, toolchain, target, profile,
                     source_revision, source_date_epoch, build_inputs_bytes,
                     cargo_lock_sha256, source_sha256, native_inputs_sha256,
                     materials=(), test_only=False):
    """Build a fresh statement from supplied evidence, without IO.

    Frozen contract requires the exact recipe digest. Its canonical JSON digest
    is also recorded as a distinct material (canonical encoding includes LF).
    Extra materials are {uri, digest:{sha256}} objects, including source purls.
    """
    recipe = contract_validator.decode(build_inputs_bytes)
    require(recipe == CONTRACT[3], 'unapproved-build-inputs')
    require(toolchain == recipe['toolchain'], 'unapproved-toolchain')
    require(type(source_date_epoch) is int and 0 <= source_date_epoch <= 9007199254740991,
            'invalid-source-epoch')
    recipe_hash = hashlib.sha256(build_inputs_bytes).hexdigest()
    require(recipe_hash == CONTRACT[4], 'build-inputs-exact-bytes-mismatch')
    hashes = {'urn:dctx:build-inputs': recipe_hash,
              'urn:dctx:build-inputs:canonical': hashlib.sha256(encode(recipe)).hexdigest(),
              'urn:dctx:lock:workspace': digest(cargo_lock_sha256),
              'urn:dctx:source': digest(source_sha256),
              'urn:dctx:native-inputs': digest(native_inputs_sha256)}
    for material in materials:
        require(set(material) == {'uri', 'digest'} and set(material['digest']) == {'sha256'},
                'unknown-material-field')
        require(material['uri'] not in hashes, 'duplicate-material-uri')
        hashes[material['uri']] = digest(material['digest']['sha256'])
    document = {'_type': 'https://in-toto.io/Statement/v1',
                'predicateType': 'https://slsa.dev/provenance/v1',
                'subject': [{'name': name, 'digest': {'sha256': digest(value)}}
                            for name, value in sorted(subjects.items())],
                'predicate': {'buildDefinition': {
                    'buildType': 'urn:dctx:release:build:v1',
                    'externalParameters': {'target': target, 'profile': profile,
                                           'source_revision': source_revision,
                                           'toolchain': copy.deepcopy(toolchain)},
                    'internalParameters': {'source_date_epoch': source_date_epoch},
                    'resolvedDependencies': [{'uri': uri, 'digest': {'sha256': value}}
                                             for uri, value in sorted(hashes.items())]},
                    'runDetails': {'builder': {'id': builder_id}}}}
    if test_only:
        check_test_shape(document)
    else:
        contract_validator.check_provenance(document, CONTRACT)
    return document


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--subject', action='append', required=True, metavar='NAME=SHA256')
    parser.add_argument('--material', action='append', default=[], metavar='URI=SHA256')
    for name in ('builder-id', 'target', 'profile', 'source-revision', 'source-sha256',
                 'native-inputs-sha256', 'cargo-lock', 'out'):
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--epoch', required=True, type=int)
    parser.add_argument('--build-inputs', default=str(CONTRACT_DIR / 'build-inputs.json'))
    args = parser.parse_args(argv)
    try:
        subjects = contract_validator.pairs(item.rsplit('=', 1) for item in args.subject)
        materials = [{'uri': uri, 'digest': {'sha256': value}}
                     for uri, value in (item.rsplit('=', 1) for item in args.material)]
        recipe_bytes = Path(args.build_inputs).read_bytes()
        recipe = contract_validator.decode(recipe_bytes)
        document = build_provenance(
            subjects=subjects, builder_id=args.builder_id, toolchain=recipe['toolchain'],
            target=args.target, profile=args.profile, source_revision=args.source_revision,
            source_date_epoch=args.epoch, build_inputs_bytes=recipe_bytes,
            cargo_lock_sha256=hashlib.sha256(Path(args.cargo_lock).read_bytes()).hexdigest(),
            source_sha256=args.source_sha256, native_inputs_sha256=args.native_inputs_sha256,
            materials=materials)
        Path(args.out).write_bytes(encode(document))
    except (ValueError, KeyError, TypeError, OSError) as error:
        parser.exit(2, 'provenance: ' + str(error) + '\n')


if __name__ == '__main__':
    main()
