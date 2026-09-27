// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::draft::ModelDraft;
use cadmpeg_ir::geometry::{
    pcurve::{PcurveGeometry, PcurveNurbs},
    Curve, CurveGeometry, ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, PcurveId, PointId, SurfaceId, VertexId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{Edge, Point, Sense, Vertex};
use cadmpeg_ir::CadIr;

use super::{
    append_path, cluster_boundary_positions, coordinate_quantum, create_boundary_vertices,
    homogeneous_pcurve_spans, linear_boundary_relationship_is_valid, linear_boundary_rings,
    pcurve_within_declared_bounds,
    BoundaryEndpoint, BoundarySpace, BoundarySurfaceKind, BoundaryVertexClusterError,
    BoundaryVertexCreationError,
    BoundaryVertexSourceEndpoint, DeclaredInterval, FaceTolerancePolicy, LinearBoundaryGeometry,
    SimpleRing,
};
use crate::loss::IgesLossCode;
use crate::test_support::test_cards::fixed_ascii_with_global;
use crate::test_support::test_drawing_and_trimming::{
    explicit_cylinder_seam_file, explicit_multi_pcurve_loop_file,
    explicit_multi_pcurve_loop_file_with_first_pcurve, independent_boundary_entities_file,
    multi_pcurve_boundary_file, multi_pcurve_boundary_file_with_first_pcurve,
    parameter_domain_trimmed_surface_file, subrange_nurbs_surface_boundary_file,
    subrange_nurbs_surface_boundary_file_with_pcurve,
    subrange_nurbs_surface_boundary_file_with_source_precision,
    subrange_nurbs_surface_boundary_file_with_source_precision_outside_nominal,
    trimmed_plane_with_boundaries,
    trimmed_plane_with_boundaries_and_inner, trimmed_plane_with_inner_loop_and_outer_pcurve,
    trimmed_plane_with_inner_loop_file,
};
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::test_support::test_procedural_surfaces::{
    trimmed_procedural_line_surface_of_revolution_file,
    trimmed_procedural_line_surface_of_revolution_file_with_global,
};
use crate::test_support::test_solids_and_structure::parametrically_bounded_plane_file;
use crate::test_support::test_surface_fixtures::{
    bounded_plane_file, bounded_plane_with_resolution_gap_file,
    bounded_plane_with_significance_gap_file, centimetre_bounded_plane_with_resolution_gap_file,
    model_curve_only_trimmed_plane_file, trimmed_circle_pcurve_file, trimmed_plane_file,
};
use crate::IgesCodec;

const EPS_BOUNDARY_ENDPOINT_MATCH: f64 = 1.0e-9;
const EPS_SOURCE_BOUND_REPRESENTATION: f64 = 5.0e-7;

fn assert_trimming_collection_refusal(bytes: &[u8], operation: &str) {
    let mut cap = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        match IgesCodec.decode(&mut Cursor::new(bytes), &DecodeOptions { policy, ..DecodeOptions::default() }) {
            Err(cadmpeg_ir::codec::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                if limit.operation == operation { return; }
                cap = limit.used.checked_add(limit.additional).unwrap();
            }
            other => panic!("expected trimming collection refusal at {operation}: {other:?}"),
        }
    }
    panic!("trimming collection refusal was not reached: {operation}");
}

fn assert_trimming_retained_refusal(bytes: &[u8], operation: &str) {
    let mut cap = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        match IgesCodec.decode(&mut Cursor::new(bytes), &DecodeOptions { policy, ..DecodeOptions::default() }) {
            Err(cadmpeg_ir::codec::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))) => {
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                if limit.operation == operation { return; }
                cap = limit.used.checked_add(limit.additional).unwrap();
            }
            other => panic!("expected trimming retained refusal at {operation}: {other:?}"),
        }
    }
    panic!("trimming retained refusal was not reached: {operation}");
}

