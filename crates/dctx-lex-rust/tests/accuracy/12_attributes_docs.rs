//! ```rust
//! fn phantom_inner_doc() {}
//! ```
/// ```rust
/// fn phantom_doc() {}
/// ```
#[doc = "fn phantom_attribute() {}"]
#[allow(dead_code)]
pub fn documented() {}
