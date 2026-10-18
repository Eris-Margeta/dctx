// Swift adapter fixture for SPEC-MAP-MODE sections 5 and 6.2.
// Used by lexers_swift.rs to check structural facts and store ownership.
import Foundation

/// Runs a job. Extra detail.
public struct Box {
    public func run() { helper() }
}

func helper() {}
