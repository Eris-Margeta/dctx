pub enum Message<T> {
    Empty,
    Data(T),
    Error { code: u32, detail: String },
}
