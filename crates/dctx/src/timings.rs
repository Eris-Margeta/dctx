// SPDX-License-Identifier: Apache-2.0
//! Opt-in, thread-local phase counters for the command-line host.
//! The host checks its environment once and explicitly enables these hooks.
//! Disabled hooks allocate nothing and never read a clock.

use std::cell::Cell;
use std::time::Instant;

/// Exclusive wall-clock phases; nested spans pause their parent.
#[derive(Clone, Copy)]
pub enum Phase {
    /// Argument, configuration and root/session setup, plus unclassified main time.
    ConfigAndRoot,
    /// Inventory, file verification, retained-object I/O and snapshot construction.
    GenerationOpen,
    /// Retained-generation decoding (including an optional in-process cache lookup).
    Decode,
    /// Query selection and answer assembly, excluding nested rendering.
    Query,
    /// Answer encoding and CLI page preparation.
    Render,
    /// CLI stdout write and flush.
    Write,
}

const NAMES: [&str; 6] = [
    "config_and_root",
    "generation_open",
    "decode",
    "query",
    "render",
    "write",
];

#[derive(Clone, Copy)]
struct Counters {
    nanos: [u128; 6],
    phase: Phase,
    since: Instant,
}

impl Counters {
    fn switch(&mut self, phase: Phase) {
        let now = Instant::now();
        self.nanos[self.phase as usize] += now.duration_since(self.since).as_nanos();
        self.since = now;
        self.phase = phase;
    }
}

thread_local! {
    static DATA: Cell<Option<Counters>> = const { Cell::new(None) };
}

/// Starts fresh counters on this thread. Only the CLI host opts in.
pub fn enable() {
    DATA.set(Some(Counters {
        nanos: [0; 6],
        phase: Phase::ConfigAndRoot,
        since: Instant::now(),
    }));
}

/// Takes and disables the counters, returning integer microseconds per phase.
/// No process-start timestamp is available before Rust enters main.
#[must_use]
pub fn take() -> Option<[(&'static str, u128); 6]> {
    DATA.take().map(|mut counters| {
        counters.switch(counters.phase);
        std::array::from_fn(|index| (NAMES[index], counters.nanos[index] / 1_000))
    })
}

/// Restores the previous phase on drop, including early returns.
pub struct Span {
    previous: Option<Phase>,
}

impl Span {
    /// Enters a phase when enabled; otherwise performs no allocation or clock read.
    #[must_use]
    pub fn begin(phase: Phase) -> Self {
        let previous = DATA.get().map(|mut counters| {
            let previous = counters.phase;
            counters.switch(phase);
            DATA.set(Some(counters));
            previous
        });
        Self { previous }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(previous) = self.previous
            && let Some(mut counters) = DATA.get()
        {
            counters.switch(previous);
            DATA.set(Some(counters));
        }
    }
}
