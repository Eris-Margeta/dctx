/// A generic container.
@available(*, deprecated)
public class Boxed<T> {
    public func copy<U>(_ value: U) -> U {
        func nested(_ item: U) -> U { return item }
        return nested(value)
    }
    public func café() -> Int { return 1 }
}