#[test]
fn trimming_projection_refuses_counted_boundary_vectors() {
    for (bytes, operation) in [
        (bounded_plane_file(), "iges Type141 boundary segments"),
        (multi_pcurve_boundary_file(), "iges Type141 segment pcurves"),
        (trimmed_plane_file(), "iges Type144 boundary sequences"),
        (bounded_plane_file(), "iges Type143 boundary sequences"),
        (bounded_plane_file(), "iges trimming linear candidates"),
        (bounded_plane_file(), "iges trimming boundary items"),
        (multi_pcurve_boundary_file(), "iges trimming segment pcurves"),
        (bounded_plane_file(), "iges trimming coedge ids"),
        (bounded_plane_file(), "iges trimming source endpoints"),
        (bounded_plane_file(), "iges trimming candidate vertex derivations"),
        (bounded_plane_file(), "loop ring validation members"),
    ] {
        assert_trimming_collection_refusal(&bytes, operation);
    }
}

#[test]
fn trimming_projection_refuses_retained_boundary_source_text() {
    let bytes = bounded_plane_file();
    for operation in [
        "iges trimming source endpoint edge text",
        "iges trimming source entity text",
        "iges boundary derivation edge text",
        "iges boundary derivation source text",
    ] {
        assert_trimming_retained_refusal(&bytes, operation);
    }
}

#[test]
fn implicit_outer_surface_attachment_refuses_procedural_slot() {
    let bytes = trimmed_plane_with_boundaries(
        "106,1,5,0,0,0,1,0,1,1,0,1,0,0;",
        "144,1,0,1,,13;",
    );
    let result = IgesCodec.decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default()).unwrap();
    assert!(!result.ir().model.procedural_surfaces.is_empty());
    let mut cap = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        match IgesCodec.decode(&mut Cursor::new(bytes.clone()), &DecodeOptions { policy, ..DecodeOptions::default() }) {
            Err(cadmpeg_ir::codec::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                if limit.operation == "iges procedural surface slots" { return; }
                cap = limit.used.checked_add(limit.additional).unwrap();
            }
            other => panic!("expected implicit outer procedural slot refusal: {other:?}"),
        }
    }
    panic!("implicit outer procedural slot refusal was not reached");
}

#[test]
fn linear_boundary_path_refuses_collection_limit_before_append() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut target = Vec::new();
    let result = append_path(&mut target, vec![1_u8, 2, 3], &ctx);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 3
    ));
    assert!(target.is_empty());

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(append_path(&mut target, vec![1_u8, 2, 3], &ctx).unwrap());
    assert_eq!(target, [1, 2, 3]);
}

#[test]
fn pcurve_bounds_keep_a_wide_finite_knot_span() {
    let geometry = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![-f64::MAX, -f64::MAX, f64::MAX, f64::MAX],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)],
            None,
            false,
        )
        .unwrap(),
    };
    assert!(pcurve_within_declared_bounds(
        &geometry,
        [-f64::MAX, f64::MAX],
        Some([Some(0.0), Some(1.0), Some(0.0), Some(1.0)]),
        [false, false],
    ));
}

#[test]
fn pcurve_bounds_use_the_active_nurbs_subrange() {
    let geometry = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            None,
            false,
        )
        .expect("valid test pcurve"),
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
        nurbs: PcurveNurbs::from_lanes(
            2,
            vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0],
            vec![
                Point2::new(2.0, 0.0),
                Point2::new(2.0, 0.0),
                Point2::new(2.0, 0.0),
                Point2::new(0.2, 0.0),
                Point2::new(0.3, 0.0),
                Point2::new(0.4, 0.0),
            ],
            None,
            false,
        )
        .expect("valid test pcurve"),
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
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.5, 0.3), Point2::new(0.5, 2.0)],
            None,
            false,
        )
        .expect("valid test pcurve"),
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
fn source_parameter_interval_must_reach_each_finite_support_bound() {
    let lower = DeclaredInterval::around(0.703_187_779_306_162, 0.0);
    let upper = DeclaredInterval::around(0.900_082_841_304_349, 0.0);
    let represented_lower =
        DeclaredInterval::around(0.703_187_779, EPS_SOURCE_BOUND_REPRESENTATION);
    let separated = DeclaredInterval::around(0.5, EPS_SOURCE_BOUND_REPRESENTATION);

    assert!(super::parameter_interval_reaches_bounds(
        represented_lower,
        Some(lower),
        Some(upper)
    ));
    assert!(!super::parameter_interval_reaches_bounds(
        separated,
        Some(lower),
        Some(upper)
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
        .any(|face| face.id.as_str() == "iges:model:face#D9"));
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
        .find(|coedge| coedge.id.as_str() == "iges:model:coedge#D9:0:0")
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
        .any(|face| face.id.as_str() == "iges:model:face#D9"));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == IgesLossCode::BoundaryPcurveOutsideSupportDomain.kind() }));
}

