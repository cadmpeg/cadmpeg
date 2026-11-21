// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::feature::patterns::{
    DesignRectangularPatternConstruction, DesignRectangularPatternConstructionWire,
    DesignRectangularPatternDirection,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignScopePayloadMut};
use cadmpeg_ir::features::geometry::FeatureDirection3;
use cadmpeg_ir::features::patterns::{LinearPatternDirection, PatternKind};
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::PositiveLength;

fn project(operation: &LegacyOperation, scope: &DesignParameterScope) -> Option<FeatureOperation> {
    super::project(operation, scope, &mut Vec::new())
}

fn scope(
    counts: [u32; 2],
    distances: [f64; 2],
    distance_type: u32,
    symmetric: bool,
) -> DesignParameterScope {
    let directions = counts
        .into_iter()
        .zip(distances)
        .enumerate()
        .filter(|(_, (count, distance))| *count > 1 && *distance != 0.0)
        .map(|(axis, _)| DesignRectangularPatternDirection {
            record_index: if axis == 0 { 20 } else { 30 },
            selection_record_index: if axis == 0 { 23 } else { 33 },
            selection_kind: 8,
            distance_type,
            origin: crate::test_support::reals([0.0; 3]),
            origin_offset: 25,
            displacement: crate::test_support::reals(if axis == 0 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            }),
            displacement_offset: 49,
        })
        .collect();
    let construction =
        DesignRectangularPatternConstruction::try_from(DesignRectangularPatternConstructionWire {
            u_count: counts[0],
            v_count: counts[1],
            u_extent: distances[0],
            v_extent: distances[1],
            owner_record_indices: [11, 12, 13, 14],
            value_offsets: [100, 108, 116, 124],
            instances: None,
            instance_search_cap: None,
            directions,
            secondary_direction: None,
            distribution: Some(
                serde_json::from_value(serde_json::json!({
                    "record_index": 15,
                    "symmetric": { "value": symmetric, "offset": 109 },
                    "symmetric_two": { "value": false, "offset": 110 },
                    "total_extent": { "value": distance_type != 7, "offset": 158 },
                    "compute_option": { "value": 2, "offset": 146 },
                    "seed_kind": { "value": 1, "offset": 150 },
                    "body_references": { "value": 0, "offset": 134 },
                    "untyped_body_references": 0
                }))
                .unwrap(),
            ),
        })
        .unwrap();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::RPattern,
        10,
    );
    let DesignScopePayloadMut::RPattern(slot) = scope.payload_mut() else {
        panic!("rectangular pattern scope");
    };
    *slot = Some(construction);
    scope
}

fn operation(
    direction: Option<Vector3>,
    second: Option<LinearPatternDirection>,
    symmetric: bool,
) -> LegacyOperation {
    LegacyOperation::Pattern {
        seeds: vec![PatternSeed::Feature(
            FeatureId::mint("f3d:model:feature#seed").unwrap(),
        )],
        pattern: PatternKind::new(PatternTransform::Linear {
            direction: direction.map(|value| FeatureDirection3::new(value).unwrap()),
            spacing: PositiveLength::new(5.0).unwrap(),
            count: 3,
            second,
            omitted_instances: Vec::new(),
            symmetric,
        })
        .unwrap(),
    }
}

fn linear(operation: FeatureOperation) -> (LinearRun, Option<LinearRun>) {
    let FeatureOperation::Pattern(pattern) = operation else {
        panic!("typed pattern");
    };
    let PatternSeeds::Features { features, .. } = pattern.seeds else {
        panic!("feature seeds");
    };
    let Picks::Named(picks) = features.picks else {
        panic!("named feature seeds");
    };
    assert_eq!(
        picks.as_slice()[0].target,
        PickTarget::FeatureResult(FeatureId::mint("f3d:model:feature#seed").unwrap())
    );
    let PatternLayout::Linear {
        first,
        second,
        seed_only,
    } = pattern.layout
    else {
        panic!("linear layout");
    };
    assert_eq!(seed_only.is_some(), second.is_some());
    (first, second)
}

#[test]
fn two_directions_keep_their_source_spans_and_resolved_axes() {
    let scope = scope([3, 2], [1.5, -1.0], 8, false);
    let operation = operation(
        Some(Vector3::new(1.0, 0.0, 0.0)),
        Some(LinearPatternDirection {
            direction: FeatureDirection3::new(Vector3::new(0.0, -1.0, 0.0)).unwrap(),
            spacing: PositiveLength::new(10.0).unwrap(),
            count: 2,
            symmetric: false,
        }),
        false,
    );
    let (first, second) = linear(project(&operation, &scope).unwrap());
    assert_eq!(
        first.direction.resolved,
        UnitVector3::new(Vector3::new(1.0, 0.0, 0.0))
    );
    assert_eq!(
        first.run.spacing,
        Spacing::Span {
            count: Dimension::Literal(3),
            span: Dimension::Literal(Length::new(15.0).unwrap())
        }
    );
    let second = second.unwrap();
    assert_eq!(
        second.direction.resolved,
        UnitVector3::new(Vector3::new(0.0, -1.0, 0.0))
    );
    assert_eq!(
        second.run.spacing,
        Spacing::Span {
            count: Dimension::Literal(2),
            span: Dimension::Literal(Length::new(10.0).unwrap())
        }
    );
    assert_eq!(first.direction.inputs.len(), 1);
    assert_eq!(second.direction.inputs.len(), 1);
}

