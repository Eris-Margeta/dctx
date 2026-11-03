// SPDX-License-Identifier: Apache-2.0
//! Typed domain records for the frozen plugin API v1 wire contract (no IO).
//! Implements common/capabilities/bootstrap/requests/invocation/results/events schemas and wire.schema.json.
//! Depends on json and refusal; used by negotiate, session and external host/worker crates.
//! Every record decodes from a closed object and encodes back to its exact schema shape.

/// Declares a closed string enumeration with its wire spellings.
macro_rules! wire_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            /// Every value in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// Wire spelling.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            /// Parses an exact wire spelling.
            pub fn parse(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }

            pub(crate) fn decode(value: &$crate::json::value::Json) -> Result<Self, $crate::refusal::Refusal> {
                value
                    .as_str()
                    .and_then(Self::parse)
                    .ok_or($crate::refusal::Refusal::schema("unknown-enum-value"))
            }

            pub(crate) fn encode(self) -> $crate::json::value::Json {
                $crate::json::value::Json::str(self.as_str())
            }
        }
    };
}

pub mod bootstrap;
pub mod broker;
pub mod bytes;
pub mod capability;
pub mod common;
pub mod envelope;
pub mod events;
pub mod failure;
pub mod invocation;
pub mod method;
pub mod requests;
pub mod results;
pub mod scalars;
