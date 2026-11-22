// SPDX-License-Identifier: Apache-2.0

use crate::design::feature_project::pattern::project_circular_pattern;
use crate::design::feature_project::pattern::project_rectangular_pattern_scalars;
use crate::design::feature_project::pattern_seeds::unprojected_pattern_seeds;
use crate::records::{
    feature::{patterns::DesignRectangularPatternConstruction, scope::DesignParameterScope},
    topology::{
        construction::DesignConstructionOperandGroup,
        construction::DesignConstructionOperandGroupFrame, extrude_selection::DesignOperandRole,
    },
};
use cadmpeg_ir::features::patterns::{PatternSeed, PatternTransform};
use cadmpeg_ir::features::selections::{BodySelection, FaceSelection};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

const EPS_SPACING: f64 = 1.0e-12;

/// Authored timeline ordinals that list the test-stream scopes with record
/// indices `record_indices`, in that order.
pub(super) fn listed(
    record_indices: &[u32],
) -> std::collections::HashMap<(&'static str, u32), u64> {
    record_indices
        .iter()
        .zip(1..)
        .map(|(record_index, ordinal)| (("f3d:Design/BulkStream.dat", *record_index), ordinal))
        .collect()
}

fn group(
    scope_record_index: u32,
    record_index: u32,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: format!(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#{record_index}"
            ),
            scope_record_index,
            scope_reference_ordinal: 1,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("313".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: record_index + 1,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 99,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(role),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "263".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

fn rectangular_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::RPattern,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::RectangularPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            DesignRectangularPatternConstruction::try_from(
                crate::records::feature::patterns::DesignRectangularPatternConstructionWire {
                    u_count: 3,
                    v_count: 1,
                    u_extent: 10.0,
                    v_extent: 0.0,
                    owner_record_indices: [11, 12, 13, 14],
                    value_offsets: [101, 102, 103, 104],
                    instances: None,
                    instance_search_cap: None,
                    directions: Vec::new(),
                    secondary_direction: None,
                    distribution: None,
                },
            )
            .unwrap(),
        );
    }
    scope
}

fn circular_scope() -> DesignParameterScope {
    use crate::records::feature::patterns::{
        DesignCircularPatternAxis, DesignCircularPatternConstruction,
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::CPattern,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::CPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignCircularPatternConstruction {
            axis_separator_flag_offset: None,
            symmetric: None,
            suppressed: None,
            count: std::num::NonZeroU32::new(3).unwrap(),
            count_record_index: 11,
            count_offset: 0,
            angle: cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU).unwrap(),
            angle_record_index: 12,
            angle_offset: 0,
            axis: DesignCircularPatternAxis::Inline {
                origin: crate::test_support::reals([0.0, 0.0, 0.0]),
                origin_offset: 0,
                direction: cadmpeg_ir::units::UnitVector3::normalized(
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
                direction_offset: 0,
            },
            axis_record_index: 13,
            selection_record_index: 14,
        });
    }
    scope
}

#[test]
fn circular_pattern_seed_role_selects_body_or_face() {
    for (role, expected_seed) in [
        (
            DesignOperandRole::BODIES_B,
            PatternSeed::Bodies(BodySelection::Native(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
            )),
        ),
        (
            DesignOperandRole::BODIES_A,
            PatternSeed::Faces(FaceSelection::Native(
                "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
            )),
        ),
    ] {
        let definition = project_circular_pattern(
            &circular_scope(),
            &[group(10, 20, role)],
            &[],
            &[],
            &[],
            &listed(&[5]),
            &mut Vec::new(),
        )
        .expect("circular pattern");
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) = definition
        else {
            panic!("circular pattern definition");
        };
        assert_eq!(seeds, vec![expected_seed]);
        assert!(matches!(
            pattern.definition(),
            PatternTransform::Circular { .. }
        ));
    }
}

#[test]
fn suppressed_circular_instances_leave_the_remaining_angles_or_are_charged() {
    // `keys` suppressed in a pattern of `count` over `angle`; `symmetric` is
    // the payload byte, and `separator` marks a 130-byte axis record.
    let with =
        |count: u32, angle: f64, symmetric: Option<bool>, separator: bool, keys: Vec<u32>| {
            let mut scope = circular_scope();
            if let crate::records::feature::scope::DesignScopePayloadMut::CPattern(Some(
                construction,
            )) = scope.payload_mut()
            {
                construction.count = std::num::NonZeroU32::new(count).unwrap();
                construction.angle = cadmpeg_ir::scalar::PositiveAngle::new(angle).unwrap();
                construction.symmetric =
                    symmetric.map(|value| crate::records::identity::Located { value, offset: 21 });
                construction.axis_separator_flag_offset = separator.then_some(40);
                construction.suppressed = Some(crate::records::identity::Located {
                    value: keys,
                    offset: 170,
                });
            }
            let mut losses = Vec::new();
            let pattern = project_circular_pattern(
                &scope,
                &[group(10, 20, DesignOperandRole::BODIES_B)],
                &[],
                &[],
                &[],
                &listed(&[5]),
                &mut losses,
            )
            .map(|definition| {
                let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
                    definition
                else {
                    panic!("circular pattern definition");
                };
                match pattern.definition() {
                    PatternTransform::CircularAngles { angles, .. } => Ok(angles
                        .iter()
                        .map(|angle| angle.get().to_degrees() * 10.0)
                        .map(f64::round)
                        .collect::<Vec<_>>()),
                    PatternTransform::Circular { count, .. } => Err(*count),
                    other => panic!("circular pattern transform: {other:?}"),
                }
            });
            (
                pattern,
                losses.into_iter().map(|loss| loss.code).collect::<Vec<_>>(),
            )
        };
    let suppression =
        crate::loss::F3dLossCode::FeatureCircularPatternSuppressionNotTransferred.kind();
    // A full turn of six steps 60 degrees; instances 3 and 5 have keys 4 and 6.
    let (pattern, losses) = with(6, std::f64::consts::TAU, Some(false), false, vec![4, 6]);
    assert_eq!(pattern, Some(Ok(vec![0.0, 600.0, 1200.0, 2400.0])));
    assert!(losses.is_empty());
    // A partial turn of five over 90 degrees steps 22.5 degrees.
    let (pattern, _) = with(5, std::f64::consts::FRAC_PI_2, Some(false), false, vec![2]);
    assert_eq!(pattern, Some(Ok(vec![0.0, 450.0, 675.0, 900.0])));
    // A symmetric pattern, or one about a 130-byte axis record, keeps every
    // instance and charges the suppression.
    for (symmetric, separator) in [(Some(true), false), (None, false), (Some(false), true)] {
        let (pattern, losses) = with(6, std::f64::consts::TAU, symmetric, separator, vec![4]);
        assert_eq!(pattern, Some(Err(6)), "{symmetric:?} {separator}");
        assert_eq!(losses, [suppression.clone()], "{symmetric:?} {separator}");
    }
}

