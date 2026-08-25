// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::draft::ModelDraft;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, PcurveGeometry, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, PcurveId, PointId, SurfaceId, VertexId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{Edge, Point, Sense, Vertex};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use super::{
    cluster_boundary_positions, coordinate_quantum, create_boundary_vertices,
    linear_boundary_relationship_is_valid, pcurve_within_declared_bounds, BoundaryEndpoint,
    BoundaryVertexClusterError, BoundaryVertexSourceEndpoint, FaceTolerancePolicy,
};
use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

const EPS_BOUNDARY_ENDPOINT_MATCH: f64 = 1.0e-9;

#[test]
fn pcurve_bounds_use_the_active_nurbs_subrange() {
    let geometry = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        weights: None,
        periodic: false,
    };
    let bounds = Some([Some(0.2), Some(0.8), None, None]);

    assert!(pcurve_within_declared_bounds(
        &geometry,
        [0.2, 0.8],
        bounds,
        [false, false]
    ));
    assert!(!pcurve_within_declared_bounds(
        &geometry,
        [0.0, 1.0],
        bounds,
        [false, false]
    ));
}

#[test]
fn pcurve_bounds_handle_a_full_multiplicity_internal_knot() {
    let geometry = PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0],
        control_points: vec![
            Point2::new(2.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(0.2, 0.0),
            Point2::new(0.3, 0.0),
            Point2::new(0.4, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    let bounds = Some([Some(0.0), Some(1.0), None, None]);

    assert!(pcurve_within_declared_bounds(
        &geometry,
        [0.5, 1.0],
        bounds,
        [false, false]
    ));
    assert!(!pcurve_within_declared_bounds(
        &geometry,
        [0.0, 0.5],
        bounds,
        [false, false]
    ));
}

#[test]
fn pcurve_bounds_keep_partial_domains_and_periodic_seams() {
    let geometry = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.5, 0.3), Point2::new(0.5, 2.0)],
        weights: None,
        periodic: false,
    };

    assert!(pcurve_within_declared_bounds(
        &geometry,
        [0.0, 1.0],
        Some([Some(0.0), Some(1.0), None, None]),
        [false, false]
    ));
    assert!(pcurve_within_declared_bounds(
        &geometry,
        [0.0, 1.0],
        Some([Some(0.0), Some(1.0), Some(0.0), Some(1.0)]),
        [false, true]
    ));
    assert!(pcurve_within_declared_bounds(
        &geometry,
        [0.0, 1.0],
        None,
        [false, false]
    ));
}

#[test]
fn decode_reports_an_out_of_domain_alternate_for_model_preferred_type_142() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(subrange_nurbs_surface_boundary_file(2)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result
        .ir()
        .model
        .faces
        .iter()
        .any(|face| face.id.0 == "iges:model:face#D9"));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == IgesLossCode::BoundaryPcurveOutsideSupportDomain.kind() }));
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id.0 == "iges:model:coedge#D9:0:0")
        .expect("trimmed boundary coedge");
    assert!(coedge.pcurves.is_empty());
}

