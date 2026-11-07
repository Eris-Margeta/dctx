/// A generic container.
#[derive(Clone)]
pub struct Boxed<T> { pub value: T }
impl<T: Clone> Boxed<T> {
    pub fn copy(&self) -> T {
        fn nested(value: i32) -> i32 { value }
        let _ = nested(1);
        self.value.clone()
    }
}
macro_rules! identity { ($value:expr) => { $value }; }
pub fn café() -> i32 { identity!(1) }