#[test]
fn a_symmetric_partial_turn_reports_its_dropped_distribution() {
    let with = |angle: f64, symmetric: bool| {
        let mut scope = circular_scope();
        if let crate::records::feature::scope::DesignScopePayloadMut::CPattern(Some(construction)) =
            scope.payload_mut()
        {
            construction.symmetric = Some(crate::records::identity::Located {
                value: symmetric,
                offset: 21,
            });
            construction.angle = cadmpeg_ir::scalar::PositiveAngle::new(angle).unwrap();
        }
        let mut losses = Vec::new();
        project_circular_pattern(
            &scope,
            &[group(10, 20, DesignOperandRole::BODIES_B)],
            &[],
            &[],
            &[],
            &listed(&[5]),
            &mut losses,
        )
        .expect("circular pattern");
        losses
    };
    let losses = with(std::f64::consts::FRAC_PI_2, true);
    let [loss] = losses.as_slice() else {
        panic!("one placement loss");
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::FeatureCircularPatternPlacementNotTransferred.kind()
    );
    assert!(loss.message.contains("symmetric distribution"));
    assert!(with(std::f64::consts::TAU, true).is_empty());
    assert!(with(std::f64::consts::FRAC_PI_2, false).is_empty());

    let mut scope = circular_scope();
    if let crate::records::feature::scope::DesignScopePayloadMut::CPattern(Some(construction)) =
        scope.payload_mut()
    {
        construction.angle =
            cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::FRAC_PI_2).unwrap();
    }
    let mut losses = Vec::new();
    project_circular_pattern(
        &scope,
        &[group(10, 20, DesignOperandRole::BODIES_B)],
        &[],
        &[],
        &[],
        &listed(&[5]),
        &mut losses,
    )
    .expect("circular pattern");
    let [loss] = losses.as_slice() else {
        panic!("one assumption loss");
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::FeatureCircularPatternDistributionAssumed.kind()
    );
}

#[test]
fn a_separator_axis_projects_only_a_full_turn() {
    let with = |angle: f64| {
        let mut scope = circular_scope();
        if let crate::records::feature::scope::DesignScopePayloadMut::CPattern(Some(construction)) =
            scope.payload_mut()
        {
            construction.axis_separator_flag_offset = Some(104);
            construction.angle = cadmpeg_ir::scalar::PositiveAngle::new(angle).unwrap();
        }
        project_circular_pattern(
            &scope,
            &[group(10, 20, DesignOperandRole::BODIES_B)],
            &[],
            &[],
            &[],
            &listed(&[5]),
            &mut Vec::new(),
        )
    };
    assert!(with(std::f64::consts::TAU).is_some());
    assert!(with(std::f64::consts::FRAC_PI_2).is_none());
}

fn assert_linear_seed(definition: FeatureDefinition, expected_seed: PatternSeed) {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) = definition
    else {
        panic!("rectangular pattern definition");
    };
    assert_eq!(seeds, vec![expected_seed]);
    let PatternTransform::Linear {
        direction,
        spacing,
        count,
        second,
        omitted_instances,
        ..
    } = pattern.definition().clone()
    else {
        panic!("linear rectangular pattern");
    };
    assert!(omitted_instances.is_empty());
    assert!(direction.is_none());
    assert!((spacing.get() - 50.0).abs() < EPS_SPACING);
    assert_eq!(count, 3);
    assert!(second.is_none());
}

#[test]
fn rectangular_pattern_seed_role_selects_body_or_face() {
    let body_scope = rectangular_scope();
    let body_group = group(10, 20, DesignOperandRole::BODIES_B);
    let body_definition = project_rectangular_pattern_scalars(
        &body_scope,
        &[body_group],
        &[],
        &[],
        &[],
        &listed(&[5]),
        &mut Vec::new(),
    )
    .expect("body rectangular pattern");
    assert_linear_seed(
        body_definition,
        PatternSeed::Bodies(BodySelection::Native(
            "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
        )),
    );

    let face_scope = rectangular_scope();
    let face_group = group(10, 30, DesignOperandRole::BODIES_A);
    let face_definition = project_rectangular_pattern_scalars(
        &face_scope,
        &[face_group],
        &[],
        &[],
        &[],
        &listed(&[5]),
        &mut Vec::new(),
    )
    .expect("face rectangular pattern");
    assert_linear_seed(
        face_definition,
        PatternSeed::Faces(FaceSelection::Native(
            "f3d:Design/BulkStream.dat:design-construction-operand-group#30".into(),
        )),
    );
}

fn direction_record(
    record_index: u32,
    displacement: [f64; 3],
) -> crate::records::feature::patterns::DesignRectangularPatternDirection {
    crate::records::feature::patterns::DesignRectangularPatternDirection {
        record_index,
        selection_record_index: record_index + 3,
        selection_kind: 8,
        distance_type: 8,
        origin: crate::test_support::reals([0.0, 0.0, 0.0]),
        origin_offset: 0,
        displacement: crate::test_support::reals(displacement),
        displacement_offset: 24,
    }
}

fn two_direction_scope(
    directions: Vec<crate::records::feature::patterns::DesignRectangularPatternDirection>,
) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::RPattern,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::RectangularPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            DesignRectangularPatternConstruction::try_from(
                crate::records::feature::patterns::DesignRectangularPatternConstructionWire {
                    u_count: 4,
                    v_count: 2,
                    u_extent: 3.0,
                    v_extent: -1.2,
                    owner_record_indices: [11, 12, 13, 14],
                    value_offsets: [101, 102, 103, 104],
                    instances: None,
                    instance_search_cap: None,
                    directions,
                    secondary_direction: None,
                    distribution: None,
                },
            )
            .unwrap(),
        );
    }
    scope
}

#[test]
fn rectangular_pattern_direction_records_supply_both_directions() {
    let scope = two_direction_scope(vec![
        direction_record(20, [2.0, 0.0, 0.0]),
        direction_record(30, [0.0, 2.5, 0.0]),
    ]);
    let definition = project_rectangular_pattern_scalars(
        &scope,
        &[],
        &[],
        &[],
        &[],
        &listed(&[5]),
        &mut Vec::new(),
    )
    .expect("two-direction rectangular pattern");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = definition else {
        panic!("rectangular pattern definition");
    };
    let PatternTransform::Linear {
        direction,
        spacing,
        count,
        second,
        omitted_instances,
        ..
    } = pattern.definition().clone()
    else {
        panic!("linear rectangular pattern");
    };
    assert!(omitted_instances.is_empty());
    let direction =
        cadmpeg_ir::features::geometry::FiniteVector3::from(direction.expect("first direction"))
            .get();
    assert!((direction.x - 1.0).abs() < EPS_SPACING);
    assert!(direction.y.abs() < EPS_SPACING && direction.z.abs() < EPS_SPACING);
    assert!((spacing.get() - 10.0).abs() < EPS_SPACING);
    assert_eq!(count, 4);
    let second = second.expect("second direction");
    let second_direction =
        cadmpeg_ir::features::geometry::FiniteVector3::from(second.direction).get();
    assert!((second_direction.y + 1.0).abs() < EPS_SPACING);
    assert!(second_direction.x.abs() < EPS_SPACING && second_direction.z.abs() < EPS_SPACING);
    assert!((second.spacing.get() - 12.0).abs() < EPS_SPACING);
    assert_eq!(second.count, 2);
}

#[test]
fn two_active_directions_without_direction_records_do_not_project() {
    assert!(project_rectangular_pattern_scalars(
        &two_direction_scope(Vec::new()),
        &[],
        &[],
        &[],
        &[],
        &listed(&[5]),
        &mut Vec::new()
    )
    .is_none());
}

