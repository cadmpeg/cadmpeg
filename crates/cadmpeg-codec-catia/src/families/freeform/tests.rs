// SPDX-License-Identifier: Apache-2.0
//! Route and surface-pool tests for the `freeform` family over synthetic byte
//! fixtures.
//!
//! The freeform route ([`try_decode_freeform_surfaces`]) runs the b5
//! object-stream topology transfer first and falls back to the a5a8/b2 surface
//! carriers when the b5 reference graph does not parse. The route tests wrap each
//! record-stream builder from [`crate::tests`] in an object-stream `.CATPart`
//! (via `object_main_catpart`, yielding [`Variant::FloatPackedInnerNoFbb`]) and
//! call the route on the resulting [`ContainerScan`](crate::container::ContainerScan), so the inputs are the same
//! freeform-routable images the golden harness pins.
//!
//! The a5a8 + consolidated surface pools ([`append_freeform_surface_pools`]) are
//! the freeform module's contribution to the *standard* route, not the freeform
//! route; the pool tests exercise that function directly against the raw record
//! streams. Its golden coverage lives in the `standard`-route `consolidated_*`
//! and `standard_a5_rolling_ball` fixtures.

#![allow(clippy::unwrap_used)]

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    CurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::LossCode;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;

use super::{
    append_freeform_surface_pools, freeform_surface_carriers, try_decode_freeform_surfaces,
};
use crate::container::scan_bytes;
use crate::tests::{
    a5_freeform_curve_stream, a5_guide_curve_stream, a5_native_edge_run_stream, a5_surface_stream,
    a8_freeform_curve_stream, a8_surface_stream, b2_cone_stream, b2_cylinder_stream,
    b5_closed_triangle_stream, le_f32, object_main_catpart,
};
use crate::variant::Variant;

/// The freeform route consumes the b5 object-stream topology first: a
/// reference-closed triangle transfers a full one-face body with its loop,
/// coedges, edges, curves, vertices, and pcurves.
#[test]
fn freeform_route_transfers_reference_closed_b5_topology() {
    let stream = b5_closed_triangle_stream();
    crate::families::b5::graph::parse(&stream).expect("reference-closed B5 graph");
    let scan = scan_bytes(object_main_catpart(&stream));
    assert_eq!(scan.variant, Variant::FloatPackedInnerNoFbb);

    let output = try_decode_freeform_surfaces(&scan).expect("freeform route decodes b5 topology");
    assert_eq!(output.ir.model.bodies.len(), 1);
    assert_eq!(output.ir.model.faces.len(), 1);
    assert_eq!(output.ir.model.loops.len(), 1);
    assert_eq!(output.ir.model.coedges.len(), 3);
    assert_eq!(output.ir.model.edges.len(), 3);
    assert_eq!(output.ir.model.curves.len(), 3);
    assert_eq!(output.ir.model.vertices.len(), 3);
    assert_eq!(output.ir.model.pcurves.len(), 3);
    assert!(output
        .ir
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.parameter_range == Some([0.0, 1.0])));
    assert!(output.report.geometry_transferred);
    assert!(output
        .report
        .losses
        .iter()
        .any(|loss| loss.code == LossCode::TopologyNotTransferred));
}

/// When no b5 graph parses, the route falls back to the a5a8 surface carriers:
/// an object-stream a8 record transfers as a single NURBS surface carrying its
/// CGM source object.
#[test]
fn freeform_route_falls_back_to_a8_nurbs_surface_carrier() {
    let scan = scan_bytes(object_main_catpart(&a8_surface_stream()));
    assert_eq!(scan.variant, Variant::FloatPackedInnerNoFbb);

    let output = try_decode_freeform_surfaces(&scan).expect("freeform route decodes a8 surface");
    assert!(output.ir.model.bodies.is_empty());
    let [surface] = output.ir.model.surfaces.as_slice() else {
        panic!("one a8 NURBS surface carrier");
    };
    assert!(matches!(surface.geometry, SurfaceGeometry::Nurbs(_)));
    assert_eq!(
        surface
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:decafbad"))
    );
}

/// The a8 rolling-ball jet pool runs after the surface carriers: an `a8 03 32`
/// record transfers as a `RollingBallJet` procedural surface.
#[test]
fn freeform_route_appends_a8_rolling_ball_jet_pool() {
    let scan = scan_bytes(object_main_catpart(&a8_freeform_curve_stream()));
    assert_eq!(scan.variant, Variant::FloatPackedInnerNoFbb);

    let output = try_decode_freeform_surfaces(&scan).expect("freeform route decodes rolling ball");
    let [procedural] = output.ir.model.procedural_surfaces.as_slice() else {
        panic!("one rolling-ball construction");
    };
    let ProceduralSurfaceDefinition::RollingBallJet {
        degree,
        knots,
        multiplicities,
        sites,
    } = &procedural.definition
    else {
        panic!("rolling-ball jet definition");
    };
    assert_eq!(*degree, 5);
    assert_eq!(knots, &[0.0, 1.0]);
    assert_eq!(multiplicities, &[6, 6]);
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[1].first_limit, Point3::new(2.0, 0.0, 0.0));
}

