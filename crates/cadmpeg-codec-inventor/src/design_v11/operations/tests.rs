// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::design::ops::{
    body as body_ops, dressup as dressup_ops,
    shared::{Limit, LinearLayout, PatternLayout, Spacing, Span},
    sheet_metal::{NormalSide, SheetMetalKind},
};
use cadmpeg_ir::design::options::{BodyForm, Boolean, SymmetricMeasure};
use cadmpeg_ir::features as old;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{FeatureContent, FeatureId};
use cadmpeg_ir::scalar::{Length, PositiveLength};
use serde_json::{json, Value};

use super::*;

/// The production v11 validation of a projected part.
fn validate(ir: &CadIr) -> Result<Vec<cadmpeg_ir::report::check::Finding>, CodecError> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    crate::validate::design_v11_findings_from_arenas(&ctx, ir)
}

fn legacy(kind: &str, value: Value) -> LegacyOperation {
    let fields = serde_json::Map::from_iter([
        ("kind".to_owned(), Value::String(kind.to_owned())),
        ("value".to_owned(), value),
    ]);
    json::from_value(Value::Object(fields)).expect("synthetic operation")
}

fn project_value(kind: &str, value: Value) -> FeatureOperation {
    let operation = typed(
        &legacy(kind, value),
        selections::SketchData::default(),
        &[],
        &[],
    )
    .expect("typed projection");
    let wire = json::to_value(&operation).expect("v11 wire");
    let decoded: FeatureOperation = json::from_value(wire).expect("admitted v11 operation");
    assert_eq!(operation, decoded);
    operation
}

fn profile() -> Value {
    json!({"kind":"native","value":"inventor:pmdc:profile#synthetic"})
}
fn native_selection() -> Value {
    json!({"kind":"native","value":"inventor:pmdc:selection#synthetic"})
}
fn length(value: f64) -> Length {
    Length::new(value).expect("length")
}
fn positive(value: f64) -> PositiveLength {
    PositiveLength::new(value).expect("positive")
}

fn extrude() -> LegacyOperation {
    legacy(
        "extrude",
        json!({"profile":{"kind":"planar","value":profile()},"direction":{"kind":"explicit","vector":[0.0,0.0,-2.0],"source":{"kind":"custom"}},"start":{"kind":"profile_plane"},"extent":{"kind":"two_sided","first":{"termination":{"kind":"blind","length":8.0}},"second":{"termination":{"kind":"blind","length":3.0}}},"op":"cut","solid":true}),
    )
}

#[test]
fn extrude_keeps_direction_sides_and_output() {
    let FeatureOperation::Extrude(value) =
        typed(&extrude(), selections::SketchData::default(), &[], &[]).expect("extrude")
    else {
        panic!("extrude");
    };
    assert_eq!(value.output.body, Some(BodyForm::Solid));
    assert!(matches!(
        value.output.boolean,
        Some(Boolean::Apply {
            op: cadmpeg_ir::design::options::BooleanOp::Cut,
            targets: None
        })
    ));
    let Some(Span::Between { start, end }) = value.span else {
        panic!("span");
    };
    assert_eq!(*start, Limit::Amount(Dimension::Literal(length(-3.0))));
    assert_eq!(*end, Limit::Amount(Dimension::Literal(length(8.0))));
    assert!(value.draft.is_none());
    let Some(cadmpeg_ir::design::ops::swept::ExtrudeDirection::Along { direction, .. }) =
        value.direction
    else {
        panic!("direction");
    };
    assert_eq!(direction.resolved.expect("direction").as_raw().z, -1.0);
}

#[test]
fn symmetric_extrude_keeps_total_travel() {
    let mut operation = extrude();
    let LegacyOperation::Extrude { extent, .. } = &mut operation else {
        panic!("extrude");
    };
    *extent = cadmpeg_ir::features::extents::ExtrudeExtent::Symmetric {
        side: cadmpeg_ir::features::extents::ExtrudeSide {
            termination: cadmpeg_ir::features::extents::LinearTermination::Blind {
                length: cadmpeg_ir::scalar::NonZeroLength::new(6.0).expect("length"),
            },
            draft: None,
        },
    };
    let FeatureOperation::Extrude(value) =
        typed(&operation, selections::SketchData::default(), &[], &[]).expect("extrude")
    else {
        panic!("extrude");
    };
    assert_eq!(
        value.span,
        Some(Span::Symmetric {
            amount: Dimension::Literal(length(6.0)),
            measure: Some(SymmetricMeasure::Total)
        })
    );
}

