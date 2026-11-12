// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::project_hole;
use crate::design::test_support::parameter_record;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};

const EPS_HOLE_EXTENT_TEST: f64 = 1.0e-12;

fn hole_parameters() -> [crate::records::parameters::DesignParameter; 3] {
    let mut tip = parse_design_parameter_record(&parameter_record(
        Some(114),
        "180 deg",
        "TipAngle",
        Some("mm"),
        "d6",
        std::f64::consts::PI,
    ))
    .unwrap();
    tip.try_set_unit_value("deg".to_owned()).unwrap();
    [
        parse_design_parameter_record(&parameter_record(
            Some(94),
            "10 mm",
            "HoleDepth",
            Some("mm"),
            "d4",
            1.0,
        ))
        .unwrap(),
        parse_design_parameter_record(&parameter_record(
            Some(104),
            "4 mm",
            "HoleDiameter",
            Some("mm"),
            "d5",
            0.4,
        ))
        .unwrap(),
        tip,
    ]
}

#[test]
fn hole_without_face_operand_falls_back_to_scope_face() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 0, false);
    let parameters = hole_parameters();
    let indexed = [
        (0, &parameters[0]),
        (1, &parameters[1]),
        (2, &parameters[2]),
    ];
    let definition = project_hole(&scope, &indexed, &[], &[], &mut Vec::new())
        .unwrap()
        .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Hole {
            face: Some(FaceSelection::Native(id)), ..
        }) if id == scope.id
    ));
}

fn set_extent(scope: &mut DesignParameterScope, control: u8, saved: bool) {
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        slot.extent = Some(crate::records::feature::hole::DesignHoleExtent {
            leading_word: None,
            extent_control: Some(control),
            saved_non_distance_depth: saved.then(|| crate::records::identity::Located {
                value: cadmpeg_ir::scalar::FiniteReal::try_from(1.0).unwrap(),
                offset: 5,
            }),
            direction_control: Some(crate::records::identity::Located {
                value: 0,
                offset: 6,
            }),
            standard: None,
            hole_type: None,
        });
    }
}

fn set_direction_control(scope: &mut DesignParameterScope, value: Option<u8>) {
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        if let Some(extent) = &mut slot.extent {
            extent.direction_control =
                value.map(|value| crate::records::identity::Located { value, offset: 6 });
        }
    }
}

fn set_carrier(
    scope: &mut DesignParameterScope,
    reference_type: u32,
    position: [f64; 3],
    locations: &[(u32, [f64; 3])],
    face_selection: bool,
) {
    let locations: Vec<_> = locations
        .iter()
        .enumerate()
        .map(|(index, (reference_type, position))| {
            serde_json::json!({
                "record_index": index, "position": position,
                "position_offset": index, "reference_type": reference_type
            })
        })
        .collect();
    let mut construction = serde_json::json!({
        "point_record_index": 1, "point_record_byte_offset": 2,
        "position": position, "position_offset": 3,
        "direction": [0.0, 0.0, 1.0], "direction_offset": 4,
        "point_parameters": [0.0, 0.0], "point_parameter_offsets": [5, 6],
        "reference_type": reference_type, "reference_type_offset": 7,
        "input_record_indices": [8], "input_record_offsets": [9],
        "locations": locations
    });
    if face_selection {
        construction["face_selection"] = serde_json::json!({
            "record_index": 1, "byte_offset": 2, "class_tag": "451",
            "asset_id": "11111111-1111-1111-1111-111111111111",
            "asset_id_offset": 3,
            "context_id": "22222222-2222-2222-2222-222222222222",
            "context_id_offset": 4,
            "identity_record_index": 5, "identity_record_offset": 6,
            "primary_identity": 7, "primary_identity_offset": 8,
            "next_record_index": 9, "next_byte_offset": 10
        });
    }
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        slot.point = Some(serde_json::from_value(construction).unwrap());
    }
}

fn projected_placements(
    scope: &DesignParameterScope,
) -> (
    Option<Vec<cadmpeg_ir::features::holes::HolePlacement>>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let parameters = hole_parameters();
    let indexed = [
        (0, &parameters[0]),
        (1, &parameters[1]),
        (2, &parameters[2]),
    ];
    let mut losses = Vec::new();
    let definition = project_hole(scope, &indexed, &[], &[], &mut losses)
        .unwrap()
        .unwrap();
    let FeatureDefinition::Operation(FeatureOperation::Hole { placements, .. }) = definition else {
        panic!("Hole definition");
    };
    (placements, losses)
}

fn directed(position: [f64; 3]) -> cadmpeg_ir::features::holes::HolePlacement {
    directed_along(position, -1.0)
}

fn directed_along(position: [f64; 3], z: f64) -> cadmpeg_ir::features::holes::HolePlacement {
    cadmpeg_ir::features::holes::HolePlacement::Directed {
        position: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            position[0],
            position[1],
            position[2],
        ))
        .unwrap(),
        direction: cadmpeg_ir::features::FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(
            0.0, 0.0, z,
        ))
        .unwrap(),
    }
}

#[test]
fn hole_placements_come_from_the_carrier_form_in_millimetres_against_the_normal() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 0, false);

    set_carrier(
        &mut scope,
        13,
        [0.0; 3],
        &[(10, [1.0, 1.5, 2.0]), (10, [3.0, 1.5, 2.0])],
        true,
    );
    let (placements, losses) = projected_placements(&scope);
    assert_eq!(
        placements,
        Some(vec![
            directed([10.0, 15.0, 20.0]),
            directed([30.0, 15.0, 20.0])
        ])
    );
    assert!(losses.is_empty());

    set_carrier(
        &mut scope,
        19,
        [1.0, 1.5, 2.0],
        &[(17, [1.0, 1.5, 2.0])],
        false,
    );
    let (placements, losses) = projected_placements(&scope);
    assert_eq!(placements, Some(vec![directed([10.0, 15.0, 20.0])]));
    assert!(losses.is_empty());
}

