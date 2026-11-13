unsafe extern "C" {
    pub fn acquire(size: usize) -> *mut u8;
    pub static mut COUNTER: usize;
    pub type Handle;
}