fn single_direction_scope_with_run(
    directions: Vec<crate::records::feature::patterns::DesignRectangularPatternDirection>,
) -> DesignParameterScope {
    use crate::records::feature::patterns::{
        DesignPatternInstance, DesignRectangularPatternInstances,
    };
    use crate::records::sketch_placement::SketchPlacementMatrix;

    let at = |x: f64| {
        let mut matrix: [[f64; 4]; 4] = SketchPlacementMatrix::IDENTITY.into();
        matrix[0][3] = x;
        SketchPlacementMatrix::try_from(matrix).unwrap()
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::RPattern,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::RectangularPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            DesignRectangularPatternConstruction::try_from(
                crate::records::feature::patterns::DesignRectangularPatternConstructionWire {
                    u_count: 3,
                    v_count: 1,
                    u_extent: 2.0,
                    v_extent: 0.0,
                    owner_record_indices: [11, 12, 13, 14],
                    value_offsets: [101, 102, 103, 104],
                    instances: Some(DesignRectangularPatternInstances::Bodies(
                        [0.0, 1.0, 2.0]
                            .into_iter()
                            .enumerate()
                            .map(|(index, x)| DesignPatternInstance {
                                record_index: 40 + u32::try_from(index).unwrap(),
                                transform: crate::records::identity::Located {
                                    value: at(x),
                                    offset: 1000 * u64::try_from(index).unwrap(),
                                },
                            })
                            .collect(),
                    )),
                    instance_search_cap: None,
                    directions,
                    secondary_direction: None,
                    distribution: None,
                },
            )
            .unwrap(),
        );
    }
    scope
}

fn linear_direction(definition: FeatureDefinition) -> Option<cadmpeg_ir::math::Vector3> {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = definition else {
        panic!("rectangular pattern definition");
    };
    let PatternTransform::Linear {
        direction,
        omitted_instances,
        ..
    } = pattern.definition().clone()
    else {
        panic!("linear rectangular pattern");
    };
    assert!(omitted_instances.is_empty());
    direction.map(|direction| cadmpeg_ir::features::geometry::FiniteVector3::from(direction).get())
}

#[test]
fn a_disagreeing_single_direction_record_keeps_the_instance_run() {
    let run = linear_direction(
        project_rectangular_pattern_scalars(
            &single_direction_scope_with_run(Vec::new()),
            &[],
            &[],
            &[],
            &[],
            &listed(&[5]),
            &mut Vec::new(),
        )
        .expect("instance-run pattern"),
    )
    .expect("instance-run direction");
    assert!((run.x - 1.0).abs() < EPS_SPACING);

    let agreeing = linear_direction(
        project_rectangular_pattern_scalars(
            &single_direction_scope_with_run(vec![direction_record(20, [1.5, 0.0, 0.0])]),
            &[],
            &[],
            &[],
            &[],
            &listed(&[5]),
            &mut Vec::new(),
        )
        .expect("agreeing pattern"),
    )
    .expect("agreeing direction");
    assert!((agreeing.x - 1.0).abs() < EPS_SPACING);

    let mut losses = Vec::new();
    let disagreeing = linear_direction(
        project_rectangular_pattern_scalars(
            &single_direction_scope_with_run(vec![direction_record(20, [0.0, 1.5, 0.0])]),
            &[],
            &[],
            &[],
            &[],
            &listed(&[5]),
            &mut losses,
        )
        .expect("disagreeing pattern"),
    )
    .expect("instance-run direction kept");
    assert!((disagreeing.x - 1.0).abs() < EPS_SPACING);
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        [crate::loss::F3dLossCode::FeatureRectangularPatternDirectionRecordDisagrees.kind()]
    );
}

pub(super) fn seed_scope(record_index: u32, history_state_id: Option<i64>) -> DesignParameterScope {
    DesignParameterScope::try_new(crate::records::feature::scope::DesignParameterScopeDraft {
        id: format!("f3d:Design/BulkStream.dat:design-parameter-scope#{record_index}"),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("353".to_owned()).unwrap(),
        record_index,
        frame_length: 128,
        kind_offset: 32,
        feature_ordinal: std::num::NonZeroU32::MIN,
        feature_ordinal_offset: 48,
        label: None,
        history_state_id,
        previous_history_state_id: None,
        previous_history_state_id_offset: None,
        reference_count_offset: 9,
        reference_members: crate::records::identity::ReferenceRun::located(vec![
            crate::records::identity::Located {
                value: record_index,
                offset: 14,
            },
        ]),
        payload: crate::records::feature::scope::DesignFeatureKind::Extrude
            .try_into()
            .unwrap(),
        unclosed_construction_operand_groups: Vec::new(),
        deleted_members: None,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("263".to_owned())
            .unwrap(),
        paired_byte_offset: 128,
    })
    .unwrap()
}

/// A compact entity selection: identity record `N + 3` stores one u64
/// identity at offset 21, and the next header follows at offset 29.
pub(super) fn compact_entity_selection(
    group_record_index: u32,
    record_index: u32,
    identity: u64,
) -> crate::records::topology::entity_selection::DesignEntitySelectionOperand {
    serde_json::from_value(serde_json::json!({
        "id": format!("f3d:Design/BulkStream.dat:design-entity-selection-operand#{record_index}"),
        "scope_record_index": 10,
        "group_record_index": group_record_index, "group_member_ordinal": 0,
        "record_index": record_index, "byte_offset": 100, "class_tag": "335",
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 130,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 210,
        "identity_record_index": record_index + 3, "identity_record_offset": 300,
        "primary_identity": identity, "primary_identity_offset": 321,
        "next_record_index": record_index + 4, "next_byte_offset": 329
    }))
    .unwrap()
}

#[test]
fn compact_entity_selection_seed_names_the_seed_feature() {
    let feature = seed_scope(5, Some(2));
    let expected = PatternSeed::Feature(crate::ids::neutral_feature_id(&feature));
    let rectangular = project_rectangular_pattern_scalars(
        &rectangular_scope(),
        &[group(10, 30, DesignOperandRole::BODIES_A)],
        &[],
        &[compact_entity_selection(30, 31, 5)],
        &[rectangular_scope(), feature.clone()],
        &listed(&[5]),
        &mut Vec::new(),
    )
    .expect("rectangular pattern");
    assert_linear_seed(rectangular, expected.clone());

    let circular = project_circular_pattern(
        &circular_scope(),
        &[group(10, 30, DesignOperandRole::BODIES_A)],
        &[],
        &[compact_entity_selection(30, 31, 5)],
        &[circular_scope(), feature],
        &listed(&[5]),
        &mut Vec::new(),
    )
    .expect("circular pattern");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = circular else {
        panic!("circular pattern definition");
    };
    assert_eq!(seeds, vec![expected]);
}