#[test]
fn revolve_keeps_axis_angles_and_output() {
    let result = project_value(
        "revolve",
        json!({"construction":{"kind":"resolved","value":{"profile":profile(),"axis":{"origin":[1.0,2.0,3.0],"direction":[0.0,-4.0,0.0]},"extent":{"kind":"two_sided","first":{"kind":"angle","angle":1.2},"second":{"kind":"angle","angle":0.4}},"solid":true}},"op":"new_body"}),
    );
    let FeatureOperation::Revolve(value) = result else {
        panic!("revolve");
    };
    assert_eq!(
        value
            .axis
            .expect("axis")
            .resolved
            .expect("axis")
            .direction
            .as_raw()
            .y,
        -1.0
    );
    let Some(Span::Between { start, end }) = value.span else {
        panic!("span");
    };
    assert_eq!(
        *start,
        Limit::Amount(Dimension::Literal(
            cadmpeg_ir::scalar::Angle::new(-0.4).expect("angle")
        ))
    );
    assert_eq!(
        *end,
        Limit::Amount(Dimension::Literal(
            cadmpeg_ir::scalar::Angle::new(1.2).expect("angle")
        ))
    );
    assert_eq!(value.output.boolean, Some(Boolean::NewBody));
}

#[test]
fn fillet_keeps_every_group_and_size() {
    let result = project_value(
        "fillet",
        json!({"groups":[{"edges":native_selection(),"radius":{"kind":"constant","radius":2.0}},{"edges":{"kind":"native","value":"inventor:pmdc:selection#second"},"radius":{"kind":"chordal","chord_length":3.0},"tangency_weight":0.5}]}),
    );
    let FeatureOperation::Fillet(value) = result else {
        panic!("fillet");
    };
    assert_eq!(value.sets.len(), 2);
    let dressup_ops::FilletSet::Edges { section, .. } = &value.sets[0] else {
        panic!("edges");
    };
    assert_eq!(
        section.size,
        Some(dressup_ops::FilletSize::Radius(Dimension::Literal(
            positive(2.0)
        )))
    );
}

#[test]
fn chamfer_keeps_size_and_flip() {
    for spec in [
        json!({"kind":"distance","distance":2.0}),
        json!({"kind":"two_distances","first":2.0,"second":2.0}),
    ] {
        let FeatureOperation::Chamfer(value) = project_value(
            "chamfer",
            json!({"groups":[{"edges":native_selection(),"spec":spec}],"flip_direction":true}),
        ) else {
            panic!("chamfer");
        };
        assert_eq!(value.sets[0].flip, Some(Dimension::Literal(true)));
        assert!(value.sets[0].size.is_some());
    }
}

#[test]
fn shell_keeps_empty_removed_faces_and_unknown_side() {
    let FeatureOperation::Shell(value) = project_value(
        "shell",
        json!({"removed_faces":{"kind":"faces","value":[]},"thickness":2.0}),
    ) else {
        panic!("shell");
    };
    assert!(
        matches!(value.removed.picks, cadmpeg_ir::design::selection::Picks::Named(members) if members.as_slice().is_empty())
    );
    assert_eq!(
        value.thickness,
        Some(cadmpeg_ir::design::options::Thickness::OneSide {
            value: Dimension::Literal(positive(2.0)),
            side: None
        })
    );
}

#[test]
fn hole_keeps_drilling_positions_head_bottom_and_extent() {
    let FeatureOperation::Hole(value) = project_value(
        "hole",
        json!({"placements":[{"kind":"directed","position":[1.0,2.0,3.0],"direction":[0.0,0.0,-1.0]},{"kind":"directed","position":[4.0,2.0,3.0],"direction":[0.0,0.0,-1.0]}],"shape":{"construction":{"kind":"form","value":{"kind":{"kind":"counterbore_drilled","diameter":4.0,"depth":2.0,"drill_point_angle":1.7}}},"diameter":2.0},"extent":{"kind":"blind","length":8.0}}),
    ) else {
        panic!("hole");
    };
    assert!(
        matches!(value.positions, dressup_ops::HolePositions::Placed(ref points) if points.len()==2)
    );
    assert_eq!(
        value
            .direction
            .expect("direction")
            .resolved
            .expect("direction")
            .as_raw()
            .z,
        -1.0
    );
    let dressup_ops::HoleShape::Bore(bore) = value.shape else {
        panic!("bore");
    };
    assert_eq!(bore.diameter, Some(Dimension::Literal(positive(2.0))));
    assert!(matches!(
        bore.head,
        Some(dressup_ops::HoleHead::Counterbore { .. })
    ));
    assert!(matches!(
        bore.bottom,
        Some(dressup_ops::HoleBottom::DrillPoint { .. })
    ));
    assert_eq!(
        value.extent,
        Some(Limit::Amount(Dimension::Literal(length(8.0))))
    );
}

#[test]
fn thicken_keeps_side_thickness_and_boolean_targets() {
    let FeatureOperation::Thicken(value) = project_value(
        "thicken",
        json!({"faces":native_selection(),"thickness":2.0,"side":"both","result":{"kind":"new_body"}}),
    ) else {
        panic!("thicken");
    };
    assert_eq!(
        value.thickness,
        Some(cadmpeg_ir::design::options::Thickness::Symmetric(
            Dimension::Literal(positive(2.0))
        ))
    );
    assert_eq!(value.boolean, Some(Boolean::NewBody));
}

