fn outer() {
    struct Local;
    impl Local {
        fn run(&self) { fn helper() {} }
    }
    const LIMIT: usize = 1;
    type Count = usize;
}
