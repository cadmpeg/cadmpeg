// SPDX-License-Identifier: Apache-2.0

use crate::decode::source_carriers::SourceUnitCarriers;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::Point3;

const ENDPOINTS: [[f64; 2]; 2] = [[0.0, 0.0], [1.0, 0.0]];
const POINTS: [[f64; 3]; 2] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];

/// Highest degree whose basis is evaluated in inline storage is 15; this
/// degree evaluates its basis in heap storage.
const DEGREE: u32 = 16;

fn plane() -> SurfaceGeometry {
    let poles = DEGREE + 1;
    let mut knots = vec![0.0; 17];
    knots.extend(vec![1.0; 17]);
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(DEGREE, knots.clone(), false),
            NurbsSurfaceAxis::new(DEGREE, knots, false),
            NurbsSurfaceLanes::new(
                (0..poles)
                    .map(|u| {
                        (0..poles)
                            .map(|v| {
                                Point3::new(
                                    f64::from(u) / f64::from(DEGREE),
                                    f64::from(v) / f64::from(DEGREE),
                                    0.0,
                                )
                            })
                            .collect()
                    })
                    .collect(),
                None,
            ),
            false,
        )
        .expect("degree-16 plane"),
    ))
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
fn two_chart_mapping_maps_degree_sixteen_endpoints() {
    let scan = crate::test_support::empty_container_scan();
    let ir = model();
    let pcurve = crate::curve::TwoChartPcurveSamples {
        curve_id: 9,
        faces: [7, 8],
        samples: vec![[[0.0, 0.0]; 2], [[1.0, 0.0]; 2]],
        offset: 0,
    };
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        super::mapped_two_chart_endpoint_sets(
            ctx,
            &scan,
            &ir,
            &pcurve,
            &SourceUnitCarriers::default(),
        )
    })
    .expect("service")
    .is_some());
}

#[test]
fn native_midpoint_evaluates_a_degree_sixteen_plane() {
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::native_pcurve_midpoint(ctx, &plane(), ENDPOINTS, POINTS)
        })
        .expect("service"),
        Some([0.5, 0.0, 0.0])
    );
}

#[test]
fn native_endpoint_orientation_keeps_matching_endpoints() {
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::oriented_native_pcurve_endpoints(ctx, &plane(), ENDPOINTS, POINTS)
        })
        .expect("service"),
        Some(ENDPOINTS)
    );
}

#[test]
fn native_pcurve_selection_picks_the_unique_candidate() {
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::unique_oriented_native_pcurve(ctx, &plane(), &[(ENDPOINTS, 4)], POINTS)
                .map(|result| result.map(|candidate| (candidate.endpoints, candidate.offset)))
        })
        .expect("service"),
        Some((ENDPOINTS, 4))
    );
}
