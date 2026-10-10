// SPDX-License-Identifier: Apache-2.0

use super::super::intersect_incident_section_carriers;

use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimit,
};
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
        let coordinate = crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits,
            &[],
            |cap| {
                with_work(cap, |ctx| {
                    let result = intersect_incident_section_carriers(&ctx, &carriers[..count]);
                    let refusal = match &result {
                        Ok(coordinate) => {
                            assert_eq!(*coordinate, (count >= 2).then_some([0.0, 0.0]));
                            None
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
                            assert!(
                                matches!(intersect_incident_section_carriers(&ctx, &carriers[..count]),
                            Err(CodecError::ResourceLimit(actual)) if actual == *original)
                            );
                            Some(*original)
                        }
                        Err(error) => panic!("unexpected error: {error:?}"),
                    };
                    finish(ctx, refusal);
                    result
                })
            },
        );
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
                let refusal = match &result {
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
                        Some(*original)
                    }
                    Ok(coordinate) => {
                        assert_eq!(*coordinate, None);
                        None
                    }
                    Err(error) => panic!("unexpected pair error: {error:?}"),
                };
                finish(ctx, refusal);
                result
            })
        },
    );
    assert_eq!(coordinate, None);
}
