// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::geometry::SurfaceGeometry;

#[test]
fn seed_alignment_keeps_the_interior_locus_limit() {
    let first = edge(vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)], true);
    let mut second = edge(
        vec![Point2::new(0.0, 0.004), Point2::new(1.0, 0.004)],
        false,
    );
    let plane = SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane"),
    );
    assert!(align_seed(&first, &mut second, &plane).is_none());
}

#[test]
fn analytic_reprojection_preserves_the_lift_limit() {
    let first = edge(vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)], true);
    let second = edge(vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)], false);
    let plane = |z| {
        SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, z),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane"),
        )
    };
    let recovered = reproject(&first, &second, &plane(0.0)).expect("analytic reprojection");
    assert_eq!(recovered.vertices, first.vertices);
    assert!(!recovered.forward);
    assert!(reproject(&first, &second, &plane(0.002)).is_none());
}

#[test]
fn shared_edge_alignment_handles_a_stationary_parameter_endpoint() {
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};

    let surface = SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::new(
            NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsPoleGrid::Polynomial {
                rows: [0.0, 0.0, 1.0]
                    .map(|x| vec![Point3::new(x, 0.0, 0.0), Point3::new(x, 1.0, 0.0)])
                    .to_vec(),
            },
            false,
        )
        .expect("quadratically parameterized plane"),
    );
    let first = edge(vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)], true);
    let mut second = edge(
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        ],
        false,
    );
    second.pcurve =
        crate::families::freeform::owned_patch::line_pcurve([0.0, 1.0], [[0.0, 0.0], [1.0, 0.0]])
            .expect("linear chart pcurve");
    align(&first, &mut second, &surface).expect("recover stationary endpoint pacing");
    for step in 0..=LOCUS_STEPS {
        let t = f64::from(step) / f64::from(LOCUS_STEPS);
        let uv = evaluate_uv(&second.pcurve, t).expect("fitted pcurve");
        let point =
            eval::decode::surface_point_solved(EvaluationAdmission::Standard, &surface, uv.u, uv.v)
                .expect("lift")
                .get();
        assert!(point.distance(curve_point(&first.curve, t).expect("edge")) <= GAP_MM);
    }
}

#[test]
fn analytic_reprojection_preserves_both_torus_chart_branches() {
    use crate::families::b2::records::{b2_torus_geometry, owned_torus_charts};
    use crate::test_support::test_b2::b2_torus_stream;
    use crate::wire::records::consolidated_records;

    let bytes = b2_torus_stream();
    let records = consolidated_records(&bytes);
    let (torus, _) = owned_torus_charts(&bytes, &records)
        .next()
        .expect("synthetic torus");
    let SurfaceGeometry::Solved(surface) = b2_torus_geometry(&torus) else {
        panic!("analytic torus");
    };
    let uv = [[6.1, 6.1], [6.1, 6.5]];
    let position = |t| {
        eval::decode::surface_point_solved(
            EvaluationAdmission::Standard,
            &surface,
            6.1,
            6.1 + 0.4 * t,
        )
        .ok()
        .map(cadmpeg_ir::features::FinitePoint3::get)
    };
    let curve = fit_edge_with_error(
        [0.0, 1.0],
        |t| {
            let p = position(t)?;
            Some([p.x, p.y, p.z])
        },
        GAP_MM / 10.0,
    )
    .expect("torus boundary");
    let first = Use {
        edge: 1,
        vertices: [
            (1, position(0.0).expect("start")),
            (2, position(1.0).expect("end")),
        ],
        curve,
        pcurve: crate::families::freeform::owned_patch::line_pcurve([0.0, 1.0], uv)
            .expect("torus pcurve"),
        forward: true,
    };
    let mut occurrence = first.clone();
    occurrence.forward = false;
    let recovered = reproject(&first, &occurrence, &surface).expect("torus reprojection");
    for t in [0.0, 0.5, 1.0] {
        let point = evaluate_uv(&recovered.pcurve, t).expect("projected torus UV");
        assert!((point.u - 6.1).abs() <= UV_FIT_FALLBACK);
        assert!((point.v - (6.1 + 0.4 * t)).abs() <= UV_FIT_FALLBACK);
    }
}
