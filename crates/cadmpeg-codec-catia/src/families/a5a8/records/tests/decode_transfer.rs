// SPDX-License-Identifier: Apache-2.0
//! A8 object-stream decode transfer cases.

use crate::test_support::test_a5a8::{
    a8_elided_surface_stream, a8_freeform_curve_stream, a8_pcurve_stream,
};
use crate::test_support::test_b5::a8_elided_surface_stream_with_native_vertex_chain;
use crate::test_support::test_container::object_main_catpart;
use crate::variant::Variant;
use crate::CatiaCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::Point3;
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;

#[test]
fn decode_geometry_fallback_transfers_an_external_a8_pole_grid() {
    let file = object_main_catpart(&a8_elided_surface_stream());
    let mut cur = Cursor::new(file);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("NURBS surface");
    };
    assert_eq!(surface.poles().len(), 9);
    assert_eq!(
        surface.poles().into_iter().nth(8).unwrap(),
        Point3::new(8.0, 2.0, 2.0)
    );
}

#[test]
fn decode_float_packed_stream_transfers_an_elided_a8_surface_with_native_topology() {
    let stream = a8_elided_surface_stream_with_native_vertex_chain();
    let graph = crate::test_support::with_service_context(|ctx| {
        crate::families::b5::graph::parse(ctx, &stream, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("generated A8 topology");
    assert!(graph.complete);
    assert_eq!(graph.faces.len(), 1);
    assert_eq!(graph.loops.len(), 1);
    assert_eq!(graph.pcurves.len(), 3);
    assert_eq!(graph.edges.len(), 3);
    assert_eq!(
        graph
            .vertices
            .logical_vertices()
            .iter()
            .map(|vertex| vertex.object_id)
            .collect::<Vec<_>>(),
        [600, 601, 602]
    );
    assert_eq!(
        graph
            .vertices
            .logical_vertices()
            .iter()
            .map(|vertex| crate::test_support::test_b5::coordinates(vertex.point))
            .collect::<Vec<_>>(),
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    );

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode elided A8 surface topology");
    assert_eq!(result.ir().model.surfaces.len(), 1);
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("NURBS surface");
    };
    assert_eq!(
        surface.poles().into_iter().nth(8).unwrap(),
        Point3::new(1.0, 1.0, 0.0)
    );
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.pcurves.len(), 3);
    assert!(result.report().losses.iter().all(|loss| {
        !matches!(
            loss.code.category(),
            cadmpeg_ir::report::loss::LossCategory::Geometry
                | cadmpeg_ir::report::loss::LossCategory::Topology
        ) || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_object_stream_does_not_promote_unbound_a8_pcurve() {
    let file = object_main_catpart(&a8_pcurve_stream());
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode unbound object-stream pcurve");
    assert!(decoded.ir().model.pcurves.is_empty());
    assert!(!decoded.ir().native_unknowns("catia").unwrap().is_empty());
}

#[test]
fn decode_object_stream_transfers_a8_rolling_ball_jet() {
    let file = object_main_catpart(&a8_freeform_curve_stream());
    assert_eq!(
        crate::test_support::with_service_context(|ctx| crate::container::scan_bytes(
            ctx,
            file.clone()
        ))
        .expect("service resource budget")
        .variant,
        Variant::FloatPackedInnerNoFbb
    );
    let decoded = EditableDecodeResult::from(
        CatiaCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("decode rolling-ball object stream"),
    );
    let [procedural] = decoded.ir().model.procedural_surfaces.as_slice() else {
        panic!("one rolling-ball construction");
    };
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RollingBallJet(jet) =
        procedural.definition()
    else {
        panic!("rolling-ball jet");
    };
    let degree = jet.degree();
    let stations = jet.stations();
    let knots: Vec<_> = stations.iter().map(|station| station.knot.get()).collect();
    let multiplicities: Vec<_> = stations
        .iter()
        .map(|station| station.multiplicity)
        .collect();
    let sites: Vec<_> = stations.iter().map(|station| &station.site).collect();
    assert_eq!(degree, 5);
    assert_eq!(knots, &[0.0, 1.0]);
    assert_eq!(multiplicities, &[6, 6]);
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[1].first_limit, Point3::new(2.0, 0.0, 0.0));
    assert_eq!(sites[1].angle.get(), std::f64::consts::FRAC_PI_2);
    let provenance = &decoded.source_fidelity().annotations.provenance[procedural.id.as_str()];
    assert_eq!(provenance.stream(), "catia:object_stream_a8_03_32");
    let tag = provenance
        .tag
        .as_deref()
        .expect("rolling-ball provenance tag");
    assert!(tag.contains("object_id:12345678"));
    assert!(tag.contains("multiplicities:[6, 6]"));
    assert_eq!(
        decoded.ir().model.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| (source.format.as_str(), source.object_id.as_str())),
        Some(("catia", "cgm-surface:12345678"))
    );
}
