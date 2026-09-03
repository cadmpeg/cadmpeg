// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{Curve, CurveGeometry};
use cadmpeg_ir::ids::{CurveId, EdgeId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::CadIr;

use crate::test_support::*;
use crate::IgesCodec;

const EPS_EDGE_ENDPOINT_MATCH: f64 = 1.0e-9;

#[test]
fn source_edge_selection_matches_the_edge_occurrence_endpoints() {
    let curve_id = CurveId("curve".into());
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.edges.extend([
        Edge {
            id: EdgeId("wrong-occurrence".into()),
            curve: Some(curve_id.clone()),
            start: VertexId("wrong-start".into()),
            end: VertexId("wrong-end".into()),
            param_range: Some([10.0, 11.0]),
            tolerance: None,
        },
        Edge {
            id: EdgeId("matching-occurrence".into()),
            curve: Some(curve_id.clone()),
            start: VertexId("matching-start".into()),
            end: VertexId("matching-end".into()),
            param_range: Some([0.0, 2.0]),
            tolerance: None,
        },
    ]);

    let source_edge = super::source_edge_for_vertices(
        &ir,
        &[0, 1],
        &ir.model.curves[0].geometry,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        EPS_EDGE_ENDPOINT_MATCH,
    )
    .expect("matching edge occurrence");
    assert_eq!(source_edge.id.0, "matching-occurrence");
}

#[test]
fn source_edge_selection_rejects_multiple_matching_occurrences() {
    let curve_id = CurveId("curve".into());
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 1.0,
        },
        source_object: None,
    });
    ir.model.edges.extend([
        Edge {
            id: EdgeId("first-occurrence".into()),
            curve: Some(curve_id.clone()),
            start: VertexId("first-start".into()),
            end: VertexId("first-end".into()),
            param_range: Some([0.0, std::f64::consts::TAU]),
            tolerance: None,
        },
        Edge {
            id: EdgeId("second-occurrence".into()),
            curve: Some(curve_id.clone()),
            start: VertexId("second-start".into()),
            end: VertexId("second-end".into()),
            param_range: Some([std::f64::consts::TAU, 2.0 * std::f64::consts::TAU]),
            tolerance: None,
        },
    ]);

    let result = super::source_edge_for_vertices(
        &ir,
        &[0, 1],
        &ir.model.curves[0].geometry,
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        EPS_EDGE_ENDPOINT_MATCH,
    );
    assert!(matches!(
        result,
        Err(super::SourceEdgeSelectionError::Ambiguous)
    ));
}

#[test]
fn decode_brackets_explicit_edge_vertex_agreement_at_the_global_resolution() {
    for (end_x, decoded) in [("1.000999", true), ("1.001001", false)] {
        let edge = format!("110,0,0,0,{end_x},0,0;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(explicit_multi_pcurve_loop_file_with_first_edge(&edge)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(
            result
                .ir()
                .model
                .bodies
                .iter()
                .any(|body| body.id.0 == "iges:model:body#D27"),
            decoded,
            "{end_x}"
        );
        assert_eq!(
            result.report().losses.iter().any(|loss| loss
                .message
                .contains("edge curve endpoints disagree with the vertex-list points")),
            !decoded,
            "{end_x}"
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_builds_a_vertex_only_pole_loop() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_vertex_loop_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id.0 == "iges:model:loop#D11:D7")
        .unwrap_or_else(|| {
            panic!(
                "loops={:#?} losses={:#?}",
                result.ir().model.loops,
                result.report().losses
            )
        });
    assert!(loop_.coedges().is_empty());
    let (vertex, pcurves) = loop_.singular_vertex().expect("vertex-loop boundary");
    assert_eq!(vertex.0, "iges:model:vertex#D11:D5:1");
    assert!(pcurves.is_empty());
    assert_eq!(
        loop_.boundary_role,
        cadmpeg_ir::topology::LoopBoundaryRole::Outer
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_a_face_with_no_explicit_outer_loop() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_vertex_loop_file_with_outer_flag(false)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id.0 == "iges:model:loop#D11:D7")
        .unwrap();
    assert_eq!(
        loop_.boundary_role,
        cadmpeg_ir::topology::LoopBoundaryRole::Unspecified
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_builds_a_solid_with_an_oriented_void_shell() {
    let (bytes, solid_sequence, outer_sequence, void_sequence) = explicit_void_solid_file();
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let body = result
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.id.0 == format!("iges:model:body#D{solid_sequence}"))
        .unwrap();
    assert_eq!(body.kind, cadmpeg_ir::topology::BodyKind::Solid);
    let region = result
        .ir()
        .model
        .regions
        .iter()
        .find(|region| region.id == body.regions[0])
        .unwrap();
    assert_eq!(region.shells.len(), 2);
    assert_eq!(
        region.shells[0].0,
        format!("iges:model:shell#D{solid_sequence}:D{outer_sequence}")
    );
    assert_eq!(
        region.shells[1].0,
        format!("iges:model:shell#D{solid_sequence}:D{void_sequence}")
    );
    let void_shell = result
        .ir()
        .model
        .shells
        .iter()
        .find(|shell| shell.id == region.shells[1])
        .unwrap();
    for face_id in &void_shell.faces {
        let face = result
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id == *face_id)
            .unwrap();
        assert_eq!(face.sense, cadmpeg_ir::topology::Sense::Reversed);
    }
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_rejects_closed_shell_with_inconsistent_radial_sense() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file_with_options(false, true)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(result
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| body.id.0 != "iges:model:body#D55"));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            == "IGES entity type 186 form 0 was not projected: closed shell does not use every edge exactly twice with opposite senses"
    }));
    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas["entities"].len(),
        28
    );
}

