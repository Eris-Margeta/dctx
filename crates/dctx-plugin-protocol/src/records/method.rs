//! Session method registry: the closed v1 method set, its sender and whether it is a broker effect.
//! Implements the method table of plugin-api wire.md §Bootstrap and lifecycle.
//! Depends on scalars::Sender; used by requests, results, session and negotiation.
//! Unknown methods (for example any network method) do not parse and are refused.

use crate::records::scalars::Sender;

wire_enum!(
    /// Session request methods (events are separate).
    Method {
        /// Host: bind the session and supply grants.
        Initialize => "initialize",
        /// Host: start one hook or command invocation.
        Invoke => "invoke",
        /// Host: responsiveness challenge.
        Probe => "probe",
        /// Host: cancel the active invocation.
        Cancel => "cancel",
        /// Host: end the session.
        Shutdown => "shutdown",
        /// Plugin: read an authorized output chunk.
        OutputRead => "output.read",
        /// Plugin: page output metadata.
        OutputList => "output.list",
        /// Plugin: read a granted file chunk.
        FileRead => "file.read",
        /// Plugin: stage upload bytes.
        FileStage => "file.stage",
        /// Plugin: atomically replace a file from a sealed upload.
        FileReplace => "file.replace",
        /// Plugin: run the constrained Git recipe.
        ProgramRun => "program.run",
        /// Plugin: read a state chunk.
        StateRead => "state.read",
        /// Plugin: replace a state object.
        StateWrite => "state.write",
        /// Plugin: reclaim a state object.
        StateReclaim => "state.reclaim",
    }
);

impl Method {
    /// The only side allowed to send this request.
    pub const fn sender(self) -> Sender {
        match self {
            Self::Initialize | Self::Invoke | Self::Probe | Self::Cancel | Self::Shutdown => Sender::Host,
            _ => Sender::Plugin,
        }
    }

    /// Whether this is a plugin broker request bound to the active invocation.
    pub const fn is_broker(self) -> bool {
        matches!(self.sender(), Sender::Plugin)
    }

    /// Whether the request carries an `action_id` replay key (S-REPLAY).
    pub const fn has_action_id(self) -> bool {
        matches!(self, Self::FileReplace | Self::ProgramRun | Self::StateWrite | Self::StateReclaim)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_table_matches_wire_md() {
        assert_eq!(Method::ALL.len(), 14);
        let host: Vec<&str> = Method::ALL.iter().filter(|m| m.sender() == Sender::Host).map(|m| m.as_str()).collect();
        assert_eq!(host, ["initialize", "invoke", "probe", "cancel", "shutdown"]);
        assert!(Method::FileRead.is_broker());
        assert!(!Method::Probe.is_broker());
        assert!(Method::parse("network.fetch").is_none());
        assert!(Method::parse("hello").is_none());
        assert!(Method::StateWrite.has_action_id() && !Method::FileStage.has_action_id());
    }
}
