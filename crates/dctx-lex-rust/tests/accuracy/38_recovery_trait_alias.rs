trait Transfer = Send + Sync;
pub trait Borrowable<'a> = AsRef<&'a str>;
@
