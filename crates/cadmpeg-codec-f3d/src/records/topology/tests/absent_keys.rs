// SPDX-License-Identifier: Apache-2.0
//! Absence spellings of the topology records: every refusal names its key.

fn refusal<T: serde::de::DeserializeOwned>(key: &str) -> String {
    let mut wire = serde_json::json!({});
    wire[key] = serde_json::Value::Null;
    let Err(refused) = serde_json::from_value::<T>(wire) else {
        panic!("{key}: null was admitted")
    };
    refused.to_string()
}

fn states_the_key(key: &str, message: &str) {
    assert!(
        message.contains(key),
        "the refusal of a null {key} states {message}"
    );
    assert!(
        message.contains("it does not state null"),
        "the refusal of a null {key} states {message}"
    );
}

/// The flattened historical-binding reader names the key it refuses.
///
/// Serde buffers a flattened field's keys into its own content map before the
/// reader runs, so no path a surrounding deserializer tracks reaches inside
/// one. The key reaches the refusal because the reading declaration states it.
#[test]
fn the_flattened_historical_binding_names_the_null_key_it_refuses() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(
            flatten,
            deserialize_with = "crate::records::topology::deserialize_historical_binding"
        )]
        #[allow(dead_code)]
        binding: Option<crate::records::topology::HistoricalBinding>,
    }
    for key in ["historical_entity_kind", "historical_entity_ref"] {
        states_the_key(key, &refusal::<Probe>(key));
    }
}

/// A top-level optional key on a topology record names itself in its refusal.
#[test]
fn a_top_level_topology_key_names_itself_in_its_refusal() {
    for key in ["transform", "transform_offset", "compact_variant"] {
        states_the_key(
            key,
            &refusal::<crate::records::topology::DesignConstructionOperandPathWire>(key),
        );
    }
    for key in ["first_related_identity", "second_related_identity"] {
        states_the_key(
            key,
            &refusal::<crate::records::topology::DesignConstructionTrackingPathWire>(key),
        );
    }
}