#[test]
fn hole_direction_control_selects_the_travel_of_every_placement() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 0, false);
    set_carrier(
        &mut scope,
        13,
        [0.0; 3],
        &[(10, [1.0, 1.5, 2.0]), (10, [3.0, 1.5, 2.0])],
        true,
    );
    set_direction_control(&mut scope, Some(1));
    let (placements, losses) = projected_placements(&scope);
    assert_eq!(
        placements,
        Some(vec![
            directed_along([10.0, 15.0, 20.0], 1.0),
            directed_along([30.0, 15.0, 20.0], 1.0)
        ])
    );
    assert!(losses.is_empty());

    set_direction_control(&mut scope, None);
    let (placements, losses) = projected_placements(&scope);
    assert_eq!(
        placements,
        Some(vec![axis([10.0, 15.0, 20.0]), axis([30.0, 15.0, 20.0])])
    );
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::HoleCarrierAxisAssumed.kind()
    );
}

fn axis(position: [f64; 3]) -> cadmpeg_ir::features::holes::HolePlacement {
    let cadmpeg_ir::features::holes::HolePlacement::Directed {
        position,
        direction,
    } = directed_along(position, 1.0)
    else {
        panic!("directed fixture")
    };
    cadmpeg_ir::features::holes::HolePlacement::Axis {
        origin: position,
        axis: direction,
    }
}

fn assert_unresolved_extent_retains_bore_and_bottom(definition: Option<FeatureDefinition>) {
    use cadmpeg_ir::features::holes::{HoleBottom, HoleConstruction, HoleKind};
    let Some(FeatureDefinition::Operation(FeatureOperation::Hole {
        extent: Some(cadmpeg_ir::features::LinearTermination::Unresolved {}),
        shape,
        bottom,
        ..
    })) = definition
    else {
        panic!("partial Hole")
    };
    assert_eq!(shape.diameter().unwrap().get(), 4.0);
    assert_eq!(
        shape.construction(),
        &HoleConstruction::form(HoleKind::Simple)
    );
    assert_eq!(bottom, Some(HoleBottom::Flat));
}

/// Carrier `refType`, carrier position, and point records.
type CarrierCase = (u32, [f64; 3], &'static [(u32, [f64; 3])]);

#[test]
fn hole_placement_without_an_admitted_form_stays_native_with_its_loss() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 0, false);
    let cases: [CarrierCase; 4] = [
        (13, [0.0; 3], &[]),
        (
            13,
            [0.0; 3],
            &[(10, [1.0, 1.5, 2.0]), (17, [3.0, 1.5, 2.0])],
        ),
        (19, [1.0, 1.5, 2.0], &[(17, [3.0, 1.5, 2.0])]),
        (18, [1.0, 1.5, 2.0], &[(17, [1.0, 1.5, 2.0])]),
    ];
    for (reference_type, position, locations) in cases {
        set_carrier(&mut scope, reference_type, position, locations, false);
        let (placements, losses) = projected_placements(&scope);
        assert_eq!(placements, None);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::HolePlacementUnproven.kind()
        );
    }
}

#[test]
fn hole_without_a_carrier_reports_its_unproved_placement() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 0, false);
    let (placements, losses) = projected_placements(&scope);
    assert_eq!(placements, None);
    assert_eq!(losses.len(), 1);
}

#[test]
fn through_all_admits_a_carrier_selection_only_on_a_sketch_point_carrier() {
    let parameters = hole_parameters();
    let indexed = [(1, &parameters[1]), (2, &parameters[2])];
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 1, true);

    set_carrier(&mut scope, 13, [0.0; 3], &[(10, [1.0, 1.5, 2.0])], true);
    let definition = project_hole(&scope, &indexed, &[], &[], &mut Vec::new())
        .unwrap()
        .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Hole {
            extent: Some(cadmpeg_ir::features::LinearTermination::ThroughAll {}),
            ..
        })
    ));

    set_carrier(
        &mut scope,
        19,
        [1.0, 1.5, 2.0],
        &[(17, [1.0, 1.5, 2.0])],
        true,
    );
    let mut losses = Vec::new();
    assert_unresolved_extent_retains_bore_and_bottom(
        project_hole(&scope, &indexed, &[], &[], &mut losses).unwrap(),
    );
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::HoleExtentUnproven.kind()
    );
}

fn face_operand(
    record_index: u32,
    ordinal: u32,
) -> crate::records::topology::face::DesignFaceOperand {
    use crate::records::topology::face::{DesignFaceOperand, DesignFaceOperandDraft};
    DesignFaceOperand::try_new(DesignFaceOperandDraft {
        id: format!("f3d:Design/BulkStream.dat:face-operand#{record_index}"),
        scope_record_index: 1,
        scope_reference_ordinal: ordinal,
        group: None,
        record_index,
        byte_offset: 10,
        class_tag: "256".to_owned().try_into().unwrap(),
        paired_byte_offset: 30,
        paired_class_tag: "257".to_owned().try_into().unwrap(),
        recipe_record_index: record_index + 3,
        recipe_record_byte_offset: 40,
        recipe_id: format!(
            "f3d:Design/BulkStream.dat:construction-recipe#{}",
            record_index + 3
        ),
        recipe_prefix_offset: 51,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: crate::records::recipes::ConstructionRecipeKind::BoundedFace,
        recipe_program_offset: 60,
        recipe_program: vec![0, -1],
        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: record_index + 20,
        next_byte_offset: 70,
    })
    .unwrap()
}

