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
    let definition = project_hole(&scope, &indexed, &[], &mut Vec::new())
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
            saved_non_distance_depth: saved.then(|| {
                crate::records::identity::Sourced::at(
                    cadmpeg_ir::scalar::FiniteReal::try_from(1.0).unwrap(),
                    5,
                )
            }),
        });
    }
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
        let definition = project_hole(&scope, &indexed, &faces, &mut Vec::new())
            .unwrap()
            .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Hole { extent, bottom, .. }) =
            definition
        else {
            panic!()
        };
        let depth = parameters
            .iter()
            .find(|parameter| parameter.source_kind() == "HoleDepth")
            .unwrap();
        assert!(
            matches!(&extent, Some(LinearTermination::Blind {length}) if *length == super::super::dimension_bindings::parameter(depth) && (super::super::design_positive_length(depth).unwrap().get()-10.0).abs() < EPS_HOLE_EXTENT_TEST)
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
        let definition = project_hole(&scope, &indexed, &faces, &mut Vec::new())
            .unwrap()
            .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Hole { extent, shape, .. }) = definition
        else {
            panic!()
        };
        assert_eq!(extent, Some(LinearTermination::ThroughAll {}));
        assert_eq!(
            super::super::hole_termination(&scope, &indexed, &[]),
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
        assert!(super::super::hole_termination(&scope, parameters, faces).is_none());
        let mut losses = Vec::new();
        assert!(project_hole(&scope, parameters, faces, &mut losses)
            .unwrap()
            .is_none());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::HoleExtentUnproven.kind()
        );
    }
    set_extent(&mut scope, 1, false);
    assert!(
        project_hole(&scope, &indexed[1..], &faces[..2], &mut Vec::new())
            .unwrap()
            .is_none()
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
        });
    }
    let parameters = hole_parameters();
    let indexed: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(i, p)| (u32::try_from(i).unwrap(), p))
        .collect();
    assert!(matches!(
        super::super::hole_termination(&scope, &indexed, &[]),
        Some(LinearTermination::Blind { .. })
    ));
    let without_depth: Vec<_> = indexed
        .iter()
        .copied()
        .filter(|(_, parameter)| parameter.source_kind() != "HoleDepth")
        .collect();
    assert_eq!(without_depth.len(), indexed.len() - 1);
    assert_eq!(
        super::super::hole_termination(&scope, &without_depth, &[]),
        None
    );
    let mut losses = Vec::new();
    assert!(project_hole(&scope, &without_depth, &[], &mut losses)
        .unwrap()
        .is_none());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::HoleExtentUnproven.kind()
    );
}
