// SPDX-License-Identifier: Apache-2.0
//! Native drawing segment-token wire fields.

use cadmpeg_ir::ids::IdentityKey;
use serde::{Deserialize, Deserializer, Serializer};

pub(super) fn serialize<S: Serializer>(
    value: &IdentityKey,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(value.as_str())
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<IdentityKey, D::Error> {
    let value = String::deserialize(deserializer)?;
    IdentityKey::try_new(value).map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::ids::IdentityKey;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct Token {
        #[serde(with = "super")]
        value: IdentityKey,
    }

    #[test]
    fn token_wire_keeps_the_key_and_rejects_blank_or_non_string_values(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let token = Token {
            value: cadmpeg_ir::identity_key!("generated"),
        };
        let value = serde_json::to_value(&token)?;
        assert_eq!(value, serde_json::json!({"value": "generated"}));
        let decoded: Token = serde_json::from_value(value)?;
        assert_eq!(decoded.value.as_str(), "generated");
        for value in [
            serde_json::json!(""),
            serde_json::json!(7),
            serde_json::json!([]),
        ] {
            assert!(serde_json::from_value::<Token>(serde_json::json!({"value": value})).is_err());
        }
        Ok(())
    }
}