#[test]
fn decode_applies_manifold_solid_placement_at_body_scope_once() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file_with_transform(true)),
            &DecodeOptions::default(),
        )
        .unwrap();

    let body = result
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.id.0 == "iges:model:body#D55")
        .unwrap();
    assert_eq!(
        body.transform.as_ref().unwrap().rows,
        [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
    let points = result
        .ir()
        .model
        .points
        .iter()
        .filter(|point| point.id.0.starts_with("iges:model:point#D55:"))
        .map(|point| point.position)
        .collect::<Vec<_>>();
    assert!(points.contains(&cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)));
    assert!(points.contains(&cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0)));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_builds_a_connected_manifold_tetrahedron() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let body = result
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.id.0 == "iges:model:body#D55")
        .unwrap();
    assert_eq!(body.kind, cadmpeg_ir::topology::BodyKind::Solid);
    let region = result
        .ir()
        .model
        .regions
        .iter()
        .find(|region| region.id == body.regions[0])
        .unwrap();
    assert_eq!(region.shells.len(), 1);
    let shell = result
        .ir()
        .model
        .shells
        .iter()
        .find(|shell| shell.id == region.shells[0])
        .unwrap();
    assert_eq!(shell.faces.len(), 4);
    let solid_edges = result
        .ir()
        .model
        .edges
        .iter()
        .filter(|edge| edge.id.0.starts_with("iges:model:edge#D55:"))
        .collect::<Vec<_>>();
    assert_eq!(solid_edges.len(), 6);
    for edge in solid_edges {
        let uses = result
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge.id)
            .collect::<Vec<_>>();
        assert_eq!(uses.len(), 2);
        assert_ne!(uses[0].sense, uses[1].sense);
        assert_eq!(uses[0].radial_next, uses[1].id);
        assert_eq!(uses[1].radial_next, uses[0].id);
    }
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_shared_explicit_open_shell_topology() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_open_shell_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let body = result
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.id.0 == "iges:model:body#D23")
        .unwrap();
    assert_eq!(body.kind, cadmpeg_ir::topology::BodyKind::Sheet);
    let shell = result
        .ir()
        .model
        .shells
        .iter()
        .find(|shell| shell.id.0 == "iges:model:shell#D23")
        .unwrap();
    assert_eq!(shell.faces.len(), 1);
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id == shell.faces[0])
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == face.loops[0])
        .unwrap();
    assert_eq!(
        loop_.boundary_role,
        cadmpeg_ir::topology::LoopBoundaryRole::Outer
    );
    assert_eq!(loop_.coedges().len(), 4);
    let explicit_edges = result
        .ir()
        .model
        .edges
        .iter()
        .filter(|edge| edge.id.0.starts_with("iges:model:edge#D23:"))
        .collect::<Vec<_>>();
    assert_eq!(explicit_edges.len(), 4);
    assert_eq!(
        explicit_edges
            .iter()
            .flat_map(|edge| [&edge.start, &edge.end])
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_a_three_use_non_manifold_radial_ring() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_non_manifold_open_shell_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let edge = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id.0 == "iges:model:edge#D37:D23:1")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    let uses = result
        .ir()
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == edge.id)
        .collect::<Vec<_>>();
    assert_eq!(uses.len(), 3);
    let by_id = uses
        .iter()
        .map(|coedge| (&coedge.id, *coedge))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut current = uses[0];
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..3 {
        assert!(visited.insert(current.id.clone()));
        current = by_id[&current.radial_next];
    }
    assert_eq!(current.id, uses[0].id);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
