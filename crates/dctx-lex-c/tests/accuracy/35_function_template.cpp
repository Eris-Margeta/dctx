template<class T>
[[nodiscard]] T identity(T value) { return value; }
template<>
int identity<int>(int value) { return value; }
