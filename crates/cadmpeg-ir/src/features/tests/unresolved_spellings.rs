// SPDX-License-Identifier: Apache-2.0
use crate::features::{
    EdgeSelection, FaceSelection, FeatureDefinition, FeatureOperation,
    FilledSurfaceContinuityState, SheetSweepSection, SurfaceBoundary, SweepSection,
};

#[test]
fn an_unresolved_sweep_section_states_its_native_name_only_where_it_has_one() {
    let unnamed = SweepSection::Unresolved(None);
    let wire = serde_json::json!({"kind": "unresolved"});
    assert_eq!(serde_json::to_value(&unnamed).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<SweepSection>(wire.clone()).unwrap(),
        unnamed
    );
    assert_eq!(
        serde_json::from_value::<SheetSweepSection>(wire).unwrap(),
        SheetSweepSection::Unresolved(None)
    );

    let named = SweepSection::Unresolved(Some("native section".to_owned()));
    let wire = serde_json::json!({"kind": "unresolved", "value": "native section"});
    assert_eq!(serde_json::to_value(&named).unwrap(), wire);
    assert_eq!(serde_json::from_value::<SweepSection>(wire).unwrap(), named);

    let error = serde_json::from_value::<SweepSection>(serde_json::json!({
        "kind": "unresolved", "value": null
    }))
    .expect_err("null is a second spelling of an unnamed unresolved section")
    .to_string();
    assert!(
        error.contains("value: this key states a value or is left out"),
        "{error}"
    );
}

#[test]
fn unresolved_filled_surface_continuity_is_left_out_and_refuses_null() {
    let definition = FeatureDefinition::Operation(FeatureOperation::FilledSurface {
        boundary: SurfaceBoundary::Edges(EdgeSelection::Unresolved),
        support_faces: FaceSelection::Faces(Vec::new()),
        continuity: FilledSurfaceContinuityState::unresolved(),
        merge_result: None,
        interior_constraints: Vec::new(),
        reverse_normal: None,
    });
    let wire = serde_json::to_value(&definition).unwrap();
    assert!(wire["value"]["value"].get("continuity").is_none(), "{wire}");

    let mut null = wire;
    null["value"]["value"]["continuity"] = serde_json::Value::Null;
    let error = serde_json::from_value::<FeatureDefinition>(null)
        .expect_err("null is a second spelling of unresolved continuity")
        .to_string();
    assert!(
        error.contains("continuity: this key states a value or is left out"),
        "{error}"
    );
}
