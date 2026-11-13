struct Counter {
    Counter operator+(const Counter &rhs) const;
    bool operator==(const Counter &rhs) const;
    int operator()(int step);
    int &operator[](unsigned index);
    Counter &operator++();
};