#[test]
fn decode_admits_pcurve_whose_source_intervals_reach_support_bounds() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(subrange_nurbs_surface_boundary_file_with_source_precision()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(
        result
            .ir()
            .model
            .faces
            .iter()
            .any(|face| face.id.as_str() == "iges:model:face#D9"),
        "losses={:#?}",
        result.report().losses
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == IgesLossCode::BoundaryPcurveOutsideSupportDomain.kind() }));
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id.as_str() == "iges:model:coedge#D9:0:0")
        .expect("trimmed boundary coedge");
    assert_eq!(coedge.pcurves.len(), 1);
}

#[test]
fn source_control_interval_fallback_refuses_unadmitted_storage() {
    let bytes = subrange_nurbs_surface_boundary_file_with_source_precision_outside_nominal();
    let result = IgesCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default()).unwrap();
    assert!(result.ir().model.faces.iter().any(|face| face.id.as_str() == "iges:model:face#D9"));
    for operation in [
        "iges source active curve nodes",
        "iges Type126 declared control intervals",
    ] {
        assert_trimming_collection_refusal(&bytes, operation);
    }
    assert_trimming_retained_refusal(&bytes, "iges source active curve ID");
}

#[test]
fn pcurve_support_check_refuses_unadmitted_span_and_split_storage() {
    let bytes = subrange_nurbs_surface_boundary_file_with_source_precision_outside_nominal();
    for operation in [
        "iges pcurve homogeneous controls",
        "iges pcurve knot copy",
        "iges pcurve span descriptors",
        "iges pcurve span controls",
        "iges pcurve split levels",
        "iges pcurve split first controls",
        "iges pcurve split level controls",
        "iges pcurve split left controls",
        "iges pcurve split right controls",
    ] {
        assert_trimming_collection_refusal(&bytes, operation);
    }
}

#[test]
fn pcurve_internal_knot_insertion_refuses_before_storage() {
    let controls = [[1.0, 0.0, 0.0, 0.0]; 4];
    let knots = [0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0];
    for operation in [
        "iges pcurve internal knots",
        "iges pcurve inserted controls",
        "iges pcurve inserted knots",
    ] {
        let mut cap = 0_u64;
        let mut reached = false;
        for _ in 0..128 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            match homogeneous_pcurve_spans(2, &knots, controls.to_vec(), &ctx) {
                Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    if limit.operation == operation {
                        reached = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                _ => panic!("expected pcurve knot refusal at {operation}"),
            }
        }
        assert!(reached, "pcurve knot refusal was not reached: {operation}");
    }
}

#[test]
fn boundary_vertex_clustering_rejects_non_transitive_tolerance_neighborhoods() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(0.75, 0.0, 0.0),
        Point3::new(1.5, 0.0, 0.0),
    ];

    assert!(matches!(
        cluster_boundary_positions(
            &points.map(|point| cadmpeg_ir::features::FinitePoint3::new(point).unwrap()),
            cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
            &ctx,
        ),
        Err(BoundaryVertexCreationError::Cluster(BoundaryVertexClusterError::NonTransitive))
    ));
}

