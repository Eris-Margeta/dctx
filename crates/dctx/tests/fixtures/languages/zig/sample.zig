const Boxed = struct {
    value: i32,
    pub fn copy(self: Boxed) i32 {
        return self.value;
    }
};
pub fn identity(comptime T: type, value: T) T {
    return value;
}
pub fn @"café"() i32 { return 1; }
