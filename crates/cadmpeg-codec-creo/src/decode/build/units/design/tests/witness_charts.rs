use super::*;
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{
    ProceduralSurface, ProceduralSurfaceDefinition, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::topology::{AnchoredVertexUse, LoopBoundary, LoopRing, PcurveUse};

#[test]
fn witness_pcurve_charts_follow_singular_anchored_and_curve_bounded_owners() {
    for owner in 0..4 {
        let mut body = cadmpeg_ir::examples::unit_cube()
            .expect("valid witness chart fixture")
            .geometry;
        let id: cadmpeg_ir::ids::PcurveId = "test:witness:pcurve#one"
            .try_into()
            .expect("valid witness chart fixture");
        body.pcurves.push(Pcurve {
            id: id.clone(),
            geometry: PcurveGeometry::Line(
                LinePcurve::try_new(Point2::new(1.0, 2.0), Point2::new(3.0, 4.0))
                    .expect("valid witness chart fixture"),
            ),
            metadata: PcurveMetadata::default(),
            native_ref: None,
        });
        let usage = PcurveUse {
            pcurve: id.clone(),
            isoparametric: None,
            parameter_range: Some(
                cadmpeg_ir::geometry::DirectedParameterRange::new([2.0, 3.0])
                    .expect("valid witness chart fixture"),
            ),
        };
        if owner == 0 {
            body.loops[0].boundary = LoopBoundary::Vertex {
                vertex: body.vertices[0].id.clone(),
                pcurves: vec![usage.clone()],
            };
        } else if owner == 1 || owner == 3 {
            let coedges = body.loops[0].coedges().to_vec();
            body.loops[0].boundary = LoopBoundary::Ring(
                LoopRing::new(
                    coedges.clone(),
                    vec![AnchoredVertexUse {
                        vertex: body.vertices[0].id.clone(),
                        after: coedges[0].clone(),
                        pcurves: vec![usage.clone()],
                    }],
                )
                .expect("valid witness chart fixture"),
            );
        }
        if owner == 2 || owner == 3 {
            body.procedural_surfaces.push(ProceduralSurface::new(
                "test:witness:procedural-surface#one"
                    .try_into()
                    .expect("valid witness chart fixture"),
                ProceduralSurfaceDefinition::CurveBounded {
                    support: body.surfaces[0].id.clone(),
                    boundaries: Vec::new(),
                    boundary_pcurves: vec![id.clone()],
                    implicit_outer: true,
                },
                None,
            ));
        }
        if owner == 3 {
            body.coedges[0].pcurves.push(usage);
        }
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        topology(
            &ctx,
            &mut body,
            PositiveReal::new(10.0).expect("valid witness chart fixture"),
        )
        .expect("valid witness chart fixture");
        let PcurveGeometry::Line(line) = &body.pcurves[0].geometry else {
            panic!("line")
        };
        assert_eq!(line.origin().get(), Point2::new(10.0, 20.0));
        assert_eq!(*line.direction().as_raw(), Point2::new(30.0, 40.0));
        if owner != 2 {
            assert_eq!(
                body.loops[0]
                    .vertex_pcurves()
                    .next()
                    .expect("valid witness chart fixture")
                    .parameter_range
                    .expect("valid witness chart fixture")
                    .endpoints(),
                [2.0, 3.0]
            );
        }
    }
}

#[test]
fn witness_pcurve_missing_chart_fails_before_length_mutation() {
    let mut body = cadmpeg_ir::examples::unit_cube()
        .expect("valid witness chart fixture")
        .geometry;
    body.pcurves.push(Pcurve {
        id: "test:witness:pcurve#unowned"
            .try_into()
            .expect("valid witness chart fixture"),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(1.0, 2.0), Point2::new(3.0, 4.0))
                .expect("valid witness chart fixture"),
        ),
        metadata: PcurveMetadata::default(),
        native_ref: None,
    });
    let original = body.clone();
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    assert!(topology(
        &ctx,
        &mut body,
        PositiveReal::new(10.0).expect("valid witness chart fixture")
    )
    .is_err());
    assert_eq!(body, original);
}

#[test]
fn witness_pcurve_conflicting_charts_fail_before_length_mutation() {
    let mut body = cadmpeg_ir::examples::unit_cube()
        .expect("valid witness chart fixture")
        .geometry;
    let id: cadmpeg_ir::ids::PcurveId = "test:witness:pcurve#one"
        .try_into()
        .expect("valid witness chart fixture");
    body.pcurves.push(Pcurve {
        id: id.clone(),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(1.0, 2.0), Point2::new(3.0, 4.0))
                .expect("valid witness chart fixture"),
        ),
        metadata: PcurveMetadata::default(),
        native_ref: None,
    });
    let usage = PcurveUse {
        pcurve: id.clone(),
        isoparametric: None,
        parameter_range: None,
    };
    body.loops[0].boundary = LoopBoundary::Vertex {
        vertex: body.vertices[0].id.clone(),
        pcurves: vec![usage.clone()],
    };
    let mut cylinder = body.surfaces[0].clone();
    cylinder.id = "test:witness:surface#cylinder"
        .try_into()
        .expect("valid witness chart fixture");
    cylinder.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
        )
        .expect("valid witness chart fixture"),
    ));
    body.procedural_surfaces.push(ProceduralSurface::new(
        "test:witness:procedural-surface#one"
            .try_into()
            .expect("valid witness chart fixture"),
        ProceduralSurfaceDefinition::CurveBounded {
            support: cylinder.id.clone(),
            boundaries: Vec::new(),
            boundary_pcurves: vec![id],
            implicit_outer: true,
        },
        None,
    ));
    body.surfaces.push(cylinder);
    let original = body.clone();
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    assert!(topology(
        &ctx,
        &mut body,
        PositiveReal::new(10.0).expect("valid witness chart fixture")
    )
    .is_err());
    assert_eq!(body, original);
}
