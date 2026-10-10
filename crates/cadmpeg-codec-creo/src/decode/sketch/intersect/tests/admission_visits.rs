// SPDX-License-Identifier: Apache-2.0

use super::super::{intersect_incident_section_carriers, trimmed_section_segment_geometry_with_missing_line};
use crate::feature::definitions::{DefinitionIdentity, FeatureDefinition, FeatureSegment, FeatureSegmentKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimit};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
use std::collections::BTreeMap;

fn line(end: [f64; 2]) -> SketchGeometry {
    SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0), end: Point2::new(end[0], end[1]),
    }).expect("nondegenerate line")
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
fn concurrent_carrier_pairs_admit_only_executed_first_rows_and_pairs() {
    let carriers = [line([1.0, 0.0]), line([0.0, 1.0]), line([1.0, 1.0]), line([1.0, -1.0])];
    for count in 0..=carriers.len() {
        let first_rows = count.saturating_sub(1);
        let pairs = count * first_rows / 2;
        let need = u64::try_from(first_rows + pairs).expect("fixed fixture count");
        for cap in 0..=need {
            with_work(cap, |ctx| {
                let result = intersect_incident_section_carriers(&ctx, &carriers[..count]);
                let refusal = match result {
                    Ok(coordinate) => {
                        assert_eq!(cap, need);
                        assert_eq!(coordinate, (count >= 2).then_some([0.0, 0.0]));
                        None
                    }
                    Err(CodecError::ResourceLimit(original)) => {
                        assert!(cap < need);
                        assert_eq!((original.dimension, original.used, original.additional, original.limit),
                            (ResourceDimension::WorkUnits, cap, 1, cap));
                        // Before each first-row visit, all pairs for earlier rows were visited.
                        let first_visit = (0..first_rows).any(|row| {
                            let preceding_pairs = row * (2 * count - row - 1) / 2;
                            u64::try_from(row + preceding_pairs).expect("fixed prefix") == cap
                        });
                        assert_eq!(original.operation, if first_visit {
                            "creo incident section first carriers"
                        } else {
                            "creo incident section second carriers"
                        });
                        assert!(matches!(intersect_incident_section_carriers(&ctx, &carriers[..count]),
                            Err(CodecError::ResourceLimit(actual)) if actual == original));
                        Some(original)
                    }
                    Err(error) => panic!("unexpected error: {error:?}"),
                };
                finish(ctx, refusal);
            });
        }
    }
}

#[test]
fn unsupported_first_carrier_pair_stops_before_other_rows() {
    let carriers = [line([1.0, 0.0]), line([2.0, 0.0]), line([0.0, 1.0]), line([1.0, 1.0])];
    for cap in 0..=2 {
        with_work(cap, |ctx| {
            let result = intersect_incident_section_carriers(&ctx, &carriers);
            let refusal = if cap < 2 {
                let Err(CodecError::ResourceLimit(original)) = result else {
                    panic!("pair visit must refuse before execution");
                };
                assert_eq!((original.dimension, original.used, original.additional, original.limit),
                    (ResourceDimension::WorkUnits, cap, 1, cap));
                assert_eq!(original.operation, if cap == 0 {
                    "creo incident section first carriers"
                } else {
                    "creo incident section second carriers"
                });
                Some(original)
            } else {
                assert_eq!(result.expect("parallel pair recovery"), None);
                None
            };
            finish(ctx, refusal);
        });
    }
}

#[test]
fn carrier_cardinality_and_missing_trim_routes_are_free_and_keep_original_refusal() {
    let carriers = [line([1.0, 0.0])];
    let definition = FeatureDefinition {
        identity: DefinitionIdentity::Parsed { schema_id: std::num::NonZeroU32::new(1), owner_feature_id: None },
        body: Vec::new(), parameter_frames: Vec::new(), outlines: Vec::new(), variables: None,
        segments: None, trim_entities: None, trim_vertices: None, order_table: None,
        section_3d: None, dimensions: None, relations: None, saved_section: None, offset: 0,
    };
    let segment = FeatureSegment {
        kind: FeatureSegmentKind::Line([1, 2]), directions: [None; 3], center_id: None,
        arc_orientation: None, vertical_horizontal: None, radius_ref: None, radius2_ref: None,
        external_id: 7, body: Vec::new(), offset: 0,
    };
    let points = BTreeMap::new();
    let radii = BTreeMap::new();
    for refused in [false, true] {
        with_work(0, |ctx| {
            let original = refused.then(|| ctx.charge_work_limit(1, "before empty carriers")
                .expect_err("zero work"));
            for _ in 0..2 {
                for count in 0..=1 {
                    let result = intersect_incident_section_carriers(&ctx, &carriers[..count]);
                    if let Some(original) = original {
                        assert!(matches!(result,
                            Err(CodecError::ResourceLimit(actual)) if actual == original));
                    } else {
                        assert_eq!(result.expect("no pairs"), None);
                    }
                }
                let result = trimmed_section_segment_geometry_with_missing_line(
                    &ctx, &definition, &points, &radii, &points, &segment, None,
                );
                if let Some(original) = original {
                    assert!(matches!(result,
                        Err(CodecError::ResourceLimit(actual)) if actual == original));
                } else {
                    assert!(result.expect("missing trim table").is_none());
                }
            }
            finish(ctx, original);
        });
    }
}
