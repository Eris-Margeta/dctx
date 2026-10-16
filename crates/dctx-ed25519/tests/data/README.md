# Verification fixture provenance

`ed25519_test.json` is the full, unmodified C2SP Wycheproof corpus:

- Source: https://github.com/C2SP/wycheproof/blob/5722833ca004983abd1a91bcb6c24596d50ac0f9/testvectors_v1/ed25519_test.json
- Revision: `5722833ca004983abd1a91bcb6c24596d50ac0f9`.
- SHA-256: `752d2ea7d7c6cf4736381b6cbacb61f8182b126ab7cd9b058f00c50084975536`.
- Upstream source label: `google-wycheproof`, version `0.9rc5`; license Apache-2.0
  (see the included upstream `WYCHEPROOF-LICENSE`).
- 151 cases, 88 valid and 63 invalid. The integration test pins all IDs and flags.

`rfc8032-section-7.1.json` transcribes **all five** public verification tuples
from https://www.rfc-editor.org/rfc/rfc8032.txt §7.1 (January 2017). Only public
keys, messages, signatures and test names are retained. The source document's
SHA-256 at extraction was
`ed63657ff389301282b169b0abde9b5dd2c7e4d524fdfa5da6ff3094fc93c4c3`.
The SHA(`abc`) case is pure Ed25519 on the 64-byte hash, not Ed25519ph.
RFC copyright is held by the IETF Trust and identified authors; see
https://trustee.ietf.org/license-info for the document's legal provisions.

`policy.json` contains first-party **public algebra** fixtures, not secret keys:

- `small_order_points` enumerates `[i]T` for `i = 0..7`, starting at the identity,
  where T has compressed encoding
  `c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a`.
- `cofactorless` uses the RFC base point B and the order-two point Q = (0,-1).
  A is B or B+Q; R is B or B+Q. For each stated message, calculate
  `k = SHA-512(R || A || M) mod L` and `S = (1+k) mod L`.
  The signature passes precisely when the Q components of `R + [k]A` cancel.
  All six pass a cofactored equation; only three pass the required equation.
- Messages are UTF-8 `PL-ED public algebra fixture N`, with the first integer
  N >= 0 giving the named torsion outcome **and** different parity between the
  full hash integer and its reduction mod L. This also detects omission of
  scalar reduction. Point operations for fixture construction used affine
  integer arithmetic modulo p, independently of the Rust limb implementation.
- The tests require both the stated algebraic result and dalek's strict result.

These files are test inputs. Tests read embedded bytes and never rewrite them.
