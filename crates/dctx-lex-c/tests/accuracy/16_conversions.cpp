class Handle {
public:
    explicit operator bool() const noexcept;
    operator const char *() const;
};
