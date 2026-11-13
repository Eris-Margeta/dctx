#[cfg(feature = "client")]
pub fn connect() {}
#[cfg(not(feature = "client"))]
pub fn connect() {}
#[cfg(any())]
mod disabled {
    pub struct Hidden;
}
