// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::draft::ModelDraft;
use cadmpeg_ir::eval::EvaluationFailure;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CurveId, EdgeId, VertexId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::CadIr;

use crate::test_support::test_drawing_and_trimming::explicit_multi_pcurve_loop_file_with_first_edge;
use crate::test_support::test_owned::{explicit_void_solid_file, owned_test_file, OwnedTestEntity};
use crate::test_support::test_solids_and_structure::{
    explicit_non_manifold_open_shell_file, explicit_open_shell_file,
    explicit_tetrahedron_solid_file, explicit_tetrahedron_solid_file_with_options,
    explicit_tetrahedron_solid_file_with_transform, explicit_vertex_loop_file,
    explicit_vertex_loop_file_with_outer_flag,
};
use crate::IgesCodec;

const EPS_EDGE_ENDPOINT_MATCH: f64 = 1.0e-9;

#[test]
fn brep_surface_endpoint_keeps_evaluator_resource_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = ctx
        .charge_collection_items(2, "iges B-rep surface evaluation")
        .unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected collection limit refusal");
    };
    let result = super::surface_point_or_refusal(Err(EvaluationFailure::ResourceLimit(limit)));
    assert!(matches!(
        result,
        Err(super::super::composite::CompositeCurveError::Budget(
            CodecError::ResourceLimit(actual)
        )) if actual == limit && actual.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn brep_counted_vectors_refuse_before_nested_allocation() {
    for operation in [
        "iges B-rep resolved pcurves",
        "iges B-rep mapped pcurves",
        "iges B-rep vertex-list points",
        "iges B-rep edge-list edges",
        "iges B-rep loop uses",
        "iges B-rep use pcurves",
        "iges B-rep shell face uses",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = crate::decode_resource::reserve_vec::<u8>(&ctx, 2, operation);
        assert!(matches!(
            result,
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.used == 0
                    && limit.additional == 2
                    && limit.operation == operation
        ));
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let admitted =
        crate::decode_resource::reserve_vec::<u8>(&ctx, 2, "iges B-rep use pcurves").unwrap();
    assert!(admitted.capacity() >= 2);
}

#[test]
fn brep_definition_nodes_and_nested_shells_refuse_before_allocation() {
    let two_loop_face = owned_test_file(&[
        OwnedTestEntity { entity_type: 116, form: 0, label: "CENTER".into(), status: "00010000", parameters: "116,0,0,0,0;".into() },
        OwnedTestEntity { entity_type: 196, form: 0, label: "SPHERE".into(), status: "00010000", parameters: "196,1,1;".into() },
        OwnedTestEntity { entity_type: 502, form: 1, label: "POLE".into(), status: "00010000", parameters: "502,1,0,0,1;".into() },
        OwnedTestEntity { entity_type: 508, form: 1, label: "VLOOP1".into(), status: "00010000", parameters: "508,1,1,5,1,0,0;".into() },
        OwnedTestEntity { entity_type: 508, form: 1, label: "VLOOP2".into(), status: "00010000", parameters: "508,1,1,5,1,0,0;".into() },
        OwnedTestEntity { entity_type: 510, form: 1, label: "FACE".into(), status: "00010000", parameters: "510,3,2,1,7,9;".into() },
        OwnedTestEntity { entity_type: 514, form: 2, label: "SHELL".into(), status: "00000000", parameters: "514,1,11,1;".into() },
    ]);
    for (bytes, operation) in [
        (explicit_vertex_loop_file(), "iges B-rep vertex-list nodes"),
        (explicit_tetrahedron_solid_file(), "iges B-rep edge-list nodes"),
        (explicit_vertex_loop_file(), "iges B-rep loop nodes"),
        (two_loop_face, "iges B-rep face loop pointers"),
        (explicit_vertex_loop_file(), "iges B-rep face nodes"),
        (explicit_vertex_loop_file(), "iges B-rep shell nodes"),
        (explicit_vertex_loop_file(), "iges B-rep sheet shell uses"),
        (explicit_vertex_loop_file(), "iges B-rep body definitions"),
        (explicit_void_solid_file().0, "iges B-rep solid shell uses"),
        (explicit_void_solid_file().0, "iges B-rep referenced closed shells"),
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..4096 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let result = IgesCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions { policy, ..DecodeOptions::default() });
            match result {
                Err(cadmpeg_ir::codec::DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    if limit.operation == operation {
                        found = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                Ok(_) => panic!("decode succeeded before B-rep definition refusal at {operation}"),
                Err(error) => panic!("unexpected B-rep definition result at {operation}: {error}"),
            }
        }
        assert!(found, "B-rep definition refusal was not reached: {operation}");
    }
}

#[test]
fn brep_projected_pcurve_uses_refuse_before_both_vector_allocations() {
    let uses = [(false, 7_u32)];
    let resolved = || vec![(
        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)
            ).unwrap(),
        ),
        [0.0, 1.0],
    )];
    let stem = crate::ids::Stem::directory(9_u32);
    for (cap, operation) in [
        (0, "iges B-rep projected pcurve uses"),
        (1, "iges B-rep pcurve slots"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut candidate = ModelDraft::new();
        let result = super::project_pcurve_uses(&mut candidate, &uses, resolved(), None, &stem, &ctx);
        assert!(matches!(result,
            Err(super::PcurveProjectionError::Resource(CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == operation
                    && limit.additional == 1
        ));
        assert!(candidate.model().pcurves.is_empty());
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut candidate = ModelDraft::new();
    let projected = super::project_pcurve_uses(&mut candidate, &uses, resolved(), None, &stem, &ctx).unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(candidate.model().pcurves.len(), 1);
}

#[test]
fn source_edge_selection_matches_the_edge_occurrence_endpoints() {
    let curve_id = CurveId::mint("test:model:curve#curve").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.edges.extend([
        Edge {
            id: EdgeId::mint("test:model:edge#wrong-occurrence").expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_id.clone()),
                Some([10.0, 11.0]),
            )
            .unwrap(),
            start: VertexId::mint("test:model:vertex#wrong-start").expect("identity grammar"),
            end: VertexId::mint("test:model:vertex#wrong-end").expect("identity grammar"),
            tolerance: None,
        },
        Edge {
            id: EdgeId::mint("test:model:edge#matching-occurrence").expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_id.clone()),
                Some([0.0, 2.0]),
            )
            .unwrap(),
            start: VertexId::mint("test:model:vertex#matching-start").expect("identity grammar"),
            end: VertexId::mint("test:model:vertex#matching-end").expect("identity grammar"),
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
    assert_eq!(
        source_edge.id.as_str(),
        "test:model:edge#matching-occurrence"
    );
}

#[test]
fn source_edge_selection_rejects_multiple_matching_occurrences() {
    let curve_id = CurveId::mint("test:model:curve#curve").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .unwrap(),
        )),
        source_object: None,
    });
    ir.model.edges.extend([
        Edge {
            id: EdgeId::mint("test:model:edge#first-occurrence").expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_id.clone()),
                Some([0.0, std::f64::consts::TAU]),
            )
            .unwrap(),
            start: VertexId::mint("test:model:vertex#first-start").expect("identity grammar"),
            end: VertexId::mint("test:model:vertex#first-end").expect("identity grammar"),
            tolerance: None,
        },
        Edge {
            id: EdgeId::mint("test:model:edge#second-occurrence").expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_id.clone()),
                Some([std::f64::consts::TAU, 2.0 * std::f64::consts::TAU]),
            )
            .unwrap(),
            start: VertexId::mint("test:model:vertex#second-start").expect("identity grammar"),
            end: VertexId::mint("test:model:vertex#second-end").expect("identity grammar"),
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
                .any(|body| body.id.as_str() == "iges:model:body#D27"),
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
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
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
        .find(|loop_| loop_.id.as_str() == "iges:model:loop#D11:D7")
        .unwrap_or_else(|| {
            panic!(
                "loops={:#?} losses={:#?}",
                result.ir().model.loops,
                result.report().losses
            )
        });
    assert!(loop_.coedges().is_empty());
    let (vertex, pcurves) = loop_.singular_vertex().expect("vertex-loop boundary");
    assert_eq!(vertex.as_str(), "iges:model:vertex#D11:D5:1");
    assert!(pcurves.is_empty());
    assert_eq!(
        result
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id == loop_.face)
            .map(|face| face.loop_role(&loop_.id))
            .unwrap_or_default(),
        cadmpeg_ir::topology::LoopBoundaryRole::Outer
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
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
        .find(|loop_| loop_.id.as_str() == "iges:model:loop#D11:D7")
        .unwrap();
    assert_eq!(
        result
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id == loop_.face)
            .map(|face| face.loop_role(&loop_.id))
            .unwrap_or_default(),
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
        .find(|body| body.id.as_str() == format!("iges:model:body#D{solid_sequence}"))
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
        region.shells[0].as_str(),
        format!("iges:model:shell#D{solid_sequence}:D{outer_sequence}")
    );
    assert_eq!(
        region.shells[1].as_str(),
        format!("iges:model:shell#D{solid_sequence}:D{void_sequence}")
    );
    let void_shell = result
        .ir()
        .model
        .shells
        .iter()
        .find(|shell| shell.id == region.shells[1])
        .unwrap();
    for face_id in void_shell.faces() {
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
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
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
        .all(|body| body.id.as_str() != "iges:model:body#D55"));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            == "IGES entity type 186 form 0 was not projected: closed shell does not use every edge exactly twice with opposite senses"
    }));
    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas()["entities"].len(),
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
        .find(|body| body.id.as_str() == "iges:model:body#D55")
        .unwrap();
    assert_eq!(
        body.transform.as_ref().unwrap().rows(),
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
        .filter(|point| point.id.as_str().starts_with("iges:model:point#D55:"))
        .map(|point| point.position().get())
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
        .find(|body| body.id.as_str() == "iges:model:body#D55")
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
    assert_eq!(shell.faces().len(), 4);
    let solid_edges = result
        .ir()
        .model
        .edges
        .iter()
        .filter(|edge| edge.id.as_str().starts_with("iges:model:edge#D55:"))
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
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
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
        .find(|body| body.id.as_str() == "iges:model:body#D23")
        .unwrap();
    assert_eq!(body.kind, cadmpeg_ir::topology::BodyKind::Sheet);
    let shell = result
        .ir()
        .model
        .shells
        .iter()
        .find(|shell| shell.id.as_str() == "iges:model:shell#D23")
        .unwrap();
    assert_eq!(shell.faces().len(), 1);
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id == shell.faces()[0])
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    assert_eq!(
        result
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id == loop_.face)
            .map(|face| face.loop_role(&loop_.id))
            .unwrap_or_default(),
        cadmpeg_ir::topology::LoopBoundaryRole::Outer
    );
    assert_eq!(loop_.coedges().len(), 4);
    let explicit_edges = result
        .ir()
        .model
        .edges
        .iter()
        .filter(|edge| edge.id.as_str().starts_with("iges:model:edge#D23:"))
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
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
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
        .find(|edge| edge.id.as_str() == "iges:model:edge#D37:D23:1")
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
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
