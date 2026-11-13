struct Meter {
    Meter();
    ~Meter();
    int read() const;
};
Meter::Meter() {}
Meter::~Meter() {}
int Meter::read() const { return 1; }
