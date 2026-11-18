// SPDX-License-Identifier: Apache-2.0

use dctx_hash::Sha256;

const VECTORS: &[(usize, &str)] = &[
    (
        0,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ),
    (
        1,
        "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
    ),
    (
        55,
        "463eb28e72f82e0a96c0a4cc53690c571281131f672aa229e0d45ae59b598b59",
    ),
    (
        56,
        "da2ae4d6b36748f2a318f23e7ab1dfdf45acdc9d049bd80e59de82a60895f562",
    ),
    (
        63,
        "29af2686fd53374a36b0846694cc342177e428d1647515f078784d69cdb9e488",
    ),
    (
        64,
        "fdeab9acf3710362bd2658cdc9a29e8f9c757fcf9811603a8c447cd1d9151108",
    ),
    (
        65,
        "4bfd2c8b6f1eec7a2afeb48b934ee4b2694182027e6d0fc075074f2fabb31781",
    ),
    (
        127,
        "92ca0fa6651ee2f97b884b7246a562fa71250fedefe5ebf270d31c546bfea976",
    ),
    (
        128,
        "471fb943aa23c511f6f72f8d1652d9c880cfa392ad80503120547703e56a2be5",
    ),
    (
        129,
        "5099c6a56203f9687f7d33f4bfdf576d31dc91f6b695ecea38b2770c87631135",
    ),
];

#[test]
fn sha256_matches_fixed_answers_for_binary_prefixes() {
    // Expected SHA-256 digests for prefixes of bytes 0 through 128, generated with Python hashlib.
    let bytes: Vec<u8> = (0..=128).collect();
    for &(len, hex) in VECTORS {
        let message = &bytes[..len];
        let expected = format!("v1:sha256:{hex}:{len}");
        assert_eq!(
            Sha256::digest(message).unwrap().to_string(),
            expected,
            "length {len}"
        );

        for split in 0..=len {
            let mut state = Sha256::new();
            state.update(&message[..split]).unwrap();
            state.update(b"").unwrap();
            state.update(&message[split..]).unwrap();
            assert_eq!(
                state.finalize().to_string(),
                expected,
                "length {len}, split {split}"
            );
        }
    }
}