#[test]
fn sweep_and_loft_keep_profiles_paths_and_outputs() {
    let FeatureOperation::Sweep(sweep) = project_value(
        "sweep",
        json!({"shape":{"kind":"solid","op":"new_body","section":{"kind":"profile","value":profile()},"sections":[]},"path":{"kind":"native","value":"inventor:pmdc:path#synthetic"},"path_tangent":false,"linearize":false}),
    ) else {
        panic!("sweep");
    };
    assert_eq!(sweep.output.boolean, Some(Boolean::NewBody));
    let FeatureOperation::Loft(loft) = project_value(
        "loft",
        json!({"sections":[{"kind":"profile","value":{"kind":"planar","value":profile()}},{"kind":"profile","value":{"kind":"planar","value":{"kind":"native","value":"inventor:pmdc:profile#second"}}}],"guidance":{"kind":"guides","value":[]},"op":"new_body","closed":false,"solid":true,"ruled":false,"linearize":false}),
    ) else {
        panic!("loft");
    };
    assert_eq!(loft.sections.len(), 2);
    assert_eq!(loft.output.boolean, Some(Boolean::NewBody));
}

#[test]
fn rectangular_pattern_keeps_grid_counts_spacing_and_omissions() {
    let FeatureOperation::Pattern(value) = project_value(
        "pattern",
        json!({"seeds":[{"kind":"feature","value":"inventor:design:feature#seed"}],"pattern":{"kind":"linear","direction":[-1.0,0.0,0.0],"spacing":3.0,"count":4,"second":{"direction":[0.0,1.0,0.0],"spacing":5.0,"count":2,"symmetric":false},"omitted_instances":[[2,1]],"symmetric":false}}),
    ) else {
        panic!("pattern");
    };
    let PatternLayout::Linear(LinearLayout {
        first,
        second: Some(second),
        ..
    }) = &value.layout
    else {
        panic!("linear");
    };
    assert_eq!(
        first.direction.resolved.expect("direction").as_raw().x,
        -1.0
    );
    assert_eq!(
        first.run.spacing,
        Spacing::Pitch {
            count: Dimension::Literal(4),
            pitch: Dimension::Literal(length(3.0))
        }
    );
    assert_eq!(
        second.run.spacing,
        Spacing::Pitch {
            count: Dimension::Literal(2),
            pitch: Dimension::Literal(length(5.0))
        }
    );
    assert!(
        matches!(value.instances, body_ops::PatternInstances::Implicit(ref overrides) if overrides.len()==1 && overrides[0].index.0 == [2,1])
    );
}

#[test]
fn mirror_keeps_unresolved_disposition_native() {
    let operation = legacy(
        "mirror_shape",
        json!({"source":native_selection(),"plane_origin":[1.0,2.0,3.0],"plane_normal":[0.0,1.0,0.0]}),
    );
    let mut ir = part(operation.clone());
    let losses = project_part(&mut ir);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        InventorLossCode::FeatureDesignOperationNative.kind()
    );
    assert_eq!(
        ir.design.features[0].definition.operation,
        retained(&operation).expect("native")
    );
}

#[test]
fn body_operations_keep_tools_motion_and_healing() {
    assert!(matches!(
        project_value(
            "split_body",
            json!({"targets":native_selection(),"tool":{"kind":"plane","value":{"plane":"inventor:design:feature#plane"}}})
        ),
        FeatureOperation::Split(_)
    ));
    let FeatureOperation::TransformBodies(value) = project_value(
        "move_body",
        json!({"bodies":native_selection(),"translation":[2.0,-3.0,4.0],"copies":2}),
    ) else {
        panic!("move");
    };
    assert_eq!(
        value.mode,
        Some(body_ops::TransformMode::Copy {
            count: Dimension::Literal(2),
            divisions: None
        })
    );
    let FeatureOperation::DeleteFaces(value) = project_value(
        "delete_face",
        json!({"faces":native_selection(),"heal":true}),
    ) else {
        panic!("delete");
    };
    assert_eq!(value.heal, Some(body_ops::Heal::Extend));
    assert!(matches!(
        project_value(
            "move_face",
            json!({"faces":native_selection(),"motion":{"kind":"offset","distance":-2.0}})
        ),
        FeatureOperation::Offset(_)
    ));
}

#[test]
fn sheet_metal_face_keeps_profile_thickness_and_side() {
    let FeatureOperation::SheetMetal(value) = project_value(
        "sheet_metal_base_flange",
        json!({"profile":profile(),"thickness":2.0,"side":"reverse"}),
    ) else {
        panic!("sheet metal");
    };
    let SheetMetalKind::Wall {
        thickness, side, ..
    } = value.kind
    else {
        panic!("wall");
    };
    assert_eq!(thickness, Some(Dimension::Literal(positive(2.0))));
    assert_eq!(side, Some(NormalSide::Against));
}

