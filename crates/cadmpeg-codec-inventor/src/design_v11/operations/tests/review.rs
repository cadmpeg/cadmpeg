// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::design::selection::Picks;
use cadmpeg_ir::features::extents::{ExtrudeDirection, ExtrudeExtent, ExtrudeStart};
use cadmpeg_ir::features::geometry::FeatureDirection3;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};

fn assert_native(operation: &LegacyOperation) {
    assert_native_part(part(operation.clone()));
}

fn assert_native_part(mut ir: CadIr) {
    let operation = ir.model.features[0]
        .evaluation
        .definition()
        .operation()
        .clone();
    let losses = project_part(&mut ir);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        InventorLossCode::FeatureDesignOperationNative.code(),
        "feature.design-operation-native"
    );
    assert_eq!(
        losses[0].code,
        InventorLossCode::FeatureDesignOperationNative.kind()
    );
    assert_eq!(
        ir.design.features[0].definition.operation,
        retained(&operation).expect("retained operands")
    );
    assert!(validate(&ir).expect("validation").is_empty());
}

#[test]
fn opaque_collections_keep_unread_members_and_group_identity() {
    let operation = project_value(
        "combine",
        json!({"operands":{"target":native_selection(),"tools":{"kind":"native","value":"inventor:pmdc:selection#tools"}},"op":"join","keep_tools":false,"destination":"target_component"}),
    );
    let FeatureOperation::Combine(value) = operation else {
        panic!("combine")
    };
    let target = value.target.expect("target");
    assert!(matches!(target.picks, Picks::Unread));
    assert_eq!(
        target.native.expect("group")[0],
        "inventor:pmdc:selection#synthetic"
    );
    assert!(matches!(value.tools.picks, Picks::Unread));
    assert_eq!(
        value.tools.native.expect("group")[0],
        "inventor:pmdc:selection#tools"
    );
}

#[test]
fn selected_profile_loops_keep_nesting_native() {
    let mut operation = extrude();
    let LegacyOperation::Extrude { profile, .. } = &mut operation else {
        panic!("extrude")
    };
    *profile = ProfileRef::Planar(PlanarProfileRef::SketchProfiles {
        sketch: cadmpeg_ir::sketches::SketchId::mint("inventor:design:sketch#synthetic")
            .expect("sketch"),
        profiles: vec![0, 1].try_into().expect("loops"),
    });
    let mut ir = part(operation);
    ir.model.sketches.push(cadmpeg_ir::sketches::Sketch {
        id: cadmpeg_ir::sketches::SketchId::mint("inventor:design:sketch#synthetic")
            .expect("sketch"),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Unresolved {},
        profiles: ["outer", "hole"]
            .into_iter()
            .map(|name| {
                vec![cadmpeg_ir::sketches::SketchEntityUse {
                    entity: cadmpeg_ir::sketches::SketchEntityId::mint(format!(
                        "inventor:design:entity#{name}"
                    ))
                    .expect("curve"),
                    reversed: false,
                }]
            })
            .collect::<Vec<_>>()
            .try_into()
            .expect("rings"),
        native_ref: None,
    });
    assert_native_part(ir);
}

#[test]
fn oriented_chamfer_sizes_keep_the_support_side_native() {
    for spec in [
        json!({"kind":"two_distances","first":2.0,"second":4.0}),
        json!({"kind":"distance_angle","distance":2.0,"angle":0.7}),
    ] {
        assert_native(&legacy(
            "chamfer",
            json!({"groups":[{"edges":native_selection(),"spec":spec}],"flip_direction":true}),
        ));
    }
}

#[test]
fn extrusion_draft_keeps_unread_sign_and_neutral_plane_native() {
    for angle in [-0.2, 0.2] {
        let mut operation = extrude();
        let LegacyOperation::Extrude {
            extent: ExtrudeExtent::TwoSided { first, .. },
            ..
        } = &mut operation
        else {
            panic!("extrude")
        };
        first.draft = Some(cadmpeg_ir::scalar::SlopeAngle::new(angle).expect("draft"));
        assert_native(&operation);
    }
}

#[test]
fn loft_and_sweep_defaults_do_not_state_source_controls() {
    let FeatureOperation::Extrude(value) = typed(&extrude(), &[], &[]).expect("extrude") else {
        panic!("extrude")
    };
    let Some(cadmpeg_ir::design::ops::swept::ExtrudeDirection::Along { direction, .. }) =
        value.direction
    else {
        panic!("direction")
    };
    assert!(direction.mode.is_none());

    let FeatureOperation::Loft(loft) = project_value(
        "loft",
        json!({"sections":[{"kind":"profile","value":{"kind":"planar","value":profile()}},{"kind":"profile","value":{"kind":"planar","value":{"kind":"native","value":"inventor:pmdc:profile#second"}}}],"guidance":{"kind":"guides","value":[]},"op":"new_body","closed":false,"solid":true,"ruled":false,"linearize":false}),
    ) else {
        panic!("loft")
    };
    assert_eq!(loft.shape.ruled, None);
    assert_eq!(loft.shape.closed, None);
    assert!(!loft
        .source_settings
        .contains_key(&cadmpeg_ir::codec_name!("inventor:linearize")));
    let FeatureOperation::Sweep(sweep) = project_value(
        "sweep",
        json!({"shape":{"kind":"solid","op":"new_body","section":{"kind":"profile","value":profile()},"sections":[]},"path":{"kind":"native","value":"inventor:pmdc:path#synthetic"},"path_tangent":false,"linearize":false}),
    ) else {
        panic!("sweep")
    };
    assert!(!sweep
        .source_settings
        .contains_key(&cadmpeg_ir::codec_name!("inventor:linearize")));
    for (closed, ruled, linearize) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        assert_native(&legacy(
            "loft",
            json!({"sections":[],"guidance":{"kind":"guides","value":[]},"op":"new_body","closed":closed,"solid":true,"ruled":ruled,"linearize":linearize}),
        ));
    }
    assert_native(&legacy(
        "sweep",
        json!({"shape":{"kind":"solid","op":"new_body","section":{"kind":"profile","value":profile()},"sections":[]},"path":{"kind":"native","value":"inventor:pmdc:path#synthetic"},"path_tangent":false,"linearize":true}),
    ));
}