#[test]
fn hole_extent_projection_checks_distance_shoulder_and_through_all() {
    use cadmpeg_ir::features::{
        holes::{HoleBottom, HoleConstruction, HoleKind},
        LinearTermination,
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#1",
        DesignFeatureKind::Hole,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::located(vec![
                crate::records::identity::Located {
                    value: 2,
                    offset: 1,
                },
                crate::records::identity::Located {
                    value: 3,
                    offset: 2,
                },
            ]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let faces = vec![face_operand(2, 0), face_operand(3, 1)];
    let mut parameters = hole_parameters().to_vec();
    for drilled in [false, true] {
        if drilled {
            parameters[2].try_set_evaluated_value(2.0).unwrap();
        }
        set_extent(&mut scope, 0, false);
        let indexed: Vec<_> = parameters
            .iter()
            .enumerate()
            .map(|(i, p)| (u32::try_from(i).unwrap(), p))
            .collect();
        let definition = project_hole(&scope, &indexed, &[], &faces, &mut Vec::new())
            .unwrap()
            .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Hole { extent, bottom, .. }) =
            definition
        else {
            panic!()
        };
        assert!(
            matches!(extent, Some(LinearTermination::Blind {length}) if (length.get()-10.0).abs() < EPS_HOLE_EXTENT_TEST)
        );
        if drilled {
            assert!(matches!(
                bottom,
                Some(HoleBottom::Angled {
                    depth_to_tip: false,
                    ..
                })
            ));
        } else {
            assert_eq!(bottom, Some(HoleBottom::Flat));
        }
    }
    parameters.remove(0);
    for counterbore in [false, true] {
        if counterbore {
            for (name, value) in [("CBDepth", 0.3), ("CBDiameter", 1.0)] {
                parameters.push(
                    parse_design_parameter_record(&parameter_record(
                        Some(5),
                        "1 mm",
                        name,
                        Some("mm"),
                        "d1",
                        value,
                    ))
                    .unwrap(),
                );
            }
        }
        set_extent(&mut scope, 1, true);
        let indexed: Vec<_> = parameters
            .iter()
            .enumerate()
            .map(|(i, p)| (u32::try_from(i).unwrap(), p))
            .collect();
        let definition = project_hole(&scope, &indexed, &[], &faces, &mut Vec::new())
            .unwrap()
            .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Hole { extent, shape, .. }) = definition
        else {
            panic!()
        };
        assert_eq!(extent, Some(LinearTermination::ThroughAll {}));
        assert_eq!(
            super::super::hole_termination(&scope, &indexed, &[], &[]),
            Some(LinearTermination::ThroughAll {})
        );
        assert_eq!(
            matches!(
                shape.construction(),
                HoleConstruction::Form {
                    kind: HoleKind::CounterboreDrilled { .. },
                    ..
                }
            ),
            counterbore
        );
    }
}

#[test]
fn hole_unproved_extent_has_loss_for_depth_extra_face_and_missing_saved_depth() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#1",
        DesignFeatureKind::Hole,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::located(vec![
                crate::records::identity::Located {
                    value: 2,
                    offset: 1,
                },
                crate::records::identity::Located {
                    value: 3,
                    offset: 2,
                },
            ]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    set_extent(&mut scope, 1, true);
    let parameters = hole_parameters();
    let indexed: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (u32::try_from(i).unwrap(), p))
        .collect();
    let faces = [face_operand(2, 0), face_operand(3, 1), face_operand(5, 2)];
    for (parameters, faces) in [(&indexed[..], &faces[..2]), (&indexed[1..], &faces[..])] {
        assert!(super::super::hole_termination(&scope, parameters, &[], faces).is_none());
        let mut losses = Vec::new();
        assert_unresolved_extent_retains_bore_and_bottom(
            project_hole(&scope, parameters, &[], faces, &mut losses).unwrap(),
        );
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::HoleExtentUnproven.kind()
        );
    }
    set_extent(&mut scope, 1, false);
    assert_unresolved_extent_retains_bore_and_bottom(
        project_hole(&scope, &indexed[1..], &[], &faces[..2], &mut Vec::new()).unwrap(),
    );
}

#[test]
fn hole_without_an_extent_control_is_distance_only_with_its_depth() {
    use cadmpeg_ir::features::LinearTermination;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#1",
        DesignFeatureKind::Hole,
        1,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        slot.extent = Some(crate::records::feature::hole::DesignHoleExtent {
            leading_word: None,
            extent_control: None,
            saved_non_distance_depth: None,
            direction_control: None,
            standard: None,
            hole_type: None,
        });
    }
    let parameters = hole_parameters();
    let indexed: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (u32::try_from(i).unwrap(), p))
        .collect();
    assert!(matches!(
        super::super::hole_termination(&scope, &indexed, &[], &[]),
        Some(LinearTermination::Blind { .. })
    ));
    let without_depth: Vec<_> = indexed
        .iter()
        .copied()
        .filter(|(_, parameter)| parameter.source_kind() != "HoleDepth")
        .collect();
    assert_eq!(without_depth.len(), indexed.len() - 1);
    assert_eq!(
        super::super::hole_termination(&scope, &without_depth, &[], &[]),
        None
    );
    let mut losses = Vec::new();
    assert_unresolved_extent_retains_bore_and_bottom(
        project_hole(&scope, &without_depth, &[], &[], &mut losses).unwrap(),
    );
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::HoleExtentUnproven.kind()
    );
}

fn hole_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    set_extent(&mut scope, 0, false);
    scope
}

/// A length parameter in centimetres or an angle parameter in radians.
fn named_parameter(
    name: &str,
    value: f64,
    angle: bool,
) -> crate::records::parameters::DesignParameter {
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(5),
        "1",
        name,
        Some("mm"),
        "d1",
        value,
    ))
    .unwrap();
    if angle {
        parameter.try_set_unit_value("deg".to_owned()).unwrap();
    }
    parameter
}

fn projected_hole(
    scope: &DesignParameterScope,
    parameters: &[crate::records::parameters::DesignParameter],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
) -> Option<FeatureOperation> {
    let indexed: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (u32::try_from(i).unwrap(), p))
        .collect();
    match project_hole(scope, &indexed, groups, &[], &mut Vec::new()).unwrap()? {
        FeatureDefinition::Operation(operation) => Some(operation),
        FeatureDefinition::PostProcess { .. } => panic!("expected an operation"),
    }
}

fn countersink(diameter: f64, angle: f64) -> Vec<crate::records::parameters::DesignParameter> {
    let mut parameters = hole_parameters().to_vec();
    parameters.push(named_parameter("CSDiameter", diameter, false));
    parameters.push(named_parameter("CSAngle", angle, true));
    parameters
}

