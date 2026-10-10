// SPDX-License-Identifier: Apache-2.0

use super::super::{section_point_locus, visit_all_section_skamps, visit_section_skamps};
use crate::feature::definitions::{
    DefinitionIdentity, FeatureDefinition, FeatureOpaqueSegment, FeatureRelationTable,
    FeatureSegmentTable, FeatureSkamp, FeatureSolverTableHeader, SolverSubtable,
};
use crate::feature::segment_rows::SegmentRow;
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimit,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::SketchId;
use std::ops::ControlFlow;

fn definition() -> FeatureDefinition {
    FeatureDefinition {
        identity: DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(1),
            owner_feature_id: None,
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: None,
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    }
}

fn skamp_definition(count: u32) -> FeatureDefinition {
    FeatureDefinition {
        relations: Some(FeatureRelationTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            skamps: Some(SolverSubtable::Declared {
                header: FeatureSolverTableHeader {
                    declared_count: count,
                    entity_ref: 0,
                    offset: 0,
                },
                rows: (1..=count)
                    .map(|id| FeatureSkamp {
                        id,
                        kind: 0,
                        flags: 0,
                        status: 1,
                        items: Vec::new(),
                        offset: 0,
                    })
                    .collect(),
            }),
            triples: None,
            offset: 0,
        }),
        ..definition()
    }
}

fn with_work<T>(cap: u64, run: impl FnOnce(DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cap;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    run(ctx)
}

fn finish(ctx: DecodeContext<'_>, refusal: Option<ResourceLimit>) {
    if let Some(original) = refusal {
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    } else {
        ctx.finish_session().expect("active session");
    }
}

#[test]
fn point_locus_visits_only_present_opaque_rows() {
    let sketch = SketchId::mint("creo:model:sketch#1").expect("fixture identity");
    for count in [0_u32, 1, 3] {
        let definition = FeatureDefinition {
            segments: Some(FeatureSegmentTable {
                declared_count: count,
                has_elided_prototype: false,
                entity_ref: None,
                rows: (1..=count)
                    .map(|external_id| {
                        SegmentRow::Opaque(FeatureOpaqueSegment {
                            kind: 99,
                            directions: [None; 3],
                            point_ids: [None; 2],
                            center_id: None,
                            arc_orientation: None,
                            vertical_horizontal: None,
                            radius_ref: None,
                            radius2_ref: None,
                            external_id,
                            body: Vec::new(),
                            offset: 0,
                        })
                    })
                    .collect(),
                offset: 0,
            }),
            ..definition()
        };
        crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
            with_work(cap, |ctx| {
                let result = section_point_locus(&ctx, &definition, &sketch, 7);
                let refusal = match &result {
                    Err(CodecError::ResourceLimit(resource)) => {
                        assert_eq!((resource.used, resource.additional), (cap, 1));
                        assert_eq!(resource.operation, "creo point locus rows");
                        Some(*resource)
                    }
                    Ok(locus) => {
                        assert!(locus.is_none());
                        assert!(
                            ctx.resource_refusal().is_none(),
                            "successful traversal stays active"
                        );
                        let probe = ctx
                            .charge_work_limit(u64::MAX, "test completed locus visits")
                            .expect_err("work probe");
                        assert_eq!(probe.used, u64::from(count));
                        Some(probe)
                    }
                    Err(error) => panic!("unexpected locus error: {error:?}"),
                };
                if let Some(original) = refusal {
                    assert!(matches!(section_point_locus(&ctx, &definition, &sketch, 7),
                        Err(CodecError::ResourceLimit(actual)) if actual == original));
                }
                finish(ctx, refusal);
                result
            })
        });
    }
}

fn skamp_walk(all_rows: bool) {
    for count in [0_u32, 1, 3] {
        let definition = skamp_definition(count);
        crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
            with_work(cap, |ctx| {
                let mut seen = [0_u32; 3];
                let mut called = 0;
                let mut visit = |row: &FeatureSkamp| {
                    seen[called] = row.id;
                    called += 1;
                    Ok(ControlFlow::<()>::Continue(()))
                };
                let result = if all_rows {
                    visit_all_section_skamps(&ctx, &definition, &mut visit)
                } else {
                    visit_section_skamps(&ctx, &definition, true, &mut visit)
                };
                let refusal = match &result {
                    Err(CodecError::ResourceLimit(resource)) => {
                        assert_eq!(resource.operation, "creo relation skamp rows");
                        assert_eq!(
                            (resource.used, resource.additional),
                            (u64::try_from(called).expect("visits"), 1)
                        );
                        Some(*resource)
                    }
                    Ok(value) => {
                        assert_eq!(*value, ControlFlow::Continue(()));
                        assert_eq!(called, usize::try_from(count).expect("rows"));
                        assert!(
                            ctx.resource_refusal().is_none(),
                            "successful traversal stays active"
                        );
                        let probe = ctx
                            .charge_work_limit(u64::MAX, "test completed SKAMP visits")
                            .expect_err("work probe");
                        assert_eq!(probe.used, u64::from(count));
                        Some(probe)
                    }
                    Err(error) => panic!("unexpected SKAMP error: {error:?}"),
                };
                assert_eq!(&seen[..called], &[1, 2, 3][..called]);
                finish(ctx, refusal);
                result.map(|_| ())
            })
        });
    }
}