#[test]
fn compact_entity_selection_seed_naming_an_unlisted_scope_stays_native() {
    let feature = seed_scope(5, Some(2));
    let native = PatternSeed::Faces(FaceSelection::Native(
        "f3d:Design/BulkStream.dat:design-construction-operand-group#30".into(),
    ));
    for (timeline, expected) in [
        (
            listed(&[5]),
            PatternSeed::Feature(crate::ids::neutral_feature_id(&feature)),
        ),
        (listed(&[]), native.clone()),
        (listed(&[10]), native),
    ] {
        for circular in [false, true] {
            let mut losses = Vec::new();
            let scope = if circular {
                circular_scope()
            } else {
                rectangular_scope()
            };
            let project = if circular {
                project_circular_pattern
            } else {
                project_rectangular_pattern_scalars
            };
            let definition = project(
                &scope,
                &[group(10, 30, DesignOperandRole::BODIES_A)],
                &[],
                &[compact_entity_selection(30, 31, 5)],
                &[scope.clone(), feature.clone()],
                &timeline,
                &mut losses,
            )
            .expect("pattern with compact seed");
            let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
                definition
            else {
                panic!("pattern definition");
            };
            assert_eq!(seeds, vec![expected.clone()]);
            if circular {
                assert!(matches!(
                    pattern.definition(),
                    PatternTransform::Circular { .. }
                ));
            } else {
                assert!(matches!(
                    pattern.definition(),
                    PatternTransform::Linear { .. }
                ));
            }
            let substituted = crate::loss::F3dLossCode::FeaturePatternFeatureSeedSubstituted.kind();
            let substitutions = losses
                .iter()
                .filter(|loss| loss.code == substituted)
                .count();
            let listed = timeline.contains_key(&("f3d:Design/BulkStream.dat", 5));
            assert_eq!(substitutions, usize::from(!listed), "{losses:?}");
        }
    }
}

#[test]
fn compact_entity_selection_seed_without_a_feature_scope_stays_native() {
    let native = PatternSeed::Faces(FaceSelection::Native(
        "f3d:Design/BulkStream.dat:design-construction-operand-group#30".into(),
    ));
    for (identity, scopes) in [
        (5, vec![seed_scope(5, None)]),
        (6, vec![seed_scope(5, Some(2))]),
        (10, vec![rectangular_scope()]),
    ] {
        let mut losses = Vec::new();
        let definition = project_rectangular_pattern_scalars(
            &rectangular_scope(),
            &[group(10, 30, DesignOperandRole::BODIES_A)],
            &[],
            &[compact_entity_selection(30, 31, identity)],
            &scopes,
            &listed(&[5]),
            &mut losses,
        )
        .expect("rectangular pattern");
        assert_linear_seed(definition, native.clone());
        let substituted = crate::loss::F3dLossCode::FeaturePatternFeatureSeedSubstituted.kind();
        assert!(
            losses.iter().all(|loss| loss.code != substituted),
            "{losses:?}"
        );
    }
}

#[test]
fn rectangular_pattern_payload_projects_both_axes_and_feature_seed() {
    for (distance_type, u_spacing, v_spacing) in [(7, 17.0, 9.0), (8, 4.25, 4.5)] {
        use crate::records::feature::patterns::DesignRectangularPatternConstructionWire;
        let mut direction = direction_record(20, [0.0, 2.0, 0.0]);
        direction.distance_type = distance_type;
        let mut scope = two_direction_scope(Vec::new());
        let mut wire = DesignRectangularPatternConstructionWire::from(
            scope.rectangular_pattern_construction().unwrap().clone(),
        );
        wire.u_count = 5;
        wire.v_count = 3;
        wire.u_extent = 1.7;
        wire.v_extent = -0.9;
        wire.directions = vec![direction];
        wire.secondary_direction = Some(crate::records::identity::Located {
            value: crate::test_support::reals([0.0, 0.0, 4.0]),
            offset: 63,
        });
        if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot) =
            scope.payload_mut()
        {
            *slot = Some(DesignRectangularPatternConstruction::try_from(wire).unwrap());
        }
        let feature = seed_scope(5, Some(2));
        let expected = PatternSeed::Feature(crate::ids::neutral_feature_id(&feature));
        let definition = project_rectangular_pattern_scalars(
            &scope,
            &[group(10, 30, DesignOperandRole::BODIES_A)],
            &[],
            &[compact_entity_selection(30, 31, 5)],
            &[feature],
            &listed(&[5]),
            &mut Vec::new(),
        )
        .expect("spacing pattern");
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) = definition
        else {
            panic!("pattern");
        };
        assert_eq!(seeds, vec![expected]);
        let PatternTransform::Linear {
            direction,
            spacing,
            count,
            second,
            omitted_instances,
            ..
        } = pattern.definition()
        else {
            panic!("linear pattern");
        };
        assert!(omitted_instances.is_empty());
        assert_eq!(*count, 5);
        assert!((spacing.get() - u_spacing).abs() < EPS_SPACING);
        assert_eq!(
            cadmpeg_ir::features::geometry::FiniteVector3::from(direction.unwrap()).get(),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
        );
        let second = second.as_ref().unwrap();
        assert_eq!(second.count, 3);
        assert!((second.spacing.get() - v_spacing).abs() < EPS_SPACING);
        assert_eq!(
            cadmpeg_ir::features::geometry::FiniteVector3::from(second.direction).get(),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0)
        );
        let record = scope.rectangular_pattern_construction().unwrap();
        assert_eq!(
            serde_json::from_value::<DesignRectangularPatternConstruction>(
                crate::json_value(record).unwrap()
            )
            .unwrap(),
            *record
        );
    }
}

pub(super) fn seed_face_operand(
    kind: crate::records::recipes::ConstructionRecipeKind,
) -> crate::records::topology::face::DesignFaceOperand {
    use crate::records::topology::{
        body_recipe::DesignOperandGroup,
        face::{DesignFaceOperand, DesignFaceOperandDraft},
    };
    DesignFaceOperand::try_new(DesignFaceOperandDraft {
        id: "f3d:Design/BulkStream.dat:face-operand#31".to_owned(),
        scope_record_index: 10,
        scope_reference_ordinal: 1,
        group: Some(DesignOperandGroup {
            group_record_index: 30,
            group_member_ordinal: 0,
        }),
        record_index: 31,
        byte_offset: 10,
        class_tag: "256".to_owned().try_into().unwrap(),
        paired_byte_offset: 30,
        paired_class_tag: "257".to_owned().try_into().unwrap(),
        recipe_record_index: 34,
        recipe_record_byte_offset: 40,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#34".to_owned(),
        recipe_prefix_offset: 51,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: kind,
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
        next_record_index: 51,
        next_byte_offset: 70,
    })
    .unwrap()
}

#[test]
fn a_face_seed_group_supplies_face_seeds_without_a_body_or_face_role_group() {
    use crate::records::recipes::ConstructionRecipeKind;

    let faces = PatternSeed::Faces(FaceSelection::Native(
        "f3d:Design/BulkStream.dat:design-construction-operand-group#30".into(),
    ));
    let bounded = [seed_face_operand(ConstructionRecipeKind::BoundedFace)];
    let rectangular = project_rectangular_pattern_scalars(
        &rectangular_scope(),
        &[group(10, 30, DesignOperandRole::ROLE_0X10)],
        &bounded,
        &[],
        &[],
        &listed(&[5]),
        &mut Vec::new(),
    )
    .expect("rectangular pattern");
    assert_linear_seed(rectangular, faces.clone());
    let circular = project_circular_pattern(
        &circular_scope(),
        &[group(10, 30, DesignOperandRole::ROLE_0X10)],
        &bounded,
        &[],
        &[],
        &listed(&[5]),
        &mut Vec::new(),
    )
    .expect("circular pattern");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = circular else {
        panic!("circular pattern definition");
    };
    assert_eq!(seeds, vec![faces]);

    let seeds = |groups: &[DesignConstructionOperandGroup], operands: &[_]| {
        unprojected_pattern_seeds(
            &rectangular_scope(),
            groups,
            operands,
            &[],
            &[],
            &listed(&[5]),
            &mut Vec::new(),
        )
    };
    let face_group = || group(10, 30, DesignOperandRole::ROLE_0X10);
    assert!(seeds(&[face_group()], &[]).is_empty());
    assert!(seeds(
        &[face_group()],
        &[seed_face_operand(ConstructionRecipeKind::Face)]
    )
    .is_empty());
    let mut later = face_group();
    later.scope_reference_ordinal = 2;
    assert!(seeds(&[later], &bounded).is_empty());
    assert!(seeds(
        &[
            group(10, 20, DesignOperandRole::BODIES_B),
            group(10, 21, DesignOperandRole::BODIES_A),
            face_group(),
        ],
        &bounded,
    )
    .is_empty());
    assert_eq!(
        seeds(
            &[group(10, 20, DesignOperandRole::BODIES_B), face_group()],
            &bounded
        ),
        vec![PatternSeed::Bodies(BodySelection::Native(
            "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
        ))]
    );
}

