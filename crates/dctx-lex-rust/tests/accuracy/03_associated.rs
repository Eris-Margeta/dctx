pub trait Store<T> where T: Clone {
    type Key: Eq;
    const CAPACITY: usize;
    fn get(&self, key: &Self::Key) -> Option<T>;
    fn empty(&self) -> bool { true }
}