#[test]
fn boundary_vertex_clustering_uses_canonical_representatives() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let points = [
        Point3::new(10.25, 0.0, 0.0),
        Point3::new(0.5, 0.0, 0.0),
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(0.0, 0.0, 0.0),
    ];
    let clusters = cluster_boundary_positions(
        &points.map(|point| cadmpeg_ir::features::FinitePoint3::new(point).unwrap()),
        cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
        &ctx,
    )
    .unwrap();

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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[], &arena, &cadmpeg_core::decode::DecodePolicy::service(),
    ).unwrap();
    let mut candidate = ModelDraft::new();
    let source_endpoints = vec![
        BoundaryVertexSourceEndpoint {
            edge: "iges:model:edge#source-a".into(),
            endpoint: BoundaryEndpoint::Start,
            position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).unwrap(),
        },
        BoundaryVertexSourceEndpoint {
            edge: "iges:model:edge#source-b".into(),
            endpoint: BoundaryEndpoint::End,
            position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
        },
    ];

    let (vertex_ids, derivations) = create_boundary_vertices(
        &mut candidate,
        &crate::ids::Stem::directory(9_u32),
        "iges:entity:directory#9",
        0,
        &source_endpoints,
        cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
        &mut crate::entities::geometry::SourceSequences::default(),
        &ctx,
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
fn boundary_vertex_creation_refuses_each_collection_before_growth() {
    let source_endpoints = [
        BoundaryVertexSourceEndpoint {
            edge: "iges:model:edge#source-a".into(),
            endpoint: BoundaryEndpoint::Start,
            position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
        },
        BoundaryVertexSourceEndpoint {
            edge: "iges:model:edge#source-b".into(),
            endpoint: BoundaryEndpoint::End,
            position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.25, 0.0, 0.0)).unwrap(),
        },
    ];
    for operation in [
        "iges boundary endpoint positions",
        "iges boundary cluster parents",
        "iges boundary cluster roots",
        "iges boundary cluster members",
        "iges boundary cluster slots",
        "iges boundary endpoint vertex slots",
        "iges boundary vertex derivations",
        "iges boundary points",
        "iges boundary vertices",
        "iges boundary derivation endpoints",
        "iges boundary result vertex ids",
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..128 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = create_boundary_vertices(
                &mut ModelDraft::new(),
                &crate::ids::Stem::directory(9_u32),
                "iges:entity:directory#9",
                0,
                &source_endpoints,
                cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
                &mut crate::entities::geometry::SourceSequences::default(),
                &ctx,
            );
            match result {
                Err(BoundaryVertexCreationError::Resource(cadmpeg_core::CodecError::ResourceLimit(limit))) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    if limit.operation == operation {
                        found = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                other => panic!("expected boundary collection refusal at {operation}: {other:?}"),
            }
        }
        assert!(found, "boundary collection refusal was not reached: {operation}");
    }
}