#[test]
fn an_unprojected_pattern_keeps_its_seeds() {
    use crate::records::recipes::ConstructionRecipeKind;

    let mirror = |previous_state: Option<i64>| {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:parameter-scope#10",
            crate::records::feature::scope::DesignFeatureKind::Mirror,
            10,
        );
        scope
            .try_edit(|draft| {
                draft.previous_history_state_id = previous_state;
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope
    };
    let feature = seed_scope(5, Some(2));
    let feature_seeds = |scope: DesignParameterScope| {
        unprojected_pattern_seeds(
            &scope,
            &[group(10, 30, DesignOperandRole::BODIES_A)],
            &[],
            &[compact_entity_selection(30, 31, 5)],
            &[scope.clone(), feature.clone()],
            &listed(&[5]),
            &mut Vec::new(),
        )
    };
    assert_eq!(
        feature_seeds(mirror(Some(2))),
        vec![PatternSeed::Feature(crate::ids::neutral_feature_id(
            &feature
        ))]
    );
    assert!(feature_seeds(mirror(Some(1))).is_empty());
    assert!(feature_seeds(mirror(None)).is_empty());
    assert_eq!(
        unprojected_pattern_seeds(
            &mirror(None),
            &[group(10, 30, DesignOperandRole::ROLE_0X10)],
            &[seed_face_operand(ConstructionRecipeKind::BoundedFace)],
            &[],
            &[],
            &listed(&[5]),
            &mut Vec::new(),
        ),
        vec![PatternSeed::Faces(FaceSelection::Native(
            "f3d:Design/BulkStream.dat:design-construction-operand-group#30".into(),
        ))]
    );
}

fn coincident_direction_scope(symmetric: bool, total_extent: bool) -> DesignParameterScope {
    coincident_direction_scope_with_seed_words(symmetric, total_extent, 0, 0, 0)
}

/// `coincident_direction_scope` whose distribution block stores `seed_kind`,
/// `body_references` body references, and `untyped` references to records of
/// another type.
fn coincident_direction_scope_with_seed_words(
    symmetric: bool,
    total_extent: bool,
    seed_kind: u32,
    body_references: u32,
    untyped: u32,
) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::RPattern,
        10,
    );
    let mut direction = direction_record(20, [2.0, 0.0, 0.0]);
    direction.distance_type = 7;
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::RectangularPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            DesignRectangularPatternConstruction::try_from(
                crate::records::feature::patterns::DesignRectangularPatternConstructionWire {
                    u_count: 3,
                    v_count: 3,
                    u_extent: 6.0,
                    v_extent: 0.0,
                    owner_record_indices: [11, 12, 13, 14],
                    value_offsets: [101, 102, 103, 104],
                    instances: None,
                    instance_search_cap: None,
                    directions: vec![direction],
                    secondary_direction: None,
                    distribution: Some(
                        crate::records::feature::patterns::DesignRectangularPatternDistribution {
                            record_index: 15,
                            symmetric: crate::records::identity::Located {
                                value: symmetric,
                                offset: 109,
                            },
                            symmetric_two: crate::records::identity::Located {
                                value: false,
                                offset: 110,
                            },
                            total_extent: crate::records::identity::Located {
                                value: total_extent,
                                offset: 158,
                            },
                            compute_option: crate::records::identity::Located {
                                value: 2,
                                offset: 146,
                            },
                            seed_kind: crate::records::identity::Located {
                                value: seed_kind,
                                offset: 150,
                            },
                            body_references: crate::records::identity::Located {
                                value: body_references,
                                offset: 134,
                            },
                            untyped_body_references: untyped,
                            suppressed: crate::records::identity::Located {
                                value: Vec::new(),
                                offset: 158,
                            },
                        },
                    ),
                },
            )
            .unwrap(),
        );
    }
    scope
}

fn single_direction(definition: FeatureDefinition) -> (f64, u32) {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = definition else {
        panic!("rectangular pattern definition");
    };
    let PatternTransform::Linear {
        direction,
        spacing,
        count,
        second,
        ..
    } = pattern.definition().clone()
    else {
        panic!("linear rectangular pattern");
    };
    assert!(second.is_none());
    let direction =
        cadmpeg_ir::features::geometry::FiniteVector3::from(direction.expect("first direction"))
            .get();
    assert!((direction.x - 1.0).abs() < EPS_SPACING);
    (spacing.get(), count)
}

#[test]
fn rectangular_pattern_drops_a_coincident_direction_and_reports_it() {
    let mut losses = Vec::new();
    let spacing = project_rectangular_pattern_scalars(
        &coincident_direction_scope(false, false),
        &[],
        &[],
        &[],
        &[],
        &listed(&[5]),
        &mut losses,
    )
    .expect("one active direction");
    let (spacing, count) = single_direction(spacing);
    assert!((spacing - 60.0).abs() < EPS_SPACING);
    assert_eq!(count, 3);
    let [loss] = losses.as_slice() else {
        panic!("one placement loss");
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::FeatureRectangularPatternPlacementNotTransferred.kind()
    );
    assert!(loss
        .message
        .contains("the 3 coincident instances of direction two"));
    assert!(!loss.message.contains("symmetric"));
}

#[test]
fn rectangular_pattern_payload_distance_type_selects_extent_or_spacing() {
    let extent = project_rectangular_pattern_scalars(
        &coincident_direction_scope(false, true),
        &[],
        &[],
        &[],
        &[],
        &listed(&[5]),
        &mut Vec::new(),
    )
    .expect("extent distance");
    let (spacing, count) = single_direction(extent);
    assert!((spacing - 30.0).abs() < EPS_SPACING);
    assert_eq!(count, 3);
}

#[test]
fn rectangular_pattern_carries_a_symmetric_spacing_direction() {
    let mut losses = Vec::new();
    let symmetric = project_rectangular_pattern_scalars(
        &coincident_direction_scope(true, false),
        &[],
        &[],
        &[],
        &[],
        &listed(&[5]),
        &mut losses,
    )
    .expect("symmetric spacing direction");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = &symmetric else {
        panic!("rectangular pattern definition");
    };
    assert!(matches!(
        pattern.definition(),
        PatternTransform::Linear {
            symmetric: true,
            ..
        }
    ));
    let (spacing, count) = single_direction(symmetric);
    assert!((spacing - 60.0).abs() < EPS_SPACING);
    assert_eq!(count, 3);
    let [loss] = losses.as_slice() else {
        panic!("one placement loss");
    };
    assert!(!loss.message.contains("symmetric"));
}

