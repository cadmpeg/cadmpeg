// SPDX-License-Identifier: Apache-2.0
use crate::features::selections::{EdgeSelection, FaceSelection};
use crate::features::surface_inputs::SurfaceBoundary;
use crate::features::sweeps::{SheetSweepSection, SweepSection};
use crate::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_core::serde_erase::json;

#[test]
fn an_unresolved_sweep_section_states_its_native_name_only_where_it_has_one() {
    let unnamed = SweepSection::Unresolved;
    let wire = serde_json::json!({"kind": "unresolved"});
    assert_eq!(json::to_value(&unnamed).unwrap(), wire);
    assert_eq!(
        json::from_value::<SweepSection>(wire.clone()).unwrap(),
        unnamed
    );
    assert_eq!(
        json::from_value::<SheetSweepSection>(wire).unwrap(),
        SheetSweepSection::Unresolved
    );

    let named = SweepSection::Native(cadmpeg_core::nonblank_literal!("native section"));
    let wire = serde_json::json!({"kind": "native", "value": "native section"});
    assert_eq!(json::to_value(&named).unwrap(), wire);
    assert_eq!(json::from_value::<SweepSection>(wire).unwrap(), named);
    assert_eq!(named.unresolved_native(), Some("native section"));

    for stated in [serde_json::Value::Null, serde_json::json!("native section")] {
        let error = json::from_value::<SweepSection>(serde_json::json!({
            "kind": "unresolved", "value": stated
        }))
        .expect_err("an unnamed unresolved section states its kind alone")
        .to_string();
        assert!(error.contains("does not state value"), "{error}");
    }
}

#[test]
fn unresolved_filled_surface_continuity_is_left_out_and_refuses_null() {
    let definition = FeatureDefinition::Operation(FeatureOperation::FilledSurface {
        boundary: SurfaceBoundary::Edges(EdgeSelection::Unresolved),
        support_faces: FaceSelection::Faces(Vec::new()),
        continuity: None,
        merge_result: None,
        interior_constraints: Vec::new(),
        reverse_normal: None,
    });
    let wire = json::to_value(&definition).unwrap();
    assert!(wire["value"]["value"].get("continuity").is_none(), "{wire}");

    let mut null = wire;
    null["value"]["value"]["continuity"] = serde_json::Value::Null;
    let error = json::from_value::<FeatureDefinition>(null)
        .expect_err("null is a second spelling of unresolved continuity")
        .to_string();
    assert!(
        error.contains("continuity: this key states a value or is left out"),
        "{error}"
    );
}
