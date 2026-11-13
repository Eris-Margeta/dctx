struct Hooks {
    int (*read)(void *context, unsigned size);
    void (*release)(void *context);
};
