trait Lending {
    type Item<'a> where Self: 'a;
    fn lend<'a>(&'a self) -> Self::Item<'a>;
}
