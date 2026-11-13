pub type Result<T> = core::result::Result<T, Error>;
type Callback = unsafe extern "C" fn(u32);
struct Error;
