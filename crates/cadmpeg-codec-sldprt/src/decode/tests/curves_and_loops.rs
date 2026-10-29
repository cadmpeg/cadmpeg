// SPDX-License-Identifier: Apache-2.0
//! Edge-curve decode and rejected-loop topology tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::sldprt_with_body;
use crate::test_support::parasolid::bridge;
use crate::test_support::parasolid::circle_carrier;
use crate::test_support::parasolid::coedge;
use crate::test_support::parasolid::edge_use;
use crate::test_support::parasolid::framed_topology_fin;
use crate::test_support::parasolid::line_carrier;
use crate::test_support::parasolid::loop_head;
use crate::test_support::parasolid::nurbs_curve_carrier;
use crate::test_support::parasolid::plane_carrier;
use crate::test_support::parasolid::triangle_body;
use crate::test_support::parasolid::typed_nurbs_curve_carrier;
use crate::test_support::parasolid::vertex_use;
use crate::test_support::parasolid::world_point;
use crate::SldprtCodec;

#[test]
fn edge_uses_decoded_line_curve() {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};

    let mut body = Vec::new();
    body.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    body.extend(edge_use(40, 70)); // curve = line carrier 70
    body.extend(edge_use(41, 0));
    body.extend(edge_use(42, 0));
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));

    crate::test_support::parasolid::complete_fixture_vertex_owners(&mut body);
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);
    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.curves.len(), 1);
    match &result.ir().model.curves[0].geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let direction = *line_curve.direction().as_raw();
            assert_eq!(direction.x, 1.0);
        }
        other => panic!("expected line, got {other:?}"),
    }
    assert_eq!(
        result
            .ir()
            .model
            .edges
            .iter()
            .filter(|e| e.curve().is_some())
            .count(),
        1
    );
    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .any(|coedge| !coedge.pcurves.is_empty()));
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn edge_uses_decode_nurbs_curve() {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};

    let mut body = triangle_body();
    body.extend(nurbs_curve_carrier(170, 171));
    let edge = body
        .windows(2)
        .position(|w| w == [0x00, 0x10])
        .expect("edge-use");
    body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());
    let f = sldprt_with_body(&body);
    let mut cur = Cursor::new(f);

    let result = SldprtCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    let nurbs = result
        .ir()
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS curve");
    assert_eq!(nurbs.degree(), 2);
    assert_eq!(nurbs.control_points().len(), 3);
    assert_eq!(nurbs.knots().as_slice(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
}

#[test]
fn edge_uses_decode_typed_reference_nurbs_curve() {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};

    let mut body = triangle_body();
    body.extend(typed_nurbs_curve_carrier(170, 171));
    let edge = body
        .windows(2)
        .position(|window| window == [0x00, 0x10])
        .expect("edge-use");
    body[edge + 24..edge + 26].copy_from_slice(&170u16.to_be_bytes());

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let nurbs = result
        .ir()
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => Some(nurbs),
            _ => None,
        })
        .expect("NURBS curve");
    assert_eq!(nurbs.degree(), 2);
    assert_eq!(nurbs.control_points().len(), 3);
}

