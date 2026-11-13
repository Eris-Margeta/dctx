struct Bytes;
impl Iterator for Bytes {
    type Item = u8;
    fn next(&mut self) -> Option<u8> { None }
}
impl Bytes {
    const WIDTH: usize = 8;
}
