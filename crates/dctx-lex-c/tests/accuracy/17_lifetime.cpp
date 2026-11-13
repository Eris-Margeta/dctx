struct Session {
    Session();
    explicit Session(int fd);
    ~Session();
    Session(const Session&) = delete;
};