fn part(operation: LegacyOperation) -> CadIr {
    let mut ir = crate::test_support::root_part_ir();
    ir.model.features.push(old::Feature {
        product_definition: None,
        id: FeatureId::mint("inventor:design:feature#synthetic").expect("id"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(old::FeatureDefinition::Operation(
            operation,
        )),
        native_ref: Some("inventor:pmdc:feature#synthetic".into()),
    });
    ir
}

fn project_part(ir: &mut CadIr) -> Vec<LossNote> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    super::super::project_history(&ctx, ir, &BTreeMap::default(), false).expect("history");
    let original = ir.model.clone();
    let mut losses = Vec::new();
    project(&ctx, ir, &mut losses).expect("operations");
    assert_eq!(ir.model, original);
    losses
}

#[test]
fn unsupported_operands_keep_the_full_native_operation_and_pin_loss() {
    let cases = [
        legacy(
            "sheet_metal_attached_plate",
            json!({"profile":profile(),"thickness":2.0,"side":"forward","attachment":native_selection()}),
        ),
        legacy(
            "sheet_metal_bend",
            json!({
                "plate":"inventor:design:feature#plate",
                "attachment":native_selection(),"inside_radius":1.0,
                "relief":{"form":"style_default","width":2.0,"depth":3.0,"minimum_remnant":4.0,"seam_gap":0.5},
                "transition":{"form":"style_default","radius":6.0}
            }),
        ),
        legacy(
            "sheet_metal_contour_flange",
            json!({"construction":{"profile":profile(),"direction":[0.0,0.0,1.0],"thickness":2.0,"neutral_factor":0.4}}),
        ),
        legacy(
            "sheet_metal_unfold",
            json!({"construction":{"stationary":native_selection(),"all_bends":true,"neutral_factor":0.4}}),
        ),
        legacy(
            "sheet_metal_refold",
            json!({"construction":{"unfold":"inventor:design:feature#unfold"}}),
        ),
        legacy(
            "sheet_metal_corner",
            json!({"bend":"inventor:design:feature#bend","effect":"unchanged"}),
        ),
    ];
    for operation in cases {
        let mut ir = part(operation.clone());
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
        let FeatureOperation::Native { native_options, .. } =
            &ir.design.features[0].definition.operation
        else {
            panic!("native");
        };
        let NativeOption::Text(Dimension::Literal(text)) =
            &native_options["inventor:legacy_operation"]
        else {
            panic!("retained");
        };
        assert_eq!(
            serde_json::from_str::<LegacyOperation>(text.as_str()).expect("legacy operands"),
            operation
        );
    }
}

#[test]
fn typed_projection_keeps_v10_and_stored_outputs() {
    let mut ir = part(extrude());
    assert!(project_part(&mut ir).is_empty());
    assert!(matches!(
        ir.design.features[0].definition.operation,
        FeatureOperation::Extrude(_)
    ));
    let mut ir = part(LegacyOperation::StoredGeometry {});
    let body = cadmpeg_ir::ids::BodyId::mint("inventor:brep:body#synthetic").expect("body");
    ir.model.features[0].evaluation.edit(|_, outputs| {
        outputs.insert(body.clone());
    });
    assert!(project_part(&mut ir).is_empty());
    assert!(
        matches!(&ir.design.features[0].definition.operation, FeatureOperation::Stored { bodies: Some(bodies), .. } if bodies == &[body])
    );
}

#[test]
fn unfold_and_refold_keep_selected_and_unread_bends() {
    let FeatureOperation::SheetMetal(value) = project_value(
        "sheet_metal_unfold",
        json!({"construction":{"stationary":native_selection(),"bends":native_selection()}}),
    ) else {
        panic!("unfold");
    };
    assert!(matches!(
        value.kind,
        SheetMetalKind::Unfold {
            fixed: Some(_),
            bends: Some(cadmpeg_ir::design::ops::sheet_metal::BendSet::Selected(_))
        }
    ));
    let FeatureOperation::SheetMetal(value) =
        project_value("sheet_metal_refold", json!({"construction":{}}))
    else {
        panic!("refold");
    };
    assert!(matches!(value.kind, SheetMetalKind::Refold { bends: None }));
}

#[test]
fn attached_plate_and_bend_keep_native_attachment_and_controls() {
    let operations = [
        legacy(
            "sheet_metal_attached_plate",
            json!({"profile":profile(),"thickness":2.0,"side":"forward","attachment":native_selection()}),
        ),
        legacy(
            "sheet_metal_bend",
            json!({"plate":"inventor:design:feature#plate","attachment":native_selection(),"inside_radius":2.0,"relief":{"form":"style_default","width":1.0,"depth":3.0,"minimum_remnant":0.5,"seam_gap":0.25},"transition":{"form":"style_default","radius":1.0}}),
        ),
    ];
    for operation in operations {
        let mut ir = part(operation);
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
    }
}

#[test]
fn combine_keeps_target_tools_operation_and_retention() {
    let FeatureOperation::Combine(value) = project_value(
        "combine",
        json!({"operands":{"target":native_selection(),"tools":{"kind":"native","value":"inventor:pmdc:selection#tool"}},"op":"cut","keep_tools":true,"destination":"target_component"}),
    ) else {
        panic!("combine");
    };
    assert!(value.target.is_some());
    assert_eq!(value.op, Some(cadmpeg_ir::design::options::BooleanOp::Cut));
    assert_eq!(
        value.tools_after,
        Some(cadmpeg_ir::design::ops::shared::ToolDisposition::Kept)
    );
}