#[test]
fn countersink_parameters_state_a_countersink_entry() {
    use cadmpeg_ir::features::holes::{HoleBottom, HoleConstruction, HoleKind};
    for (tip, bottom_is_flat) in [(std::f64::consts::PI, true), (118_f64.to_radians(), false)] {
        let mut parameters = countersink(1.0, std::f64::consts::FRAC_PI_2);
        parameters[2].try_set_evaluated_value(tip).unwrap();
        let Some(FeatureOperation::Hole { shape, bottom, .. }) =
            projected_hole(&hole_scope(), &parameters, &[])
        else {
            panic!("expected a hole");
        };
        let HoleConstruction::Form {
            kind: HoleKind::Countersink { diameter, angle },
            ..
        } = shape.construction()
        else {
            panic!("expected a countersink, got {:?}", shape.construction());
        };
        assert!((diameter.get() - 10.0).abs() < EPS_HOLE_EXTENT_TEST);
        assert!((angle.get() - std::f64::consts::FRAC_PI_2).abs() < EPS_HOLE_EXTENT_TEST);
        assert_eq!(matches!(bottom, Some(HoleBottom::Flat)), bottom_is_flat);
        assert_eq!(
            matches!(
                bottom,
                Some(HoleBottom::Angled {
                    depth_to_tip: false,
                    ..
                })
            ),
            !bottom_is_flat
        );
    }
}

#[test]
fn incompatible_countersink_parameters_retain_the_bore_as_a_partial_shape() {
    for parameters in [
        countersink(0.4, std::f64::consts::FRAC_PI_2),
        countersink(0.3, std::f64::consts::FRAC_PI_2),
        countersink(1.0, std::f64::consts::PI),
    ] {
        let Some(FeatureOperation::Hole { shape, bottom, .. }) =
            projected_hole(&hole_scope(), &parameters, &[])
        else {
            panic!("partial Hole")
        };
        assert_eq!(shape.diameter().unwrap().get(), 4.0);
        assert!(
            matches!(shape.construction(), cadmpeg_ir::features::holes::HoleConstruction::Form { kind, .. } if kind.is_unresolved())
        );
        assert_eq!(bottom, Some(cadmpeg_ir::features::holes::HoleBottom::Flat));
    }
}

#[test]
fn mixed_entry_parameters_retain_independent_bore_and_tip_values() {
    let mut parameters = hole_parameters().to_vec();
    parameters.push(named_parameter("CBDepth", 0.3, false));
    parameters.push(named_parameter(
        "CSAngle",
        std::f64::consts::FRAC_PI_2,
        true,
    ));
    let Some(FeatureOperation::Hole { shape, bottom, .. }) =
        projected_hole(&hole_scope(), &parameters, &[])
    else {
        panic!("partial Hole")
    };
    assert_eq!(shape.diameter().unwrap().get(), 4.0);
    assert_eq!(
        shape.construction(),
        &cadmpeg_ir::features::holes::HoleConstruction::form(
            cadmpeg_ir::features::holes::HoleKind::Unresolved(None)
        )
    );
    assert_eq!(bottom, Some(cadmpeg_ir::features::holes::HoleBottom::Flat));
    let mut parameters = hole_parameters().to_vec();
    parameters[2]
        .try_set_evaluated_value(118_f64.to_radians())
        .unwrap();
    parameters.push(named_parameter("CSDiameter", 1.0, false));
    parameters.push(named_parameter("CBDiameter", 1.0, false));
    let Some(FeatureOperation::Hole { shape, bottom, .. }) =
        projected_hole(&hole_scope(), &parameters, &[])
    else {
        panic!("partial Hole")
    };
    assert_eq!(shape.diameter().unwrap().get(), 4.0);
    assert!(matches!(
        shape.construction(),
        cadmpeg_ir::features::holes::HoleConstruction::Form {
            kind: cadmpeg_ir::features::holes::HoleKind::Unresolved(None),
            ..
        }
    ));
    assert!(
        matches!(bottom, Some(cadmpeg_ir::features::holes::HoleBottom::Angled { included_angle, depth_to_tip: false }) if included_angle.get() == 118_f64.to_radians())
    );
}

fn extent_group(
    role: crate::records::topology::extrude_selection::DesignOperandRole,
    members: &[u32],
) -> crate::records::topology::construction::DesignConstructionOperandGroup {
    let member_offsets: Vec<u64> = (0..u64::try_from(members.len()).unwrap())
        .map(|ordinal| 1026 + ordinal * 11)
        .collect();
    serde_json::from_value(serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:group#40",
        "scope_record_index": 32,
        "scope_reference_ordinal": 8,
        "record_index": 40,
        "byte_offset": 1000,
        "class_tag": "332",
        "members": members,
        "member_offsets": member_offsets,
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": role.raw(),
        "role_offset": 1054,
        "paired_class_tag": "259",
        "paired_byte_offset": 1125
    }))
    .unwrap()
}

#[test]
fn an_extents_to_hole_ends_on_the_face_of_its_extent_group() {
    use crate::records::topology::extrude_selection::DesignOperandRole;
    use cadmpeg_ir::features::LinearTermination;
    let mut scope = hole_scope();
    set_extent(&mut scope, 0, true);
    let without_depth = hole_parameters()[1..].to_vec();
    let group = extent_group(DesignOperandRole::ROLE_0X5, &[7]);
    let Some(FeatureOperation::Hole { extent, .. }) =
        projected_hole(&scope, &without_depth, std::slice::from_ref(&group))
    else {
        panic!("expected a hole");
    };
    assert_eq!(
        extent,
        Some(LinearTermination::ToFace {
            face: FaceSelection::Native(group.id.clone()),
            offset: None,
        })
    );
}

fn extent_member(slots: Vec<i64>) -> crate::records::topology::face::DesignFaceOperand {
    let mut operand = face_operand(7, 9);
    operand.scope_record_index = 32;
    operand.recipe_kind = crate::records::recipes::ConstructionRecipeKind::Face;
    operand.resolved_face_slots = slots;
    operand
}

