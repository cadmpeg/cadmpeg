// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::feature::fillet_set::{DesignFilletSetList, DesignFilletSetListForm};
use crate::records::feature::scope::{DesignFeatureKind, DesignScopePayloadMut};
use crate::records::identity::RecordedValue;
use crate::records::parameters::{DesignParameterDraft, DesignParameterSource};

const STREAM: &str = "f3d:Design/BulkStream.dat";
const TOLERANCE: f64 = 1e-12;

fn parameter(owner: u32, kind: &str, unit: Option<&str>, value: f64) -> DesignParameter {
    let record_index = owner + 1;
    let at = u64::from(record_index) * 100;
    DesignParameter::try_from(DesignParameterDraft {
        id: format!("{STREAM}:design-parameter#{record_index}"),
        byte_offset: at,
        class_tag: "305".to_owned().try_into().unwrap(),
        record_index,
        source_ordinal: owner,
        source: DesignParameterSource::new(kind.to_owned(), Some(owner), None).unwrap(),
        expression: value.to_string(),
        expression_offset: at + 20,
        source_kind_offset: at + 40,
        unit: unit.map(|value| RecordedValue {
            value: value.to_owned(),
            offset: at + 50,
        }),
        name: format!("d{owner}"),
        name_offset: at + 60,
        evaluated_value: value,
        evaluated_value_offset: at + 80,
    })
    .unwrap()
}

fn scope_with_sets(sets: Vec<DesignFilletSet>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#90"),
        DesignFeatureKind::Fillet,
        90,
    );
    let DesignScopePayloadMut::Fillet(slot) = scope.payload_mut() else {
        panic!("Fillet scope");
    };
    slot.sets = Some(DesignFilletSetList {
        form: DesignFilletSetListForm::Compact,
        rolling_ball_corner: true,
        sets,
    });
    scope
}

fn short_set(group_record_index: u32, radius_owner_record_index: u32) -> DesignFilletSet {
    DesignFilletSet {
        record_index: group_record_index - 1,
        byte_offset: u64::from(group_record_index) * 100,
        frame_length: 80,
        group_record_index,
        tangency_weight_owner_record_index: None,
        tangent_chain: None,
        curvature_continuity: None,
        law: DesignFilletSetLaw::Constant {
            radius_owner_record_index,
        },
    }
}

#[test]
fn short_fillet_sets_project_each_explicit_radius_with_unresolved_edges() {
    let mut scope = scope_with_sets(vec![short_set(92, 108), short_set(95, 102)]);
    crate::design::test_support::name_scope_references(&mut scope, [92, 95, 102, 108]);
    let sets = scope.fillet_sets().unwrap();
    assert!(sets.projects_short_sets([95, 92]));
    assert!(!sets.projects_short_sets([95, 92, 99]));
    let radii = [
        parameter(102, "Radius", Some("cm"), 0.2),
        parameter(108, "Radius", Some("cm"), 0.3),
    ];
    let parameters = radii.each_ref().map(|parameter| (0, parameter));
    crate::test_support::with_decode_context(|ctx| {
        let mut losses = Vec::new();
        let Some(FeatureOperation::Fillet { groups }) = project(
            &scope,
            &parameters,
            &ProjectInputs::default(),
            ctx,
            &mut losses,
        )
        .unwrap() else {
            panic!("short constant Fillet");
        };
        let [first, second] = groups.as_slice() else {
            panic!("two Fillet groups");
        };
        for (group, expected) in [(first, 3.0), (second, 2.0)] {
            let RadiusSpec::Constant { radius } = group.radius else {
                panic!("constant radius");
            };
            assert!((radius.get() - expected).abs() < TOLERANCE);
            assert_eq!(group.edges, EdgeSelection::Unresolved);
            assert!(group.tangency_weight.is_none());
        }
        assert_eq!(losses.len(), 2);
        assert!(losses
            .iter()
            .all(|loss| loss.code == F3dLossCode::FeatureFilletSetGroupUnresolved.kind()));
    });
}

