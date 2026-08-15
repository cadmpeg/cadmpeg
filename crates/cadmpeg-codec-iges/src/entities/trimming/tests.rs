// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::WritePath;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use super::{cluster_boundary_positions, BoundaryVertexClusterError};
use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

const EPS_BOUNDARY_ENDPOINT_MATCH: f64 = 1.0e-9;

#[test]
fn boundary_vertex_clustering_rejects_non_transitive_tolerance_neighborhoods() {
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(0.75, 0.0, 0.0),
        Point3::new(1.5, 0.0, 0.0),
    ];

    assert_eq!(
        cluster_boundary_positions(&points, 1.0),
        Err(BoundaryVertexClusterError::NonTransitive)
    );
}

#[test]
fn boundary_vertex_clustering_uses_canonical_representatives() {
    let points = [
        Point3::new(10.25, 0.0, 0.0),
        Point3::new(0.5, 0.0, 0.0),
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(0.0, 0.0, 0.0),
    ];
    let clusters = cluster_boundary_positions(&points, 1.0).unwrap();

    assert_eq!(
        clusters
            .iter()
            .map(|cluster| cluster.representative)
            .collect::<Vec<_>>(),
        vec![Point3::new(10.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)]
    );
}

#[test]
fn boundary_edge_selection_uses_the_unique_pcurve_endpoint_match() {
    let curve_id = CurveId("curve".into());
    let surface_id = SurfaceId("surface".into());
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let candidates = vec![
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
            curve: Some(curve_id),
            start: VertexId("matching-start".into()),
            end: VertexId("matching-end".into()),
            param_range: Some([0.0, 2.0]),
            tolerance: None,
        },
    ];
    ir.model.points.extend([
        Point {
            id: PointId("wrong-point-start".into()),
            position: Point3::new(10.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("wrong-point-end".into()),
            position: Point3::new(11.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("matching-point-start".into()),
            position: Point3::new(0.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("matching-point-end".into()),
            position: Point3::new(2.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: VertexId("wrong-start".into()),
            point: PointId("wrong-point-start".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("wrong-end".into()),
            point: PointId("wrong-point-end".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("matching-start".into()),
            point: PointId("matching-point-start".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("matching-end".into()),
            point: PointId("matching-point-end".into()),
            tolerance: None,
        },
    ]);

    let pcurves = vec![(
        PcurveGeometry::Line {
            origin: Point2::new(0.0, 0.0),
            direction: Point2::new(2.0, 0.0),
        },
        [0.0, 1.0],
    )];
    let index = cadmpeg_ir::index::ModelIndex::new(&ir);
    let (selected, start, end, pcurves_agree) = super::select_boundary_edge(
        &candidates,
        &index,
        &surface_id,
        &pcurves,
        Sense::Forward,
        EPS_BOUNDARY_ENDPOINT_MATCH,
        true,
    )
    .expect("unique pcurve-compatible edge");
    assert_eq!(selected.id.0, "matching-occurrence");
    assert_eq!(start, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(end, Point3::new(2.0, 0.0, 0.0));
    assert!(pcurves_agree);
}

#[test]
fn decode_commits_a_large_batch_of_trimmed_surfaces_without_quadratic_growth() {
    let trimmed_count = 1_000;
    let mut entities = Vec::with_capacity(trimmed_count + 1);
    entities.push(OwnedTestEntity {
        entity_type: 128,
        form: 0,
        label: "SURFACE".into(),
        status: "00010000",
        parameters:
            "128,1,1,1,1,0,0,1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,0,0,0,1,0,0,0,1,0,1,1,0,0,1,0,1;".into(),
    });
    for index in 0..trimmed_count {
        entities.push(OwnedTestEntity {
            entity_type: 144,
            form: 0,
            label: format!("TRIM{index}"),
            status: "00000000",
            parameters: "144,1,0,0,0;".into(),
        });
    }

    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&entities)),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.faces.len(), trimmed_count);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_classifies_explicit_outer_and_inner_trimmed_surface_loops() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_with_inner_loop_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D15")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    assert_eq!(face.loops.len(), 2);
    let roles = face
        .loops
        .iter()
        .map(|id| {
            result
                .ir()
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == *id)
                .unwrap()
                .boundary_role
        })
        .collect::<Vec<_>>();
    assert_eq!(
        roles,
        vec![
            cadmpeg_ir::topology::LoopBoundaryRole::Outer,
            cadmpeg_ir::topology::LoopBoundaryRole::Inner,
        ]
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
fn decode_preserves_parameter_domain_as_implicit_outer_boundary() {
    for parameters in ["144,1,0,0,0;", "144,1,0,0,;", "144,1,0,0;"] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(parameter_domain_trimmed_surface_file(parameters)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let face = result
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.id.0 == "iges:model:face#D3")
            .unwrap_or_else(|| {
                panic!(
                    "parameters={parameters} losses={:#?}",
                    result.report().losses
                )
            });
        assert!(face.loops.is_empty());
        assert!(
            result.report().losses.is_empty(),
            "parameters={parameters} losses={:#?}",
            result.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_rejects_a_nonzero_implicit_outer_boundary_pointer() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parameter_domain_trimmed_surface_file("144,1,0,0,3;")),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.report().losses.iter().any(|loss| loss
        .message
        .contains("outer-boundary pointer is neither zero nor omitted")));
}

#[test]
fn decode_retains_inner_boundaries_after_an_omitted_outer_pointer() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_with_boundaries(
                "106,1,5,0,0,0,1,0,1,1,0,1,0,0;",
                "144,1,0,1,,13;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D15")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    assert_eq!(face.loops.len(), 1);
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == face.loops[0])
        .unwrap();
    assert_eq!(
        loop_.boundary_role,
        cadmpeg_ir::topology::LoopBoundaryRole::Inner
    );
}

#[test]
fn decode_accepts_independent_boundary_entities() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(independent_boundary_entities_file(false)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_does_not_blame_a_boundary_for_its_owning_surface_failure() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(independent_boundary_entities_file(true)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(
        result.report().losses.len(),
        1,
        "{:#?}",
        result.report().losses
    );
    assert!(result.report().losses[0]
        .message
        .contains("IGES entity type 144 form 0"));
    assert!(result.report().losses[0]
        .message
        .contains("boundary definition names a different support surface"));
}

#[test]
fn decode_brackets_curve_on_surface_carrier_agreement_at_the_global_resolution() {
    for (shift, decoded) in [("0.000999", true), ("0.001001", false)] {
        let shifted_one = 1.0 + shift.parse::<f64>().unwrap();
        let shifted_outer =
            format!("106,1,5,0,{shift},0,{shifted_one},0,{shifted_one},1,{shift},1,{shift},0;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(trimmed_plane_with_inner_loop_and_outer_pcurve(
                    &shifted_outer,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(
            result
                .ir()
                .model
                .faces
                .iter()
                .any(|face| face.id.0 == "iges:model:face#D15"),
            decoded,
            "{shift}"
        );
        assert_eq!(
            result.report().losses.iter().any(|loss| loss
                .message
                .contains("carriers disagree beyond the minimum resolution")),
            !decoded,
            "{shift}"
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_uses_model_curve_when_type_142_prefers_it() {
    let shifted_outer = "106,1,5,0,0.1,0,1.1,0,1.1,1,0.1,1,0.1,0;";
    let mut bytes = trimmed_plane_with_inner_loop_and_outer_pcurve(shifted_outer);
    let original = b"142,0,1,5,3,3;";
    let start = bytes
        .windows(original.len())
        .position(|window| window == original)
        .expect("outer Type 142 record");
    bytes[start + original.len() - 2] = b'2';
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D15")
        .expect("model-preferred trimmed face");
    let outer_loop = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == face.loops[0])
        .expect("outer loop");
    assert!(outer_loop.coedges.iter().all(|id| result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == *id)
        .is_some_and(|coedge| coedge.pcurves.is_empty())));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_ordered_type_141_pcurve_collections() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(multi_pcurve_boundary_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id.0 == "iges:model:coedge#D11:0:0")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    assert_eq!(coedge.pcurves.len(), 2);
    let endpoints = coedge
        .pcurves
        .iter()
        .map(|pcurve_use| {
            let pcurve = result
                .ir()
                .model
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == pcurve_use.pcurve)
                .expect("coedge pcurve resolves");
            (
                cadmpeg_ir::eval::pcurve_uv(&pcurve.geometry, 0.0).expect("start evaluates"),
                cadmpeg_ir::eval::pcurve_uv(&pcurve.geometry, 1.0).expect("end evaluates"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        endpoints,
        [
            (Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)),
            (Point2::new(1.0, 1.0), Point2::new(0.0, 0.0)),
        ]
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
fn decode_retains_agreeing_pcurves_when_type_141_prefers_model_curves() {
    let mut bytes = multi_pcurve_boundary_file();
    let original = b"141,1,3,";
    let start = bytes
        .windows(original.len())
        .position(|window| window == original)
        .expect("Type 141 record");
    bytes[start + 6] = b'1';
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id.0 == "iges:model:coedge#D11:0:0")
        .expect("model-preferred boundary coedge");
    assert_eq!(coedge.pcurves.len(), 2);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_brackets_type_141_pcurve_agreement_at_the_global_resolution() {
    for (shift, decoded) in [("0.000999", true), ("0.001001", false)] {
        let shifted = format!("126,1,1,1,0,1,0,0,0,1,1,1,1,{shift},0,0,1,1,0,0,1,0,0,1;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(multi_pcurve_boundary_file_with_first_pcurve(&shifted)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(
            result
                .ir()
                .model
                .bodies
                .iter()
                .any(|body| body.id.0 == "iges:model:body#D11"),
            decoded,
            "{shift}"
        );
        assert_eq!(
            result.report().losses.iter().any(|loss| loss
                .message
                .contains("curve-on-surface carriers disagree beyond the minimum resolution")),
            !decoded,
            "{shift}"
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_preserves_two_uses_and_periodic_images_of_a_cylinder_seam() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_cylinder_seam_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id.0 == "iges:model:loop#D21:D17")
        .unwrap();
    assert_eq!(loop_.coedges.len(), 2);
    let coedges = loop_
        .coedges
        .iter()
        .map(|id| {
            result
                .ir()
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(coedges[0].edge, coedges[1].edge);
    assert_ne!(coedges[0].sense, coedges[1].sense);
    assert_eq!(coedges[0].radial_next, coedges[1].id);
    assert_eq!(coedges[1].radial_next, coedges[0].id);
    let seam_u = coedges
        .iter()
        .map(|coedge| {
            let pcurve = result
                .ir()
                .model
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == coedge.pcurves[0].pcurve)
                .unwrap();
            cadmpeg_ir::eval::pcurve_uv(&pcurve.geometry, 0.0)
                .unwrap()
                .u
        })
        .collect::<Vec<_>>();
    assert!((seam_u[0] - 0.0).abs() < 1.0e-12);
    assert!((seam_u[1] - std::f64::consts::TAU).abs() < 1.0e-12);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_preserves_ordered_loop_pcurve_collection_and_isoparametric_flags() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_multi_pcurve_loop_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id.0 == "iges:model:coedge#D27:D23:0")
        .unwrap();
    assert_eq!(coedge.pcurves.len(), 2);
    assert_eq!(coedge.pcurves[0].isoparametric, Some(true));
    assert_eq!(coedge.pcurves[1].isoparametric, Some(false));
    assert!(coedge.pcurves[0].pcurve.0.ends_with(":0:0"));
    assert!(coedge.pcurves[1].pcurve.0.ends_with(":0:1"));
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id.0 == "iges:model:loop#D27:D23")
        .unwrap();
    assert_eq!(loop_.vertex_uses.len(), 1);
    assert_eq!(loop_.vertex_uses[0].vertex.0, "iges:model:vertex#D27:D15:2");
    assert_eq!(loop_.vertex_uses[0].after.as_ref(), Some(&coedge.id));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_brackets_explicit_loop_pcurve_agreement_at_the_global_resolution() {
    for (shift, decoded) in [("0.000999", true), ("0.001001", false)] {
        let shifted = format!("126,1,1,1,0,1,0,0,0,1,1,1,1,{shift},0,0,0.5,0,0,0,1,0,0,1;");
        let result = IgesCodec
            .decode(
                &mut Cursor::new(explicit_multi_pcurve_loop_file_with_first_pcurve(&shifted)),
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
            "{shift}"
        );
        assert_eq!(
            result.report().losses.iter().any(|loss| loss
                .message
                .contains("loop edge-use pcurves disagree with the edge vertices")),
            !decoded,
            "{shift}"
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_builds_a_parametrically_bounded_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(parametrically_bounded_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D9")
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == face.loops[0])
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges[0])
        .unwrap();
    assert_eq!(
        loop_.boundary_role,
        cadmpeg_ir::topology::LoopBoundaryRole::Unspecified
    );
    assert!(!coedge.pcurves.is_empty());
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_an_ordered_multi_segment_bounded_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(bounded_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D13")
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == face.loops[0])
        .unwrap();
    assert_eq!(loop_.coedges.len(), 4);
    let senses = loop_
        .coedges
        .iter()
        .map(|id| {
            result
                .ir()
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .unwrap()
                .sense
        })
        .collect::<Vec<_>>();
    assert_eq!(
        senses,
        vec![
            cadmpeg_ir::topology::Sense::Forward,
            cadmpeg_ir::topology::Sense::Reversed,
            cadmpeg_ir::topology::Sense::Forward,
            cadmpeg_ir::topology::Sense::Forward,
        ]
    );
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.owner_loop == loop_.id)
        .all(|coedge| coedge.pcurves.is_empty()));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_accepts_a_bounded_sheet_join_within_global_resolution() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(bounded_plane_with_resolution_gap_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D13")
        .expect("bounded face within the declared resolution");
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == face.loops[0])
        .expect("bounded loop");
    assert_eq!(loop_.coedges.len(), 4);
    assert_eq!(face.tolerance, Some(0.001));
    assert!(result
        .ir()
        .model
        .vertices
        .iter()
        .any(|vertex| vertex.tolerance == Some(0.001)));
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .any(|edge| edge.tolerance == Some(0.001)));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_rejects_a_bounded_sheet_join_just_beyond_global_resolution() {
    let mut bytes = bounded_plane_file();
    let original = b"110,1,1,0,1,0,0;";
    let replacement = b"110,1,1,0,1,0.001001,0;";
    let start = bytes
        .windows(original.len())
        .position(|window| window == original)
        .expect("bounded-plane edge parameter record");
    let line_start = bytes[..start]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let payload_end = line_start + 64;
    bytes[start..start + replacement.len()].copy_from_slice(replacement);
    bytes[start + replacement.len()..payload_end].fill(b' ');

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result
        .ir()
        .model
        .faces
        .iter()
        .all(|face| face.id.0 != "iges:model:face#D13"));
    assert!(
        result.report().losses.iter().any(|loss| {
            loss.message
                .contains("boundary segments do not form a closed ring")
        }),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_converts_non_millimetre_resolution_before_sewing_a_bounded_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(centimetre_bounded_plane_with_resolution_gap_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D13")
        .expect("bounded face within the unit-converted resolution");
    assert_eq!(face.tolerance, Some(0.01));
    assert!(result
        .ir()
        .model
        .vertices
        .iter()
        .any(|vertex| vertex.tolerance == Some(0.01)));
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .any(|edge| edge.tolerance == Some(0.01)));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_sews_boundary_roundoff_with_declared_coordinate_significance() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(bounded_plane_with_significance_gap_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D13")
        .expect("bounded face within one declared coordinate quantum");
    assert_eq!(face.tolerance, Some(0.01));
    assert!(result
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.fit_tolerance.is_none()));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_valid_face_local_trimmed_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let sheet = result
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.id.0 == "iges:model:body#D9")
        .unwrap();
    assert_eq!(sheet.kind, cadmpeg_ir::topology::BodyKind::Sheet);
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D9")
        .unwrap();
    assert_eq!(face.surface.0, "iges:model:surface#D1");
    assert_eq!(face.loops.len(), 1);
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
    assert_eq!(loop_.coedges.len(), 1);
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges[0])
        .unwrap();
    assert_eq!(coedge.radial_next, coedge.id);
    assert!(!coedge.pcurves.is_empty());
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_trimmed_sheet_from_a_native_circle_pcurve() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_circle_pcurve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D9")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == face.loops[0])
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges[0])
        .unwrap();
    assert_eq!(coedge.pcurves.len(), 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_builds_a_model_curve_only_trimmed_sheet() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(model_curve_only_trimmed_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.0 == "iges:model:face#D9")
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == face.loops[0])
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges[0])
        .unwrap();
    assert!(coedge.pcurves.is_empty());
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
