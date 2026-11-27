// SPDX-License-Identifier: Apache-2.0
//! The §2.5 wire forms of the process types.

use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;

use super::fixtures::{self, constant, parameter, text, uuid};
use super::{read_desktop, refuses, to_value};
use crate::process::cycle::Cycle;
use crate::process::operation::{Operation, OperationClass, OperationKind};
use crate::process::setup::{AxisClass, JobKind};
use crate::process::stock::WcsOrigin;
use crate::process::technology::Technology;
use crate::process::tool::{Machine, RotaryAxes, Uuid};
use crate::process::Choice;

#[test]
fn a_uuid_is_its_hyphenated_lower_case_string() {
    assert_eq!(
        to_value(&uuid()),
        json!("123e4567-e89b-12d3-a456-426614174000")
    );
    let back: Uuid = read_desktop("\"123e4567-e89b-12d3-a456-426614174000\"").unwrap();
    assert_eq!(back, uuid());
    for text in [
        "\"123E4567-E89B-12D3-A456-426614174000\"",
        "\"{123e4567-e89b-12d3-a456-426614174000}\"",
        "\"123e4567e89b12d3a456426614174000\"",
        "\"123e4567-e89b-12d3-a456-42661417400\"",
        "\"123e4567-e89b-12d3-a456-42661417400g\"",
        "\"123e45-67e89b-12d3-a456-426614174000\"",
    ] {
        assert!(refuses::<Uuid>(text).contains("lower case"), "{text}");
    }
}

