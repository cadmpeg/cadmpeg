// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::{FeatureTrimEntity, FeatureTrimEntityTable, TrimEntityKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn assert_free_metadata_route(run: impl Fn(&DecodeContext<'_>) -> Result<bool, CodecError>) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    for refused in [false, true] {
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let original = refused.then(|| {
            ctx.charge_work_limit(1, "before radius metadata recovery")
                .expect_err("zero work")
        });
        for _ in 0..2 {
            if let Some(original) = original {
                assert!(
                    matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
            } else {
                assert!(run(&ctx).expect("metadata requires no source work"));
            }
        }
        if let Some(original) = original {
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
        } else {
            ctx.finish_session().expect("active free route");
        }
    }
}

#[test]
fn absent_trim_ids_are_free_and_preserve_original_refusal() {
    let definition = super::arc_radius_definition([3.0, 3.0]);
    assert_free_metadata_route(|ctx| {
        super::super::trim_segment_ids(ctx, &definition).map(|ids| ids.is_empty())
    });
}

#[test]
fn absent_arc_carrier_kind_is_free_and_preserves_original_refusal() {
    let mut segment = super::arc_carrier_segment();
    segment.kind = crate::feature::definitions::FeatureSegmentKind::Point(7);
    assert_free_metadata_route(|ctx| {
        super::super::section_arc_carrier(ctx, &BTreeMap::default(), &BTreeMap::default(), &segment)
            .map(|carrier| carrier.is_none())
    });
}

#[test]
fn absent_arc_carrier_fields_are_free_and_preserve_original_refusal() {
    let mut segment = super::arc_carrier_segment();
    segment.radius_ref = None;
    assert_free_metadata_route(|ctx| {
        super::super::section_arc_carrier(ctx, &BTreeMap::default(), &BTreeMap::default(), &segment)
            .map(|carrier| carrier.is_none())
    });
}

#[test]
fn absent_axis_carrier_kind_is_free_and_preserves_original_refusal() {
    let segment = super::arc_carrier_segment();
    assert_free_metadata_route(|ctx| {
        super::super::section_axis_line_carrier_with_points(ctx, &BTreeMap::default(), &segment)
            .map(|carrier| carrier.is_none())
    });
}

#[test]
fn absent_axis_carrier_selector_is_free_and_preserves_original_refusal() {
    let mut segment = super::arc_carrier_segment();
    segment.kind = crate::feature::definitions::FeatureSegmentKind::Line([1, 2]);
    assert_free_metadata_route(|ctx| {
        super::super::section_axis_line_carrier_with_points(ctx, &BTreeMap::default(), &segment)
            .map(|carrier| carrier.is_none())
    });
}

fn assert_absent_radius_relation(kind: u32) {
    let definition = super::arc_radius_definition([3.0, 3.0]);
    let relation = crate::feature::definitions::FeatureRelation {
        relation_id: 1,
        used: 0,
        operands: Vec::new(),
        operand_vectors: None,
        sign: 1,
        dimension_id: 0,
        relation_type: kind,
        body: Vec::new(),
        offset: 0,
    };
    assert_free_metadata_route(|ctx| {
        super::super::section_radius_relation_arc(ctx, &definition, &relation)
            .map(|carrier| carrier.is_none())
    });
}

#[test]
fn absent_radius_relation_kind_is_free_and_preserves_original_refusal() {
    assert_absent_radius_relation(0);
}

#[test]
fn absent_radius_relation_type5_dimension_is_free_and_preserves_original_refusal() {
    assert_absent_radius_relation(5);
}

#[test]
fn absent_radius_relation_type6_dimension_is_free_and_preserves_original_refusal() {
    assert_absent_radius_relation(6);
}

