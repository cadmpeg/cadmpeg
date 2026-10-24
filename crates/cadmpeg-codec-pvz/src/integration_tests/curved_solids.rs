// SPDX-License-Identifier: Apache-2.0
//! Code-built closed solids with curved, B-spline, singular, and nested
//! faces, decoded through the codec and measured in model space.
//!
//! Expected volumes and areas come from the solid dimensions of
//! [`crate::test_support::solids`], in millimetres.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::report::check::Check;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::topology::{BodyKind, EdgeCarrier, FaceLoops};
use cadmpeg_ir::CadIr;
use cadmpeg_measure::report::BodyMeasures;
use cadmpeg_measure::value::Quantity;
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection, SurfaceKind};

use super::{decode, loss_codes};
use crate::loss::PvzLossCode;
use crate::test_support::exact::CUBE_SIZE;
use crate::test_support::solids::{
    cone_stream, cylinder_stream, holed_block, polyhedron_stream, PlanarCarrier, BLOCK_SIDE,
    BLOCK_THICKNESS, HEIGHT, HOLE_SIDE, RADIUS,
};
use crate::test_support::{cube_package, solid_package};

/// Neutral millimetres per stream metre (§8.21).
const MM: f64 = 1000.0;
/// Allowance of a measured value beyond its stated numeric error, relative
/// to the expected magnitude.
const MEASURE_RELATIVE_FLOOR: f64 = 1.0e-9;

/// The decoded document of `bytes`, with no neutral finding of warning
/// severity or worse. Its losses are the zero geometry-element bounds and
/// the appearance tail of [`solid_package`], and the
/// losses `exact`.
fn decode_clean(bytes: &[u8], exact: &[PvzLossCode]) -> CadIr {
    let result = decode(bytes);
    let mut expected = vec![
        PvzLossCode::StructureBoundsNotTransferred.kind(),
        PvzLossCode::AttributeAppearanceTail.kind(),
    ];
    expected.extend(exact.iter().map(|code| code.kind()));
    let codes = loss_codes(&result);
    assert_eq!(codes.len(), expected.len(), "{codes:?}");
    assert!(
        expected.iter().all(|code| codes.contains(code)),
        "{codes:?}"
    );
    assert!(result.report().geometry_transferred());
    let report = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
    let serious: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| finding.severity >= Severity::Warning)
        .collect();
    assert!(serious.is_empty(), "{serious:?}");
    result.ir().clone()
}

/// The measured body row of the one solid body of `ir`. The solid has no
/// orientation finding.
fn measured_solid(ir: &CadIr) -> BodyMeasures {
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.bodies[0].kind, BodyKind::Solid);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let input = MeasureInput {
        ir,
        fidelity: None,
        geometry_transferred: true,
    };
    let report = Measurer::new(input, MeasureOptions::default())
        .measure(&ctx, &Selection::default())
        .unwrap();
    let orientation: Vec<_> = cadmpeg_measure::findings(&ctx, ir, MeasureOptions::default())
        .unwrap()
        .into_iter()
        .filter(|finding| finding.check == Check::Orientation)
        .collect();
    ctx.finish_session().unwrap();
    assert!(orientation.is_empty(), "{orientation:?}");
    let [body] = report.bodies().as_slice() else {
        panic!("one body row");
    };
    body.clone()
}

/// Asserts that `quantity` is measured within its numeric error of
/// `expected`.
fn assert_measured(
    quantity: Option<&Quantity<cadmpeg_measure::value::Scalar>>,
    expected: f64,
    what: &str,
) {
    let measured = quantity.and_then(Quantity::measured).expect(what);
    let (value, error) = (measured.value().value.get(), measured.numeric_error().get());
    let allowed = error + MEASURE_RELATIVE_FLOOR * expected.abs().max(1.0);
    assert!(
        (value - expected).abs() <= allowed,
        "{what}: measured {value}, expected {expected}, allowed {allowed}"
    );
}

/// Asserts the surface kinds, volume, and area of the solid of `ir`.
fn assert_solid(ir: &CadIr, kinds: &[(SurfaceKind, usize)], volume: f64, area: f64) {
    let body = measured_solid(ir);
    let expected: BTreeMap<SurfaceKind, usize> = kinds.iter().copied().collect();
    assert_eq!(body.surface_kinds(), &expected);
    assert_measured(body.volume().as_ref(), volume, "volume");
    assert_measured(body.area().as_ref(), area, "area");
}