#[test]
fn section_skamp_walk_has_no_terminal_visit() {
    skamp_walk(false);
}

#[test]
fn all_section_skamp_walk_has_no_terminal_visit() {
    skamp_walk(true);
}

#[test]
fn inactive_skamps_still_consume_their_present_row_visit() {
    let mut definition = skamp_definition(3);
    definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("SKAMP table")
        .rows_mut()[0]
        .status = 0;
    crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &["creo relation skamp rows"],
        |cap| {
            with_work(cap, |ctx| {
                let mut called = 0_u64;
                let result = visit_section_skamps::<()>(&ctx, &definition, true, |_| {
                    called += 1;
                    Ok(ControlFlow::Continue(()))
                });
                let visited = match &result {
                    Err(CodecError::ResourceLimit(resource)) => {
                        assert_eq!(resource.operation, "creo relation skamp rows");
                        assert_eq!(resource.additional, 1);
                        resource.used
                    }
                    Ok(value) => {
                        assert_eq!(*value, ControlFlow::Continue(()));
                        3
                    }
                    Err(error) => panic!("unexpected SKAMP error: {error:?}"),
                };
                if visited == 0 {
                    assert_eq!(called, 0);
                } else {
                    assert_eq!(called.checked_add(1).expect("visited rows fit"), visited);
                }
                let refusal = match &result {
                    Err(CodecError::ResourceLimit(resource)) => Some(*resource),
                    Ok(_) => {
                        assert!(
                            ctx.resource_refusal().is_none(),
                            "successful traversal stays active"
                        );
                        let probe = ctx
                            .charge_work_limit(u64::MAX, "test completed inactive SKAMP visits")
                            .expect_err("work probe");
                        assert_eq!(probe.used, 3);
                        Some(probe)
                    }
                    _ => None,
                };
                finish(ctx, refusal);
                result.map(|_| ())
            })
        },
    );
}

#[test]
fn skamp_callback_break_stops_before_remaining_rows() {
    let definition = skamp_definition(3);
    for all_rows in [false, true] {
        crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits,
            &["creo relation skamp rows"],
            |cap| {
                with_work(cap, |ctx| {
                    let mut called = 0;
                    let mut visit = |row: &FeatureSkamp| {
                        called += 1;
                        Ok(ControlFlow::Break(row.id))
                    };
                    let result = if all_rows {
                        visit_all_section_skamps(&ctx, &definition, &mut visit)
                    } else {
                        visit_section_skamps(&ctx, &definition, false, &mut visit)
                    };
                    let refusal = match &result {
                        Err(CodecError::ResourceLimit(resource)) => {
                            assert_eq!(resource.operation, "creo relation skamp rows");
                            assert_eq!((resource.used, resource.additional), (0, 1));
                            assert_eq!(called, 0);
                            Some(*resource)
                        }
                        Ok(value) => {
                            assert_eq!(*value, ControlFlow::Break(1));
                            assert_eq!(called, 1);
                            assert!(
                                ctx.resource_refusal().is_none(),
                                "successful traversal stays active"
                            );
                            let probe = ctx
                                .charge_work_limit(u64::MAX, "test completed SKAMP callback visits")
                                .expect_err("work probe");
                            assert_eq!(probe.used, 1);
                            Some(probe)
                        }
                        Err(error) => panic!("unexpected callback error: {error:?}"),
                    };
                    finish(ctx, refusal);
                    result.map(|_| ())
                })
            },
        );
    }
}

#[test]
fn missing_empty_and_incomplete_locus_routes_are_free_and_keep_original_refusal() {
    let sketch = SketchId::mint("creo:model:sketch#1").expect("fixture identity");
    let mut incomplete = skamp_definition(0);
    incomplete
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("SKAMP table")
        .header_mut()
        .expect("header")
        .declared_count = 1;
    let mut empty_segments = definition();
    empty_segments.segments = Some(FeatureSegmentTable {
        declared_count: 0,
        has_elided_prototype: false,
        entity_ref: None,
        rows: crate::feature::segment_rows::SegmentRows::default(),
        offset: 0,
    });
    let mut absent_skamps = skamp_definition(0);
    absent_skamps.relations.as_mut().expect("relations").skamps = None;
    for definition in [
        definition(),
        empty_segments,
        absent_skamps,
        skamp_definition(0),
        incomplete,
    ] {
        for refused in [false, true] {
            with_work(0, |ctx| {
                let original = refused.then(|| {
                    ctx.charge_work_limit(1, "before empty locus")
                        .expect_err("zero work cap")
                });
                for _ in 0..2 {
                    let results = [
                        section_point_locus(&ctx, &definition, &sketch, 7)
                            .map(|value| value.is_none()),
                        visit_section_skamps::<()>(&ctx, &definition, true, |_| {
                            panic!("empty SKAMP callback")
                        })
                        .map(|value| value == ControlFlow::Continue(())),
                        visit_all_section_skamps::<()>(&ctx, &definition, |_| {
                            panic!("empty SKAMP callback")
                        })
                        .map(|value| value == ControlFlow::Continue(())),
                    ];
                    for result in results {
                        if let Some(original) = original {
                            assert!(matches!(result,
                                Err(CodecError::ResourceLimit(actual)) if actual == original));
                        } else {
                            assert!(result.expect("active recovery"));
                        }
                    }
                }
                finish(ctx, original);
            });
        }
    }
}
