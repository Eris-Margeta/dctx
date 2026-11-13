struct Ring<T, const N: usize>([T; N]);
impl<T, const N: usize> Ring<T, N> where T: Copy {
    const SIZE: usize = N;
    fn len(&self) -> usize { N }
}
