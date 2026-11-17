#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Validate pack JSON from stdin against the checked-in Draft 2020-12 schema (offline)."""

import json
from pathlib import Path
import sys

from jsonschema import Draft202012Validator

schema = json.loads((Path(__file__).resolve().parents[1] / "schemas/dctx.pack-1.schema.json").read_text())
Draft202012Validator.check_schema(schema)
Draft202012Validator(schema).validate(json.load(sys.stdin))
