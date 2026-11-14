class Startup {
    static { class Boot { void load() {} } new Boot().load(); }
    { class Hook { void attach() {} } new Hook().attach(); }
    static int version;
    Startup() {}
}
