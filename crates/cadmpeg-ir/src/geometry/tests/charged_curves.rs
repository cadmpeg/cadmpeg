// SPDX-License-Identifier: Apache-2.0

use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
use crate::geometry::{
    CompositeCurveSegment, CompositeCurveSegments, CompositeCurveTransition, CurveGeometry,
    PlacedCurve, SolvedCurveGeometry,
};
use crate::ids::{CurveId, ProceduralCurveId, UnknownId};
use crate::math::Point3;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn curves() -> Vec<CurveGeometry> {
    let unknown = SolvedCurveGeometry::Unknown {
        record: Some(UnknownId::mint("test:model:unknown#curve").unwrap()),
    };
    let basis =
        PlacedCurve::try_new(Box::new(unknown), crate::transform::Transform::identity()).unwrap();
    let placed = SolvedCurveGeometry::Transformed(
        PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Transformed(basis)),
            crate::transform::Transform::identity(),
        )
        .unwrap(),
    );
    let mut result = vec![
        CurveGeometry::Procedural {
            construction: ProceduralCurveId::mint("test:model:procedural-curve#copy").unwrap(),
            cache: Some(placed),
        },
        CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments: CompositeCurveSegments::try_from(vec![
                CompositeCurveSegment {
                    curve: CurveId::mint("test:model:curve#first").unwrap(),
                    same_sense: true,
                    transition: CompositeCurveTransition::Continuous,
                },
                CompositeCurveSegment {
                    curve: CurveId::mint("test:model:curve#second").unwrap(),
                    same_sense: false,
                    transition: CompositeCurveTransition::Discontinuous,
                },
            ])
            .unwrap(),
            self_intersect: Some(false),
        }),
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 
                1,
                vec![0., 0., 1., 1.],
                vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
                Some(vec![1., 2.]),
                false,
            ).expect("fixture constructor admission")
            .unwrap(),
        )),
    ];
    for samples in [
        PolylineSamples::Unparameterized {
            points: vec![Point3::new(0., 0., 0.), Point3::new(1., 2., 3.)]
                .try_into()
                .unwrap(),
        },
        PolylineSamples::Parameterized {
            vertices: vec![
                PolylineVertex {
                    parameter: 2.,
                    point: Point3::new(0., 0., 0.),
                },
                PolylineVertex {
                    parameter: 1.,
                    point: Point3::new(1., 2., 3.),
                },
            ]
            .try_into()
            .unwrap(),
        },
    ] {
        result.push(CurveGeometry::Solved(SolvedCurveGeometry::Polyline(
            PolylineCurve::new(samples, 0.125).unwrap(),
        )));
    }
    result
}

fn copy_curves(policy: &DecodePolicy) -> Result<(), cadmpeg_core::CodecError> {
    let source = curves();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy).unwrap();
    for curve in &source {
        assert_eq!(curve.try_clone_for_decode(&ctx, "copy test curve")?, *curve);
    }
    Ok(())
}

fn set_limit(policy: &mut DecodePolicy, dimension: ResourceDimension, limit: u64) {
    match dimension {
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
        ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = limit,
        ResourceDimension::WorkUnits => policy.limits.max_work_units = limit,
        _ => panic!("unsupported test dimension"),
    }
}

fn assert_copy_limit(dimension: ResourceDimension) {
    copy_curves(&DecodePolicy::service()).unwrap();
    let mut upper = 1;
    let mut policy = DecodePolicy::service();
    loop {
        set_limit(&mut policy, dimension, upper);
        if copy_curves(&policy).is_ok() {
            break;
        }
        upper *= 2;
        assert!(upper <= 65536);
    }
    let mut lower = 0;
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        set_limit(&mut policy, dimension, middle);
        if copy_curves(&policy).is_ok() {
            upper = middle;
        } else {
            lower = middle + 1;
        }
    }
    assert!(upper > 0);
    set_limit(&mut policy, dimension, upper);
    copy_curves(&policy).unwrap();
    set_limit(&mut policy, dimension, upper - 1);
    assert!(
        matches!(copy_curves(&policy), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == dimension)
    );
}

#[test]
fn charged_curve_copy_refuses_collection_limit() {
    assert_copy_limit(ResourceDimension::CollectionItems);
}
#[test]
fn charged_curve_copy_refuses_retained_limit() {
    assert_copy_limit(ResourceDimension::RetainedBytes);
}
#[test]
fn charged_curve_copy_refuses_nesting_limit() {
    assert_copy_limit(ResourceDimension::RecursionDepth);
}
#[test]
fn charged_curve_copy_refuses_work_limit() {
    assert_copy_limit(ResourceDimension::WorkUnits);
}
