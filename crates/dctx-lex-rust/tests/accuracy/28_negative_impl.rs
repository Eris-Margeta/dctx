struct LocalOnly;
impl !Send for LocalOnly {}
unsafe trait Marker {}
unsafe impl Marker for LocalOnly {}