#[test]
fn boundary_vertex_clustering_refuses_pairwise_work_before_comparisons() {
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(0.25, 0.0, 0.0),
        Point3::new(0.5, 0.0, 0.0),
    ].map(|point| cadmpeg_ir::features::FinitePoint3::new(point).unwrap());
    for (cap, operation, used) in [
        (2, "iges boundary clustering comparisons", 0),
        (5, "iges boundary cluster transitivity comparisons", 3),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = cluster_boundary_positions(
            &points,
            cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
            &ctx,
        );
        assert!(matches!(result,
            Err(BoundaryVertexCreationError::Resource(cadmpeg_core::CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == operation
                    && limit.used == used
                    && limit.additional == 3
        ));
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(cluster_boundary_positions(
        &points,
        cadmpeg_ir::scalar::PositiveReal::new(1.0).unwrap(),
        &ctx,
    ).unwrap().len(), 1);
}

#[test]
fn face_tolerance_policy_separates_declared_and_coordinate_bounds() {
    let global = crate::test_support::parse_global(
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
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let curve_id = CurveId::mint("test:model:curve#curve").expect("identity grammar");
    let surface_id = SurfaceId::mint("test:model:surface#surface").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
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
    let candidates = vec![
        Edge {
            id: EdgeId::mint("test:model:edge#wrong-occurrence").expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curve_id.clone()),
                Some([1.0, 2.0]),
            )
            .unwrap(),
            start: VertexId::mint("test:model:vertex#wrong-start").expect("identity grammar"),
            end: VertexId::mint("test:model:vertex#wrong-end").expect("identity grammar"),
            tolerance: None,
        },
        Edge {
            id: EdgeId::mint("test:model:edge#matching-occurrence").expect("identity grammar"),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve_id), Some([0.0, 2.0]))
                .unwrap(),
            start: VertexId::mint("test:model:vertex#matching-start").expect("identity grammar"),
            end: VertexId::mint("test:model:vertex#matching-end").expect("identity grammar"),
            tolerance: None,
        },
    ];
    ir.model.points.extend([
        Point::new(
            PointId::mint("test:model:point#wrong-point-start").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(10.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
        Point::new(
            PointId::mint("test:model:point#wrong-point-end").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(11.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
        Point::new(
            PointId::mint("test:model:point#matching-point-start").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
        Point::new(
            PointId::mint("test:model:point#matching-point-end").expect("identity grammar"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(2.0, 0.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ),
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: VertexId::mint("test:model:vertex#wrong-start").expect("identity grammar"),
            point: PointId::mint("test:model:point#wrong-point-start").expect("identity grammar"),
            tolerance: None,
        },
        Vertex {
            id: VertexId::mint("test:model:vertex#wrong-end").expect("identity grammar"),
            point: PointId::mint("test:model:point#wrong-point-end").expect("identity grammar"),
            tolerance: None,
        },
        Vertex {
            id: VertexId::mint("test:model:vertex#matching-start").expect("identity grammar"),
            point: PointId::mint("test:model:point#matching-point-start")
                .expect("identity grammar"),
            tolerance: None,
        },
        Vertex {
            id: VertexId::mint("test:model:vertex#matching-end").expect("identity grammar"),
            point: PointId::mint("test:model:point#matching-point-end").expect("identity grammar"),
            tolerance: None,
        },
    ]);

    let pcurves = vec![(
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, 0.0),
                Point2::new(2.0, 0.0),
            )
            .unwrap(),
        ),
        [0.0, 1.0],
    )];
    let index = cadmpeg_ir::index::ModelIndex::new(&ir);
    assert!(!super::edge_range_matches_curve(
        &candidates[0],
        &index,
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(11.0, 0.0, 0.0),
        EPS_BOUNDARY_ENDPOINT_MATCH,
    )
    .unwrap());
    assert!(super::edge_range_matches_curve(
        &candidates[1],
        &index,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        EPS_BOUNDARY_ENDPOINT_MATCH,
    )
    .unwrap());
    let (selected, start, end, pcurves_agree) = super::select_boundary_edge(
        &candidates,
        &index,
        super::BoundaryMatch {
            surface_id: &surface_id,
            pcurves: &pcurves,
            sense: Sense::Forward,
            tolerance: EPS_BOUNDARY_ENDPOINT_MATCH,
            parameter_curves_authoritative: true,
        },
        &ctx,
    )
    .expect("unique pcurve-compatible edge");
    assert_eq!(selected.id.as_str(), "test:model:edge#matching-occurrence");
    assert_eq!(start, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(end, Point3::new(2.0, 0.0, 0.0));
    assert!(pcurves_agree);

    let mut ambiguous_candidates = candidates.clone();
    ambiguous_candidates.push(Edge {
        id: EdgeId::mint("test:model:edge#duplicate-occurrence").expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(
            Some(CurveId::mint("test:model:curve#curve").expect("identity grammar")),
            Some([0.0, 2.0]),
        )
        .unwrap(),
        start: VertexId::mint("test:model:vertex#matching-start").expect("identity grammar"),
        end: VertexId::mint("test:model:vertex#matching-end").expect("identity grammar"),
        tolerance: None,
    });
    assert!(matches!(
        super::select_boundary_edge(
            &ambiguous_candidates,
            &index,
            super::BoundaryMatch {
                surface_id: &surface_id,
                pcurves: &[],
                sense: Sense::Forward,
                tolerance: EPS_BOUNDARY_ENDPOINT_MATCH,
                parameter_curves_authoritative: false,
            },
            &ctx,
        ),
        Err(super::BoundaryEdgeSelectionError::Ambiguous)
    ));
}

#[test]
fn trimmed_pcurve_mapping_refuses_before_an_absent_surface_candidate() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let ir = CadIr::empty();
    let index = cadmpeg_ir::index::ModelIndex::new(&ir);
    let surface_id = SurfaceId::mint("test:model:surface#absent").expect("identity grammar");
    let pcurves = [(
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
            )
            .unwrap(),
        ),
        [0.0, 1.0],
    )];
    let result = super::pcurves_agree(
        &index,
        &surface_id,
        &pcurves,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        EPS_BOUNDARY_ENDPOINT_MATCH,
        &ctx,
    );
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 1
                && limit.operation == "iges trimmed mapped pcurves"
    ));
}

