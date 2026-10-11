// SPDX-License-Identifier: Apache-2.0

use crate::sketches::{
    SketchConstraintDefinitionInput as Constraint, SketchDistanceMeasurement, SketchDistancePair,
    SketchLocus, SpatialSketchConstraintDefinitionInput as SpatialConstraint,
    SpatialSketchEntityId, SpatialSketchEntityPair,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn visit_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_work_units = work;
    policy
}

fn locus(suffix: &str) -> SketchLocus {
    SketchLocus::Entity(format!("test:model:entity#{suffix}").try_into().unwrap())
}

fn spatial_id(suffix: &str) -> SpatialSketchEntityId {
    format!("test:model:entity#{suffix}").try_into().unwrap()
}

#[test]
fn sketch_fixed_locus_slots_borrow_without_scan_or_storage_charges() {
    let cases = [
        Constraint::ScalarEquality {
            first: 1,
            second: 2,
        },
        Constraint::Midpoint {
            point: locus("point"),
            entity: "test:model:entity#line".try_into().unwrap(),
        },
        Constraint::SnellsLaw {
            incident: locus("incident"),
            refracted: locus("refracted"),
            interface: "test:model:entity#interface".try_into().unwrap(),
            parameter: "test:model:parameter#ratio".try_into().unwrap(),
        },
        Constraint::EqualDistance {
            first: SketchDistancePair {
                first: locus("a"),
                second: locus("b"),
            },
            second: SketchDistancePair {
                first: locus("c"),
                second: locus("d"),
            },
        },
    ];
    for definition in &cases {
        let expected: &[&SketchLocus] = match definition {
            Constraint::ScalarEquality { .. } => &[],
            Constraint::Midpoint { point, .. } => &[point],
            Constraint::SnellsLaw {
                incident,
                refracted,
                ..
            } => &[incident, refracted],
            Constraint::EqualDistance { first, second } => {
                &[&first.first, &first.second, &second.first, &second.second]
            }
            _ => panic!("fixed fixture"),
        };
        let arena = DecodeArena::new();
        let policy = visit_policy(0);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut count = 0;
        super::super::visit_constraint_loci(&ctx, definition, |actual| {
            assert!(std::ptr::eq(actual, expected[count]));
            count += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(count, expected.len());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn sketch_variable_locus_visits_refuse_before_advancing_and_keep_original_fuse() {
    let cases = [
        Constraint::Group {
            elements: vec![locus("first"), locus("last")],
        },
        Constraint::CoincidentLoci {
            loci: vec![locus("first"), locus("last")],
        },
        Constraint::Text {
            elements: vec![locus("first"), locus("last")],
            text: "label".into(),
            font: None,
            is_text_height: false,
        },
    ];
    for definition in &cases {
        let elements = match definition {
            Constraint::Group { elements } | Constraint::Text { elements, .. } => elements,
            Constraint::CoincidentLoci { loci } => loci,
            _ => panic!("variable fixture"),
        };
        for cap in 0..=2 {
            let arena = DecodeArena::new();
            let policy = visit_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut count = 0;
            let result = super::super::visit_constraint_loci(&ctx, definition, |actual| {
                assert!(std::ptr::eq(actual, &raw const elements[count]));
                count += 1;
                Ok(())
            });
            assert_eq!(count, usize::try_from(cap).unwrap());
            if cap == 2 {
                result.unwrap();
                ctx.finish_session().unwrap();
            } else {
                let Err(CodecError::ResourceLimit(limit)) = result else {
                    panic!("visit refusal");
                };
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                assert_eq!(limit.operation, "sketch constraint locus scan");
                let replay = super::super::visit_constraint_loci(&ctx, &cases[0], |_| {
                    panic!("fused callback")
                });
                assert!(
                    matches!(replay, Err(CodecError::ResourceLimit(original)) if original == limit)
                );
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
                );
            }
        }
    }
}

#[test]
fn sketch_repeated_distance_visits_charge_measurements_once() {
    let definition = Constraint::RepeatedDistance {
        measurements: vec![
            SketchDistanceMeasurement::Distance {
                first: locus("a"),
                second: locus("b"),
            },
            SketchDistanceMeasurement::Horizontal {
                first: locus("c"),
                second: locus("d"),
            },
            SketchDistanceMeasurement::Vertical {
                first: locus("e"),
                second: locus("f"),
            },
        ],
        parameter: "test:model:parameter#distance".try_into().unwrap(),
    };
    let Constraint::RepeatedDistance { measurements, .. } = &definition else {
        panic!("fixture");
    };
    let expected = measurements
        .iter()
        .flat_map(|measurement| match measurement {
            SketchDistanceMeasurement::Distance { first, second }
            | SketchDistanceMeasurement::Horizontal { first, second }
            | SketchDistanceMeasurement::Vertical { first, second } => [first, second],
        })
        .collect::<Vec<_>>();
    for cap in 0..=3 {
        let arena = DecodeArena::new();
        let policy = visit_policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut count = 0;
        let result = super::super::visit_constraint_loci(&ctx, &definition, |actual| {
            assert!(std::ptr::eq(actual, expected[count]));
            count += 1;
            Ok(())
        });
        assert_eq!(count, 2 * usize::try_from(cap).unwrap());
        if cap == 3 {
            result.unwrap();
            ctx.finish_session().unwrap();
        } else {
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("measurement refusal");
            };
            assert_eq!(limit.operation, "sketch distance measurement scan");
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
        }
    }
}

#[test]
fn spatial_fixed_member_slots_borrow_without_scan_or_storage_charges() {
    let cases: [SpatialConstraint; 4] = [
        SpatialConstraint::Native {
            native_kind: "source".try_into().unwrap(),
            native_state: None,
            parameter: None,
            operands: vec![crate::sketches::SketchNativeOperand {
                native_kind: "axis".try_into().unwrap(),
                field: None,
                object_index: None,
                native_ref: None,
            }],
        },
        SpatialConstraint::LineLength {
            entity: spatial_id("line"),
            parameter: "test:model:parameter#length".try_into().unwrap(),
        },
        SpatialConstraint::Midpoint {
            point: spatial_id("point"),
            entity: spatial_id("line"),
        },
        SpatialConstraint::Symmetric {
            first: spatial_id("a"),
            second: spatial_id("b"),
            axis: spatial_id("axis"),
        },
    ];
    for definition in &cases {
        let expected: &[&SpatialSketchEntityId] = match definition {
            SpatialConstraint::Native { .. } => &[],
            SpatialConstraint::LineLength { entity, .. } => &[entity],
            SpatialConstraint::Midpoint { point, entity } => &[point, entity],
            SpatialConstraint::Symmetric {
                first,
                second,
                axis,
            } => &[first, second, axis],
            _ => panic!("fixed spatial fixture"),
        };
        let arena = DecodeArena::new();
        let policy = visit_policy(0);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut count = 0;
        super::super::visit_spatial_constraint_entities(&ctx, definition, |actual| {
            assert!(std::ptr::eq(actual, expected[count]));
            count += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(count, expected.len());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn spatial_variable_members_keep_list_order_and_refuse_at_reached_prefix() {
    let cases: [SpatialConstraint; 4] = [
        SpatialConstraint::SplineGroup {
            entities: vec![spatial_id("a"), spatial_id("b"), spatial_id("c")],
        },
        SpatialConstraint::RepeatedLineLength {
            entities: vec![spatial_id("a"), spatial_id("b"), spatial_id("c")],
            parameter: "test:model:parameter#length".try_into().unwrap(),
        },
        SpatialConstraint::ParallelLineSetDistance {
            first: vec![spatial_id("a")],
            second: vec![spatial_id("b"), spatial_id("c")],
            parameter: "test:model:parameter#distance".try_into().unwrap(),
        },
        SpatialConstraint::Offset {
            sources: vec![spatial_id("a"), spatial_id("b")],
            results: vec![spatial_id("c")],
            normal: crate::math::Vector3::new(0.0, 0.0, 1.0),
            distance: crate::scalar::Length::new(1.0).unwrap(),
            parameter: None,
        },
    ];
    for definition in &cases {
        let expected: Vec<_> = match definition {
            SpatialConstraint::SplineGroup { entities }
            | SpatialConstraint::RepeatedLineLength { entities, .. } => entities.iter().collect(),
            SpatialConstraint::ParallelLineSetDistance { first, second, .. }
            | SpatialConstraint::Offset {
                sources: first,
                results: second,
                ..
            } => first.iter().chain(second).collect(),
            _ => panic!("variable spatial fixture"),
        };
        for cap in 0..=3 {
            let arena = DecodeArena::new();
            let policy = visit_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut count = 0;
            let result =
                super::super::visit_spatial_constraint_entities(&ctx, definition, |actual| {
                    assert!(std::ptr::eq(actual, expected[count]));
                    count += 1;
                    Ok(())
                });
            assert_eq!(count, usize::try_from(cap).unwrap());
            if cap == 3 {
                result.unwrap();
                ctx.finish_session().unwrap();
            } else {
                let Err(CodecError::ResourceLimit(limit)) = result else {
                    panic!("member refusal");
                };
                assert_eq!(limit.operation, "spatial constraint member scan");
                let replay =
                    super::super::visit_spatial_constraint_entities(&ctx, definition, |_| {
                        panic!("fused spatial callback")
                    });
                assert!(
                    matches!(replay, Err(CodecError::ResourceLimit(original)) if original == limit)
                );
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
                );
            }
        }
    }
}

#[test]
fn spatial_repeated_parallel_pairs_charge_each_pair_once() {
    let definition: SpatialConstraint = SpatialConstraint::RepeatedParallelLineDistance {
        pairs: vec![
            SpatialSketchEntityPair {
                first: spatial_id("a"),
                second: spatial_id("b"),
            },
            SpatialSketchEntityPair {
                first: spatial_id("c"),
                second: spatial_id("d"),
            },
        ],
        parameter: "test:model:parameter#distance".try_into().unwrap(),
    };
    let SpatialConstraint::RepeatedParallelLineDistance { pairs, .. } = &definition else {
        panic!("fixture");
    };
    let expected = [
        &pairs[0].first,
        &pairs[0].second,
        &pairs[1].first,
        &pairs[1].second,
    ];
    for cap in 0..=2 {
        let arena = DecodeArena::new();
        let policy = visit_policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut count = 0;
        let result = super::super::visit_spatial_constraint_entities(&ctx, &definition, |actual| {
            assert!(std::ptr::eq(actual, expected[count]));
            count += 1;
            Ok(())
        });
        assert_eq!(count, 2 * usize::try_from(cap).unwrap());
        if cap == 2 {
            result.unwrap();
            ctx.finish_session().unwrap();
        } else {
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("pair refusal");
            };
            assert_eq!(limit.operation, "spatial constraint pair scan");
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
            );
        }
    }
}