#[test]
fn reused_carrier_attribute_resolves_by_geometry_kind() {
    use cadmpeg_ir::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };

    let mut body = triangle_body();
    let bridge = body
        .windows(2)
        .position(|window| window == [0x00, 0x0e])
        .expect("bridge");
    body[bridge + 26..bridge + 28].copy_from_slice(&70u16.to_be_bytes());
    let edge = body
        .windows(2)
        .position(|window| window == [0x00, 0x10])
        .expect("edge-use");
    body[edge + 24..edge + 26].copy_from_slice(&70u16.to_be_bytes());
    body.extend(line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    body.extend(plane_carrier(
        70,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(matches!(
        result.ir().model.curves[0].geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
    ));
    assert!(matches!(
        result.ir().model.surfaces[0].geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
    ));
}

#[test]
fn false_later_loop_candidate_does_not_replace_owned_loop() {
    let mut body = triangle_body();
    body.extend(loop_head(20, 30, 999));

    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(
        result.ir().model.loops[0].id.as_str(),
        "sldprt:brep:loop#20"
    );
}

#[test]
fn decode_removes_edges_and_vertices_from_a_rejected_loop() {
    let mut body = triangle_body();
    body.extend(plane_carrier(
        200,
        [2.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(bridge(110, 120, 200));
    body.extend(loop_head(120, 130, 110));
    body.extend(coedge(130, 120, 131, 150, 0, 140, false));
    body.extend(coedge(131, 120, 132, 151, 0, 141, false));
    body.extend(coedge(132, 120, 130, 152, 0, 142, false));
    body.extend(edge_use(140, 0));
    body.extend(edge_use(141, 0));
    body.extend(edge_use(142, 0));
    body.extend(vertex_use(150, 160));
    body.extend(vertex_use(151, 161));
    body.extend(vertex_use(152, 162));
    body.extend(world_point(160, [2.0, 0.0, 0.0]));
    body.extend(world_point(161, [3.0, 0.0, 0.0]));

    crate::test_support::parasolid::complete_fixture_vertex_owners(&mut body);
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.points.len(), 3);
    assert!(
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );
}

fn patch_fin_ref(body: &mut [u8], attr: u16, slot: usize, value: u16) {
    let id = attr.to_be_bytes();
    let at = body
        .windows(4)
        .position(|window| {
            window[0] == 0x00 && window[1] == 0x11 && window[2] == id[0] && window[3] == id[1]
        })
        .expect("synthetic FIN record");
    let start = at + 4 + 2 * slot;
    body[start..start + 2].copy_from_slice(&value.to_be_bytes());
}

fn patch_edge_use_canonical(body: &mut [u8], attr: u16, canonical: u16) {
    let id = attr.to_be_bytes();
    let at = body
        .windows(4)
        .position(|window| {
            window[0] == 0x00 && window[1] == 0x10 && window[2] == id[0] && window[3] == id[1]
        })
        .expect("synthetic edge-use record");
    body[at + 18..at + 20].copy_from_slice(&canonical.to_be_bytes());
}

/// A boundary dummy FIN: loop and sibling links are null sentinels, the
/// start vertex-use names the sheet edge start, and `other` names the
/// walked reciprocal partner.
fn boundary_dummy_fin(attr: u16, start_vuse: u16, partner: u16, edge: u16) -> Vec<u8> {
    framed_topology_fin(
        attr,
        [0, 0, 0, 0, start_vuse, partner, edge, 0, 0],
        false,
        false,
        false,
    )
}

#[test]
fn dummy_partner_recovers_sheet_boundary_edge() {
    let mut body = triangle_body();
    patch_fin_ref(&mut body, 30, 5, 33);
    patch_edge_use_canonical(&mut body, 40, 33);
    body.extend(boundary_dummy_fin(33, 51, 30, 40));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.local_code() != "topology.fin-partner-unresolved"));
    assert!(
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("validate the neutral model")
            .is_ok()
    );
}

#[test]
fn dummy_partner_mismatch_withholds_the_edge() {
    let mut body = triangle_body();
    patch_fin_ref(&mut body, 30, 5, 33);
    patch_edge_use_canonical(&mut body, 40, 33);
    body.extend(boundary_dummy_fin(33, 50, 30, 40));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 0);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "topology.fin-partner-unresolved")
            .count(),
        1
    );
}

#[test]
fn dummy_partner_null_nose_withholds_the_edge() {
    let mut body = triangle_body();
    patch_fin_ref(&mut body, 30, 5, 33);
    patch_edge_use_canonical(&mut body, 40, 33);
    body.extend(boundary_dummy_fin(33, 1, 30, 40));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 0);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "topology.fin-partner-unresolved")
            .count(),
        1
    );
}

#[test]
fn nonreciprocal_backward_link_withholds_the_ring() {
    let mut body = triangle_body();
    patch_fin_ref(&mut body, 30, 2, 31);
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 0);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "topology.fin-link-unresolved")
            .count(),
        1
    );
}

#[test]
fn vertexless_ring_edge_without_reciprocal_partner_is_withheld() {
    let mut body = Vec::new();
    body.extend(plane_carrier(
        100,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend(circle_carrier(200, [1.0, 2.0, 0.0], [0.0, 0.0, 1.0], 0.5));
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 30, 1, 0, 40, false));
    body.extend(edge_use(40, 200));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&body)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), 0);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "topology.ring-edge-withheld")
            .count(),
        1
    );
}