#[test]
fn validator_checks_the_shared_projection_and_native_gate() {
    let mut ir = part(extrude());
    project_part(&mut ir);
    assert!(validate(&ir).expect("valid").is_empty());
    let FeatureOperation::Extrude(value) = &mut ir.design.features[0].definition.operation else {
        panic!("extrude")
    };
    value.span = Some(Span::Between {
        start: Box::new(Limit::AtOrigin),
        end: Box::new(Limit::Amount(Dimension::Literal(length(1.0)))),
    });
    assert_eq!(validate(&ir).expect("altered amount").len(), 1);
    let duplicate = ir.design.features[0].clone();
    ir.design.features.push(duplicate);
    assert_eq!(validate(&ir).expect("duplicate feature").len(), 1);
    ir.design.features.clear();
    assert_eq!(validate(&ir).expect("missing feature").len(), 1);
    let mut operation = extrude();
    let LegacyOperation::Extrude {
        extent: ExtrudeExtent::TwoSided { first, .. },
        ..
    } = &mut operation
    else {
        panic!("extrude")
    };
    first.draft = Some(cadmpeg_ir::scalar::SlopeAngle::new(0.2).expect("draft"));
    let mut ir = part(operation);
    project_part(&mut ir);
    assert!(validate(&ir).expect("valid native gate").is_empty());
    ir.design.features[0].definition.operation =
        typed(&extrude(), &[], &[]).expect("unsupported replacement");
    assert_eq!(validate(&ir).expect("gate bypass").len(), 1);
}

#[test]
fn extrusion_amounts_require_resolved_travel_and_origin() {
    for unknown_direction in [false, true] {
        let mut operation = extrude();
        let LegacyOperation::Extrude {
            direction, start, ..
        } = &mut operation
        else {
            panic!("extrude")
        };
        if unknown_direction {
            *direction = ExtrudeDirection::Unresolved {};
        } else {
            *start = ExtrudeStart::Unresolved {};
        }
        assert_native(&operation);
    }
    let mut ir = part(extrude());
    assert!(project_part(&mut ir).is_empty());
    assert!(matches!(
        ir.design.features[0].definition.operation,
        FeatureOperation::Extrude(_)
    ));
    assert!(validate(&ir)
        .expect("resolved travel and origin")
        .is_empty());
}

#[test]
fn native_draft_operands_validate_after_canonical_zero_round_trip() {
    let mut operation = extrude();
    let LegacyOperation::Extrude {
        profile,
        direction,
        extent: ExtrudeExtent::TwoSided { first, .. },
        ..
    } = &mut operation
    else {
        panic!("extrude")
    };
    *profile = ProfileRef::Planar(PlanarProfileRef::Native(
        "inventor:pmdc:profile#text-0.0".into(),
    ));
    *direction = ExtrudeDirection::Explicit {
        vector: FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(-0.0, 0.0, 1.0))
            .expect("direction"),
        source: None,
    };
    first.draft = Some(cadmpeg_ir::scalar::SlopeAngle::new(0.2).expect("draft"));
    assert_canonical_native_round_trip(&operation);
}

#[test]
fn native_body_scale_operands_validate_after_canonical_zero_round_trip() {
    assert_canonical_native_round_trip(&legacy(
        "scale",
        json!({"bodies":native_selection(),"center":{"kind":"point","value":[-0.0,1.0,2.0]},"factors":{"kind":"uniform","factor":2.0}}),
    ));
}

fn assert_canonical_native_round_trip(operation: &LegacyOperation) {
    let mut ir = part(operation.clone());
    let losses = project_part(&mut ir);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        InventorLossCode::FeatureDesignOperationNative.kind()
    );
    assert!(matches!(
        ir.design.features[0].definition.operation,
        FeatureOperation::Native { .. }
    ));
    let FeatureOperation::Native { native_options, .. } =
        &ir.design.features[0].definition.operation
    else {
        panic!("native")
    };
    let NativeOption::Text(Dimension::Literal(text)) = &native_options["inventor:legacy_operation"]
    else {
        panic!("retained")
    };
    assert_eq!(
        &serde_json::from_str::<LegacyOperation>(text.as_str()).expect("retained operands"),
        operation
    );
    assert!(validate(&ir).expect("decoded operands").is_empty());
    let wire = ir.to_canonical_json().expect("canonical document");
    let restored = CadIr::from_json(&wire).expect("restored document");
    assert!(validate(&restored).expect("restored operands").is_empty());
}
