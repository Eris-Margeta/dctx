//! Shared signed-fixture builder and fake world for the PL-I integration tests.
//! Test keys are generated here from fixed seeds with the dev-only ed25519-dalek signer; they are
//! conspicuously nonproduction and never leave the test process. Verification is always dctx-ed25519.
//! Used by every file in tests/.

#![allow(dead_code)]

use dctx_plugin_distribution::domain::signature::{INDEX_DOMAIN, MANIFEST_DOMAIN};
use dctx_plugin_distribution::fakes::{FakeNetwork, FixedClock, MemoryStore};
use dctx_plugin_distribution::{
    Distribution, InstallPolicy, KeyId, OfflineBundle, PluginId, Sha256Hex, Target, TrustStore, fingerprint,
};
use ed25519_dalek::{Signer, SigningKey};

pub const NOW: u64 = 1_000_000;
pub const OFFICIAL: &str = "official-1";
pub const GIT: &str = "dctx/git";
pub const BASE: &str = "https://releases.example.org/dctx-git";
pub const NARROW: &str = r#"{"required":[{"kind":"file.read","scope":"project","path":".gitignore","max_bytes":1048576}],"optional":[],"outputs":[],"network":[]}"#;
pub const WIDE: &str = r#"{"required":[{"kind":"file.read","scope":"project","path":".gitignore","max_bytes":1048576}],"optional":[{"kind":"program.run","program":"git","operation":"git.add-ignore","resources":["git-admin:stage-ignore"],"max_output_bytes":65536}],"outputs":[],"network":[]}"#;

/// Deterministic test key; seed 0 is never used so keys stay distinct from zero material.
pub fn test_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

pub fn public(key: &SigningKey) -> [u8; 32] {
    key.verifying_key().to_bytes()
}

pub fn sign(domain: &[u8], document: &[u8], key: &SigningKey) -> Vec<u8> {
    let mut message = domain.to_vec();
    message.extend_from_slice(document);
    key.sign(&message).to_bytes().to_vec()
}

pub fn id(text: &str) -> PluginId {
    PluginId::parse(text).unwrap()
}

/// A release under construction; `manifest()` renders exact bytes.
#[derive(Clone, Debug)]
pub struct Release {
    pub id: String,
    pub version: String,
    pub sequence: u64,
    pub key_id: String,
    pub capabilities: String,
    pub artifacts: Vec<(String, String, Vec<u8>)>,
    pub resources: Vec<(String, Vec<u8>)>,
}

impl Release {
    pub fn git(version: &str, sequence: u64) -> Self {
        let executable = format!("#!/bin/sh\necho installed-code-ran-{version} > /tmp/dctx-pl-i-marker\n").into_bytes();
        Self {
            id: GIT.into(),
            version: version.into(),
            sequence,
            key_id: OFFICIAL.into(),
            capabilities: NARROW.into(),
            artifacts: vec![("aarch64-apple-darwin".into(), format!("dctx-plugin-git-{version}-aarch64-apple-darwin"), executable)],
            resources: Vec::new(),
        }
    }

    pub fn executable(&self) -> &[u8] {
        &self.artifacts[0].2
    }

    pub fn digest(&self) -> Sha256Hex {
        Sha256Hex::of(self.executable()).unwrap()
    }

    pub fn manifest(&self) -> Vec<u8> {
        let payload = |file: &str, bytes: &[u8]| {
            format!("\"file\":\"{file}\",\"bytes\":{},\"sha256\":\"{}\"", bytes.len(), Sha256Hex::of(bytes).unwrap())
        };
        let resources: Vec<String> = self.resources.iter().map(|(file, bytes)| format!("{{{}}}", payload(file, bytes))).collect();
        let artifacts: Vec<String> = self
            .artifacts
            .iter()
            .enumerate()
            .map(|(index, (target, file, bytes))| {
                let declared = if index == 0 { resources.join(",") } else { String::new() };
                format!(
                    "{{\"target\":\"{target}\",{},\"format\":\"native-executable\",\"min_os\":\"14.0\",\"runtime_resources\":[{declared}]}}",
                    payload(file, bytes)
                )
            })
            .collect();
        format!(
            concat!(
                "{{\"manifest_version\":1,\"id\":\"{}\",\"name\":\"dctx-plugin-git\",\"version\":\"{}\",",
                "\"release_sequence\":{},\"publisher_key\":\"{}\",\"license\":\"Apache-2.0\",",
                "\"protocol\":[{{\"major\":1,\"min_minor\":0,\"max_minor\":0}}],\"host_api\":{{\"major\":1,\"min_minor\":0}},",
                "\"hooks\":[],\"commands\":[],\"capabilities\":{},\"config\":{{}},\"limits\":{{}},",
                "\"reason_codes\":[],\"diagnostic_codes\":[],\"artifacts\":[{}]}}"
            ),
            self.id, self.version, self.sequence, self.key_id, self.capabilities, artifacts.join(",")
        )
        .into_bytes()
    }

