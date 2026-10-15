// SPDX-License-Identifier: Apache-2.0

use super::super::intersect_incident_section_carriers;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};

fn line(end: [f64; 2]) -> SketchGeometry {
    SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(end[0], end[1]),
    })
    .expect("nondegenerate line")
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

#[test]
fn concurrent_carrier_pairs_admit_only_executed_first_rows_and_pairs() {
    let carriers = [
        line([1.0, 0.0]),
        line([0.0, 1.0]),
        line([1.0, 1.0]),
        line([1.0, -1.0]),
    ];
    for count in 0..=carriers.len() {
        let first_rows = (0..count).skip(1).len();
        let observed = std::cell::RefCell::new(Vec::new());
        let mut expected = Vec::new();
        for row in 0..first_rows {
            expected.push("creo incident section first carriers");
            expected.extend(std::iter::repeat_n(
                "creo incident section second carriers",
                count - row - 1,
            ));
        }
        let coordinate =
            crate::test_support::assert_refusal_order(ResourceDimension::WorkUnits, &[], |cap| {
                with_work(cap, |ctx| {
                    let result = intersect_incident_section_carriers(&ctx, &carriers[..count]);
                    match &result {
                        Ok(coordinate) => {
                            assert_eq!(*coordinate, (count >= 2).then_some([0.0, 0.0]));
                        }
                        Err(CodecError::ResourceLimit(original)) => {
                            assert_eq!(
                                (
                                    original.dimension,
                                    original.used,
                                    original.additional,
                                    original.limit
                                ),
                                (ResourceDimension::WorkUnits, cap, 1, cap)
                            );
                            observed.borrow_mut().push(original.operation);
                        }
                        Err(error) => panic!("unexpected error: {error:?}"),
                    }
                    result
                })
            });
        assert_eq!(coordinate, (count >= 2).then_some([0.0, 0.0]));
        assert_eq!(*observed.borrow(), expected);
    }
}

#[test]
fn unsupported_first_carrier_pair_stops_before_other_rows() {
    let carriers = [
        line([1.0, 0.0]),
        line([2.0, 0.0]),
        line([0.0, 1.0]),
        line([1.0, 1.0]),
    ];
    let coordinate = crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &[
            "creo incident section first carriers",
            "creo incident section second carriers",
        ],
        |cap| {
            with_work(cap, |ctx| {
                let result = intersect_incident_section_carriers(&ctx, &carriers);
                match &result {
                    Err(CodecError::ResourceLimit(original)) => {
                        assert_eq!(
                            (
                                original.dimension,
                                original.used,
                                original.additional,
                                original.limit
                            ),
                            (ResourceDimension::WorkUnits, cap, 1, cap)
                        );
                    }
                    Ok(coordinate) => {
                        assert_eq!(*coordinate, None);
                    }
                    Err(error) => panic!("unexpected pair error: {error:?}"),
                }
                result
            })
        },
    );
    assert_eq!(coordinate, None);
}

#[test]
fn missing_trim_recovery_is_free() {
    use crate::feature::definitions::{
        DefinitionIdentity, FeatureDefinition, FeatureSegment, FeatureSegmentKind,
    };
    use std::collections::BTreeMap;
    let definition = FeatureDefinition {
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
    };
    let segment = FeatureSegment {
        kind: FeatureSegmentKind::Line([1, 2]),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 7,
        body: Vec::new(),
        offset: 0,
    };
    let points = BTreeMap::new();
    let radii = BTreeMap::new();
    with_work(0, |ctx| {
        assert!(
            super::super::trimmed_section_segment_geometry_with_missing_line(
                &ctx,
                &definition,
                &points,
                &radii,
                &points,
                &segment,
                None,
            )
            .expect("missing trim table")
            .is_none()
        );
    });
}
