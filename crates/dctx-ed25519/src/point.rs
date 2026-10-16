//! Extended Edwards coordinates for RFC 8032 §5.1.3–5.1.4.
//! Enforces canonical compressed encodings, including the x=0 sign rule.
//! Uses field arithmetic only; the verifier consumes public-point operations.

use crate::field::Field;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Point {
    x: Field,
    y: Field,
    z: Field,
    t: Field,
}

impl Point {
    const IDENTITY: Self = Self {
        x: Field::ZERO,
        y: Field::ONE,
        z: Field::ONE,
        t: Field::ZERO,
    };

    pub(crate) fn base() -> Self {
        Self {
            x: Field::BASE_X,
            y: Field::BASE_Y,
            z: Field::ONE,
            t: Field::BASE_X.mul(Field::BASE_Y),
        }
    }

    pub(crate) fn decode(encoded: &[u8; 32]) -> Option<Self> {
        let sign = encoded[31] >> 7 != 0;
        let mut y_bytes = *encoded;
        y_bytes[31] &= 0x7f;
        let y = Field::decode(&y_bytes)?;
        let y_squared = y.square();
        let numerator = y_squared.sub(Field::ONE);
        let denominator = Field::D.mul(y_squared).add(Field::ONE);
        if denominator == Field::ZERO {
            return None;
        }
        let mut x = numerator.mul(denominator.invert()).sqrt()?;
        if x == Field::ZERO && sign {
            return None;
        }
        if x.is_odd() != sign {
            x = x.negate();
        }
        Some(Self {
            x,
            y,
            z: Field::ONE,
            t: x.mul(y),
        })
    }

    pub(crate) fn add(self, rhs: Self) -> Self {
        let a = self.y.sub(self.x).mul(rhs.y.sub(rhs.x));
        let b = self.y.add(self.x).mul(rhs.y.add(rhs.x));
        let c = self.t.mul(rhs.t).mul(Field::D.add(Field::D));
        let zz = self.z.mul(rhs.z);
        let d = zz.add(zz);
        let e = b.sub(a);
        let f = d.sub(c);
        let g = d.add(c);
        let h = b.add(a);
        Self {
            x: e.mul(f),
            y: g.mul(h),
            z: f.mul(g),
            t: e.mul(h),
        }
    }

    fn double(self) -> Self {
        let a = self.x.square();
        let b = self.y.square();
        let zz = self.z.square();
        let c = zz.add(zz);
        let d = a.negate();
        let e = self.x.add(self.y).square().sub(a).sub(b);
        let g = d.add(b);
        let f = g.sub(c);
        let h = d.sub(b);
        Self {
            x: e.mul(f),
            y: g.mul(h),
            z: f.mul(g),
            t: e.mul(h),
        }
    }

    pub(crate) fn mul(self, scalar: &[u8; 32]) -> Self {
        let mut result = Self::IDENTITY;
        // Variable-time branches are on public signature/challenge bits only.
        for bit in (0..256).rev() {
            result = result.double();
            if (scalar[bit / 8] >> (bit % 8)) & 1 != 0 {
                result = result.add(self);
            }
        }
        result
    }

    pub(crate) fn equals(self, rhs: Self) -> bool {
        self.x.mul(rhs.z) == rhs.x.mul(self.z) && self.y.mul(rhs.z) == rhs.y.mul(self.z)
    }

    pub(crate) fn is_small_order(self) -> bool {
        self.double().double().double().equals(Self::IDENTITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scalar::ORDER;
    use proptest::prelude::*;

    fn on_curve(point: Point) -> bool {
        point.z != Field::ZERO
            && point.x.mul(point.y) == point.z.mul(point.t)
            && point.y.square().sub(point.x.square())
                == point.z.square().add(Field::D.mul(point.t.square()))
    }

    #[test]
    fn base_point_and_complete_addition() {
        let mut encoded = [0x66; 32];
        encoded[0] = 0x58;
        let base = Point::decode(&encoded).unwrap();
        assert!(base.equals(Point::base()));
        assert!(!base.is_small_order());
        assert!(base.mul(&ORDER).equals(Point::IDENTITY));
        assert!(base.mul(&[0; 32]).equals(Point::IDENTITY));
        assert!(on_curve(base));
        assert!(base.add(Point::IDENTITY).equals(base));
        assert!(base.add(base).equals(base.double()));
        encoded[31] |= 0x80;
        let negative = Point::decode(&encoded).unwrap();
        assert!(!negative.equals(base));
        assert!(base.add(negative).equals(Point::IDENTITY));
    }

    #[test]
    fn canonical_encoding_and_small_order() {
        let mut identity = [0; 32];
        identity[0] = 1;
        assert!(Point::decode(&identity).unwrap().is_small_order());
        identity[31] = 0x80;
        assert!(Point::decode(&identity).is_none());
        let mut noncanonical = [0xff; 32];
        noncanonical[0] = 0xed;
        noncanonical[31] = 0x7f;
        assert!(Point::decode(&noncanonical).is_none());
        let mut nonsquare = [0; 32];
        nonsquare[0] = 2;
        assert!(Point::decode(&nonsquare).is_none());
        assert!(Point::decode(&[0; 32]).unwrap().is_small_order());
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]
        #[test]
        fn extended_coordinate_invariants(encoded in any::<[u8; 32]>()) {
            if let Some(point) = Point::decode(&encoded) {
                let sum = point.add(Point::base());
                let doubled = point.double();
                prop_assert!(on_curve(point));
                prop_assert!(on_curve(sum));
                prop_assert!(on_curve(doubled));
                prop_assert!(point.add(point).equals(doubled));
                prop_assert!(sum.equals(Point::base().add(point)));
            }
        }
    }
}
