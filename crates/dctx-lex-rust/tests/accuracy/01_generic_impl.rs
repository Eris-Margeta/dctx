pub struct Cache<K, V>(K, V);
impl<K, V> Cache<K, V>
where K: Eq, V: Clone,
{
    pub fn lookup(&self) -> &V { &self.1 }
    pub fn clear(&mut self) {}
}