fn extents_to_scope() -> DesignParameterScope {
    let mut scope = hole_scope();
    set_extent(&mut scope, 0, true);
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(3);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn an_extents_to_hole_ends_at_the_one_face_its_member_resolves() {
    use crate::records::topology::extrude_selection::DesignOperandRole;
    let scope = extents_to_scope();
    let groups = [extent_group(DesignOperandRole::ROLE_0X5, &[7])];
    let face = |operands: &[crate::records::topology::face::DesignFaceOperand]| {
        super::super::hole_extent_face(&scope, &groups, operands).expect("extent face")
    };
    let FaceSelection::Historical { faces, native, .. } = face(&[extent_member(vec![63])]) else {
        panic!("expected a historical extent face");
    };
    assert_eq!(faces.len(), 1);
    assert_eq!(native.as_str(), groups[0].id);

    let native_group = FaceSelection::Native(groups[0].id.clone());
    assert_eq!(face(&[]), native_group);
    assert_eq!(face(&[extent_member(Vec::new())]), native_group);
    assert_eq!(face(&[extent_member(vec![63, 41])]), native_group);
    let mut bounded = extent_member(vec![63]);
    bounded.recipe_kind = crate::records::recipes::ConstructionRecipeKind::BoundedFace;
    assert_eq!(face(&[bounded]), native_group);
    assert_eq!(
        face(&[extent_member(vec![63]), extent_member(vec![63])]),
        native_group
    );
    let mut stateless = hole_scope();
    set_extent(&mut stateless, 0, true);
    assert_eq!(
        super::super::hole_extent_face(&stateless, &groups, &[extent_member(vec![63])]),
        Some(native_group)
    );
}

#[test]
fn a_resolved_extents_to_face_reports_its_assumption() {
    use crate::records::topology::extrude_selection::DesignOperandRole;
    let scope = extents_to_scope();
    let parameters = hole_parameters()[1..].to_vec();
    let indexed: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (u32::try_from(i).unwrap(), p))
        .collect();
    let groups = [extent_group(DesignOperandRole::ROLE_0X5, &[7])];
    let mut losses = Vec::new();
    project_hole(
        &scope,
        &indexed,
        &groups,
        &[extent_member(vec![63])],
        &mut losses,
    )
    .unwrap()
    .expect("hole");
    assert!(losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::HoleExtentFaceAssumed.kind()));
}

#[test]
fn an_extents_to_hole_needs_one_extent_group_with_one_member() {
    use crate::records::topology::extrude_selection::DesignOperandRole;
    let mut scope = hole_scope();
    set_extent(&mut scope, 0, true);
    let without_depth = hole_parameters()[1..].to_vec();
    for groups in [
        Vec::new(),
        vec![extent_group(DesignOperandRole::ROLE_0X5, &[7, 8])],
        vec![extent_group(DesignOperandRole::BODIES_A, &[7])],
        vec![
            extent_group(DesignOperandRole::ROLE_0X5, &[7]),
            extent_group(DesignOperandRole::ROLE_0X5, &[8]),
        ],
    ] {
        let Some(FeatureOperation::Hole { shape, extent, .. }) =
            projected_hole(&scope, &without_depth, &groups)
        else {
            panic!("partial Hole")
        };
        assert_eq!(shape.diameter().unwrap().get(), 4.0);
        assert_eq!(
            extent,
            Some(cadmpeg_ir::features::LinearTermination::Unresolved {})
        );
    }
}

#[test]
fn a_hole_depth_or_an_unsaved_depth_keeps_the_distance_extent() {
    use crate::records::topology::extrude_selection::DesignOperandRole;
    use cadmpeg_ir::features::LinearTermination;
    let group = extent_group(DesignOperandRole::ROLE_0X5, &[7]);
    let mut saved = hole_scope();
    set_extent(&mut saved, 0, true);
    let Some(FeatureOperation::Hole { extent, .. }) =
        projected_hole(&saved, &hole_parameters(), std::slice::from_ref(&group))
    else {
        panic!("expected a hole");
    };
    assert!(matches!(extent, Some(LinearTermination::Blind { .. })));
    let Some(FeatureOperation::Hole { shape, extent, .. }) = projected_hole(
        &hole_scope(),
        &hole_parameters()[1..],
        std::slice::from_ref(&group),
    ) else {
        panic!("partial Hole")
    };
    assert_eq!(shape.diameter().unwrap().get(), 4.0);
    assert_eq!(extent, Some(LinearTermination::Unresolved {}));
}

/// A Through All scope whose reference entries 2 to 7 are records 12 to 17.
fn through_all_scope(reference_type: u32) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#1",
        DesignFeatureKind::Hole,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::located(
                (10..18)
                    .map(|value| crate::records::identity::Located {
                        value,
                        offset: u64::from(value - 9),
                    })
                    .collect(),
            );
            draft.frame_length = 400;
            draft.paired_byte_offset = 400;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    set_extent(&mut scope, 1, true);
    set_carrier(
        &mut scope,
        reference_type,
        [1.0, 1.5, 2.0],
        &[(17, [1.0, 1.5, 2.0])],
        false,
    );
    scope
}

fn through_all(
    scope: &DesignParameterScope,
    faces: &[crate::records::topology::face::DesignFaceOperand],
) -> bool {
    use cadmpeg_ir::features::LinearTermination;
    let parameters = hole_parameters();
    let indexed = [(1, &parameters[1]), (2, &parameters[2])];
    matches!(
        project_hole(scope, &indexed, &[], faces, &mut Vec::new()).unwrap(),
        Some(FeatureDefinition::Operation(FeatureOperation::Hole {
            extent: Some(LinearTermination::ThroughAll {}),
            ..
        }))
    )
}

fn start_operands(ordinals: &[u32]) -> Vec<crate::records::topology::face::DesignFaceOperand> {
    ordinals
        .iter()
        .map(|&ordinal| face_operand(10 + ordinal, ordinal))
        .collect()
}

#[test]
fn a_face_point_through_all_hole_admits_four_start_operands() {
    use crate::records::recipes::ConstructionRecipeKind;
    let scope = through_all_scope(19);
    assert!(through_all(&scope, &start_operands(&[2, 3, 4, 5])));
    let mut face_first = start_operands(&[2, 3, 4, 5]);
    face_first[0].recipe_kind = ConstructionRecipeKind::Face;
    assert!(through_all(&scope, &face_first));
}