#[test]
fn short_fillet_set_weights_follow_scope_group_order() {
    let mut scope = scope_with_sets(vec![short_set(95, 102), short_set(92, 108)]);
    crate::design::test_support::name_scope_references(&mut scope, [92, 95, 102, 108]);
    let radii = [
        parameter(102, "Radius", Some("cm"), 0.2),
        parameter(108, "Radius", Some("cm"), 0.3),
    ];
    let weights = [
        parameter(120, "TangencyWeight", None, 0.7),
        parameter(122, "TangencyWeight", None, 0.4),
        parameter(124, "TangencyWeight", None, 0.9),
    ];
    for (weight_count, expected) in [(2, [Some(0.7), Some(0.4)]), (3, [None, None])] {
        let mut parameters = vec![
            (0, &radii[0]),
            (1, &radii[1]),
            (3, &weights[0]),
            (2, &weights[1]),
        ];
        if weight_count == 3 {
            parameters.push((4, &weights[2]));
        }
        crate::test_support::with_decode_context(|ctx| {
            let mut losses = Vec::new();
            let Some(FeatureOperation::Fillet { groups }) = project(
                &scope,
                &parameters,
                &ProjectInputs::default(),
                ctx,
                &mut losses,
            )
            .unwrap() else {
                panic!("short constant Fillet");
            };
            assert_eq!(groups.len(), expected.len());
            for (group, expected) in groups.iter().zip(expected) {
                match (group.tangency_weight, expected) {
                    (Some(weight), Some(expected)) => {
                        assert!((weight.get() - expected).abs() < TOLERANCE);
                    }
                    (None, None) => (),
                    _ => panic!("unexpected tangency-weight assignment"),
                }
            }
            assert_eq!(
                losses
                    .iter()
                    .filter(|loss| loss.code == F3dLossCode::FeatureFilletSetInputUnresolved.kind())
                    .count(),
                1
            );
        });
    }
}

#[test]
fn asymmetric_fillet_reports_an_undecoded_edge_group() {
    let group_record_index = 92;
    let mut scope = scope_with_sets(vec![DesignFilletSet {
        record_index: 91,
        byte_offset: 700,
        frame_length: u64::try_from(crate::layout::fillet_set_asymmetric::LEN).unwrap(),
        group_record_index,
        tangency_weight_owner_record_index: None,
        tangent_chain: None,
        curvature_continuity: None,
        law: DesignFilletSetLaw::Asymmetric {
            offset_one_owner_record_index: 100,
            offset_two_owner_record_index: 102,
        },
    }]);
    crate::design::test_support::name_scope_references(&mut scope, [group_record_index]);
    let offsets = [
        parameter(100, "EdgeOffset1", Some("cm"), 0.2),
        parameter(102, "EdgeOffset2", Some("cm"), 0.3),
    ];
    let parameters = offsets.each_ref().map(|parameter| (0, parameter));
    crate::test_support::with_decode_context(|ctx| {
        let mut losses = Vec::new();
        let Some(FeatureOperation::Fillet { groups }) = project(
            &scope,
            &parameters,
            &ProjectInputs::default(),
            ctx,
            &mut losses,
        )
        .unwrap() else {
            panic!("asymmetric Fillet");
        };
        let group = &groups.as_slice()[0];
        assert_eq!(group.edges, EdgeSelection::Unresolved);
        let RadiusSpec::Asymmetric {
            offset_one,
            offset_two,
        } = group.radius
        else {
            panic!("asymmetric radius law");
        };
        assert!((offset_one.get() - 2.0).abs() < TOLERANCE);
        assert!((offset_two.get() - 3.0).abs() < TOLERANCE);
        let [loss] = losses.as_slice() else {
            panic!("one Fillet group loss");
        };
        assert_eq!(
            loss.code,
            F3dLossCode::FeatureFilletSetGroupUnresolved.kind()
        );
    });
}

