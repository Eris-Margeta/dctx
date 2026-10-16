//! Fixture crate for exact parser facts.
//! Exercises SPEC-MAP-MODE sections 5.1 and 5.3.
//! Uses its support module; consumed by parser integration tests.
#![forbid(unsafe_code)]

mod support;
pub use support::{helper as assist, Value};
use support::helper;

/// A public widget. Further description.
pub struct Widget;

pub enum Mode {
    Ready,
    Busy,
}

pub trait Work {
    fn run(&self) -> u32;
}

impl Work for Widget {
    fn run(&self) -> u32 {
        helper()
    }
}

impl Widget {
    pub fn value(&self) -> u32 {
        self.run()
    }
}

pub const LIMIT: u32 = 3;
pub type Alias = Widget;

macro_rules! local_value {
    () => { 3 };
}

#[cfg(test)]
mod tests {
    use super::Widget;

    #[test]
    fn checks() {
        let value = Widget;
        value.value();
    }
}