#[test]
fn trim_keeps_unknown_and_reverse_sides() {
    for (side, expected) in [
        ("unresolved", None),
        ("forward", Some(false)),
        ("reverse", Some(true)),
    ] {
        let FeatureOperation::Split(value) = project_value(
            "trim_bodies_at_plane",
            json!({"targets":native_selection(),"plane":"inventor:design:feature#plane","keep":side}),
        ) else {
            panic!("split");
        };
        let body_ops::SplitMode::ByTools { kept, .. } = value.mode else {
            panic!("tools");
        };
        assert_eq!(
            kept,
            expected.map(|value| body_ops::Kept::Side {
                side: Dimension::Literal(value)
            })
        );
    }
}

#[test]
fn cosmetic_thread_keeps_standard_size_pitch_and_model_state() {
    let FeatureOperation::Thread(value) = project_value(
        "cosmetic_thread",
        json!({"face":native_selection(),"standard":"Synthetic","designation":"M4","diameter":4.0,"pitch":0.75,"modeled":false,"extent":{"kind":"blind","length":6.0}}),
    ) else {
        panic!("thread");
    };
    assert_eq!(
        value.spec.major_diameter,
        Some(Dimension::Literal(positive(4.0)))
    );
    assert_eq!(value.spec.pitch, Some(Dimension::Literal(positive(0.75))));
    assert_eq!(value.spec.modeled, Some(Dimension::Literal(false)));
    assert!(matches!(
        value.spec.length,
        Some(dressup_ops::ThreadLength::Blind(_))
    ));
}

#[test]
fn unresolved_and_reversing_operands_take_the_native_gate() {
    let mut operation = extrude();
    let LegacyOperation::Extrude { profile, .. } = &mut operation else {
        panic!("extrude");
    };
    *profile = cadmpeg_ir::features::profiles::ProfileRef::Planar(
        cadmpeg_ir::features::profiles::PlanarProfileRef::Unresolved(
            "inventor:pmdc:profile#synthetic".into(),
        ),
    );
    let mut ir = part(operation);
    assert_eq!(project_part(&mut ir).len(), 1);
    let hole = legacy(
        "hole",
        json!({"placements":[{"kind":"axis","origin":[0.0,0.0,0.0],"axis":[0.0,0.0,1.0]}],"shape":{"construction":{"kind":"form","value":{"kind":{"kind":"simple"}}},"diameter":2.0}}),
    );
    let mut ir = part(hole);
    assert_eq!(project_part(&mut ir).len(), 1);
}

#[test]
fn split_face_and_surface_trim_keep_every_selection() {
    let FeatureOperation::Split(value) = project_value(
        "split_face",
        json!({"targets":native_selection(),"tool":{"kind":"plane","value":{"plane":"inventor:design:feature#plane"}},"method":cadmpeg_core::serde_erase::json::to_value(cadmpeg_ir::features::surfaces::SplitFaceMethod::default()).expect("method")}),
    ) else {
        panic!("split face");
    };
    assert!(matches!(
        value.mode,
        body_ops::SplitMode::ByTools {
            tools: body_ops::SplitTool::Plane(_),
            kept: Some(body_ops::Kept::All {
                form: Some(body_ops::PieceForm::Faces)
            }),
            ..
        }
    ));
    let FeatureOperation::Split(value) = project_value(
        "trim_bodies",
        json!({"operands":{"targets":native_selection(),"tools":{"kind":"native","value":"inventor:pmdc:selection#tool"}},"keep":"unresolved"}),
    ) else {
        panic!("trim bodies");
    };
    assert!(matches!(
        value.mode,
        body_ops::SplitMode::ByTools {
            tools: body_ops::SplitTool::Geometry { extend: None, .. },
            kept: None,
            ..
        }
    ));
}

/// §6.5: a split body's tool extension is `extend` on its `Geometry` tool,
/// `Extended` with no law where the source stores one Boolean.
#[test]
fn split_body_faces_keep_their_tool_extension() {
    for (stored, extend) in [
        (true, body_ops::ToolExtension::Extended { method: None }),
        (false, body_ops::ToolExtension::Off),
    ] {
        let FeatureOperation::Split(value) = project_value(
            "split_body",
            json!({"targets":native_selection(),"tool":{"kind":"faces","value":native_selection()},"extend_tool":stored}),
        ) else {
            panic!("split body");
        };
        let body_ops::SplitMode::ByTools {
            tools: body_ops::SplitTool::Geometry { extend: found, .. },
            ..
        } = value.mode
        else {
            panic!("geometry tool");
        };
        assert_eq!(found, Some(extend));
    }
}

