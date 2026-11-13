struct Wrapper<T>(T);
impl<T> AsRef<T> for Wrapper<T>
where T: Send,
{
    fn as_ref(&self) -> &T { &self.0 }
}