#[test]
fn pcurve_geometry_refuses_mapped_polynomial_and_rational_pole_storage() {
    let polynomial = trimmed_procedural_line_surface_of_revolution_file();
    assert_trimming_collection_refusal(&polynomial, "iges pcurve mapped polynomial poles");
    let rational = subrange_nurbs_surface_boundary_file_with_pcurve(
        3,
        "126,2,2,1,1,0,0,0,0,0,1,1,1,1,0.5,1,0.2,0.2,0,0.1,0.5,0,0.2,0.2,0,0,1,0,0,1;",
    );
    assert_trimming_collection_refusal(&rational, "iges pcurve mapped rational poles");
    IgesCodec.decode(&mut Cursor::new(polynomial), &DecodeOptions::default()).unwrap();
    IgesCodec.decode(&mut Cursor::new(rational), &DecodeOptions::default()).unwrap();
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
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
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
        .find(|face| face.id.as_str() == "iges:model:face#D15")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    assert_eq!(face.loops.len(), 2);
    let roles = face
        .loops
        .iter()
        .map(|id| face.loop_role(id))
        .collect::<Vec<_>>();
    assert_eq!(
        roles,
        vec![
            cadmpeg_ir::topology::LoopBoundaryRole::Outer,
            cadmpeg_ir::topology::LoopBoundaryRole::Inner,
        ]
    );
    // The PTO field names the outer boundary, so the face states it in the
    // `outer` key and the remaining loops are inner.
    let cadmpeg_ir::topology::FaceLoops::Classified { outer, inner } = &face.loops else {
        panic!("an explicit outer boundary states a classified face");
    };
    assert_eq!(Some(outer), face.loops.iter().next());
    assert_eq!(inner.len(), 1);
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
            .find(|face| face.id.as_str() == "iges:model:face#D3")
            .unwrap_or_else(|| {
                panic!(
                    "parameters={parameters} losses={:#?}",
                    result.report().losses
                )
            });
        assert!(face.loops.is_empty());
        // A parameter-domain outer boundary is no classification: the face
        // states an empty loop list, not a classified face with no outer.
        assert!(matches!(
            face.loops,
            cadmpeg_ir::topology::FaceLoops::Unspecified { .. }
        ));
        assert!(
            result.report().losses.is_empty(),
            "parameters={parameters} losses={:#?}",
            result.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
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
        .find(|face| face.id.as_str() == "iges:model:face#D15")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    assert_eq!(face.loops.len(), 1);
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    // The face states no outer boundary, so it states no classification: the
    // parameter domain supplies the exterior, and the procedural surface
    // below is where that fact lives.
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
    assert!(matches!(
        face.loops,
        cadmpeg_ir::topology::FaceLoops::Unspecified { .. }
    ));
    assert_eq!(
        face.surface.as_str(),
        "iges:model:surface#D15:implicit-outer"
    );
    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| {
            result.ir().model.procedural_surface_owner(&surface.id) == Some(&face.surface)
        })
        .unwrap();
    match procedural.definition() {
        ProceduralSurfaceDefinition::CurveBounded {
            support,
            boundaries,
            boundary_pcurves,
            implicit_outer,
        } => {
            assert_eq!(support.as_str(), "iges:model:surface#D1");
            assert_eq!(
                boundaries,
                &[CurveId::mint("iges:model:curve#D9").expect("identity grammar")]
            );
            assert_eq!(
                boundary_pcurves,
                &[PcurveId::mint("iges:model:pcurve#D15:0:0:0").expect("identity grammar")]
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
    assert!(rings
        .into_iter()
        .map(SimpleRing::new)
        .collect::<Result<Vec<_>, _>>()
        .is_err());
}

#[test]
fn linear_boundary_relationship_rejects_a_self_intersecting_outer_boundary() {
    let candidates = [Some(LinearBoundaryGeometry::Parameter(vec![
        [0.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [1.0, 0.0],
        [0.0, 0.0],
    ]))];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let rings = linear_boundary_rings(&candidates, BoundarySpace::Parameter, &ctx).unwrap().unwrap();
    let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));

    assert_eq!(
        linear_boundary_relationship_is_valid(
            rings.as_deref(),
            BoundarySurfaceKind::Trimmed,
            true,
            &plane,
            None,
            [false, false],
        ),
        Some(false)
    );
}

#[test]
fn linear_boundary_rings_refuse_outer_and_nested_point_storage() {
    let candidates = [Some(LinearBoundaryGeometry::Parameter(vec![
        [0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0],
    ]))];
    for (cap, operation) in [
        (0, "iges linear boundary ring slots"),
        (1, "iges linear boundary ring points"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = linear_boundary_rings(&candidates, BoundarySpace::Parameter, &ctx);
        assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation));
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(linear_boundary_rings(&candidates, BoundarySpace::Parameter, &ctx)
        .unwrap().unwrap().is_ok());
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
        .any(|face| face.id.as_str() == "iges:model:face#D15"));
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
        .any(|face| face.id.as_str() == "iges:model:face#D15"));
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
        .any(|face| face.id.as_str() == "iges:model:face#D9"));
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
                .any(|face| face.id.as_str() == "iges:model:face#D15"),
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
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
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
        .find(|face| face.id.as_str() == "iges:model:face#D15")
        .expect("model-preferred trimmed face");
    let outer_loop = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .expect("outer loop");
    assert!(outer_loop.coedges().iter().all(|id| result
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
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
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
        .find(|coedge| coedge.id.as_str() == "iges:model:coedge#D11:0:0")
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
                cadmpeg_ir::eval::pcurve_uv(&pcurve.geometry, 0.0)
                    .expect("start evaluates")
                    .get(),
                cadmpeg_ir::eval::pcurve_uv(&pcurve.geometry, 1.0)
                    .expect("end evaluates")
                    .get(),
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
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
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
        .find(|coedge| coedge.id.as_str() == "iges:model:coedge#D11:0:0")
        .expect("model-preferred boundary coedge");
    assert_eq!(coedge.pcurves.len(), 2);
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
                .any(|body| body.id.as_str() == "iges:model:body#D11"),
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
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
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
        .find(|loop_| loop_.id.as_str() == "iges:model:loop#D21:D17")
        .unwrap();
    assert_eq!(loop_.coedges().len(), 2);
    let coedges = loop_
        .coedges()
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
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
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
        .find(|coedge| coedge.id.as_str() == "iges:model:coedge#D27:D23:0")
        .unwrap();
    assert_eq!(coedge.pcurves.len(), 2);
    assert_eq!(coedge.pcurves[0].isoparametric, Some(true));
    assert_eq!(coedge.pcurves[1].isoparametric, Some(false));
    assert!(coedge.pcurves[0].pcurve.as_str().ends_with(":0:0"));
    assert!(coedge.pcurves[1].pcurve.as_str().ends_with(":0:1"));
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id.as_str() == "iges:model:loop#D27:D23")
        .unwrap();
    let [vertex_use] = loop_.anchored_vertex_uses() else {
        panic!("edge loop retains one anchored vertex use");
    };
    assert_eq!(vertex_use.vertex.as_str(), "iges:model:vertex#D27:D15:2");
    assert_eq!(vertex_use.after, coedge.id);
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
                .any(|body| body.id.as_str() == "iges:model:body#D27"),
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
        let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
            .expect("resource allocation did not fail");
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
        .find(|face| face.id.as_str() == "iges:model:face#D9")
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges()[0])
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
    assert_eq!(coedge.pcurves.len(), 1);
    assert_eq!(
        coedge.pcurves[0].pcurve.as_str(),
        "iges:model:pcurve#D9:0:0:0"
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
        .find(|face| face.id.as_str() == "iges:model:face#D13")
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    assert_eq!(loop_.coedges().len(), 4);
    let senses = loop_
        .coedges()
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
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
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
        .find(|face| face.id.as_str() == "iges:model:face#D13")
        .expect("bounded face within the declared resolution");
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .expect("bounded loop");
    assert_eq!(loop_.coedges().len(), 4);
    assert_eq!(
        face.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.001)
    );
    assert!(result
        .ir()
        .model
        .vertices
        .iter()
        .any(|vertex| vertex.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get) == Some(0.001)));
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .any(|edge| edge.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get) == Some(0.001)));
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
        .all(|face| face.id.as_str() != "iges:model:face#D13"));
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
        .find(|face| face.id.as_str() == "iges:model:face#D13")
        .expect("bounded face within the unit-converted resolution");
    assert_eq!(
        face.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.01)
    );
    assert!(result
        .ir()
        .model
        .vertices
        .iter()
        .any(|vertex| vertex.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get) == Some(0.01)));
    assert!(result
        .ir()
        .model
        .edges
        .iter()
        .any(|edge| edge.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get) == Some(0.01)));
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
        .find(|face| face.id.as_str() == "iges:model:face#D13")
        .expect("bounded face within one declared coordinate quantum");
    assert_eq!(
        face.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.01)
    );
    assert!(result
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.fit_tolerance().is_none()));
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
        .find(|body| body.id.as_str() == "iges:model:body#D9")
        .unwrap();
    assert_eq!(sheet.kind, cadmpeg_ir::topology::BodyKind::Sheet);
    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D9")
        .unwrap();
    assert_eq!(face.surface.as_str(), "iges:model:surface#D1");
    assert_eq!(face.loops.len(), 1);
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
    assert_eq!(loop_.coedges().len(), 1);
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges()[0])
        .unwrap();
    assert_eq!(coedge.radial_next, coedge.id);
    assert_eq!(coedge.pcurves.len(), 1);
    assert_eq!(
        coedge.pcurves[0].pcurve.as_str(),
        "iges:model:pcurve#D9:0:0:0"
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
        .find(|face| face.id.as_str() == "iges:model:face#D9")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges()[0])
        .unwrap();
    assert_eq!(coedge.pcurves.len(), 1);
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
fn decode_maps_a_line_generatrix_pcurve_to_the_neutral_distance_parameter() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_procedural_line_surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D13")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
        .unwrap();
    let SurfaceGeometry::Procedural { construction, .. } = &surface.geometry else {
        panic!("expected a procedural revolution surface");
    };
    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.id == *construction)
        .unwrap();
    let ProceduralSurfaceDefinition::Revolution(definition_payload_0) = procedural.definition()
    else {
        panic!("expected a bounded procedural revolution");
    };
    let Some(parameter_interval) = &definition_payload_0
        .parameter_interval()
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        panic!("expected a bounded procedural revolution");
    };
    assert_eq!(*parameter_interval, [0.0, 1.0]);
    let carrier_interval = procedural.record_bounds().unwrap().get();
    assert!(carrier_interval[1].is_some_and(|value| value > 3.0));

    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges()[0])
        .unwrap();
    assert_eq!(coedge.pcurves.len(), 1);
    let pcurve = result
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id == coedge.pcurves[0].pcurve)
        .unwrap();
    let PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry else {
        panic!("expected a NURBS pcurve, got {:?}", pcurve.geometry);
    };
    let expected_u =
        (11.762_109_22_f64 - 6.814_348_186).hypot(-6.969_522_429_f64 - -2.592_356_749_f64) * 0.5;
    assert!((nurbs.control_points()[0].u - expected_u).abs() <= EPS_BOUNDARY_ENDPOINT_MATCH);
    assert!(nurbs.control_points()[0].v.abs() <= EPS_BOUNDARY_ENDPOINT_MATCH);
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
fn decode_unscales_procedural_pcurve_coordinates_before_neutral_mapping() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_procedural_line_surface_of_revolution_file_with_global(
                b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,64,38,6,308,15,0H,1.0,1,4HINCH,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    let face = result
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id.as_str() == "iges:model:face#D13")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges()[0])
        .unwrap();
    let pcurve = result
        .ir()
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id == coedge.pcurves[0].pcurve)
        .unwrap();
    let PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry else {
        panic!("expected a NURBS pcurve, got {:?}", pcurve.geometry);
    };
    let expected_u = (11.762_109_22_f64 - 6.814_348_186)
        .hypot(-6.969_522_429_f64 - -2.592_356_749_f64)
        * 0.5
        * 25.4;
    assert!((nurbs.control_points()[0].u - expected_u).abs() <= EPS_BOUNDARY_ENDPOINT_MATCH);
    assert!(nurbs.control_points()[0].v.abs() <= EPS_BOUNDARY_ENDPOINT_MATCH);
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
fn procedural_parameter_conversion_preserves_declared_endpoints() {
    let upper_u = 0.898_025_612_106_907_5;
    let mapped = super::source_parameter_point_to_neutral(
        Point2::new(25.4, 2.0 * 25.4),
        (upper_u, 0.0, 1.0, 0.0),
        25.4,
    );

    assert_eq!(mapped.u, upper_u);
    assert_eq!(mapped.v, 2.0);
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
        .find(|face| face.id.as_str() == "iges:model:face#D9")
        .unwrap();
    let loop_ = result
        .ir()
        .model
        .loops
        .iter()
        .find(|loop_| Some(&loop_.id) == face.loops.iter().next())
        .unwrap();
    let coedge = result
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id == loop_.coedges()[0])
        .unwrap();
    assert!(coedge.pcurves.is_empty());
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}
