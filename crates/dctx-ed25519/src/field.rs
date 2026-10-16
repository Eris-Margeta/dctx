//! Arithmetic modulo 2^255 - 19 for RFC 8032 §5.1 point operations.
//! Five radix-2^51 limbs are always canonical; products use bounded u128 sums.
//! Pure, allocation-free arithmetic used only by the private point module.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Field([u64; 5]);

const MASK: u64 = (1 << 51) - 1;
const MODULUS: [u64; 5] = [MASK - 18, MASK, MASK, MASK, MASK];

impl Field {
    pub(crate) const ZERO: Self = Self([0; 5]);
    pub(crate) const ONE: Self = Self([1, 0, 0, 0, 0]);
    pub(crate) const BASE_X: Self = Self([
        1738742601995546,
        1146398526822698,
        2070867633025821,
        562264141797630,
        587772402128613,
    ]);
    pub(crate) const BASE_Y: Self = Self([
        1801439850948184,
        1351079888211148,
        450359962737049,
        900719925474099,
        1801439850948198,
    ]);
    pub(crate) const D: Self = Self([
        929955233495203,
        466365720129213,
        1662059464998953,
        2033849074728123,
        1442794654840575,
    ]);
    const SQRT_MINUS_ONE: Self = Self([
        1718705420411056,
        234908883556509,
        2233514472574048,
        2117202627021982,
        765476049583133,
    ]);

    pub(crate) fn decode(bytes: &[u8; 32]) -> Option<Self> {
        if bytes[31] & 0x80 != 0 {
            return None;
        }
        let mut limbs = [0; 5];
        for bit in 0..255 {
            limbs[bit / 51] |= u64::from((bytes[bit / 8] >> (bit % 8)) & 1) << (bit % 51);
        }
        let value = Self(limbs);
        (Self::reduce(limbs.map(u128::from)) == value).then_some(value)
    }

    pub(crate) fn is_odd(self) -> bool {
        self.0[0] & 1 != 0
    }

    pub(crate) fn add(self, rhs: Self) -> Self {
        Self::reduce(core::array::from_fn(|i| {
            u128::from(self.0[i]) + u128::from(rhs.0[i])
        }))
    }

    pub(crate) fn sub(self, rhs: Self) -> Self {
        Self::reduce(core::array::from_fn(|i| {
            u128::from(self.0[i]) + 2 * u128::from(MODULUS[i]) - u128::from(rhs.0[i])
        }))
    }

    pub(crate) fn negate(self) -> Self {
        Self::ZERO.sub(self)
    }

    pub(crate) fn mul(self, rhs: Self) -> Self {
        let mut product = [0u128; 5];
        for (i, a) in self.0.iter().enumerate() {
            for (j, b) in rhs.0.iter().enumerate() {
                let factor = if i + j >= 5 { 19 } else { 1 };
                product[(i + j) % 5] += u128::from(*a) * u128::from(*b) * factor;
            }
        }
        Self::reduce(product)
    }

    pub(crate) fn square(self) -> Self {
        self.mul(self)
    }

    pub(crate) fn invert(self) -> Self {
        let mut exponent = [0xff; 32];
        exponent[0] = 0xeb;
        exponent[31] = 0x7f;
        self.pow(&exponent)
    }

    pub(crate) fn sqrt(self) -> Option<Self> {
        let mut exponent = [0xff; 32];
        exponent[0] = 0xfe;
        exponent[31] = 0x0f;
        let mut root = self.pow(&exponent);
        if root.square() != self {
            root = root.mul(Self::SQRT_MINUS_ONE);
        }
        (root.square() == self).then_some(root)
    }

    fn pow(self, exponent: &[u8; 32]) -> Self {
        let mut result = Self::ONE;
        for bit in (0..256).rev() {
            result = result.square();
            if (exponent[bit / 8] >> (bit % 8)) & 1 != 0 {
                result = result.mul(self);
            }
        }
        result
    }

    fn reduce(mut limbs: [u128; 5]) -> Self {
        // Each product sum is < 2^109. First carry leaves limb 0 < 2^63;
        // second leaves it <= MASK+19; third leaves all limbs <= MASK.
        for _ in 0..3 {
            for i in 0..4 {
                limbs[i + 1] += limbs[i] >> 51;
                limbs[i] &= u128::from(MASK);
            }
            limbs[0] += 19 * (limbs[4] >> 51);
            limbs[4] &= u128::from(MASK);
        }
        let mut reduced = limbs.map(|limb| limb as u64);
        if reduced[1..] == [MASK; 4] && reduced[0] >= MODULUS[0] {
            reduced[0] -= MODULUS[0];
            reduced[1..].fill(0);
        }
        Self(reduced)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn modulus_boundaries_and_constants() {
        let mut p = [0xff; 32];
        p[0] = 0xed;
        p[31] = 0x7f;
        assert!(Field::decode(&p).is_none());
        p[0] -= 1;
        let minus_one = Field::decode(&p).unwrap();
        assert_eq!(minus_one.add(Field::ONE), Field::ZERO);
        assert_eq!(minus_one.square(), Field::ONE);
        assert_eq!(Field::SQRT_MINUS_ONE.square(), minus_one);
        let a = Field([121665, 0, 0, 0, 0]);
        let b = Field([121666, 0, 0, 0, 0]);
        assert_eq!(Field::D.mul(b), a.negate());
        assert_eq!(
            Field::reduce([u128::from(MASK); 5]),
            Field([18, 0, 0, 0, 0])
        );
        let maximal = Field::decode(&p).unwrap();
        assert_eq!(maximal.mul(maximal), Field::ONE);
        assert_eq!(Field::ZERO.sqrt(), Some(Field::ZERO));
        assert!(Field([2, 0, 0, 0, 0]).sqrt().is_none());
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]
        #[test]
        fn field_invariants(a in any::<[u64; 5]>(), b in any::<[u64; 5]>(), c in any::<[u64; 5]>()) {
            let a = Field::reduce(a.map(u128::from));
            let b = Field::reduce(b.map(u128::from));
            let c = Field::reduce(c.map(u128::from));
            prop_assert_eq!(a.add(b).sub(b), a);
            prop_assert_eq!(a.mul(b.add(c)), a.mul(b).add(a.mul(c)));
            prop_assert_eq!(a.mul(b).mul(c), a.mul(b.mul(c)));
            prop_assert_eq!(a.mul(b), b.mul(a));
            if a != Field::ZERO {
                prop_assert_eq!(a.mul(a.invert()), Field::ONE);
            }
            prop_assert_eq!(a.square().sqrt().unwrap().square(), a.square());
            prop_assert!(a.0.iter().all(|limb| *limb <= MASK));
        }
    }
}
