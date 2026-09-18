// SPDX-License-Identifier: Apache-2.0
//! Variable fillet midpoints and the flattened historical binding.

use crate::records::test_support::refusal;
use crate::records::topology::test_support::states_the_key;

#[test]
fn variable_fillet_midpoints_preserve_wire_and_reject_unpaired_records() {
    let wire = r#"{"kind":"variable","start_radius_parameter_record_index":51,"end_radius_parameter_record_index":61,"middle_radius_parameter_record_indices":[71],"middle_parameter_record_indices":[81]}"#;
    let law: super::DesignFilletRadiusLaw = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&law).unwrap(), wire);
    for invalid in [wire.replace("[71]", "[]"), wire.replace("[81]", "[]")] {
        let error = serde_json::from_str::<super::DesignFilletRadiusLaw>(&invalid)
            .unwrap_err()
            .to_string();
        assert!(error.contains("middle_radius_parameter_record_indices"));
        assert!(error.contains("middle_parameter_record_indices"));
    }
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
        #[serde(flatten, deserialize_with = "super::deserialize_historical_binding")]
        binding: Option<super::HistoricalBinding>,
    }
    for key in ["historical_entity_kind", "historical_entity_ref"] {
        states_the_key(key, &refusal::<Probe>(key));
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.binding.is_none());
}