    pub fn signature(&self, key: &SigningKey) -> Vec<u8> {
        sign(MANIFEST_DOMAIN, &self.manifest(), key)
    }

    pub fn url(&self) -> String {
        format!("{BASE}/{}/plugin.json", self.version)
    }

    /// Bundle entries: manifest, signature, every payload of the first artifact.
    pub fn bundle_with(&self, key: &SigningKey, extra: &[(&str, &[u8])]) -> Vec<u8> {
        let manifest = self.manifest();
        let signature = self.signature(key);
        let mut entries: Vec<(&str, &[u8])> = vec![("plugin.json", manifest.as_slice()), ("plugin.json.sig", signature.as_slice())];
        entries.push((self.artifacts[0].1.as_str(), self.artifacts[0].2.as_slice()));
        for (file, bytes) in &self.resources {
            entries.push((file.as_str(), bytes.as_slice()));
        }
        entries.extend_from_slice(extra);
        OfflineBundle::encode(&entries)
    }

    pub fn bundle(&self, key: &SigningKey) -> Vec<u8> {
        self.bundle_with(key, &[])
    }
}

/// Fake network, shared store, clock, trust and policy.
pub struct World {
    pub net: FakeNetwork,
    pub store: MemoryStore,
    pub clock: FixedClock,
    pub trust: TrustStore,
    pub policy: InstallPolicy,
    pub official: SigningKey,
}

impl World {
    pub fn new() -> Self {
        let official = test_key(1);
        let mut trust = TrustStore::new();
        let signer = trust.pin_official(KeyId::parse(OFFICIAL).unwrap(), public(&official)).unwrap();
        trust.pin_publisher(id(GIT), signer);
        trust.record_revocations([], [], NOW);
        Self {
            net: FakeNetwork::new(),
            store: MemoryStore::new(),
            clock: FixedClock::at(NOW),
            trust,
            policy: InstallPolicy::new(Target::AppleArm64),
            official,
        }
    }

    pub fn dist(&self) -> Distribution<'_> {
        Distribution::new(&self.net, &self.store, &self.clock, &self.trust, &self.policy)
    }

    /// Serves manifest, signature and payloads beside `release.url()`.
    pub fn publish(&self, release: &Release, key: &SigningKey) {
        let dir = format!("{BASE}/{}", release.version);
        self.net.serve(&release.url(), &release.manifest());
        self.net.serve(&format!("{dir}/plugin.json.sig"), &release.signature(key));
        for (_, file, bytes) in &release.artifacts {
            self.net.serve(&format!("{dir}/{file}"), bytes);
        }
        for (file, bytes) in &release.resources {
            self.net.serve(&format!("{dir}/{file}"), bytes);
        }
    }

    pub fn install(&self, release: &Release) -> Result<dctx_plugin_distribution::InstallReport, dctx_plugin_distribution::DistributionError> {
        let url = dctx_plugin_distribution::HttpsUrl::parse(&release.url()).unwrap();
        self.dist().install_url(&id(GIT), &url)
    }

    pub fn publish_and_install(&self, release: &Release) -> dctx_plugin_distribution::InstallReport {
        self.publish(release, &self.official);
        self.install(release).unwrap()
    }

    pub fn active(&self) -> Option<Sha256Hex> {
        self.dist().registry().unwrap().get(&id(GIT)).and_then(|record| record.active)
    }
}

/// Signed index bytes for `entries_json` under `key`.
pub fn signed_index(entries_json: &str, sequence: u64, key: &SigningKey) -> (Vec<u8>, Vec<u8>) {
    let bytes = format!("{{\"schema\":1,\"sequence\":{sequence},\"plugins\":[{entries_json}]}}").into_bytes();
    let signature = sign(INDEX_DOMAIN, &bytes, key);
    (bytes, signature)
}

pub fn index_entry(release: &Release, key: &SigningKey) -> String {
    format!(
        "{{\"id\":\"{}\",\"repository\":\"fixture/dctx-plugin-git\",\"summary\":\"Git protection\",\"publisher_key_fingerprint\":\"{}\",\"releases\":[{{\"version\":\"{}\",\"release_sequence\":{},\"manifest_url\":\"{}\",\"manifest_sha256\":\"{}\"}}]}}",
        release.id,
        fingerprint(&public(key)),
        release.version,
        release.sequence,
        release.url(),
        Sha256Hex::of(&release.manifest()).unwrap()
    )
}