#[test]
fn face_motions_keep_signed_translation_and_rotation() {
    let FeatureOperation::MoveFaces(value) = project_value(
        "move_face",
        json!({"faces":native_selection(),"motion":{"kind":"translate","direction":[0.0,1.0,0.0],"distance":-3.0}}),
    ) else {
        panic!("move faces");
    };
    let [body_ops::TransformStep::Translate { distance, .. }] =
        value.transformation.steps.as_slice()
    else {
        panic!("translate");
    };
    assert_eq!(*distance, Dimension::Literal(length(-3.0)));
    let FeatureOperation::MoveFaces(value) = project_value(
        "move_face",
        json!({"faces":native_selection(),"motion":{"kind":"rotate","axis_origin":[1.0,2.0,3.0],"axis_dir":[0.0,0.0,-2.0],"angle":-0.4}}),
    ) else {
        panic!("move faces");
    };
    let [body_ops::TransformStep::Rotate { axis, angle }] = value.transformation.steps.as_slice()
    else {
        panic!("rotate");
    };
    assert_eq!(
        axis.resolved.as_ref().expect("axis").direction.as_raw().z,
        -1.0
    );
    assert_eq!(
        *angle,
        Dimension::Literal(cadmpeg_ir::scalar::Angle::new(-0.4).expect("angle"))
    );
}

#[test]
fn thicken_forward_and_reverse_sides_stay_distinct() {
    for (side, expected) in [
        ("forward", cadmpeg_ir::design::options::WallSide::Outside),
        ("reverse", cadmpeg_ir::design::options::WallSide::Inside),
    ] {
        let FeatureOperation::Thicken(value) = project_value(
            "thicken",
            json!({"faces":native_selection(),"thickness":2.0,"side":side}),
        ) else {
            panic!("thicken");
        };
        assert_eq!(
            value.thickness,
            Some(cadmpeg_ir::design::options::Thickness::OneSide {
                value: Dimension::Literal(positive(2.0)),
                side: Some(expected)
            })
        );
    }
}

#[test]
fn stored_geometry_without_separate_bodies_keeps_absent_content() {
    let mut ir = part(LegacyOperation::StoredGeometry {});
    assert!(project_part(&mut ir).is_empty());
    assert!(matches!(
        ir.design.features[0].definition.operation,
        FeatureOperation::Stored { bodies: None, .. }
    ));
}

#[test]
fn cleanup_and_boolean_tolerance_survive_projection() {
    let mut ir = part(extrude());
    ir.model.features[0].evaluation =
        FeatureEvaluation::from_definition(old::FeatureDefinition::PostProcess {
            operation: extrude(),
            refine: false,
            fuzzy_tolerance: cadmpeg_ir::features::operation_settings::FuzzyTolerance::Explicit(
                positive(0.2),
            ),
        });
    assert!(project_part(&mut ir).is_empty());
    assert_eq!(ir.design.features[0].definition.refine, Some(false));
    assert_eq!(
        ir.design.features[0].definition.fuzzy_tolerance,
        Some(cadmpeg_ir::design::feature::FuzzyTolerance::Explicit(
            Dimension::Literal(positive(0.2))
        ))
    );
}

#[test]
fn next_material_exit_stays_native_when_v11_has_a_first_face_limit() {
    let mut operation = extrude();
    let LegacyOperation::Extrude { extent, .. } = &mut operation else {
        panic!("extrude");
    };
    *extent = cadmpeg_ir::features::extents::ExtrudeExtent::OneSided {
        side: cadmpeg_ir::features::extents::ExtrudeSide {
            termination: cadmpeg_ir::features::extents::LinearTermination::ThroughNext {},
            draft: None,
        },
    };
    let mut ir = part(operation);
    assert_eq!(project_part(&mut ir).len(), 1);
    assert!(matches!(
        ir.design.features[0].definition.operation,
        FeatureOperation::Native { .. }
    ));
}

#[test]
fn unsupported_body_move_order_and_symmetric_indices_stay_native() {
    let move_body = legacy(
        "move_body",
        json!({"bodies":native_selection(),"translation":[1.0,0.0,0.0],"rotation":{"origin":[0.0,0.0,0.0],"direction":[0.0,0.0,1.0],"angle":0.5},"copies":0}),
    );
    let symmetric = legacy(
        "pattern",
        json!({"seeds":[{"kind":"feature","value":"inventor:design:feature#seed"}],"pattern":{"kind":"linear","direction":[1.0,0.0,0.0],"spacing":3.0,"count":3,"omitted_instances":[],"symmetric":true}}),
    );
    for operation in [move_body, symmetric] {
        let mut ir = part(operation);
        assert_eq!(project_part(&mut ir).len(), 1);
        assert!(matches!(
            ir.design.features[0].definition.operation,
            FeatureOperation::Native { .. }
        ));
    }
}

