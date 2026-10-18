import Foundation
import struct Other.Value

/// Stores a value. Extra detail.
@main
public struct Box<T>: Sendable where T: Sendable {
    public private(set) var value: T
    private let count = 1
    public init(_ value: T) { self.value = value }
    public func map<U>(_ transform: (T) -> U) -> U where U: Sendable {
        let closure = { (x: T) in transform(x) }
        return closure(value)
    }
    public subscript(index: Int) -> T { get { value } }
}

/** Available choices. More detail. */
public enum Choice {
    case empty, value(Int)
    indirect case branch(Choice)
}

@available(macOS 13, *)
public protocol Service {
    func load() async throws -> String
    var ready: Bool { get }
}
public actor Worker { func run() { client?.send(); client.status } }
open class Parent { public func run() {} }
extension Box where T == String { func upper() -> String { value.uppercased() } }
typealias Name = String
infix operator <+>: AdditionPrecedence
func <+>(lhs: Int, rhs: Int) -> Int { lhs + rhs }
#if DEBUG
func debugOnly() { log() }
#else
func releaseOnly() { log() }
#endif