#[test]
fn an_inactive_u_direction_keeps_its_run_before_v() {
    let scope = scope([1, 3], [1.0, 2.0], 7, false);
    let operation = operation(Some(Vector3::new(0.0, 1.0, 0.0)), None, false);
    let (first, second) = linear(project(&operation, &scope).unwrap());
    assert_eq!(first.direction.resolved, None);
    assert!(matches!(
        first.run.spacing,
        Spacing::Pitch {
            count: Dimension::Literal(1),
            ..
        }
    ));
    let second = second.unwrap();
    assert_eq!(
        second.direction.resolved,
        UnitVector3::new(Vector3::new(0.0, 1.0, 0.0))
    );
    assert_eq!(
        second.run.spacing,
        Spacing::Pitch {
            count: Dimension::Literal(3),
            pitch: Dimension::Literal(Length::new(20.0).unwrap())
        }
    );
}

#[test]
fn a_symmetric_extent_keeps_the_total_count_and_per_side_span() {
    let scope = scope([3, 1], [2.0, 0.0], 8, true);
    let operation = operation(Some(Vector3::new(1.0, 0.0, 0.0)), None, true);
    let (first, _) = linear(project(&operation, &scope).unwrap());
    assert_eq!(
        first.run.spacing,
        Spacing::Span {
            count: Dimension::Literal(3),
            span: Dimension::Literal(Length::new(20.0).unwrap())
        }
    );
    assert_eq!(
        first.run.sides,
        Some(RunSides::Symmetric {
            count: Some(SymmetricMeasure::Total),
            extent: Some(SymmetricMeasure::PerSide)
        })
    );
}

#[test]
fn an_unresolved_direction_keeps_the_native_operation() {
    let scope = scope([3, 1], [1.0, 0.0], 8, false);
    assert!(project(&operation(None, None, false), &scope).is_none());
}

#[test]
fn a_count_mismatch_keeps_the_native_operation() {
    let scope = scope([4, 1], [1.0, 0.0], 8, false);
    assert!(project(
        &operation(Some(Vector3::new(1.0, 0.0, 0.0)), None, false),
        &scope
    )
    .is_none());
}

#[test]
fn an_unread_distribution_keeps_the_known_run_and_charges_a_pinned_loss() {
    let mut scope = scope([3, 1], [1.0, 0.0], 8, false);
    let DesignScopePayloadMut::RPattern(Some(construction)) = scope.payload_mut() else {
        panic!("rectangular pattern");
    };
    construction.set_distribution(None);
    let mut losses = Vec::new();
    let (first, _) = linear(
        super::project(
            &operation(Some(Vector3::new(1.0, 0.0, 0.0)), None, false),
            &scope,
            &mut losses,
        )
        .unwrap(),
    );
    assert_eq!(first.run.sides, None);
    assert_eq!(
        first.run.spacing,
        Spacing::Span {
            count: Dimension::Literal(3),
            span: Dimension::Literal(Length::new(10.0).unwrap()),
        }
    );
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::FeatureRectangularPatternDesignNative.kind()));
}

#[test]
fn an_unread_inactive_direction_charges_a_pinned_loss_and_keeps_both_runs() {
    let scope = scope([3, 1], [1.0, 0.0], 8, false);
    let mut losses = Vec::new();
    let projected = super::project(
        &operation(Some(Vector3::new(1.0, 0.0, 0.0)), None, false),
        &scope,
        &mut losses,
    )
    .unwrap();
    let (_, second) = linear(projected);
    let second = second.unwrap();
    assert_eq!(second.direction.resolved, None);
    assert_eq!(
        second.run.spacing,
        Spacing::Span {
            count: Dimension::Literal(1),
            span: Dimension::Literal(Length::new(0.0).unwrap()),
        }
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureRectangularPatternDesignNative.kind()
    );
}

#[test]
fn face_and_body_seed_groups_keep_their_native_selections_beside_the_typed_layout() {
    use cadmpeg_ir::features::selections::{BodySelection, FaceSelection};
    let scope = scope([3, 1], [1.0, 0.0], 8, false);
    for seed in [
        PatternSeed::Faces(FaceSelection::Native("f3d:seed-group#1".into())),
        PatternSeed::Bodies(BodySelection::Native("f3d:seed-group#1".into())),
    ] {
        let mut operation = operation(Some(Vector3::new(1.0, 0.0, 0.0)), None, false);
        let LegacyOperation::Pattern { seeds, .. } = &mut operation else {
            panic!("legacy pattern");
        };
        *seeds = vec![seed];
        let mut losses = Vec::new();
        let FeatureOperation::Pattern(pattern) =
            super::project(&operation, &scope, &mut losses).unwrap()
        else {
            panic!("typed pattern");
        };
        let selection = match &pattern.seeds {
            PatternSeeds::Faces(faces) => faces,
            PatternSeeds::Bodies { bodies, join } => {
                assert_eq!(*join, Some(false));
                bodies
            }
            PatternSeeds::Features { .. } => panic!("geometric seeds"),
        };
        let Picks::Named(picks) = &selection.picks else {
            panic!("native selection");
        };
        assert_eq!(
            picks.as_slice()[0].native.as_ref().unwrap().value,
            NativeNameValue::Text(cadmpeg_core::nonblank_literal!("f3d:seed-group#1"))
        );
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::FeatureRectangularPatternDesignNative.kind()));
    }
}
