// SPDX-License-Identifier: Apache-2.0
//! Pcurve ends mapped through their face support before orientation.

use super::super::pcurve_orientation_context;
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{PcurveUse, Sense};
use cadmpeg_ir::CadIr;

#[test]
fn a_pcurve_end_whose_support_point_overflows_is_refused_as_non_finite() {
    // The plane's origin is the largest finite x coordinate: the pcurve end
    // u = MAX maps to a point without a finite x, and the start u = -MAX
    // maps to the model origin.
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(f64::MAX, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    let id = PcurveId::mint("test:iges:pcurve#overflow").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.pcurves.push(Pcurve {
        id: id.clone(),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(f64::MAX, 0.0))
                .expect("valid LinePcurve fixture"),
        ),
        metadata: PcurveMetadata::general(
            None,
            Some(cadmpeg_ir::units::FiniteVector::new([-1.0, 1.0]).expect("finite range")),
            None,
        ),
    });
    let context = pcurve_orientation_context(
        &ir,
        &surface,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Sense::Forward,
        1.0e-6,
        "edge test",
    );
    let error = context
        .map(&[PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        }])
        .expect_err("the end has no finite point");
    assert_eq!(
        error.to_string(),
        cadmpeg_core::CodecError::malformed(
            "IGES point edge test pcurve test:iges:pcurve#overflow end has non-finite coordinates"
        )
        .to_string()
    );
}
