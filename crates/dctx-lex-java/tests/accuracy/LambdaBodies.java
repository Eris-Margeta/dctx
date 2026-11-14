class Pipeline {
    java.util.function.Supplier<String> label = () -> "class PhantomLambda {}";
    Runnable task = () -> {
        class Local { void execute() {} }
        new Local().execute();
    };
    void run() { task.run(); }
}
