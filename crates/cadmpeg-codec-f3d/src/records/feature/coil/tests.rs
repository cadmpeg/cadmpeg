// SPDX-License-Identifier: Apache-2.0
//! Absence spellings of the flattened coil-record readers.

#[test]
fn coil_secondary_identity_wire_refuses_a_null_identity() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(
            flatten,
            deserialize_with = "super::deserialize_coil_secondary_identity"
        )]
        secondary: Option<crate::records::identity::DesignSecondaryIdentity<u64>>,
    }
    for key in ["secondary_identity", "curve_secondary_identity"] {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<Probe>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.secondary.is_none());
}

#[test]
fn coil_recipe_design_wire_refuses_a_null_design_key() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(flatten, deserialize_with = "super::deserialize_coil_recipe_design")]
        design: Option<crate::records::recipes::ConstructionRecipeDesign<String>>,
    }
    for key in ["design_id", "design_selector"] {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<Probe>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.design.is_none());
}

/// Every flattened coil reader names the key it refuses.
///
/// Serde buffers a flattened field's keys into its own content map before the
/// reader runs, so no path a surrounding deserializer tracks reaches inside
/// one. The key reaches the refusal because the reading declaration states it.
#[test]
fn a_flattened_coil_reader_names_the_null_key_it_refuses() {
    use cadmpeg_test_support::refusal::refusal;

    #[derive(serde::Deserialize)]
    struct Secondary {
        #[serde(
            flatten,
            deserialize_with = "super::deserialize_coil_secondary_identity"
        )]
        #[allow(dead_code)]
        value: Option<crate::records::identity::DesignSecondaryIdentity<u64>>,
    }
    #[derive(serde::Deserialize)]
    struct Design {
        #[serde(flatten, deserialize_with = "super::deserialize_coil_recipe_design")]
        #[allow(dead_code)]
        value: Option<crate::records::recipes::ConstructionRecipeDesign<String>>,
    }

    let refusals = [
        (
            "secondary_identity",
            refusal::<Secondary>("secondary_identity"),
        ),
        (
            "curve_secondary_identity",
            refusal::<Secondary>("curve_secondary_identity"),
        ),
        ("design_id", refusal::<Design>("design_id")),
        ("design_selector", refusal::<Design>("design_selector")),
    ];
    for (key, message) in refusals {
        assert!(
            message.starts_with(&format!("{key}: ")),
            "the refusal of a null {key} states {message}"
        );
        assert!(
            message.contains("it does not state null"),
            "the refusal of a null {key} states {message}"
        );
    }
}
