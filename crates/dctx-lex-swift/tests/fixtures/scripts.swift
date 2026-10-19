#!/usr/bin/env swift
// Script syntax regression corpus; SPEC-MAP-MODE symbol extraction.
// Depends on Swift syntax only; used by dctx_lex_swift integration tests.
// Keep the hashbang first so script-comment handling is exercised.
import Foundation

@objc(Plugin)
public final class Plugin {
    @Wrapper(name: "title")
    public var title: String = ""

    @resultBuilder
    struct Rows {}

    public func render() async -> some Rows {
        guard let name = optionalName() else { return Rows() }
        let note = """
        hello \(name)
        """
        if #available(iOS 14, *) {
            show(animated: true) { banner(name) }
        } else {
            showLegacy(name)
        }
        if #unavailable(macOS 11) {
            fallback()
        }
        _ = note
        await finish(label: name) { done in
            report(done)
        }
    }
}

public actor Session {
    func load() async -> any Sendable { await fetch() }
}

print("boot")
func kept() {}
for item in items { use(item) }
