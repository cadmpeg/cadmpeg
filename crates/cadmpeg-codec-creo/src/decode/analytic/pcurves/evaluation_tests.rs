// SPDX-License-Identifier: Apache-2.0

use super::super::equations::{CarrierEquation, PlaneEquation};
use crate::decode::source_carriers::SourceUnitCarriers;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::BTreeMap;
use std::num::NonZeroU32;

const ENDPOINTS: [[f64; 2]; 2] = [[0.0, 0.0], [1.0, 0.0]];
const POINTS: [[f64; 3]; 2] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];

fn plane() -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                (0..3)
                    .map(|u| {
                        (0..3)
                            .map(|v| Point3::new(f64::from(u) * 0.5, f64::from(v) * 0.5, 0.0))
                            .collect()
                    })
                    .collect(),
                None,
            ),
            false,
        )
        .expect("fixture constructor admission")
        .expect("bilinear plane"),
    ))
}

fn context_test(test: impl FnOnce(&DecodeContext<'_>), cap: u64) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    test(&ctx);
}

fn basis_refusal<T>(run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>) {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "IR B-spline basis",
        run,
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(resource) if resource.operation == "IR B-spline basis")
    );
}

fn model() -> CadIr {
    let mut ir = CadIr::empty();
    for face in [7, 8] {
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint(format!("creo:visibgeom:surface#{face}"))
                .expect("surface identity"),
            geometry: plane(),
            source_object: None,
        });
    }
    ir
}

#[test]
fn two_chart_mapping_propagates_evaluator_refusal() {
    let scan = crate::test_support::empty_container_scan();
    let ir = model();
    let pcurve = crate::curve::TwoChartPcurveSamples {
        curve_id: 9,
        faces: [7, 8],
        samples: vec![[[0.0, 0.0]; 2], [[1.0, 0.0]; 2]],
        offset: 0,
    };
    basis_refusal(|ctx| {
        super::mapped_two_chart_endpoint_sets(
            ctx,
            &scan,
            &ir,
            &pcurve,
            &SourceUnitCarriers::default(),
        )
    });
    context_test(
        |ctx| {
            assert!(super::mapped_two_chart_endpoint_sets(
                ctx,
                &scan,
                &ir,
                &pcurve,
                &SourceUnitCarriers::default()
            )
            .expect("service")
            .is_some());
        },
        u64::MAX,
    );
}

#[test]
fn endpoint_carrier_status_propagates_evaluator_refusal() {
    let carriers = BTreeMap::from([
        (
            7,
            CarrierEquation::Plane(PlaneEquation {
                origin: [0.0; 3],
                normal: [0.0, 0.0, 1.0],
            }),
        ),
        (
            8,
            CarrierEquation::Plane(PlaneEquation {
                origin: [0.0; 3],
                normal: [0.0, 1.0, 0.0],
            }),
        ),
    ]);
    basis_refusal(|ctx| {
        super::pcurve_endpoint_carrier_status(
            ctx,
            &model(),
            &carriers,
            ([NonZeroU32::new(7), NonZeroU32::new(8)], 0),
            ENDPOINTS,
            &SourceUnitCarriers::default(),
            &super::SurfaceIndex::new(
                &cadmpeg_test_support::service_decode_context(),
                &model().model.surfaces,
            )
            .expect("surface index fixture"),
        )
    });
}

#[test]
fn pcurve_path_mapping_propagates_evaluator_refusal() {
    basis_refusal(|ctx| {
        super::map_pcurve_paths(
            ctx,
            &model(),
            [(NonZeroU32::new(7), ENDPOINTS)],
            &SourceUnitCarriers::default(),
            &super::SurfaceIndex::new(
                &cadmpeg_test_support::service_decode_context(),
                &model().model.surfaces,
            )
            .expect("surface index fixture"),
        )
    });
}

#[test]
fn native_midpoint_propagates_endpoint_and_midpoint_evaluator_refusals() {
    basis_refusal(|ctx| super::native_pcurve_midpoint(ctx, &plane(), ENDPOINTS, POINTS));

    context_test(
        |ctx| {
            assert_eq!(
                super::native_pcurve_midpoint(ctx, &plane(), ENDPOINTS, POINTS).expect("service"),
                Some([0.5, 0.0, 0.0])
            );
        },
        u64::MAX,
    );
}

#[test]
fn native_endpoint_orientation_propagates_evaluator_refusal() {
    basis_refusal(|ctx| super::oriented_native_pcurve_endpoints(ctx, &plane(), ENDPOINTS, POINTS));
    context_test(
        |ctx| {
            assert_eq!(
                super::oriented_native_pcurve_endpoints(ctx, &plane(), ENDPOINTS, POINTS)
                    .expect("service"),
                Some(ENDPOINTS)
            );
        },
        u64::MAX,
    );
}

#[test]
fn native_pcurve_selection_propagates_evaluator_refusal() {
    basis_refusal(|ctx| {
        super::unique_oriented_native_pcurve(ctx, &plane(), &[(ENDPOINTS, 4)], POINTS)
            .map(|result| result.map(|candidate| (candidate.endpoints, candidate.offset)))
    });
    context_test(
        |ctx| {
            assert_eq!(
                super::unique_oriented_native_pcurve(ctx, &plane(), &[(ENDPOINTS, 4)], POINTS)
                    .map(|result| result.map(|candidate| (candidate.endpoints, candidate.offset)))
                    .expect("service"),
                Some((ENDPOINTS, 4))
            );
        },
        u64::MAX,
    );
}

#[test]
fn pcurve_backed_conic_selection_propagates_evaluator_refusal() {
    let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("circle"),
    ));
    let candidates = BTreeMap::from([((9, 7), vec![(ENDPOINTS, 4)])]);
    basis_refusal(|ctx| {
        super::pcurve_backed_periodic_conic_parameter_range(
            ctx,
            &circle,
            (9, [7, 8]),
            &candidates,
            &model().model.surfaces,
            POINTS,
            &SourceUnitCarriers::default(),
        )
    });
}