#[test]
fn trim_last_unmatched_row_keeps_its_unique_segment_without_an_empty_visit() {
    let mut definition = super::arc_radius_definition([3.0, 3.0]);
    definition
        .segments
        .as_mut()
        .expect("segments")
        .declared_count = 1;
    definition.trim_entities = Some(FeatureTrimEntityTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: vec![FeatureTrimEntity {
            external_id: 99,
            mode: Some(0),
            vertices: [2, 3],
            kind: TrimEntityKind::Arc { center_vertex: 1 },
            offset: 0,
        }],
        solved_external_ids: vec![99],
        offset: 0,
    });
    let ids = crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &["creo trim segment IDs"],
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || super::super::trim_segment_ids(&ctx, &definition);
            let result = run();
            let original = match &result {
                Err(CodecError::ResourceLimit(original)) => *original,
                Ok(ids) => {
                    assert_eq!(*ids, vec![Some(10)]);
                    ctx.charge_work_limit(1, "after trim partner boundary")
                        .expect_err("all route work used")
                }
                Err(error) => panic!("unexpected trim route: {error:?}"),
            };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            for _ in 0..2 {
                assert!(
                    matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
            }
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
            result
        },
    );
    assert_eq!(ids, vec![Some(10)]);
}

#[test]
fn repeated_point_axis_reads_one_held_coordinate() {
    use super::super::{axis_reference_line, section_axis_reference_line_geometry, SectionAxis};
    use crate::feature::definitions::{
        FeatureRelationTable, FeatureSegmentKind, FeatureSkamp, FeatureSkampItem,
        FeatureSolverTableHeader, SolverSubtable,
    };
    let mut definition = super::arc_radius_definition([3.0, 3.0]);
    let mut segment = super::arc_carrier_segment();
    segment.kind = FeatureSegmentKind::Point(7);
    let entity_id = segment.external_id;
    let item = || FeatureSkampItem {
        entity_id,
        sense: 0,
    };
    for axis in SectionAxis::ALL {
        segment.vertical_horizontal = Some(u32::try_from(axis.index()).expect("axis index"));
        definition.relations = Some(FeatureRelationTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            skamps: Some(SolverSubtable::Declared {
                header: FeatureSolverTableHeader {
                    declared_count: 2,
                    entity_ref: 0,
                    offset: 0,
                },
                rows: vec![
                    FeatureSkamp {
                        id: 1,
                        kind: if axis == SectionAxis::U { 2 } else { 1 },
                        flags: 0,
                        status: 1,
                        items: vec![item()],
                        offset: 0,
                    },
                    FeatureSkamp {
                        id: 2,
                        kind: 14,
                        flags: 0,
                        status: 1,
                        items: vec![item(), item(), item()],
                        offset: 0,
                    },
                ],
            }),
            triples: None,
            offset: 0,
        });
        for held in [Some(3.0), None, Some(f64::INFINITY)] {
            let mut coordinates = [None; 2];
            coordinates[axis.index()] = held;
            let points = std::collections::BTreeMap::from([(7, coordinates)]);
            let geometry = crate::test_support::assert_work_boundaries(
                &["creo section variable point lookup"],
                |ctx| section_axis_reference_line_geometry(ctx, &definition, &points, &segment),
            );
            assert_eq!(
                geometry,
                held.filter(|value| value.is_finite())
                    .and_then(|value| axis_reference_line(value, axis))
            );
        }
    }
}

#[test]
fn radius_resolution_reuses_one_coordinate_solution() {
    use crate::feature::definitions::{ScalarLane, VariableType};
    for incomplete_point in [None, Some(2), Some(1)] {
        let mut definition = super::arc_radius_definition([3.0, 3.0]);
        if let Some(point_id) = incomplete_point {
            for row in &mut definition.variables.as_mut().expect("variables").rows {
                if row.variable_type == VariableType::V && row.key == point_id {
                    row.value = ScalarLane::Undefined;
                    row.guess = ScalarLane::Undefined;
                }
            }
        }
        let coordinate_walks = std::cell::Cell::new(0);
        let radii = crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits,
            &["creo saved section ordinary segment rows"],
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let result = super::super::resolved_section_radii(&ctx, &definition);
                if let Err(CodecError::ResourceLimit(refusal)) = &result {
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                    if refusal.operation == "creo saved section ordinary segment rows" {
                        coordinate_walks.set(coordinate_walks.get() + 1);
                    }
                }
                result
            },
        );
        let expected = if incomplete_point == Some(1) {
            std::collections::BTreeMap::new()
        } else {
            std::collections::BTreeMap::from([(42, 3.0)])
        };
        assert_eq!(radii, expected);
        assert_eq!(coordinate_walks.get(), 1);
    }
}
