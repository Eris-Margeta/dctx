open module sample.runtime {
    requires java.logging;
    exports sample.api;
    opens sample.model;
    uses java.lang.Runnable;
    provides java.lang.Runnable with sample.Runner;
}
