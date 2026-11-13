template<class T>
class Buffer {
public:
    T *data;
    T get(unsigned index) const;
};