#[test]
fn four_start_operands_need_a_face_point_carrier_and_one_consecutive_run() {
    use crate::records::recipes::ConstructionRecipeKind;
    assert!(!through_all(
        &through_all_scope(13),
        &start_operands(&[2, 3, 4, 5])
    ));
    let scope = through_all_scope(19);
    assert!(!through_all(&scope, &start_operands(&[2, 3, 4, 6])));
    assert!(!through_all(&scope, &start_operands(&[2, 3, 4, 5, 6])));
    assert!(!through_all(&scope, &start_operands(&[2, 3, 3, 4, 5])));
    assert!(!through_all(&scope, &start_operands(&[2, 3, 4])));
    let mut face_second = start_operands(&[2, 3, 4, 5]);
    face_second[1].recipe_kind = ConstructionRecipeKind::Face;
    assert!(!through_all(&scope, &face_second));
    let mut unbound = start_operands(&[2, 3, 4, 5]);
    unbound[3] = face_operand(30, 5);
    assert!(!through_all(&scope, &unbound));
}

fn reference_dimension_hole(
    dimensions: &[(&str, f64)],
    edge_groups: usize,
) -> (
    Option<FeatureOperation>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    use crate::records::topology::extrude_selection::DesignOperandRole;
    let mut parameters = hole_parameters().to_vec();
    for (name, value) in dimensions {
        parameters.push(named_parameter(name, *value, false));
    }
    let groups: Vec<_> = (0..edge_groups)
        .map(|_| extent_group(DesignOperandRole::ROLE_0X21, &[7]))
        .collect();
    let indexed: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (u32::try_from(i).unwrap(), p))
        .collect();
    let mut losses = Vec::new();
    let operation = match project_hole(&hole_scope(), &indexed, &groups, &[], &mut losses).unwrap()
    {
        Some(FeatureDefinition::Operation(operation)) => Some(operation),
        Some(FeatureDefinition::PostProcess { .. }) => panic!("expected an operation"),
        None => None,
    };
    (operation, losses)
}

#[test]
fn a_hole_placed_by_two_reference_dimensions_keeps_them_native() {
    use cadmpeg_ir::features::LinearTermination;
    let (operation, losses) = reference_dimension_hole(&[("RefDim1", 1.0), ("RefDim2", 1.0)], 2);
    let Some(FeatureOperation::Hole { extent, .. }) = operation else {
        panic!("expected a hole");
    };
    assert!(matches!(extent, Some(LinearTermination::Blind { .. })));
    assert!(losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::HoleReferenceDimensionsNative.kind()));
}

#[test]
fn unproved_reference_constraints_retain_the_hole_dimensions() {
    for (dimensions, edge_groups) in [
        (&[("RefDim1", 1.0)][..], 2),
        (&[("RefDim1", 1.0), ("RefDim2", 1.0)][..], 1),
        (&[("RefDim1", 1.0), ("RefDim2", 1.0)][..], 3),
        (&[("RefDim1", 1.0), ("RefDim2", 0.0)][..], 2),
    ] {
        let (operation, losses) = reference_dimension_hole(dimensions, edge_groups);
        let Some(FeatureOperation::Hole { shape, extent, .. }) = operation else {
            panic!("Hole")
        };
        assert_eq!(shape.diameter().unwrap().get(), 4.0);
        assert!(
            matches!(extent, Some(cadmpeg_ir::features::LinearTermination::Blind { length }) if length.get() == 10.0)
        );
        assert!(losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::F3dLossCode::HoleParametersIndependentAssumed.kind()));
    }
    let (_, losses) = reference_dimension_hole(&[], 2);
    assert!(!losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::HoleReferenceDimensionsNative.kind()));
    assert!(
        losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::F3dLossCode::HoleParametersIndependentAssumed.kind())
    );
}

#[test]
fn one_reference_distance_and_one_edge_group_keep_the_hole() {
    let (operation, losses) = reference_dimension_hole(&[("RefDim1", 0.7)], 1);
    let Some(FeatureOperation::Hole { shape, extent, .. }) = operation else {
        panic!("Hole")
    };
    assert_eq!(shape.diameter().unwrap().get(), 4.0);
    assert!(
        matches!(extent, Some(cadmpeg_ir::features::LinearTermination::Blind { length }) if length.get() == 10.0)
    );
    assert!(losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::HoleReferenceDimensionsNative.kind()));
    assert!(
        !losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::F3dLossCode::HoleParametersIndependentAssumed.kind())
    );
}

#[test]
fn unresolved_extent_retains_counterbore_tip_and_directed_points() {
    use cadmpeg_ir::features::holes::{HoleBottom, HoleConstruction, HoleKind};
    let mut scope = hole_scope();
    set_extent(&mut scope, 1, false);
    set_direction_control(&mut scope, Some(1));
    set_carrier(
        &mut scope,
        13,
        [0.0; 3],
        &[(10, [1.0, 1.5, 2.0]), (10, [3.0, 1.5, 2.0])],
        false,
    );
    let mut parameters = hole_parameters().to_vec();
    parameters[2]
        .try_set_evaluated_value(118_f64.to_radians())
        .unwrap();
    parameters.push(named_parameter("CBDepth", 0.3, false));
    parameters.push(named_parameter("CBDiameter", 1.0, false));
    let indexed: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (u32::try_from(i).unwrap(), p))
        .collect();
    let mut losses = Vec::new();
    let Some(FeatureDefinition::Operation(FeatureOperation::Hole {
        shape,
        extent,
        bottom,
        placements,
        ..
    })) = project_hole(&scope, &indexed, &[], &[], &mut losses).unwrap()
    else {
        panic!("Hole")
    };
    assert_eq!(shape.diameter().unwrap().get(), 4.0);
    assert!(
        matches!(shape.construction(), HoleConstruction::Form { kind: HoleKind::CounterboreDrilled { diameter, depth, drill_point_angle }, .. }
        if diameter.get() == 10.0 && depth.get() == 3.0 && drill_point_angle.get() == 118_f64.to_radians())
    );
    assert_eq!(
        extent,
        Some(cadmpeg_ir::features::LinearTermination::Unresolved {})
    );
    assert!(
        matches!(bottom, Some(HoleBottom::Angled { included_angle, depth_to_tip: false }) if included_angle.get() == 118_f64.to_radians())
    );
    assert_eq!(
        placements,
        Some(vec![
            directed_along([10.0, 15.0, 20.0], 1.0),
            directed_along([30.0, 15.0, 20.0], 1.0)
        ])
    );
    assert!(losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::HoleExtentUnproven.kind()));
    assert!(
        losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::F3dLossCode::HoleParametersIndependentAssumed.kind())
    );
    assert!(!losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::HoleShapePartialAssumed.kind()));
}

