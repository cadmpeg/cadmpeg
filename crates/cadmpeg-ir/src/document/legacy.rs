// SPDX-License-Identifier: Apache-2.0
//! Refusal diagnostics for unsupported CADIR wires.

use super::IR_VERSION;

pub(super) fn unsupported_version(version: Option<&str>) -> String {
    format!(
        "unsupported ir_version {version:?}; expected {IR_VERSION}; wire v8 replaces decode_depth with scope and changes decode report transfer to delivered scope"
    )
}

/// Admit the version before decoding fields whose shape changed between wires.
/// Only fields before `ir_version` are buffered; current writers put it first.
pub(super) fn deserialize_wire<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<super::CadIrReadWire, D::Error> {
    struct VersionedDocument;
    impl<'de> serde::de::Visitor<'de> for VersionedDocument {
        type Value = super::CadIrReadWire;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a versioned CADIR document")
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            use serde::Deserialize;
            let mut prefix = Vec::new();
            while let Some(key) = map.next_key::<String>()? {
                let value = map.next_value::<serde_json::Value>()?;
                let version_found = key == "ir_version";
                if version_found {
                    super::check_ir_version(Some(&value))?;
                }
                prefix.push((key, value));
                if version_found {
                    return super::CadIrReadWire::deserialize(
                        serde::de::value::MapAccessDeserializer::new(VersionedFields {
                            prefix: prefix.into_iter(),
                            pending: None,
                            remaining: map,
                        }),
                    );
                }
            }
            Err(serde::de::Error::custom(unsupported_version(None)))
        }
    }
    deserializer.deserialize_map(VersionedDocument)
}

struct VersionedFields<A> {
    prefix: std::vec::IntoIter<(String, serde_json::Value)>,
    pending: Option<serde_json::Value>,
    remaining: A,
}

impl<'de, A: serde::de::MapAccess<'de>> serde::de::MapAccess<'de> for VersionedFields<A> {
    type Error = A::Error;

    fn next_key_seed<K: serde::de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        use serde::de::IntoDeserializer;
        if let Some((key, value)) = self.prefix.next() {
            self.pending = Some(value);
            seed.deserialize(key.into_deserializer()).map(Some)
        } else {
            self.remaining.next_key_seed(seed)
        }
    }

    fn next_value_seed<V: serde::de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Self::Error> {
        if let Some(value) = self.pending.take() {
            seed.deserialize(value).map_err(serde::de::Error::custom)
        } else {
            self.remaining.next_value_seed(seed)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::super::CadIr;

    #[test]
    fn wire_v6_refusal_names_scope_and_report_changes() {
        let mut value = serde_json::to_value(CadIr::empty()).unwrap();
        value["ir_version"] = serde_json::json!("6");
        let error = CadIr::from_json(&serde_json::to_string(&value).unwrap()).unwrap_err();
        for message in ["expected 8", "decode_depth", "scope", "report"] {
            assert!(error.to_string().contains(message), "{error}");
        }
        // Older writers state the wire version before their depth field.
        let old = r#"{"ir_version":"6","decode_depth":"full"}"#;
        let error = serde_json::from_str::<CadIr>(old).unwrap_err();
        for message in ["expected 8", "decode_depth", "scope", "report"] {
            assert!(error.to_string().contains(message), "{error}");
        }
    }

    fn rejected_legacy_shape(version: &str, depth: Option<&str>) {
        let mut value = serde_json::to_value(CadIr::empty()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.insert("ir_version".into(), serde_json::json!(version));
        object.remove("scope");
        if let Some(depth) = depth {
            object.insert("decode_depth".into(), serde_json::json!(depth));
        }
        let json = serde_json::to_string(&value).unwrap();
        for error in [
            CadIr::from_json(&json).unwrap_err(),
            serde_json::from_str::<CadIr>(&json).unwrap_err(),
        ] {
            for message in ["expected 8", "decode_depth", "scope", "report"] {
                assert!(error.to_string().contains(message), "{error}");
            }
        }
    }

    #[test]
    fn wire_v061_refusal_names_scope_and_report_changes() {
        rejected_legacy_shape("6", None);
    }

    #[test]
    fn wire_v062_refusal_names_scope_and_report_changes() {
        rejected_legacy_shape("6", Some("full"));
    }

    #[test]
    fn wire_main_v7_refusal_names_scope_and_report_changes() {
        rejected_legacy_shape("7", Some("full"));
    }
}