/// The loss codes of `losses`, in order.
fn codes(losses: &[cadmpeg_ir::report::loss::LossNote]) -> Vec<cadmpeg_ir::report::loss::LossKind> {
    losses.iter().map(|loss| loss.code.clone()).collect()
}

#[test]
fn an_odd_symmetric_extent_direction_reads_the_extent_from_the_seed_to_either_final_instance() {
    // Direction one: 3 instances, extent 6 cm from the seed to each final
    // instance, so 60 mm apart.
    let mut losses = Vec::new();
    let definition = project_rectangular_pattern_scalars(
        &coincident_direction_scope(true, true),
        &[],
        &[],
        &[],
        &[],
        &listed(&[5]),
        &mut losses,
    )
    .expect("symmetric extent direction");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = &definition
    else {
        panic!("rectangular pattern definition");
    };
    assert!(matches!(
        pattern.definition(),
        PatternTransform::Linear {
            symmetric: true,
            ..
        }
    ));
    let (spacing, count) = single_direction(definition);
    assert!((spacing - 60.0).abs() < EPS_SPACING);
    assert_eq!(count, 3);
    assert_eq!(
        codes(&losses),
        [crate::loss::F3dLossCode::FeatureRectangularPatternPlacementNotTransferred.kind()]
    );
}

#[test]
fn a_payload_seed_kind_or_body_reference_that_disagrees_with_the_seed_group_is_charged() {
    use crate::design::feature_project::pattern_seeds::distribution_seed_conflict;
    use crate::records::{
        feature::patterns::DesignRectangularPatternDistribution, identity::Located,
    };

    let scope = rectangular_scope();
    let distribution =
        |seed_kind: u32, bodies: u32, untyped: u32| DesignRectangularPatternDistribution {
            record_index: 40,
            symmetric: Located {
                value: false,
                offset: 109,
            },
            symmetric_two: Located {
                value: false,
                offset: 110,
            },
            total_extent: Located {
                value: false,
                offset: 158,
            },
            compute_option: Located {
                value: 1,
                offset: 146,
            },
            seed_kind: Located {
                value: seed_kind,
                offset: 150,
            },
            body_references: Located {
                value: bodies,
                offset: 134,
            },
            untyped_body_references: untyped,
            suppressed: Located {
                value: Vec::new(),
                offset: 170,
            },
        };
    let faces = [PatternSeed::Faces(FaceSelection::Native(
        "faces".to_owned(),
    ))];
    let bodies = [PatternSeed::Bodies(BodySelection::Native(
        "bodies".to_owned(),
    ))];
    let features = [PatternSeed::Feature(crate::ids::neutral_feature_id(&scope))];
    let code = |seeds: &[PatternSeed], distribution: &DesignRectangularPatternDistribution| {
        distribution_seed_conflict(&scope, distribution, seeds).map(|note| note.code)
    };
    let charged = Some(crate::loss::F3dLossCode::FeaturePatternPayloadSeedUnconfirmed.kind());
    // Agreeing words charge nothing; a feature seed can store body references.
    assert_eq!(code(&faces, &distribution(0, 0, 0)), None);
    assert_eq!(code(&features, &distribution(1, 2, 0)), None);
    assert_eq!(code(&features, &distribution(1, 0, 0)), None);
    assert_eq!(code(&bodies, &distribution(2, 0, 0)), None);
    // A seed kind that names another kind of seed.
    assert_eq!(code(&faces, &distribution(1, 0, 0)), charged);
    assert_eq!(code(&bodies, &distribution(0, 0, 0)), charged);
    // Body references with a face or body seed.
    assert_eq!(code(&faces, &distribution(0, 1, 0)), charged);
    assert_eq!(code(&bodies, &distribution(2, 1, 0)), charged);
    // A body reference that names no body browser node.
    assert_eq!(code(&features, &distribution(1, 2, 1)), charged);
    // Unresolved seeds settle no seed kind.
    let native = [PatternSeed::Native("seed".to_owned())];
    assert_eq!(code(&native, &distribution(1, 0, 0)), None);
    assert_eq!(code(&[], &distribution(1, 0, 0)), None);
    let note = distribution_seed_conflict(&scope, &distribution(1, 1, 0), &faces).unwrap();
    assert!(note
        .message
        .contains("seed kind 1 at byte 150 where its seeds are faces"));
    assert!(note
        .message
        .contains("1 body references at byte 134 with a face or body seed"));
    // Block-internal words charge without resolved seeds.
    assert_eq!(code(&[], &distribution(3, 0, 0)), charged);
    assert_eq!(code(&native, &distribution(3, 0, 0)), charged);
    assert_eq!(code(&[], &distribution(0, 1, 0)), charged);
    assert_eq!(code(&[], &distribution(2, 1, 0)), charged);
    assert_eq!(code(&[], &distribution(1, 1, 1)), charged);
    assert_eq!(code(&[], &distribution(1, 1, 0)), None);
    let note = distribution_seed_conflict(&scope, &distribution(3, 0, 0), &[]).unwrap();
    assert!(note
        .message
        .contains("seed kind 3 at byte 150, above the last seed kind 2"));
}

#[test]
fn a_symmetric_extent_pattern_charges_a_disagreeing_payload_seed() {
    let charged = crate::loss::F3dLossCode::FeaturePatternPayloadSeedUnconfirmed.kind();
    let project = |scope: &DesignParameterScope, groups: &[DesignConstructionOperandGroup]| {
        let mut losses = Vec::new();
        let definition = project_rectangular_pattern_scalars(
            scope,
            groups,
            &[],
            &[],
            &[],
            &std::collections::HashMap::new(),
            &mut losses,
        )
        .expect("symmetric extent direction");
        let seed_losses = losses
            .into_iter()
            .filter(|loss| loss.code == charged)
            .collect::<Vec<_>>();
        (definition, seed_losses)
    };
    let body_group = [group(10, 30, DesignOperandRole::BODIES_B)];
    // Seed kind 0 against a body seed group.
    let (definition, losses) = project(
        &coincident_direction_scope_with_seed_words(true, true, 0, 0, 0),
        &body_group,
    );
    assert_eq!(single_direction(definition).1, 3);
    let [loss] = losses.as_slice() else {
        panic!("one seed loss: {losses:?}");
    };
    assert!(loss
        .message
        .contains("seed kind 0 at byte 150 where its seeds are bodies"));
    // A seed kind above 2, and a body reference to a record of another type.
    for (seed_kind, bodies, untyped) in [(3, 0, 0), (1, 1, 1)] {
        let (_, losses) = project(
            &coincident_direction_scope_with_seed_words(true, true, seed_kind, bodies, untyped),
            &[],
        );
        assert_eq!(losses.len(), 1, "{seed_kind} {bodies} {untyped}");
    }
    // The agreeing body seed charges no seed loss.
    let (_, losses) = project(
        &coincident_direction_scope_with_seed_words(true, true, 2, 0, 0),
        &body_group,
    );
    assert!(losses.is_empty());
}