#[test]
fn missing_bore_and_tip_parameters_leave_each_other_and_the_depth_intact() {
    use cadmpeg_ir::features::holes::HoleBottom;
    for (bore, tip) in [(false, true), (true, false), (false, false)] {
        let mut parameters = hole_parameters().to_vec();
        parameters[2]
            .try_set_evaluated_value(118_f64.to_radians())
            .unwrap();
        parameters.retain(|parameter| match parameter.source_kind_str() {
            "HoleDiameter" => bore,
            "TipAngle" => tip,
            _ => true,
        });
        let indexed: Vec<_> = parameters
            .iter()
            .enumerate()
            .map(|(i, p)| (u32::try_from(i).unwrap(), p))
            .collect();
        let mut losses = Vec::new();
        let Some(FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            extent,
            bottom,
            ..
        })) = project_hole(&hole_scope(), &indexed, &[], &[], &mut losses).unwrap()
        else {
            panic!("Hole")
        };
        assert_eq!(
            shape
                .diameter()
                .map(cadmpeg_ir::scalar::PositiveLength::get),
            bore.then_some(4.0)
        );
        assert!(
            matches!(extent, Some(cadmpeg_ir::features::LinearTermination::Blind { length }) if length.get() == 10.0)
        );
        if tip {
            assert!(
                matches!(bottom, Some(HoleBottom::Angled { included_angle, depth_to_tip: false }) if included_angle.get() == 118_f64.to_radians())
            );
        } else {
            assert_eq!(bottom, None);
        }
        assert!(losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::HoleShapePartialAssumed.kind()));
    }
}

#[test]
fn single_entry_dimensions_keep_their_specification_units() {
    use cadmpeg_ir::features::holes::{HoleConstruction, HoleKind, PartialPair};
    for (role, value, angle) in [
        ("CBDiameter", 1.0, false),
        ("CBDepth", 0.3, false),
        ("CSDiameter", 1.0, false),
        ("CSAngle", std::f64::consts::FRAC_PI_2, true),
    ] {
        let mut parameters = hole_parameters().to_vec();
        parameters.push(named_parameter(role, value, angle));
        let indexed: Vec<_> = parameters
            .iter()
            .enumerate()
            .map(|(i, p)| (u32::try_from(i).unwrap(), p))
            .collect();
        let mut losses = Vec::new();
        let Some(FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            extent,
            bottom,
            ..
        })) = project_hole(&hole_scope(), &indexed, &[], &[], &mut losses).unwrap()
        else {
            panic!("Hole")
        };
        assert_eq!(shape.diameter().unwrap().get(), 4.0);
        let HoleConstruction::Form { kind, .. } = shape.construction() else {
            panic!("form")
        };
        match (role, kind) {
            ("CBDiameter", HoleKind::PartialCounterbore(PartialPair::First(diameter)))
            | ("CSDiameter", HoleKind::PartialCountersink(PartialPair::First(diameter))) => {
                assert_eq!(diameter.get(), 10.0);
            }
            ("CBDepth", HoleKind::PartialCounterbore(PartialPair::Second(depth))) => {
                assert_eq!(depth.get(), 3.0);
            }
            ("CSAngle", HoleKind::PartialCountersink(PartialPair::Second(angle))) => {
                assert_eq!(angle.get(), std::f64::consts::FRAC_PI_2);
            }
            _ => panic!("partial entry dimension"),
        }
        assert!(
            matches!(extent, Some(cadmpeg_ir::features::LinearTermination::Blind { length }) if length.get() == 10.0)
        );
        assert_eq!(bottom, Some(cadmpeg_ir::features::holes::HoleBottom::Flat));
        assert!(losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::HoleShapePartialAssumed.kind()));
    }
}

#[test]
fn a_hole_without_scalar_parameters_retains_its_unoriented_entry_axes() {
    use cadmpeg_ir::features::holes::{HoleConstruction, HoleKind};
    let mut scope = hole_scope();
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        slot.extent = None;
    }
    set_carrier(
        &mut scope,
        13,
        [0.0; 3],
        &[(10, [1.0, 1.5, 2.0]), (10, [3.0, 1.5, 2.0])],
        false,
    );
    let mut losses = Vec::new();
    let Some(FeatureDefinition::Operation(FeatureOperation::Hole {
        shape,
        extent,
        placements,
        bottom,
        ..
    })) = project_hole(&scope, &[], &[], &[], &mut losses).unwrap()
    else {
        panic!("Hole")
    };
    assert_eq!(shape.diameter(), None);
    assert_eq!(
        shape.construction(),
        &HoleConstruction::form(HoleKind::Unresolved(None))
    );
    assert_eq!(
        extent,
        Some(cadmpeg_ir::features::LinearTermination::Unresolved {})
    );
    assert_eq!(bottom, None);
    assert_eq!(
        placements,
        Some(vec![axis([10.0, 15.0, 20.0]), axis([30.0, 15.0, 20.0])])
    );
    for code in [
        crate::loss::F3dLossCode::HoleExtentUnproven,
        crate::loss::F3dLossCode::HoleShapePartialAssumed,
        crate::loss::F3dLossCode::HoleParametersIndependentAssumed,
        crate::loss::F3dLossCode::HoleCarrierAxisAssumed,
    ] {
        assert!(losses.iter().any(|loss| loss.code == code.kind()));
    }
    assert!(!losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::HolePlacementUnproven.kind()));
}

