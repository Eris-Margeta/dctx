#define IDENTITY(value) (value)
template<typename T>
class Boxed {
public:
    T copy(T value) {
        auto nested = [](T item) { return item; };
        return nested(value);
    }
};
[[nodiscard]] int café() { return IDENTITY(1); }