#[test]
fn a_flagged_direction_without_a_distribution_block_reads_an_extent_and_charges_it() {
    let scope = |distance_type: u32| {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:parameter-scope#10",
            crate::records::feature::scope::DesignFeatureKind::RPattern,
            10,
        );
        let mut direction = direction_record(20, [2.0, 0.0, 0.0]);
        direction.distance_type = distance_type;
        if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::RectangularPattern(slot) =
            scope.payload_mut()
        {
            *slot = Some(
                DesignRectangularPatternConstruction::try_from(
                    crate::records::feature::patterns::DesignRectangularPatternConstructionWire {
                        u_count: 3,
                        v_count: 1,
                        u_extent: 10.0,
                        v_extent: 0.0,
                        owner_record_indices: [11, 12, 13, 14],
                        value_offsets: [101, 102, 103, 104],
                        instances: None,
                        instance_search_cap: None,
                        directions: vec![direction],
                        secondary_direction: None,
                        distribution: None,
                    },
                )
                .unwrap(),
            );
        }
        scope
    };
    for (distance_type, charged) in [(10, true), (8, false)] {
        let mut losses = Vec::new();
        let definition = project_rectangular_pattern_scalars(
            &scope(distance_type),
            &[],
            &[],
            &[],
            &[],
            &std::collections::HashMap::new(),
            &mut losses,
        )
        .expect("extent direction");
        let (spacing, count) = single_direction(definition);
        assert!((spacing - 50.0).abs() < EPS_SPACING);
        assert_eq!(count, 3);
        assert_eq!(
            losses
                .iter()
                .map(|loss| loss.code.clone())
                .collect::<Vec<_>>(),
            if charged {
                vec![crate::loss::F3dLossCode::FeatureRectangularPatternDistanceFormAssumed.kind()]
            } else {
                Vec::new()
            },
            "distance type {distance_type}"
        );
    }
}

#[test]
fn rectangular_pattern_without_a_direction_source_charges_the_unresolved_direction() {
    let mut losses = Vec::new();
    let definition = project_rectangular_pattern_scalars(
        &rectangular_scope(),
        &[],
        &[],
        &[],
        &[],
        &listed(&[5]),
        &mut losses,
    )
    .expect("one active direction");
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = &definition
    else {
        panic!("rectangular pattern definition");
    };
    assert!(matches!(
        pattern.definition(),
        PatternTransform::Linear {
            direction: None,
            ..
        }
    ));
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        [crate::loss::F3dLossCode::FeatureRectangularPatternDirectionUnresolved.kind()]
    );

    let mut scope = rectangular_scope();
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(Some(construction)) =
        scope.payload_mut()
    {
        let mut wire =
            crate::records::feature::patterns::DesignRectangularPatternConstructionWire::from(
                construction.clone(),
            );
        wire.directions = vec![direction_record(20, [2.0, 0.0, 0.0])];
        *construction = DesignRectangularPatternConstruction::try_from(wire).unwrap();
    }
    let mut losses = Vec::new();
    project_rectangular_pattern_scalars(&scope, &[], &[], &[], &[], &listed(&[5]), &mut losses)
        .expect("one active direction");
    assert!(losses.is_empty());
}

/// A pattern whose direction one repeats three instances in place and whose
/// direction two places two instances 1.5 cm apart, from
/// `coincident_direction_scope`'s symmetric adjacent distribution block.
fn inactive_direction_one_scope(
    directions: Vec<crate::records::feature::patterns::DesignRectangularPatternDirection>,
    secondary: Option<[f64; 3]>,
    distribution: bool,
) -> DesignParameterScope {
    let mut scope = coincident_direction_scope(true, false);
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(Some(construction)) =
        scope.payload_mut()
    {
        let mut wire =
            crate::records::feature::patterns::DesignRectangularPatternConstructionWire::from(
                construction.clone(),
            );
        wire.u_count = 3;
        wire.u_extent = 0.0;
        wire.v_count = 2;
        wire.v_extent = 1.5;
        wire.directions = directions;
        wire.secondary_direction = secondary.map(|value| crate::records::identity::Located {
            value: crate::test_support::reals(value),
            offset: 63,
        });
        if !distribution {
            wire.distribution = None;
        }
        *construction = DesignRectangularPatternConstruction::try_from(wire).unwrap();
    }
    scope
}

fn linear_parts(
    definition: &FeatureDefinition,
) -> (Option<cadmpeg_ir::math::Vector3>, f64, u32, bool) {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = definition else {
        panic!("rectangular pattern definition");
    };
    let PatternTransform::Linear {
        direction,
        spacing,
        count,
        second,
        symmetric,
        ..
    } = pattern.definition().clone()
    else {
        panic!("linear rectangular pattern");
    };
    assert!(second.is_none());
    (
        direction
            .map(|direction| cadmpeg_ir::features::geometry::FiniteVector3::from(direction).get()),
        spacing.get(),
        count,
        symmetric,
    )
}

#[test]
fn an_inactive_symmetric_direction_one_keeps_direction_two_and_drops_the_flag() {
    let mut u_record = direction_record(20, [2.0, 0.0, 0.0]);
    u_record.distance_type = 10;
    let dropped =
        crate::loss::F3dLossCode::FeatureRectangularPatternInactiveSymmetricDropped.kind();
    let placement =
        crate::loss::F3dLossCode::FeatureRectangularPatternPlacementNotTransferred.kind();
    let assumed =
        crate::loss::F3dLossCode::FeatureRectangularPatternDirectionOneRecordAssumed.kind();
    let codes = |losses: &[cadmpeg_ir::report::loss::LossNote]| {
        losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>()
    };

    let mut losses = Vec::new();
    let definition = project_rectangular_pattern_scalars(
        &inactive_direction_one_scope(vec![u_record], Some([0.0, 3.0, 0.0]), true),
        &[],
        &[],
        &[],
        &[],
        &listed(&[5]),
        &mut losses,
    )
    .expect("payload direction two");
    let (direction, spacing, count, symmetric) = linear_parts(&definition);
    let direction = direction.expect("payload direction");
    assert!((direction.y - 1.0).abs() < EPS_SPACING);
    assert!(direction.x.abs() < EPS_SPACING && direction.z.abs() < EPS_SPACING);
    assert!((spacing - 15.0).abs() < EPS_SPACING);
    assert_eq!(count, 2);
    assert!(!symmetric);
    assert_eq!(
        codes(&losses),
        [dropped.clone(), assumed, placement.clone()]
    );
    assert!(losses[2]
        .message
        .contains("the 3 coincident instances of direction one"));

    let mut losses = Vec::new();
    let definition = project_rectangular_pattern_scalars(
        &inactive_direction_one_scope(vec![u_record], None, true),
        &[],
        &[],
        &[],
        &[],
        &listed(&[5]),
        &mut losses,
    )
    .expect("direction two without a payload vector");
    let (direction, spacing, count, symmetric) = linear_parts(&definition);
    let direction = direction.expect("record direction");
    assert!((direction.x - 1.0).abs() < EPS_SPACING);
    assert!(direction.y.abs() < EPS_SPACING && direction.z.abs() < EPS_SPACING);
    assert!((spacing - 15.0).abs() < EPS_SPACING);
    assert_eq!(count, 2);
    assert!(!symmetric);
    assert_eq!(codes(&losses), [dropped, placement.clone()]);

    let mut losses = Vec::new();
    let definition = project_rectangular_pattern_scalars(
        &inactive_direction_one_scope(vec![direction_record(30, [0.0, 0.0, 4.0])], None, false),
        &[],
        &[],
        &[],
        &[],
        &listed(&[5]),
        &mut losses,
    )
    .expect("direction two record");
    let (direction, _, count, symmetric) = linear_parts(&definition);
    let direction = direction.expect("record direction");
    assert!((direction.z - 1.0).abs() < EPS_SPACING);
    assert_eq!(count, 2);
    assert!(!symmetric);
    assert_eq!(codes(&losses), [placement]);
}

