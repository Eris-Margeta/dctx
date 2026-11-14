[[nodiscard]] int acquire();
struct [[nodiscard]] Result {
    [[nodiscard]] int value() const;
    int error;
};
