// SPDX-License-Identifier: Apache-2.0
//! Validator findings where an evaluator arm reports a non-finite value.

use super::{check_procedural_support_consistency, mapped_surface_curve_with_pcurve};
use crate::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use crate::math::Point2;

#[test]
fn a_support_side_whose_nurbs_pcurve_overflows_misses_its_contract_by_nan() {
    // The linear NURBS pcurve through (0, 0) and (f64::MAX, 0) extrapolates
    // past its knot interval: at 2 and 3 its u coordinate is twice and three
    // times f64::MAX. The plane maps the infinite u to a point with no finite
    // coordinate.
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(f64::MAX, 0.0)],
            None,
            false,
        )
        .unwrap(),
    };
    let ir = mapped_surface_curve_with_pcurve(pcurve, [2.0, 3.0]);
    let mut findings = Vec::new();
    check_procedural_support_consistency(&ir, &mut findings);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(
        findings[0].message,
        "procedural support side 0 misses its endpoint distance contract by NaN"
    );
}

#[test]
fn a_support_side_whose_placed_support_overflows_misses_its_contract_by_inf() {
    // The pcurve maps the range to u = 1e300 and u = 2e300; the placement
    // adds f64::MAX to x, which leaves the finite range at both ends.
    let mut ir = super::mapped_surface_curve([1.0e300, 2.0e300]);
    let plane = ir.model.surfaces[0].geometry.clone();
    let crate::geometry::SurfaceGeometry::Solved(plane) = plane else {
        panic!("the fixture support is solved");
    };
    ir.model.surfaces[0].geometry = crate::geometry::SurfaceGeometry::Solved(
        crate::geometry::SolvedSurfaceGeometry::Transformed(
            crate::geometry::PlacedSurface::try_new(
                Box::new(plane),
                crate::transform::Transform::affine([
                    [1.0, 0.0, 0.0, f64::MAX],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ])
                .unwrap(),
            )
            .unwrap(),
        ),
    );
    let mut findings = Vec::new();
    check_procedural_support_consistency(&ir, &mut findings);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(
        findings[0].message,
        "procedural support side 0 misses its endpoint distance contract by inf"
    );
}

#[test]
fn a_coedge_placed_pcurve_whose_mapped_points_overflow_misses_the_vertices_by_nan() {
    // The vertical line at u = MAX, placed by a transform that doubles u,
    // reaches u = +inf at every parameter, and the face plane maps it to a
    // point with no finite coordinate.
    let mut ir = crate::examples::unit_cube().expect("valid unit cube fixture");
    let id = crate::ids::PcurveId::mint("synthetic:cube:pcurve#placed-overflow")
        .expect("valid identity");
    ir.model.pcurves.push(crate::geometry::pcurve::Pcurve {
        id: id.clone(),
        geometry: PcurveGeometry::Transformed(
            crate::geometry::pcurve::PlacedPcurve::try_new(
                Box::new(PcurveGeometry::Line(
                    crate::geometry::pcurve::LinePcurve::try_new(
                        Point2::new(f64::MAX, 0.0),
                        Point2::new(0.0, 1.0),
                    )
                    .unwrap(),
                )),
                crate::transform::Transform2::affine([[2.0, 0.0, 0.0], [0.0, 1.0, 0.0]]).unwrap(),
            )
            .unwrap(),
        ),
        metadata: crate::geometry::pcurve::PcurveMetadata::default(),
    });
    let coedge = ir
        .model
        .coedges
        .iter_mut()
        .find(|coedge| {
            coedge.id.as_str().contains("bottom") && coedge.edge.as_str() == "synthetic:cube:edge#0"
        })
        .expect("bottom face uses edge #0");
    coedge.pcurves = vec![crate::topology::PcurveUse {
        pcurve: id,
        isoparametric: None,
        parameter_range: None,
    }];
    let coedge_id = coedge.id.as_str().to_owned();
    let mut findings = Vec::new();
    super::check_pcurve_surface_consistency(&ir, &mut findings);
    assert!(
        findings.iter().any(|finding| {
            finding.entity.as_deref() == Some(coedge_id.as_str())
                && finding.message
                    == "pcurve mapped through the face surface misses the edge's vertex positions \
                        by NaN"
        }),
        "{findings:?}"
    );
}
