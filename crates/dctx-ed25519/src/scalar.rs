//! Public scalar range checks and reduction modulo the Ed25519 subgroup order.
//! Implements RFC 8032 §5.1.7: S is canonical, SHA-512 challenges reduce mod L.
//! Byte arithmetic is bounded; verification never handles secret scalars.

pub(crate) const ORDER: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
];

pub(crate) fn is_canonical(scalar: &[u8; 32]) -> bool {
    scalar.iter().rev().cmp(ORDER.iter().rev()).is_lt()
}

pub(crate) fn reduce(wide: &[u8; 64]) -> [u8; 32] {
    let mut remainder = [0u8; 32];
    for bit in (0..512).rev() {
        let mut carry = u16::from((wide[bit / 8] >> (bit % 8)) & 1);
        for byte in &mut remainder {
            let doubled = 2 * u16::from(*byte) + carry;
            *byte = doubled as u8;
            carry = doubled >> 8;
        }
        // Before doubling remainder < L; afterwards it is < 2L < 2^254.
        if !is_canonical(&remainder) {
            let mut borrow = 0i16;
            for (byte, modulus) in remainder.iter_mut().zip(ORDER) {
                let difference = i16::from(*byte) - i16::from(modulus) - borrow;
                *byte = difference as u8;
                borrow = i16::from(difference < 0);
            }
        }
    }
    remainder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_boundaries() {
        let mut less = ORDER;
        less[0] -= 1;
        assert!(is_canonical(&less));
        assert!(is_canonical(&[0; 32]));
        assert!(!is_canonical(&ORDER));
        assert!(!is_canonical(&[0xff; 32]));
        let mut wide = [0; 64];
        wide[..32].copy_from_slice(&ORDER);
        assert_eq!(reduce(&wide), [0; 32]);
        wide[0] += 1;
        let mut one = [0; 32];
        one[0] = 1;
        assert_eq!(reduce(&wide), one);
        assert_eq!(
            reduce(&[0xff; 64]),
            hex::decode("000f9c44e31106a447938568a71b0ed065bef517d273ecce3d9a307c1b419903")
                .unwrap()
                .as_slice()
        );
    }
}
