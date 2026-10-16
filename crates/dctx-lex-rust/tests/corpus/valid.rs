//! Snippet corpus for the syn/tree-sitter differential test.
//! Exercises generics, lifetimes, nested scopes, raw strings, Unicode and macros.
//! Not compiled; read with include_str! by tests/differential.rs.
#![allow(dead_code)]

use std::collections::{BTreeMap as Map, HashSet};
use std::fmt::{self, Display};
pub use self::inner::Deep;

/// Raw strings and comments must not declare: "fn fake() {}".
pub const RAW: &str = r#"fn not_real() { struct Nope; }"#;
// fn commented() {}
/* struct Hidden; */

pub(crate) static mut COUNTER: u32 = 0;

pub struct Pair<'a, T: Display> {
    left: &'a T,
    right: Option<Box<T>>,
}

impl<'a, T: Display> Pair<'a, T> {
    /// Builds a pair.
    pub fn new(left: &'a T) -> Self {
        Self { left, right: None }
    }

    fn render(&self) -> String {
        let text = format!("{}", self.left);
        text.trim().to_owned()
    }
}

impl<T: Display> fmt::Display for Pair<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.left)
    }
}

pub enum Shape {
    Circle { radius: f64 },
    Square(f64),
}

pub trait Area {
    const SIDES: u32;
    type Unit;
    fn area(&self) -> f64;
    fn double(&self) -> f64 {
        self.area() * 2.0
    }
}

impl Area for Shape {
    const SIDES: u32 = 0;
    type Unit = f64;
    fn area(&self) -> f64 {
        match self {
            Shape::Circle { radius } => radius * radius * 3.0,
            Shape::Square(side) => side.powi(2),
        }
    }
}

pub mod inner {
    pub struct Deep;

    pub fn outer() -> u32 {
        fn nested() -> u32 {
            7
        }
        nested() + helpers::value()
    }

    mod helpers {
        pub fn value() -> u32 {
            super::super::compute(1)
        }
    }
}

pub fn compute(x: u32) -> u32 {
    let items: Vec<u32> = (0..x).map(|v| v + 1).collect::<Vec<_>>();
    let set: HashSet<u32> = HashSet::new();
    items.len() as u32 + set.len() as u32 + inner::outer()
}

pub async fn fetch() -> Option<u8> {
    None
}

pub unsafe fn raw_ptr(p: *const u8) -> u8 {
    unsafe { *p }
}

extern "C" {
    fn abs(input: i32) -> i32;
}

pub union Bits {
    int: u32,
    float: f32,
}

type Callback = fn(u32) -> u32;

macro_rules! square {
    ($x:expr) => {
        $x * $x
    };
}

pub fn r#match() -> u32 {
    square!(compute(2))
}

/// Unicode stays inside literals: “quoted” — ok.
pub fn unicode_text() -> &'static str {
    "héllo — wörld"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes() {
        assert_eq!(compute(0), 7);
    }

    #[tokio::test]
    async fn fetches() {
        fetch().await;
    }
}