#[test]
fn variable_fillet_sorts_complete_midpoint_pairs_and_refuses_duplicate_positions() {
    let set = DesignFilletSet {
        record_index: 50,
        byte_offset: 700,
        frame_length: 153,
        group_record_index: 110,
        tangency_weight_owner_record_index: Some(42),
        tangent_chain: None,
        curvature_continuity: None,
        law: DesignFilletSetLaw::Variable {
            start_radius_owner_record_index: 10,
            end_radius_owner_record_index: 12,
            middle: [
                DesignFilletMidpointOwners {
                    radius_owner_record_index: 14,
                    position_owner_record_index: 18,
                },
                DesignFilletMidpointOwners {
                    radius_owner_record_index: 16,
                    position_owner_record_index: 20,
                },
            ],
        },
    };
    let scope = scope_with_sets(vec![set]);
    let inputs = |second_position| {
        [
            parameter(10, "StartRadius", Some("cm"), 0.1),
            parameter(12, "EndRadius", Some("cm"), 0.3),
            parameter(14, "MidRadius", Some("cm"), 0.42),
            parameter(16, "MidRadius", Some("cm"), 0.11),
            parameter(18, "MidParams", None, 0.65),
            parameter(20, "MidParams", None, second_position),
            parameter(42, "TangencyWeight", None, 1.0),
        ]
    };
    let read = |input: &[DesignParameter]| {
        let parameters = input
            .iter()
            .map(|parameter| (0, parameter))
            .collect::<Vec<_>>();
        crate::test_support::with_decode_context(|ctx| {
            let mut losses = Vec::new();
            let Some(FeatureOperation::Fillet { groups }) = project(
                &scope,
                &parameters,
                &ProjectInputs::default(),
                ctx,
                &mut losses,
            )
            .unwrap() else {
                panic!("variable Fillet");
            };
            (
                groups,
                losses.into_iter().map(|loss| loss.code).collect::<Vec<_>>(),
            )
        })
    };
    let (groups, losses) = read(&inputs(0.15));
    let RadiusSpec::Variable { points } = &groups.as_slice()[0].radius else {
        panic!("reordered variable radius law");
    };
    assert_eq!(
        losses,
        [
            F3dLossCode::FeatureFilletMidpointOrderAssumed.kind(),
            F3dLossCode::FeatureFilletSetGroupUnresolved.kind(),
        ]
    );
    assert_eq!(points.as_slice().len(), 4);
    for (point, (position, radius)) in
        points
            .as_slice()
            .iter()
            .zip([(0.0, 1.0), (0.15, 1.1), (0.65, 4.2), (1.0, 3.0)])
    {
        assert!((point.parameter.get() - position).abs() < TOLERANCE);
        assert!((point.radius.get() - radius).abs() < TOLERANCE);
    }
    let (groups, losses) = read(&inputs(0.65));
    assert!(matches!(
        groups.as_slice()[0].radius,
        RadiusSpec::Unresolved {
            form: Some(RadiusForm::Variable)
        }
    ));
    assert_eq!(
        losses,
        [
            F3dLossCode::FeatureFilletSetInputUnresolved.kind(),
            F3dLossCode::FeatureFilletSetGroupUnresolved.kind(),
        ]
    );
}

#[test]
fn chordal_fillet_sets_pair_lengths_and_weights_by_explicit_owner() {
    let set = |record_index, group_record_index, length_owner, weight_owner, tangent_chain| {
        DesignFilletSet {
            record_index,
            byte_offset: u64::from(record_index) * 100,
            frame_length: 97,
            group_record_index,
            tangency_weight_owner_record_index: Some(weight_owner),
            tangent_chain: Some(tangent_chain),
            curvature_continuity: None,
            law: DesignFilletSetLaw::Chordal {
                chord_length_owner_record_index: length_owner,
            },
        }
    };
    let scope = scope_with_sets(vec![
        set(91, 92, 100, 106, true),
        set(93, 94, 102, 104, false),
    ]);
    let lengths_and_weights = [
        parameter(100, "ChordLen", Some("cm"), 0.7),
        parameter(102, "ChordLen", Some("cm"), 0.2),
        parameter(104, "TangencyWeight", None, 0.8),
        parameter(106, "TangencyWeight", None, 0.6),
        parameter(108, "TangencyWeight", None, 0.9),
    ];
    let parameters = lengths_and_weights
        .each_ref()
        .map(|parameter| (0, parameter));
    crate::test_support::with_decode_context(|ctx| {
        let mut losses = Vec::new();
        let Some(FeatureOperation::Fillet { groups }) = project(
            &scope,
            &parameters,
            &ProjectInputs::default(),
            ctx,
            &mut losses,
        )
        .unwrap() else {
            panic!("two-set chordal operation");
        };
        assert_eq!(groups.as_slice().len(), 2);
        for (group, (length, weight)) in groups.as_slice().iter().zip([(7.0, 0.6), (2.0, 0.8)]) {
            let RadiusSpec::Chordal { chord_length } = group.radius else {
                panic!("chordal radius law");
            };
            assert!((chord_length.get() - length).abs() < TOLERANCE);
            assert!((group.tangency_weight.unwrap().get() - weight).abs() < TOLERANCE);
        }
        assert_eq!(
            losses.into_iter().map(|loss| loss.code).collect::<Vec<_>>(),
            [
                F3dLossCode::FeatureFilletSetInputUnresolved.kind(),
                F3dLossCode::FeatureFilletSetGroupUnresolved.kind(),
                F3dLossCode::FeatureFilletSetGroupUnresolved.kind(),
            ]
        );
    });
}
