#[repr(C)]
pub union Register {
    pub word: u32,
    pub bytes: [u8; 4],
}