#[test]
fn enums_take_the_tag_form_their_variants_need() {
    assert_eq!(to_value(&AxisClass::TwoAndHalf), json!("two_and_half"));
    assert_eq!(to_value(&JobKind::Milling), json!({"kind": "milling"}));
    assert_eq!(
        to_value(&JobKind::Native(text("f3d:inspection"))),
        json!({"kind": "native", "value": "f3d:inspection"})
    );
    assert_eq!(
        to_value(&OperationKind::Machining {
            class: OperationClass::Probing,
            phase: None,
        }),
        json!({"kind": "machining", "class": {"kind": "probing"}})
    );
    assert_eq!(to_value(&RotaryAxes::Absent {}), json!({"kind": "absent"}));
    assert_eq!(
        to_value(&RotaryAxes::Present { count: None }),
        json!({"kind": "present"})
    );
    refuses::<JobKind>(r#"{"kind": "milling", "extra": 1}"#);
    refuses::<RotaryAxes>(r#"{"kind": "absent", "extra": 1}"#);
    refuses::<AxisClass>(r#""unknown""#);
}

#[test]
fn optional_fields_are_left_out_and_refuse_null() {
    let value = to_value(&fixtures::programs().remove(1));
    assert!(value.get("name").is_none(), "{value}");
    assert!(value.get("post").is_none(), "{value}");
    assert_eq!(value["steps"], json!([]), "collections are always written");
    let message = refuses::<Choice<JobKind>>(r#"{"value": null, "parameters": []}"#);
    assert!(message.contains("null"), "{message}");
    let message = refuses::<Machine>(
        &json!({"id": "test:process:machine#1", "linear_axes": null, "pockets": []}).to_string(),
    );
    assert!(message.contains("null"), "{message}");
}

#[test]
fn sets_are_sorted_arrays_and_refuse_a_repeated_member() {
    let choice = Choice {
        value: Some(WcsOrigin::ModelOrigin),
        parameters: [parameter(2), parameter(1)].into(),
    };
    assert_eq!(
        to_value(&choice),
        json!({
            "value": {"kind": "model_origin"},
            "parameters": ["test:process:process-parameter#1", "test:process:process-parameter#2"],
        })
    );
    let repeated = json!({
        "parameters": ["test:process:process-parameter#1", "test:process:process-parameter#1"],
    });
    let message = refuses::<Choice<WcsOrigin>>(&repeated.to_string());
    assert!(message.contains("twice"), "{message}");

    let machine = fixtures::machines().remove(0);
    assert_eq!(to_value(&machine)["linear_axes"], json!(["x", "y", "z"]));
    let mut value = to_value(&machine);
    value["linear_axes"] = json!(["x", "x"]);
    let message = refuses::<Machine>(&value.to_string());
    assert!(message.contains("twice"), "{message}");

    let message = refuses::<Machine>(
        r#"{"id":"test:process:machine#1","pockets":[],"linear_axes":["z","x"]}"#,
    );
    assert!(message.contains("out of order"), "{message}");
    let sorted = r#"{"id":"test:process:machine#1","linear_axes":["x","z"],"pockets":[]}"#;
    let machine: Machine = serde_json::from_str(sorted).expect("sorted axes read");
    assert_eq!(json::to_string(&machine).expect("writes"), sorted);
}

#[test]
fn maps_are_objects_or_sorted_pairs_and_refuse_a_repeated_key() {
    let operation = fixtures::operations().remove(0);
    let value = to_value(&operation);
    assert!(value["heights"].is_object(), "{value}");
    assert!(value["heights"].get("clearance").is_some(), "{value}");
    assert!(value["radii"].get("outer_clearance").is_some(), "{value}");
    assert!(value["axial"].get("front").is_some(), "{value}");
    assert!(value["configuration_states"].is_object(), "{value}");

    let empty = to_value(&fixtures::operations().remove(1));
    assert_eq!(empty["heights"], json!({}));
    assert_eq!(empty["configuration_states"], json!({}));

    let text = json::to_string(&operation).unwrap();
    assert!(text.contains(r#""bottom":{}"#), "{text}");
    let text = text.replacen(r#""heights":{"#, r#""heights":{"bottom":{},"#, 1);
    let message = refuses::<Operation>(&text);
    assert!(message.contains("duplicate key bottom"), "{message}");

    let technology = fixtures::full_technology();
    let feeds = &to_value(&technology)["feeds"];
    assert!(feeds.is_array(), "FeedRole keys write [key, value] pairs");
    assert_eq!(feeds[0][0], json!({"kind": "cutting"}));
    assert_eq!(to_value(&fixtures::empty_technology())["feeds"], json!([]));
    let pair = json!([
        {"kind": "plunge"},
        {"kind": "per_time", "value": {"kind": "literal", "value": 5.0}},
    ]);
    let message = refuses::<Technology>(&json!({"feeds": [pair, pair]}).to_string());
    assert!(message.contains("twice"), "{message}");

    let cycle = fixtures::cycles().remove(0);
    let value = to_value(&cycle);
    assert!(value["parameters"].is_object(), "{value}");
    assert!(value["feeds"].is_array(), "{value}");
    let mut text = json::to_string(&cycle).unwrap();
    let parameter = r#""accumulatedDepth":{"kind":"real","value":6.0}"#;
    assert!(text.contains(parameter), "{text}");
    text = text.replacen(parameter, &format!("{parameter},{parameter}"), 1);
    refuses::<Cycle>(&text);
}

#[test]
fn a_role_key_displays_as_its_wire_name() {
    use crate::process::limits::{AxialRole, HeightRole, RadiusRole};
    fn check<T: serde::Serialize + std::fmt::Display>(roles: &[T]) {
        for role in roles {
            assert_eq!(to_value(role), json!(role.to_string()));
        }
    }
    check(&[
        HeightRole::Clearance,
        HeightRole::Retract,
        HeightRole::Feed,
        HeightRole::Top,
        HeightRole::Bottom,
    ]);
    check(&[
        RadiusRole::Outer,
        RadiusRole::Inner,
        RadiusRole::OuterClearance,
        RadiusRole::InnerClearance,
        RadiusRole::OuterRetract,
    ]);
    check(&[AxialRole::Front, AxialRole::Back]);
}

#[test]
fn a_constant_choice_states_no_parameters() {
    assert_eq!(
        to_value(&constant(JobKind::Turning)),
        json!({"value": {"kind": "turning"}, "parameters": []})
    );
}