#[test]
fn a_cylinder_with_a_seam_decodes_to_a_closed_solid() {
    let ir = decode_clean(&solid_package(cylinder_stream()), &[]);
    let (r, h) = (RADIUS * MM, HEIGHT * MM);
    assert_solid(
        &ir,
        &[(SurfaceKind::Plane, 2), (SurfaceKind::Cylinder, 1)],
        PI * r * r * h,
        2.0 * PI * r * r + 2.0 * PI * r * h,
    );
    let model = &ir.model;
    assert_eq!((model.edges.len(), model.vertices.len()), (3, 2));
    let seam = model
        .edges
        .iter()
        .find(|edge| edge.start != edge.end)
        .unwrap();
    let uses: Vec<_> = model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == seam.id)
        .collect();
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].owner_loop, uses[1].owner_loop);
    assert_ne!(uses[0].sense, uses[1].sense);
}

#[test]
fn a_cone_with_a_singular_apex_decodes_to_a_closed_solid() {
    let ir = decode_clean(&solid_package(cone_stream()), &[]);
    let (r, h) = (RADIUS * MM, HEIGHT * MM);
    assert_solid(
        &ir,
        &[(SurfaceKind::Plane, 1), (SurfaceKind::Cone, 1)],
        PI * r * r * h / 3.0,
        PI * r * r + PI * r * r.hypot(h),
    );
    let model = &ir.model;
    let apex = model
        .edges
        .iter()
        .find(|edge| {
            let EdgeCarrier::Curve(curve) = &edge.carrier else {
                return false;
            };
            model.curves.iter().any(|candidate| {
                candidate.id == *curve
                    && matches!(
                        candidate.geometry,
                        CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(_))
                    )
            })
        })
        .expect("apex edge on a degenerate curve");
    assert_eq!(apex.start, apex.end);
}

#[test]
fn a_rational_bspline_face_closes_a_cube() {
    let ir = decode_clean(&cube_package(1, PlanarCarrier::RationalBspline), &[]);
    let s = CUBE_SIZE * MM;
    assert_solid(
        &ir,
        &[(SurfaceKind::Plane, 5), (SurfaceKind::Nurbs, 1)],
        s * s * s,
        6.0 * s * s,
    );
}

#[test]
fn a_face_with_secondary_pcurves_keeps_its_plane_carrier() {
    let ir = decode_clean(
        &cube_package(1, PlanarCarrier::SecondaryPcurves),
        &[PvzLossCode::ExactSecondaryPcurveOmitted],
    );
    let s = CUBE_SIZE * MM;
    assert_solid(&ir, &[(SurfaceKind::Plane, 6)], s * s * s, 6.0 * s * s);
    let face = ir
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "pvz:model:face#part.ol:1:0")
        .expect("face 1");
    let pcurves = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| {
            ir.model
                .loops
                .iter()
                .any(|loop_| loop_.id == coedge.owner_loop && loop_.face == face.id)
        })
        .map(|coedge| coedge.pcurves.len())
        .collect::<Vec<_>>();
    assert_eq!(pcurves, [0; 4]);
}

#[test]
fn inner_loops_nest_in_the_faces_of_a_holed_block() {
    let ir = decode_clean(&solid_package(polyhedron_stream(&holed_block())), &[]);
    let (side, hole, thickness) = (BLOCK_SIDE * MM, HOLE_SIDE * MM, BLOCK_THICKNESS * MM);
    let face_area = side * side - hole * hole;
    assert_solid(
        &ir,
        &[(SurfaceKind::Plane, 10)],
        face_area * thickness,
        2.0 * face_area + 4.0 * (side + hole) * thickness,
    );
    let nested: Vec<usize> = ir
        .model
        .faces
        .iter()
        .filter_map(|face| match &face.loops {
            FaceLoops::Classified { inner, .. } if !inner.is_empty() => Some(inner.len()),
            _ => None,
        })
        .collect();
    assert_eq!(nested, [1, 1]);
}
