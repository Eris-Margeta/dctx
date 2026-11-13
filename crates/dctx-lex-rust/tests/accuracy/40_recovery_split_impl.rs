struct Queue<T>(T);
impl<T>
    Queue<T>
where
    T: Clone,
{
    fn pop(&mut self) {}
}
@
