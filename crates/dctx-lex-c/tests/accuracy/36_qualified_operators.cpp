struct Number {
    Number operator+(Number other) const;
    operator bool() const;
};
Number Number::operator+(Number other) const { return other; }
Number::operator bool() const { return true; }
