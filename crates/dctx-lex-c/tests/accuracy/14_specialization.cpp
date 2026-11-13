template<class T> struct Codec { T encode(T value); };
template<> struct Codec<int> { int encode(int value); };
template<class T> struct Codec<T*> { T *encode(T *value); };
