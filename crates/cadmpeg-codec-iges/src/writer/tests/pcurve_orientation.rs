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

/// The error the orientation context states for the single use of a line
/// pcurve over `[-1, 1]` on `surface`.
fn line_pcurve_end_error(surface: &SurfaceGeometry, line: LinePcurve) -> String {
    let id = PcurveId::mint("test:iges:pcurve#overflow").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.pcurves.push(Pcurve {
        id: id.clone(),
        geometry: PcurveGeometry::Line(line),
        metadata: PcurveMetadata::general(
            None,
            Some(cadmpeg_ir::units::FiniteVector::new([-1.0, 1.0]).expect("finite range")),
            None,
        ),
    });
    let context = pcurve_orientation_context(
        &ir,
        surface,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Sense::Forward,
        1.0e-6,
        "edge test",
    );
    context
        .map(&[PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        }])
        .expect_err("the end has no finite point")
        .to_string()
}

fn origin_plane() -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    )
}

#[test]
fn a_pcurve_end_whose_placed_support_point_overflows_is_refused_as_non_finite() {
    // The placement adds the largest finite x coordinate: the pcurve end
    // u = MAX maps to a point without a finite x, and the start u = -MAX
    // maps to the model origin.
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        cadmpeg_ir::geometry::PlacedSurface::try_new(
            Box::new(origin_plane()),
            cadmpeg_ir::transform::Transform::affine([
                [1.0, 0.0, 0.0, f64::MAX],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("affine transform"),
        )
        .expect("valid PlacedSurface fixture"),
    ));
    let line = LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(f64::MAX, 0.0))
        .expect("valid LinePcurve fixture");
    assert_eq!(
        line_pcurve_end_error(&surface, line),
        cadmpeg_core::CodecError::malformed(
            "IGES point edge test pcurve test:iges:pcurve#overflow end has non-finite coordinates"
        )
        .to_string()
    );
}

#[test]
fn a_pcurve_end_whose_line_point_overflows_is_refused_as_non_finite() {
    // The line reaches u = MAX + MAX at its end and u = 0 at its start; the
    // plane maps the end to a point without a finite coordinate.
    let line = LinePcurve::try_new(Point2::new(f64::MAX, 0.0), Point2::new(f64::MAX, 0.0))
        .expect("valid LinePcurve fixture");
    assert_eq!(
        line_pcurve_end_error(&SurfaceGeometry::Solved(origin_plane()), line),
        cadmpeg_core::CodecError::malformed(
            "IGES point edge test pcurve test:iges:pcurve#overflow end has non-finite coordinates"
        )
        .to_string()
    );
}

#[test]
fn a_pcurve_on_an_extrusion_whose_directrix_acceleration_overflows_maps_to_finite_points() {
    // The directrix is the circle of radius 2 about (-2, 0, 0) stretched by
    // MAX in x: at angle 0 its point is the origin and its acceleration
    // overflows. The extrusion point reads the directrix point alone, so the
    // pcurve (0, 1 + t) over [-1, 1] maps to the origin and to (0, 0, 2).
    use cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction;
    use cadmpeg_ir::geometry::{
        CacheContract, Curve, CurveGeometry, PlacedCurve, ProceduralSurface,
        ProceduralSurfaceDefinition, SolvedCurveGeometry,
    };
    let directrix =
        cadmpeg_ir::ids::CurveId::mint("test:iges:curve#directrix").expect("identity grammar");
    let construction = cadmpeg_ir::ids::ProceduralSurfaceId::mint("test:iges:procedural#extrusion")
        .expect("identity grammar");
    let id = PcurveId::mint("test:iges:pcurve#extrusion").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: directrix.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
            PlacedCurve::try_new(
                Box::new(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::new(-2.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        2.0,
                    )
                    .expect("valid CircleCurve fixture"),
                )),
                cadmpeg_ir::transform::Transform::affine([
                    [f64::MAX, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ])
                .expect("affine transform"),
            )
            .expect("valid PlacedCurve fixture"),
        )),
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction.clone(),
        ProceduralSurfaceDefinition::Extrusion(
            ExtrusionSurfaceConstruction::try_new(
                directrix,
                None,
                Vector3::new(0.0, 0.0, 1.0),
                None,
                CacheContract::from_form(None),
            )
            .expect("valid extrusion fixture"),
        ),
        None,
    ));
    ir.model.pcurves.push(Pcurve {
        id: id.clone(),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 1.0), Point2::new(0.0, 1.0))
                .expect("valid LinePcurve fixture"),
        ),
        metadata: PcurveMetadata::general(
            None,
            Some(cadmpeg_ir::units::FiniteVector::new([-1.0, 1.0]).expect("finite range")),
            None,
        ),
    });
    let surface = SurfaceGeometry::Procedural {
        construction,
        cache: None,
    };
    let context = pcurve_orientation_context(
        &ir,
        &surface,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(0.0, 0.0, 2.0),
        Sense::Forward,
        1.0e-6,
        "edge test",
    );
    assert_eq!(
        context
            .map(&[PcurveUse {
                pcurve: id,
                isoparametric: None,
                parameter_range: None,
            }])
            .map_err(|error| error.to_string()),
        Ok(vec![(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 2.0)
        )])
    );
}