#[test]
fn extra_parameters_and_native_sizing_keep_the_admitted_hole_dimensions() {
    use cadmpeg_ir::features::holes::{HoleConstruction, HoleKind};
    let mut scope = hole_scope();
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        let extent = slot.extent.as_mut().unwrap();
        extent.leading_word = Some(crate::records::identity::Located {
            value: 2,
            offset: 5,
        });
        extent.standard = Some(crate::records::feature::hole::DesignHoleStandard {
            offset: 10,
            standard: "ISO".into(),
            fit: "Close".into(),
            size: "M3".into(),
            fastener: "Socket".into(),
        });
    }
    let mut parameters = hole_parameters().to_vec();
    parameters.push(named_parameter("AuxiliaryDepth", 0.2, false));
    let indexed: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (u32::try_from(i).unwrap(), p))
        .collect();
    let mut losses = Vec::new();
    let Some(FeatureDefinition::Operation(FeatureOperation::Hole {
        shape,
        extent,
        bottom,
        ..
    })) = project_hole(&scope, &indexed, &[], &[], &mut losses).unwrap()
    else {
        panic!("Hole")
    };
    assert_eq!(shape.diameter().unwrap().get(), 4.0);
    assert_eq!(
        shape.construction(),
        &HoleConstruction::Form {
            kind: HoleKind::Simple,
            specification: None
        }
    );
    assert!(
        matches!(extent,Some(cadmpeg_ir::features::LinearTermination::Blind { length }) if length.get()==10.0)
    );
    assert_eq!(bottom, Some(cadmpeg_ir::features::holes::HoleBottom::Flat));
    for code in [
        crate::loss::F3dLossCode::HoleParametersIndependentAssumed,
        crate::loss::F3dLossCode::HoleExtentLeadingWordAssumed,
        crate::loss::F3dLossCode::HoleStandardSizingAssumed,
    ] {
        assert!(losses.iter().any(|loss| loss.code == code.kind()));
    }
}

#[test]
fn competing_bore_parameters_keep_both_expressions_and_an_unresolved_diameter() {
    let scope = hole_scope();
    let mut parameters = hole_parameters().to_vec();
    parameters.push(
        parse_design_parameter_record(&parameter_record(
            Some(124),
            "6 mm",
            "HoleDiameter",
            Some("mm"),
            "d8",
            0.6,
        ))
        .unwrap(),
    );
    let indexed: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (u32::try_from(i).unwrap(), p))
        .collect();
    let mut losses = Vec::new();
    let Some(FeatureDefinition::Operation(FeatureOperation::Hole {
        shape,
        extent,
        bottom,
        ..
    })) = project_hole(&scope, &indexed, &[], &[], &mut losses).unwrap()
    else {
        panic!("partial Hole")
    };
    assert_eq!(shape.diameter(), None);
    assert!(
        matches!(extent,Some(cadmpeg_ir::features::LinearTermination::Blind { length }) if length.get()==10.0)
    );
    assert_eq!(bottom, Some(cadmpeg_ir::features::holes::HoleBottom::Flat));
    let properties =
        super::super::scope_properties(&scope, "f3d:Design/BulkStream.dat", &[], &indexed).unwrap();
    assert_eq!(
        properties.get("parameter:d5").map(String::as_str),
        Some("4 mm")
    );
    assert_eq!(
        properties.get("parameter:d8").map(String::as_str),
        Some("6 mm")
    );
    for code in [
        crate::loss::F3dLossCode::HoleShapePartialAssumed,
        crate::loss::F3dLossCode::HoleParametersIndependentAssumed,
    ] {
        assert!(losses.iter().any(|loss| loss.code == code.kind()));
    }
}

#[test]
fn encoded_entry_types_retain_partial_counterbore_and_countersink_families() {
    use cadmpeg_ir::features::holes::{HoleConstruction, HoleForm, HoleKind};
    for (value, expected) in [
        (0, HoleKind::Simple),
        (1, HoleKind::Unresolved(Some(HoleForm::Counterbore))),
        (2, HoleKind::Unresolved(Some(HoleForm::Countersink))),
    ] {
        let mut scope = hole_scope();
        if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) =
            scope.payload_mut()
        {
            slot.extent.as_mut().unwrap().hole_type =
                Some(crate::records::identity::Located { value, offset: 7 });
        }
        let Some(FeatureOperation::Hole {
            shape,
            extent,
            bottom,
            ..
        }) = projected_hole(&scope, &hole_parameters(), &[])
        else {
            panic!("Hole")
        };
        assert_eq!(shape.construction(), &HoleConstruction::form(expected));
        assert_eq!(shape.diameter().unwrap().get(), 4.0);
        assert!(
            matches!(extent,Some(cadmpeg_ir::features::LinearTermination::Blind { length }) if length.get()==10.0)
        );
        assert_eq!(bottom, Some(cadmpeg_ir::features::holes::HoleBottom::Flat));
    }
}

#[test]
fn an_encoded_simple_form_retains_unmatched_counterbore_parameters() {
    use cadmpeg_ir::features::holes::{HoleConstruction, HoleKind};
    let mut scope = hole_scope();
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        slot.extent.as_mut().unwrap().hole_type = Some(crate::records::identity::Located {
            value: 0,
            offset: 7,
        });
    }
    let mut parameters = hole_parameters().to_vec();
    for (owner, name, role, expression, value) in [
        (124, "d7", "CBDepth", "3 mm", 0.3),
        (134, "d8", "CBDiameter", "10 mm", 1.0),
    ] {
        parameters.push(
            parse_design_parameter_record(&parameter_record(
                Some(owner),
                expression,
                role,
                Some("mm"),
                name,
                value,
            ))
            .unwrap(),
        );
    }
    let indexed: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (u32::try_from(i).unwrap(), p))
        .collect();
    let mut losses = Vec::new();
    let Some(FeatureDefinition::Operation(FeatureOperation::Hole { shape, .. })) =
        project_hole(&scope, &indexed, &[], &[], &mut losses).unwrap()
    else {
        panic!("Hole")
    };
    assert_eq!(
        shape.construction(),
        &HoleConstruction::form(HoleKind::Simple)
    );
    assert_eq!(shape.diameter().unwrap().get(), 4.0);
    let properties =
        super::super::scope_properties(&scope, "f3d:Design/BulkStream.dat", &[], &indexed).unwrap();
    for (key, value) in [
        ("hole:type", "0"),
        ("parameter:d7", "3 mm"),
        ("parameter:d8", "10 mm"),
    ] {
        assert_eq!(properties.get(key).map(String::as_str), Some(value));
    }
    assert!(
        losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::F3dLossCode::HoleParametersIndependentAssumed.kind())
    );
}
