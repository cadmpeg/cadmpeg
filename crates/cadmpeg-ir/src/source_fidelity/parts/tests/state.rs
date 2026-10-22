// SPDX-License-Identifier: Apache-2.0
//! Every part-state arm refuses unknown fields at its owning serde boundary.

use crate::ids::UnknownId;
use crate::source_fidelity::parts::{PartRecord, PartState};

#[test]
fn empty_part_states_keep_the_exact_existing_json_and_round_trip() {
    for (state, json) in [
        (PartState::Decoded {}, r#"{"state":"decoded"}"#),
        (PartState::GivenUp {}, r#"{"state":"given_up"}"#),
    ] {
        assert_eq!(serde_json::to_string(&state).expect("part state"), json);
        assert_eq!(
            serde_json::from_str::<PartState>(json).expect("unchanged wire state"),
            state
        );
    }
    let state = PartState::Covered {
        by: UnknownId::mint("step:source:part#parent").expect("part identity"),
    };
    let json = r#"{"state":"covered","by":"step:source:part#parent"}"#;
    assert_eq!(serde_json::to_string(&state).expect("covered state"), json);
    assert_eq!(
        serde_json::from_str::<PartState>(json).expect("covered wire state"),
        state
    );
}

#[test]
fn formerly_unit_part_states_reject_unknown_keys_before_or_after_the_tag() {
    for json in [
        r#"{"state":"decoded","unexpected":true}"#,
        r#"{"unexpected":true,"state":"decoded"}"#,
        r#"{"state":"given_up","unexpected":true}"#,
        r#"{"unexpected":true,"state":"given_up"}"#,
        r#"{"state":"decoded","by":"step:source:part#parent"}"#,
        r#"{"state":"given_up","by":"step:source:part#parent"}"#,
    ] {
        let error = serde_json::from_str::<PartState>(json).expect_err("unknown state member");
        assert!(
            error.to_string().contains("unknown field"),
            "{json}: {error}"
        );
    }
}

#[test]
fn covered_state_still_requires_its_owner_and_rejects_unknown_members() {
    for json in [
        r#"{"state":"covered"}"#,
        r#"{"state":"covered","by":"step:source:part#parent","unexpected":true}"#,
        r#"{"state":"covered","by":"step:source:part#parent","by":"step:source:part#other"}"#,
    ] {
        assert!(serde_json::from_str::<PartState>(json).is_err(), "{json}");
    }
}

#[test]
fn duplicate_or_unknown_state_tags_remain_invalid() {
    for json in [
        r#"{"state":"decoded","state":"decoded"}"#,
        r#"{"state":"given_up","state":"given_up"}"#,
        r#"{"state":"future"}"#,
    ] {
        assert!(serde_json::from_str::<PartState>(json).is_err(), "{json}");
    }
}

#[test]
fn nested_part_records_cannot_smuggle_extra_state_payload() {
    for state in ["decoded", "given_up"] {
        let json = serde_json::json!({
            "level":"step.simple_entity_instance",
            "stream":"source.step",
            "offset":0,
            "byte_len":16,
            "parent":null,
            "state":{"state":state},
            "framing_bytes":[{"start":0,"end":3},{"start":15,"end":16}]
        });
        let valid: PartRecord = serde_json::from_value(json.clone()).expect("bounded part record");
        assert_eq!(valid.byte_len(), 16);
        assert_eq!(
            serde_json::to_value(&valid).expect("record round trip"),
            json
        );
        let mut corrupt = json;
        corrupt["state"]["unexpected"] = serde_json::json!({"payload":[1,2,3]});
        let error =
            serde_json::from_value::<PartRecord>(corrupt).expect_err("nested state must be strict");
        assert!(error.to_string().contains("unknown field"), "{error}");
    }
}