/// A b2 cylinder carrier reaches the surface pool through the fallback path and
/// transfers as an analytic cylinder surface.
#[test]
fn freeform_route_collects_b2_cylinder_carrier() {
    let scan = scan_bytes(object_main_catpart(&b2_cylinder_stream()));
    assert_eq!(scan.variant, Variant::FloatPackedInnerNoFbb);

    let output = try_decode_freeform_surfaces(&scan).expect("freeform route decodes b2 cylinder");
    assert!(output
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| matches!(surface.geometry, SurfaceGeometry::Cylinder { radius, .. } if radius == 2.0)));
}

/// With neither a b5 graph nor any a5a8/b2 carrier, the route declines so the
/// orchestrator can fall through.
#[test]
fn freeform_route_declines_without_topology_or_carriers() {
    let scan = scan_bytes(object_main_catpart(&[0u8; 64]));
    assert_eq!(scan.variant, Variant::FloatPackedInnerNoFbb);
    assert!(try_decode_freeform_surfaces(&scan).is_none());
}

/// [`freeform_surface_carriers`] walks the a5a8 and b2 surface vocabularies into
/// a single carrier list, tagging each with its originating record family.
#[test]
fn freeform_surface_carriers_walk_a5_a8_and_b2_records() {
    let mut data = a8_surface_stream();
    data.extend_from_slice(&a5_surface_stream());
    data.extend_from_slice(&b2_cylinder_stream());
    data.extend_from_slice(&b2_cone_stream());

    let carriers = freeform_surface_carriers(&data);
    let mut kinds = carriers
        .iter()
        .map(|(_, _, _, kind)| *kind)
        .collect::<Vec<_>>();
    kinds.sort_unstable();
    assert_eq!(kinds, ["b2_03_28", "b2_03_29", "freeform", "freeform"]);
    assert!(carriers
        .iter()
        .any(|(_, object_id, geometry, _)| *object_id == 0xdeca_fbad
            && matches!(geometry, SurfaceGeometry::Nurbs(_))));
}

/// The a5 surface pool transfers an `a5 03 34` record as a NURBS surface with a
/// CGM source object.
#[test]
fn surface_pools_transfer_a5_nurbs_surface_carrier() {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    append_freeform_surface_pools(&mut ir, &mut annotations, &a5_surface_stream());
    let [surface] = ir.model.surfaces.as_slice() else {
        panic!("one a5 NURBS surface carrier");
    };
    assert!(matches!(surface.geometry, SurfaceGeometry::Nurbs(_)));
    assert!(surface
        .source_object
        .as_ref()
        .is_some_and(|source| source.object_id.starts_with("cgm-surface:")));
}

/// The a5 guide-curve pool lifts an `a5 03 39` quintic jet into a degree-5 NURBS
/// curve.
#[test]
fn surface_pools_transfer_a5_guide_curve() {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    append_freeform_surface_pools(&mut ir, &mut annotations, &a5_guide_curve_stream());
    let guide = ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id.0.starts_with("catia:guide:curve#"))
        .expect("guide NURBS curve");
    let CurveGeometry::Nurbs(nurbs) = &guide.geometry else {
        panic!("guide curve must be NURBS");
    };
    assert_eq!(nurbs.degree, 5);
    assert_eq!(nurbs.control_points.first().unwrap().x, 0.0);
    assert_eq!(nurbs.control_points.last().unwrap().z, 4.0);
}

/// The a5 freeform-curve pool transfers an `a5 03 32` record as a
/// `RollingBallJet` procedural surface.
#[test]
fn surface_pools_transfer_a5_freeform_rolling_ball_jet() {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    append_freeform_surface_pools(&mut ir, &mut annotations, &a5_freeform_curve_stream());
    let [procedural] = ir.model.procedural_surfaces.as_slice() else {
        panic!("one rolling-ball construction");
    };
    assert!(matches!(
        procedural.definition,
        ProceduralSurfaceDefinition::RollingBallJet { degree: 5, .. }
    ));
}

/// The consolidated surface-curve pool resolves a co-parametric edge run against
/// a b2 cylinder support, emitting a `catia:consolidated:construction` procedural
/// curve whose two resolved sides form an intersection.
#[test]
fn surface_pools_transfer_resolved_consolidated_cylinder_surface_curve() {
    let mut records = b2_cylinder_stream();
    for point in [
        [1.0f32, 4.0, 3.0],
        [2.0, 2.0 + 2.0 * 0.5f32.cos(), 3.0 + 2.0 * 0.5f32.sin()],
    ] {
        records.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            records.extend_from_slice(&le_f32(value));
        }
    }
    records.extend_from_slice(&a5_native_edge_run_stream(6, 139, 142));

    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    append_freeform_surface_pools(&mut ir, &mut annotations, &records);
    let procedural = ir
        .model
        .procedural_curves
        .iter()
        .find(|curve| curve.id.0.starts_with("catia:consolidated:construction#"))
        .expect("resolved consolidated construction");
    let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
        panic!("two resolved support sides form an intersection");
    };
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
}
