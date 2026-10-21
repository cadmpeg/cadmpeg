// SPDX-License-Identifier: Apache-2.0
//! Helix curve and surface carriers at STEP export.
#![allow(clippy::unwrap_used)]

use super::reports::edgeless_doc;
use crate::{export::write_step, loss::StepLossCode, StepSchema, StepWriteOptions};
use cadmpeg_ir::{
    geometry::{
        Curve, CurveGeometry, HelixCircleProfile, HelixCurveConstruction, HelixFrame,
        HelixPathConstruction, HelixSurfaceConstruction, HelixSurfaceProfile, ProceduralCurve,
        ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, SurfaceGeometry,
    },
    ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId},
    math::{Point3, Vector3},
    topology::{EdgeCarrier, Point},
    CadIr,
};

/// The edgeless document with its one edge closed on a full turn of a
/// pitchless helix in the face plane, from and to `(1, 0, 0)`.
fn helix_edge_doc(cache: bool) -> CadIr {
    let mut ir = edgeless_doc();
    let curve_id = CurveId::mint("test:model:curve#helix").unwrap();
    let construction_id = ProceduralCurveId::mint("test:model:procedural_curve#helix").unwrap();
    let helix = HelixCurveConstruction::try_new(
        [0.0, std::f64::consts::TAU],
        HelixFrame {
            center: Point3::new(0.0, 0.0, 0.0),
            major: Vector3::new(1.0, 0.0, 0.0),
            minor: Vector3::new(0.0, 1.0, 0.0),
            pitch: Vector3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
        },
        0.0,
        None,
    )
    .unwrap();
    ir.model.points[0] = Point::new(
        ir.model.points[0].id.clone(),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).unwrap(),
        None,
    );
    ir.model.edges[0].end = ir.model.edges[0].start.clone();
    let fitted = cadmpeg_ir::eval::helix_nurbs::helix_nurbs(&helix, ir.tolerances.linear).unwrap();
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: curve_id.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction_id.clone(),
            cache: cache.then_some(cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(fitted)),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve::new(
        construction_id,
        ProceduralCurveDefinition::Helix(helix),
    ));
    ir.model.edges[0].carrier = EdgeCarrier::unbounded(Some(curve_id));
    ir
}

fn entity_count(bytes: &[u8], name: &str) -> usize {
    let (exchange, _) =
        crate::test_support::with_service_context(bytes, crate::parse::parse_inner).unwrap();
    exchange
        .records()
        .values()
        .flat_map(|record| &record.partials)
        .filter(|partial| partial.name == name)
        .count()
}

#[test]
fn an_edge_on_an_uncached_helix_is_written_on_a_fitted_bspline() {
    let ir = helix_edge_doc(false);
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();

    assert_eq!(entity_count(&bytes, "EDGE_CURVE"), 1);
    assert_eq!(entity_count(&bytes, "B_SPLINE_CURVE_WITH_KNOTS"), 1);
    let codes: Vec<_> = report.losses.iter().map(|loss| loss.code.clone()).collect();
    assert!(codes.contains(&StepLossCode::HelixFittedToBspline.kind()));
    assert!(!codes.contains(&StepLossCode::EdgeCurveUnsupported.kind()));
    assert!(!codes.contains(&StepLossCode::ProceduralReducedToCarrier.kind()));
    assert!(!codes.contains(&StepLossCode::ProceduralDefinitionNotWritten.kind()));
}

#[test]
fn an_edge_on_a_cached_helix_is_written_on_its_cache() {
    let ir = helix_edge_doc(true);
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();

    assert_eq!(entity_count(&bytes, "EDGE_CURVE"), 1);
    let codes: Vec<_> = report.losses.iter().map(|loss| loss.code.clone()).collect();
    assert!(!codes.contains(&StepLossCode::HelixFittedToBspline.kind()));
    assert!(codes.contains(&StepLossCode::ProceduralReducedToCarrier.kind()));
}

/// The uncached helix edge document with its face on an uncached
/// spring-wire surface.
fn helix_surface_doc() -> CadIr {
    let mut ir = helix_edge_doc(false);
    let construction = HelixSurfaceConstruction::try_new(
        [-std::f64::consts::PI, std::f64::consts::PI],
        [0.0, std::f64::consts::TAU],
        HelixPathConstruction::try_new(
            [0.0, std::f64::consts::TAU],
            HelixFrame {
                center: Point3::new(0.0, 0.0, 0.0),
                major: Vector3::new(6.0, 0.0, 0.0),
                minor: Vector3::new(0.0, 6.0, 0.0),
                pitch: Vector3::new(0.0, 0.0, 2.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
            0.0,
        )
        .unwrap(),
        HelixSurfaceProfile::Circle(HelixCircleProfile::try_new(0.0, 0.5).unwrap()),
    )
    .unwrap();
    let owner = ir.model.surfaces[0].id.clone();
    ir.model
        .add_procedural_surface(
            &owner,
            ProceduralSurface::new(
                ProceduralSurfaceId::mint("test:model:procedural_surface#wire").unwrap(),
                ProceduralSurfaceDefinition::Helix {
                    construction: Box::new(construction),
                },
                None,
            ),
        )
        .unwrap();
    let SurfaceGeometry::Procedural { cache, .. } = &mut ir.model.surfaces[0].geometry else {
        panic!("construction owner")
    };
    *cache = None;
    ir
}

#[test]
fn a_face_on_an_uncached_helix_surface_is_written_on_a_fitted_bspline_surface() {
    let ir = helix_surface_doc();
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();

    assert_eq!(entity_count(&bytes, "ADVANCED_FACE"), 1);
    assert_eq!(entity_count(&bytes, "B_SPLINE_SURFACE_WITH_KNOTS"), 1);
    assert_eq!(entity_count(&bytes, "RATIONAL_B_SPLINE_SURFACE"), 1);
    let codes: Vec<_> = report.losses.iter().map(|loss| loss.code.clone()).collect();
    assert!(codes.contains(&StepLossCode::HelixFittedToBspline.kind()));
    assert!(!codes.contains(&StepLossCode::UnknownSurfaceFaceOmitted.kind()));
    assert!(!codes.contains(&StepLossCode::ProceduralReducedToCarrier.kind()));
    assert!(!codes.contains(&StepLossCode::ProceduralDefinitionNotWritten.kind()));
}