#[test]
fn whole_profile_regions_keep_outer_and_hole_members() {
    let sketch =
        cadmpeg_ir::sketches::SketchId::mint("inventor:design:sketch#synthetic").expect("sketch");
    let ids = ["outer", "hole"].map(|key| {
        cadmpeg_ir::sketches::SketchEntityId::mint(format!("inventor:design:entity#{key}"))
            .expect("entity")
    });
    let source = cadmpeg_ir::sketches::Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Unresolved {},
        profiles: ids
            .iter()
            .map(|id| {
                vec![cadmpeg_ir::sketches::SketchEntityUse {
                    entity: id.clone(),
                    reversed: false,
                }]
            })
            .collect::<Vec<_>>()
            .try_into()
            .expect("profiles"),
        native_ref: None,
    };
    let mut operation = extrude();
    let LegacyOperation::Extrude { profile, .. } = &mut operation else {
        panic!("extrude");
    };
    *profile = cadmpeg_ir::features::profiles::ProfileRef::Planar(
        cadmpeg_ir::features::profiles::PlanarProfileRef::sketch_regions(
            sketch.clone(),
            vec![
                cadmpeg_ir::features::profiles::SketchProfileRegion::loops(0, vec![1])
                    .expect("region"),
            ],
        )
        .expect("regions"),
    );
    let FeatureOperation::Extrude(value) = typed(
        &operation,
        selections::SketchData {
            sketches: &[source],
            entities: &[],
            arrangements: None,
        },
        &[],
        &[],
    )
    .expect("profile projection") else {
        panic!("extrude");
    };
    let cadmpeg_ir::design::selection::Picks::Named(picks) = value.profile.picks else {
        panic!("picks");
    };
    let [cadmpeg_ir::design::selection::Picked {
        target:
            cadmpeg_ir::design::selection::PickTarget::SketchRegion {
                sketch: selected,
                region,
            },
        ..
    }] = picks.as_slice()
    else {
        panic!("region pick");
    };
    assert_eq!(selected, &sketch);
    assert_eq!(region.outer[0].entity, ids[0]);
    assert_eq!(region.holes[0][0].entity, ids[1]);
}

#[test]
fn trimmed_profile_intervals_keep_native_operation_and_loss() {
    let sketch =
        cadmpeg_ir::sketches::SketchId::mint("inventor:design:sketch#synthetic").expect("sketch");
    let boundary = cadmpeg_ir::features::profiles::SketchProfileBoundaryUse {
        entity: cadmpeg_ir::sketches::SketchEntityId::mint("inventor:design:entity#synthetic")
            .expect("entity"),
        parameter_range: cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange::new([
            0.25, 0.75,
        ])
        .expect("interval"),
        reversed: true,
    };
    let region =
        cadmpeg_ir::features::profiles::SketchProfileRegion::trimmed(vec![boundary], Vec::new())
            .expect("region");
    let mut operation = extrude();
    let LegacyOperation::Extrude { profile, .. } = &mut operation else {
        panic!("extrude");
    };
    *profile = cadmpeg_ir::features::profiles::ProfileRef::Planar(
        cadmpeg_ir::features::profiles::PlanarProfileRef::sketch_regions(sketch, vec![region])
            .expect("regions"),
    );
    let mut ir = part(operation.clone());
    assert_eq!(project_part(&mut ir).len(), 1);
    assert_eq!(
        ir.design.features[0].definition.operation,
        retained(&operation).expect("native operands")
    );
}

fn contour_fixture(reversed: bool) -> (LegacyOperation, cadmpeg_ir::sketches::Sketch) {
    let sketch =
        cadmpeg_ir::sketches::SketchId::mint("inventor:design:sketch#synthetic").expect("sketch");
    let entity = cadmpeg_ir::sketches::SketchEntityId::mint("inventor:design:entity#synthetic")
        .expect("entity");
    let profile = cadmpeg_ir::features::profiles::PlanarProfileRef::sketch_entities(
        sketch.clone(),
        vec![entity],
    )
    .expect("profile");
    let operation = LegacyOperation::SheetMetalContourFlange {
        construction: old::sheet_metal::SheetMetalContourFlangeConstruction {
            profile: Some(profile),
            direction: Some(
                cadmpeg_ir::features::geometry::FeatureDirection3::new(
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, if reversed { -1.0 } else { 1.0 }),
                )
                .expect("direction"),
            ),
            extent: Some(cadmpeg_ir::features::extents::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::extents::ExtrudeSide {
                    termination: cadmpeg_ir::features::extents::LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(6.0).expect("length"),
                    },
                    draft: None,
                },
            }),
            thickness: Some(positive(2.0)),
            inside_radius: Some(cadmpeg_ir::scalar::NonNegativeLength::new(1.5).expect("radius")),
            side: Some(old::sheet_metal::SheetMetalContourThicknessSide::Right),
            neutral_factor: None,
        },
    };
    let source = cadmpeg_ir::sketches::Sketch {
        id: sketch,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("placement"),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    };
    (operation, source)
}

