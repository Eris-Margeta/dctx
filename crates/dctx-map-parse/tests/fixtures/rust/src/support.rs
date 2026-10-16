//! Support declarations for the parser fixture crate.
//! Exercises source-file modules and type aliases in spec section 5.1.
//! No external dependencies; imported by the fixture root.
#![forbid(unsafe_code)]

pub type Value = u32;

pub fn helper() -> u32 {
    3
}
