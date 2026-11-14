import java.lang.annotation.*;
@Retention(RetentionPolicy.RUNTIME)
@interface Route {
    String value();
    String[] verbs() default {"GET", "HEAD"};
    Class<?> handler() default Object.class;
}
