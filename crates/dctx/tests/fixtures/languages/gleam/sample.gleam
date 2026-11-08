pub type Boxed(a) {
  Boxed(value: a)
}
pub fn copy(value: a) -> a {
  let nested = fn(item) { item }
  nested(value)
}