#[test]
fn contour_flange_direction_gates_signed_span_and_keeps_bend_operands() {
    for reversed in [false, true] {
        let (operation, sketch) = contour_fixture(reversed);
        let mut ir = part(operation);
        ir.model.sketches.push(sketch);
        assert!(project_part(&mut ir).is_empty());
        let FeatureOperation::SheetMetal(value) = &ir.design.features[0].definition.operation
        else {
            panic!("sheet metal");
        };
        let SheetMetalKind::ContourFlange {
            extent: Some(Span::Between { start, end }),
            thickness,
            bend,
            ..
        } = &value.kind
        else {
            panic!("contour flange");
        };
        let (expected_start, expected_end) = if reversed {
            (
                Limit::Amount(Dimension::Literal(length(-6.0))),
                Limit::AtOrigin,
            )
        } else {
            (
                Limit::AtOrigin,
                Limit::Amount(Dimension::Literal(length(6.0))),
            )
        };
        assert_eq!(**start, expected_start);
        assert_eq!(**end, expected_end);
        assert_eq!(
            *thickness,
            Some(cadmpeg_ir::design::options::Thickness::OneSide {
                value: Dimension::Literal(positive(2.0)),
                side: Some(cadmpeg_ir::design::options::WallSide::Right)
            })
        );
        assert_eq!(bend.radius, Some(Dimension::Literal(positive(1.5))));
        assert_eq!(
            bend.radius_side,
            Some(cadmpeg_ir::design::ops::sheet_metal::RadiusSide::Inside)
        );
    }
}

#[test]
fn contour_flange_oblique_direction_and_zero_radius_stay_native() {
    for oblique in [false, true] {
        let (mut operation, sketch) = contour_fixture(false);
        let LegacyOperation::SheetMetalContourFlange { construction } = &mut operation else {
            panic!("contour");
        };
        if oblique {
            construction.direction = Some(
                cadmpeg_ir::features::geometry::FeatureDirection3::new(
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("direction"),
            );
        } else {
            construction.inside_radius =
                Some(cadmpeg_ir::scalar::NonNegativeLength::new(0.0).expect("radius"));
        }
        let mut ir = part(operation);
        ir.model.sketches.push(sketch);
        let losses = project_part(&mut ir);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            InventorLossCode::FeatureDesignOperationNative.kind()
        );
    }
}

#[test]
fn extrude_plane_targets_keep_frames_native_names_and_source_modes() {
    let plane = cadmpeg_ir::features::geometry::FeatureSupportPlaneFrame::new(
        cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("plane");
    let mut operation = extrude();
    let LegacyOperation::Extrude { start, extent, .. } = &mut operation else {
        panic!("extrude");
    };
    *start = cadmpeg_ir::features::extents::ExtrudeStart::FromPlane {
        plane,
        native: cadmpeg_core::nonblank_literal!("inventor:pmdc:plane#start"),
        source: Some(cadmpeg_ir::features::extents::PlaneTargetSource::WorkPlane),
    };
    *extent = cadmpeg_ir::features::extents::ExtrudeExtent::OneSided {
        side: cadmpeg_ir::features::extents::ExtrudeSide {
            termination: cadmpeg_ir::features::extents::LinearTermination::ToPlane {
                plane,
                native: cadmpeg_core::nonblank_literal!("inventor:pmdc:plane#end"),
                source: Some(cadmpeg_ir::features::extents::PlaneTargetSource::FaceSurface),
            },
            draft: None,
        },
    };
    let FeatureOperation::Extrude(value) =
        typed(&operation, selections::SketchData::default(), &[], &[]).expect("extrude")
    else {
        panic!("extrude");
    };
    let Some(cadmpeg_ir::design::ops::swept::ExtrudeOrigin::Plane(start)) = value.origin else {
        panic!("origin");
    };
    assert_eq!(
        start.resolved.as_ref().expect("frame").origin,
        plane.origin()
    );
    assert_eq!(
        start.mode.expect("source").method,
        cadmpeg_ir::design::names::NativeNameValue::Text(cadmpeg_core::nonblank_literal!(
            "work_plane"
        ))
    );
    let Some(Span::Between { end, .. }) = value.span else {
        panic!("span");
    };
    let Limit::ToPlane(end) = *end else {
        panic!("plane limit");
    };
    assert_eq!(end.resolved.as_ref().expect("frame").origin, plane.origin());
    assert_eq!(
        end.mode.expect("source").method,
        cadmpeg_ir::design::names::NativeNameValue::Text(cadmpeg_core::nonblank_literal!(
            "face_surface"
        ))
    );
}

#[test]
fn split_face_path_keeps_native_projection_controls() {
    let operation = LegacyOperation::SplitFace {
        targets: cadmpeg_ir::features::selections::FaceSelection::Native(
            "inventor:pmdc:selection#synthetic".into(),
        ),
        tool: cadmpeg_ir::features::direct_edit::SplitFaceTool::Path(
            cadmpeg_ir::features::profiles::PathRef::Native("inventor:pmdc:path#synthetic".into()),
        ),
        method: cadmpeg_ir::features::surfaces::SplitFaceMethod::default(),
    };
    let mut ir = part(operation);
    let losses = project_part(&mut ir);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        InventorLossCode::FeatureDesignOperationNative.kind()
    );
}

mod review;
