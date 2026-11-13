macro_rules! register {
    ($name:ident) => { fn phantom_macro() {} };
}
macro_rules! count ( () => { 0 }; );
register!(ignored);