/// `scope` with a pattern payload distribution that stores the symmetric
/// bytes of both directions and the given distance type.
fn with_distribution(
    scope: &DesignParameterScope,
    [one, two]: [bool; 2],
    total_extent: bool,
    counts: Option<(u32, f64)>,
) -> DesignParameterScope {
    use crate::records::feature::patterns::{
        DesignRectangularPatternConstructionWire, DesignRectangularPatternDistribution,
    };
    use crate::records::identity::Located;
    let mut wire = DesignRectangularPatternConstructionWire::from(
        scope.rectangular_pattern_construction().unwrap().clone(),
    );
    if let Some((u_count, u_extent)) = counts {
        wire.u_count = u_count;
        wire.u_extent = u_extent;
        // An inactive direction one keeps no direction record.
        wire.directions.remove(0);
    }
    wire.distribution = Some(DesignRectangularPatternDistribution {
        record_index: 15,
        symmetric: Located {
            value: one,
            offset: 109,
        },
        symmetric_two: Located {
            value: two,
            offset: 110,
        },
        total_extent: Located {
            value: total_extent,
            offset: 158,
        },
        compute_option: Located {
            value: 2,
            offset: 146,
        },
        seed_kind: Located {
            value: 0,
            offset: 150,
        },
        body_references: Located {
            value: 0,
            offset: 134,
        },
        untyped_body_references: 0,
        suppressed: Located {
            value: Vec::new(),
            offset: 170,
        },
    });
    let mut scope = scope.clone();
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::RectangularPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignRectangularPatternConstruction::try_from(wire).unwrap());
    }
    scope
}

/// The symmetric flags of the two directions of a projected linear pattern.
fn symmetric_flags(scope: &DesignParameterScope) -> Option<(bool, Option<bool>)> {
    let definition = project_rectangular_pattern_scalars(
        scope,
        &[],
        &[],
        &[],
        &[],
        &std::collections::HashMap::new(),
        &mut Vec::new(),
    )?;
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = definition else {
        panic!("rectangular pattern definition");
    };
    let PatternTransform::Linear {
        symmetric, second, ..
    } = pattern.definition().clone()
    else {
        panic!("linear rectangular pattern");
    };
    Some((symmetric, second.map(|second| second.symmetric)))
}

#[test]
fn rectangular_pattern_carries_the_symmetric_byte_of_each_direction() {
    let scope = two_direction_scope(vec![
        direction_record(20, [2.0, 0.0, 0.0]),
        direction_record(30, [0.0, 2.5, 0.0]),
    ]);
    for flags in [[false, false], [true, false], [false, true], [true, true]] {
        assert_eq!(
            symmetric_flags(&with_distribution(&scope, flags, false, None)),
            Some((flags[0], Some(flags[1]))),
            "{flags:?}"
        );
    }
}

/// Count, spacing in mm, and symmetric flag of each direction of a projected
/// two-direction linear pattern.
fn two_directions(definition: FeatureDefinition) -> [(u32, f64, bool); 2] {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) = definition else {
        panic!("rectangular pattern definition");
    };
    let PatternTransform::Linear {
        spacing,
        count,
        second: Some(second),
        symmetric,
        ..
    } = pattern.definition().clone()
    else {
        panic!("two-direction linear rectangular pattern");
    };
    [
        (count, spacing.get(), symmetric),
        (second.count, second.spacing.get(), second.symmetric),
    ]
}

#[test]
fn a_symmetric_extent_direction_keeps_the_other_direction() {
    // Direction one: 4 instances, extent 3 cm; symmetric, two of them lie
    // along the direction. Direction two: 2 instances, extent 1.2 cm.
    let scope = two_direction_scope(vec![
        direction_record(20, [2.0, 0.0, 0.0]),
        direction_record(30, [0.0, 2.5, 0.0]),
    ]);
    for flags in [[true, false], [false, true], [true, true], [false, false]] {
        let mut losses = Vec::new();
        let definition = project_rectangular_pattern_scalars(
            &with_distribution(&scope, flags, true, None),
            &[],
            &[],
            &[],
            &[],
            &std::collections::HashMap::new(),
            &mut losses,
        )
        .expect("two-direction extent pattern");
        let [(count, spacing, symmetric), (second_count, second_spacing, second_symmetric)] =
            two_directions(definition);
        assert_eq!((count, symmetric), (4, flags[0]), "{flags:?}");
        let expected = if flags[0] { 15.0 } else { 10.0 };
        assert!((spacing - expected).abs() < EPS_SPACING, "{flags:?}");
        assert_eq!((second_count, second_symmetric), (2, flags[1]), "{flags:?}");
        assert!((second_spacing - 12.0).abs() < EPS_SPACING, "{flags:?}");
        assert!(losses.is_empty(), "{flags:?} {losses:?}");
    }
}

#[test]
fn a_coincident_symmetric_direction_two_is_dropped_and_direction_one_kept() {
    let scope = with_distribution(
        &two_direction_scope(vec![
            direction_record(20, [2.0, 0.0, 0.0]),
            direction_record(30, [0.0, 2.5, 0.0]),
        ]),
        [false, true],
        false,
        None,
    );
    let mut wire =
        crate::records::feature::patterns::DesignRectangularPatternConstructionWire::from(
            scope.rectangular_pattern_construction().unwrap().clone(),
        );
    wire.v_extent = 0.0;
    // A coincident direction keeps no direction record.
    wire.directions.truncate(1);
    let mut scope = scope.clone();
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::RectangularPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignRectangularPatternConstruction::try_from(wire).unwrap());
    }
    let mut losses = Vec::new();
    let definition = project_rectangular_pattern_scalars(
        &scope,
        &[],
        &[],
        &[],
        &[],
        &std::collections::HashMap::new(),
        &mut losses,
    )
    .expect("direction one");
    let (spacing, count) = single_direction(definition);
    assert!((spacing - 30.0).abs() < EPS_SPACING);
    assert_eq!(count, 4);
    assert_eq!(
        codes(&losses),
        [
            crate::loss::F3dLossCode::FeatureRectangularPatternInactiveSymmetricDropped.kind(),
            crate::loss::F3dLossCode::FeatureRectangularPatternPlacementNotTransferred.kind()
        ]
    );
    assert!(losses[0]
        .message
        .contains("symmetric flag of its inactive direction two"));
    assert!(losses[1]
        .message
        .contains("the 2 coincident instances of direction two"));
}

#[test]
fn the_only_active_direction_takes_its_own_symmetric_byte() {
    // Direction one has count one, so direction two is the pattern's one direction.
    let scope = two_direction_scope(vec![
        direction_record(20, [2.0, 0.0, 0.0]),
        direction_record(30, [0.0, 2.5, 0.0]),
    ]);
    let only_two = |flags| with_distribution(&scope, flags, false, Some((1, 3.0)));
    assert_eq!(
        symmetric_flags(&only_two([false, true])),
        Some((true, None))
    );
    assert_eq!(
        symmetric_flags(&only_two([true, false])),
        Some((false, None))
    );
}
