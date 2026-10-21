// SPDX-License-Identifier: Apache-2.0
//! Refusal diagnostics for unsupported CADIR wires.

use super::IR_VERSION;

mod prefix;

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
                let value = map.next_value_seed(prefix::PrefixValue)?;
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
    // SPDX-License-Identifier: Apache-2.0
    //! Version-prefix ordering and strict key admission.

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

    fn current_with_prefix(field: &str, raw: &str) -> String {
        let mut value = serde_json::to_value(CadIr::empty()).unwrap();
        value.as_object_mut().unwrap().remove(field);
        let remaining = serde_json::to_string(&value).unwrap();
        format!("{{{field:?}:{raw},{}", &remaining[1..])
    }

    #[test]
    fn current_version_after_model_and_native_rejects_nested_duplicate_keys() {
        for (field, raw, key) in [
            (
                "model",
                r#"{"attributes":[],"attributes":[]}"#,
                "attributes",
            ),
            (
                "native",
                r#"{"iges":{"entities":[],"entities":[]}}"#,
                "entities",
            ),
            (
                "model",
                r#"{"attributes":[{"name":"first","name":"second"}]}"#,
                "name",
            ),
        ] {
            let json = current_with_prefix(field, raw);
            assert!(json.find(field).unwrap() < json.find("ir_version").unwrap());
            let error = serde_json::from_str::<CadIr>(&json)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("duplicate") && error.contains(key),
                "{error}"
            );
        }
    }

    #[test]
    fn current_version_after_prefix_still_refuses_unknown_keys() {
        for (field, raw, unknown) in [
            ("unknown_prefix", "7", "unknown_prefix"),
            (
                "model",
                r#"{"unknown_model_prefix":false}"#,
                "unknown_model_prefix",
            ),
        ] {
            let error = serde_json::from_str::<CadIr>(&current_with_prefix(field, raw))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("unknown field") && error.contains(unknown),
                "{error}"
            );
        }
    }

    #[test]
    fn current_version_after_prefix_refuses_repeated_top_level_fields() {
        let scope =
            serde_json::to_string(&serde_json::to_value(CadIr::empty()).unwrap()["scope"]).unwrap();
        let json = current_with_prefix("scope", &scope);
        let repeated = format!("{{\"scope\":{scope},{}", &json[1..]);
        let error = serde_json::from_str::<CadIr>(&repeated)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("duplicate field") && error.contains("scope"),
            "{error}"
        );
    }

    #[test]
    fn unsupported_version_after_changed_model_shape_keeps_version_diagnostic() {
        for version in ["6", "7"] {
            let json = format!(
                r#"{{"model":{{"features":{{"legacy":true}}}},"ir_version":"{version}","decode_depth":"full"}}"#
            );
            let error = serde_json::from_str::<CadIr>(&json)
                .unwrap_err()
                .to_string();
            for expected in [
                "unsupported ir_version",
                "expected 8",
                "decode_depth",
                "scope",
                "report",
            ] {
                assert!(error.contains(expected), "{error}");
            }
        }
    }

    #[test]
    fn current_version_first_and_after_valid_prefix_have_the_same_document() {
        let expected = CadIr::empty();
        let model =
            serde_json::to_string(&serde_json::to_value(&expected).unwrap()["model"]).unwrap();
        let json = current_with_prefix("model", &model);
        assert_eq!(serde_json::from_str::<CadIr>(&json).unwrap(), expected);
        assert_eq!(
            serde_json::from_str::<CadIr>(&serde_json::to_string(&expected).unwrap()).unwrap(),
            expected
        );
    }
}