#[test]
fn decode_rejects_an_out_of_domain_parameter_preferred_type_142() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(subrange_nurbs_surface_boundary_file(3)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!result
        .ir()
        .model
        .faces
        .iter()
        .any(|face| face.id.0 == "iges:model:face#D9"));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == IgesLossCode::BoundaryPcurveOutsideSupportDomain.kind() }));
}

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
fn boundary_vertex_creation_retains_every_source_endpoint() {
    let mut candidate = ModelDraft::new();
    let source_endpoints = vec![
        BoundaryVertexSourceEndpoint {
            edge: "iges:model:edge#source-a".into(),
            endpoint: BoundaryEndpoint::Start,
            position: Point3::new(1.0, 0.0, 0.0),
        },
        BoundaryVertexSourceEndpoint {
            edge: "iges:model:edge#source-b".into(),
            endpoint: BoundaryEndpoint::End,
            position: Point3::new(0.0, 0.0, 0.0),
        },
    ];

    let (vertex_ids, derivations) = create_boundary_vertices(
        &mut candidate,
        "D9",
        "iges:entity:directory#9",
        0,
        &source_endpoints,
        1.0,
    )
    .unwrap();

    assert_eq!(vertex_ids[0], vertex_ids[1]);
    assert_eq!(derivations.len(), 1);
    assert_eq!(derivations[0].source_entity, "iges:entity:directory#9");
    assert_eq!(derivations[0].representative, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(derivations[0].tolerance, 1.0);
    assert_eq!(derivations[0].source_endpoints.len(), 2);
    assert_eq!(
        derivations[0].source_endpoints[0].position,
        Point3::new(1.0, 0.0, 0.0)
    );
    assert_eq!(
        derivations[0].source_endpoints[1].position,
        Point3::new(0.0, 0.0, 0.0)
    );
}

#[test]
fn face_tolerance_policy_separates_declared_and_coordinate_bounds() {
    let global = crate::global::parse(
        &crate::card::scan(&fixed_ascii_with_global(
            b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,3,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
        ))
        .unwrap(),
    )
    .unwrap()
    .0
    .length_context()
    .unwrap();
    let points = [Point3::new(100.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)];
    let policy = FaceTolerancePolicy::from_global(&global, points.into_iter());

    assert!((global.minimum_resolution_mm() - 0.001).abs() <= f64::EPSILON * 64.0);
    assert!((coordinate_quantum(&global, points.into_iter()) - 1.0).abs() <= f64::EPSILON);
    assert!((policy.topology_sewing - 1.0).abs() <= f64::EPSILON);
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
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let candidates = vec![
        Edge {
            id: EdgeId("wrong-occurrence".into()),
            curve: Some(curve_id.clone()),
            start: VertexId("wrong-start".into()),
            end: VertexId("wrong-end".into()),
            param_range: Some([1.0, 2.0]),
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
    assert!(!super::edge_range_matches_curve(
        &candidates[0],
        &index,
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(11.0, 0.0, 0.0),
        EPS_BOUNDARY_ENDPOINT_MATCH,
    ));
    assert!(super::edge_range_matches_curve(
        &candidates[1],
        &index,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        EPS_BOUNDARY_ENDPOINT_MATCH,
    ));
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

    let mut ambiguous_candidates = candidates.clone();
    ambiguous_candidates.push(Edge {
        id: EdgeId("duplicate-occurrence".into()),
        curve: Some(CurveId("curve".into())),
        start: VertexId("matching-start".into()),
        end: VertexId("matching-end".into()),
        param_range: Some([0.0, 2.0]),
        tolerance: None,
    });
    assert!(matches!(
        super::select_boundary_edge(
            &ambiguous_candidates,
            &index,
            &surface_id,
            &[],
            Sense::Forward,
            EPS_BOUNDARY_ENDPOINT_MATCH,
            false,
        ),
        Err(super::BoundaryEdgeSelectionError::Ambiguous)
    ));
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
    assert_eq!(face.surface.0, "iges:model:surface#D15:implicit-outer");
    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| surface.surface == face.surface)
        .unwrap();
    match &procedural.definition {
        ProceduralSurfaceDefinition::CurveBounded {
            support,
            boundaries,
            boundary_pcurves,
            implicit_outer,
        } => {
            assert_eq!(support.0, "iges:model:surface#D1");
            assert_eq!(boundaries, &[CurveId("iges:model:curve#D9".into())]);
            assert_eq!(
                boundary_pcurves,
                &[PcurveId("iges:model:pcurve#D15:0:0:0".into())]
            );
            assert!(*implicit_outer);
        }
        definition => panic!("unexpected implicit-domain definition: {definition:?}"),
    }
}

#[test]
fn type_144_rejects_a_self_intersecting_linear_outer_boundary() {
    let rings = vec![vec![
        [0.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [1.0, 0.0],
        [0.0, 0.0],
    ]];
    let plane = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };

    assert_eq!(
        linear_boundary_relationship_is_valid(&rings, true, true, &plane, None, [false, false]),
        Some(false)
    );
}

#[test]
fn decode_rejects_a_linear_type_144_inner_boundary_outside_the_outer() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_with_boundaries_and_inner(
                "106,1,5,0,0,0,1,0,1,1,0,1,0,0;",
                "106,1,5,0,0.75,0.25,1.25,0.25,1.25,0.75,0.75,0.75,0.75,0.25;",
                "144,1,1,1,7,13;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result
        .ir()
        .model
        .faces
        .iter()
        .any(|face| face.id.0 == "iges:model:face#D15"));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss
                .message
                .contains("trimmed-surface boundary loops are not simple")
    }));
}

#[test]
fn decode_rejects_a_trimmed_surface_pointer_to_a_non_type_142_entity() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_with_boundaries(
                "106,1,5,0,0,0,1,0,1,1,0,1,0,0;",
                "144,1,1,1,5,13;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!result
        .ir()
        .model
        .faces
        .iter()
        .any(|face| face.id.0 == "iges:model:face#D15"));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
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
fn decode_rejects_a_bounded_surface_pointer_to_a_non_type_141_entity() {
    let source = String::from_utf8(parametrically_bounded_plane_file()).unwrap();
    let source = source.replace("143,1,1,1,7;", "143,1,1,1,5;");
    let result = IgesCodec
        .decode(
            &mut Cursor::new(source.into_bytes()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!result
        .ir()
        .model
        .faces
        .iter()
        .any(|face| face.id.0 == "iges:model:face#D9"));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
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
    assert_eq!(
        result.report().losses[0].code,
        IgesLossCode::EntityNotProjected.kind()
    );
    // D13 is the Type 144 owner, the seventh entity in the fixture. Pinning
    // the provenance tag is what separates this test from the bug it guards
    // against: the loss must land on the trimmed surface, never on the Type
    // 141 boundary or the Type 142 curve-on-surface it names.
    assert_eq!(
        result.report().losses[0]
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D13")
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
    assert_eq!(coedge.pcurves.len(), 1);
    assert_eq!(coedge.pcurves[0].pcurve.0, "iges:model:pcurve#D9:0:0:0");
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
    assert_eq!(coedge.pcurves.len(), 1);
    assert_eq!(coedge.pcurves[0].pcurve.0, "iges:model:pcurve#D9:0:0:0");
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
