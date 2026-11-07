// SPDX-License-Identifier: Apache-2.0
//! Public distribution decoders; inputs stay in memory and never install or run plugins.
use dctx_plugin_distribution::{
    domain::registry_codec, KeyId, Manifest, OfflineBundle, PluginId, SafeFileName, Target, Version,
};
use proptest::prelude::*;

proptest! {
    #[test]
    fn robustness_distribution_decoders(
        bytes in prop_oneof![
            prop::collection::vec(any::<u8>(), 0..4096),
            prop::collection::vec(any::<char>(), 0..512)
                .prop_map(|chars| chars.into_iter().collect::<String>().into_bytes()),
        ],
    ) {
        let _ = Manifest::parse(&bytes);
        let _ = OfflineBundle::parse(&bytes, 8192);
        if let Ok(registry) = registry_codec::decode(&bytes) {
            prop_assert!(registry_codec::decode(&registry_codec::encode(&registry)).is_ok());
        }
        if let Ok(text) = std::str::from_utf8(&bytes) {
            let _ = PluginId::parse(text);
            let _ = KeyId::parse(text);
            let _ = SafeFileName::parse(text);
            let _ = Version::parse(text);
            let _ = Target::parse(text);
        }
    }
}
