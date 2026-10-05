// SPDX-License-Identifier: Apache-2.0

use crate::decode::analytic::equations::{CylinderEquation, PlaneEquation};
use crate::decode::analytic::planes::{
    fc05_cylinder_branch_witnesses, fc05_cylinder_model_witness,
    native_positional_cylinder_carriers, plane_candidate_pcurve_lies_on_carrier, plane_candidates,
    round_edge_envelopes_for_plane, select_stored_frame_branches,
    select_stored_frame_carrier_pcurve_branches, stored_parameter_normal_candidates,
    unique_round_edge_origin_candidate, PlaneCandidate, PlaneChart,
};
use crate::surface::{LocalSystemClassification, OutlinePlane, PlaneLocalSystem};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn fc05_witness_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 1,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 4,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code01,
            next_surface: 0,
            offset: 1,
        },
        crate::surface::SurfaceRow {
            id: 2,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 4,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code01,
            next_surface: 0,
            offset: 2,
        },
    ]);
    scan.curves.fc05_circles.push(crate::curve::Fc05Circle {
        curve_id: 7,
        center_row_frame: [0.0, 0.0],
        radius_mm: 1.0,
        sample_direction_row_frame: cadmpeg_ir::units::HypotDirection2::normalized_with_length([
            1.0, 0.0,
        ])
        .expect("unit sample direction")
        .0,
        angle_parameter: crate::curve::Fc05AngleParameterRelation::Consistent {
            sense: crate::curve::ParameterSense::Increasing,
            reference_direction_row_frame: [1.0, 0.0],
        },
        cap_ordinate_row_frame: Some(0.0),
        point_count: 8,
        max_residual: 0.0,
        offset: 7,
    });
    scan.references.circles.push(
        crate::reference::ReferenceCircle::try_new(
            7,
            crate::reference::ReferenceCircleCenter::Stored(
                cadmpeg_ir::features::FinitePoint3::new([1.0, 0.0, 0.5].into())
                    .expect("finite center"),
            ),
            cadmpeg_ir::scalar::PositiveLength::new(1.0).expect("positive radius"),
            cadmpeg_ir::units::UnitVector3::Y_AXIS,
            [
                cadmpeg_ir::features::FinitePoint3::new([2.0, 0.0, 0.5].into())
                    .expect("finite start"),
                cadmpeg_ir::features::FinitePoint3::new([1.0, 0.0, 1.5].into())
                    .expect("finite end"),
            ],
            8,
        )
        .expect("checked reference geometry"),
    );
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 7,
            type_byte: 5,
            feature_id: 4,
            directions: [0; 2],
            faces: [std::num::NonZeroU32::new(2), std::num::NonZeroU32::new(1)],
            next_edges: [0; 2],
            offset: 9,
        });
    scan.planes.local_systems.push(PlaneLocalSystem {
        surface_id: 1,
        body: Vec::new(),
        slots: [
            Some(0.8),
            Some(0.0),
            Some(-0.6),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.6),
            Some(0.0),
            Some(0.8),
            Some(0.0),
            Some(0.0),
            Some(0.0),
        ],
        layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
        classification: LocalSystemClassification::Unclassified,
        row_offset: 10,
        offset: 11,
    });

    scan
}

fn fc05_witness_limit_error(limit: u64) -> CodecError {
    let scan = fc05_witness_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    fc05_cylinder_model_witness(
        &ctx,
        &scan,
        2,
        CylinderEquation {
            origin: [0.0, 0.0, 0.0],
            axis: [0.0, 1.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 1.0,
        },
    )
    .err()
    .expect("FC05 witness exceeds collection limit")
}

#[test]
fn fc05_witness_curve_id_node_refuses_collection_limit() {
    let error = fc05_witness_limit_error(0);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo FC05 witness curve ID nodes"));
}

#[test]
fn fc05_witness_circle_vector_refuses_collection_limit() {
    let error = fc05_witness_limit_error(1);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo FC05 witness circles"));
}

#[test]
fn fc05_tangent_plane_id_node_refuses_collection_limit() {
    let error = fc05_witness_limit_error(2);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo FC05 tangent plane ID nodes"));
}

fn fc05_branch_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = fc05_witness_scan();
    scan.planes.outlines.push(OutlinePlane {
        surface_id: 1,
        origin: [0.0, 0.0, 0.0],
        normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
        offset: 10,
    });
    scan
}

fn fc05_branch_limit_error(limit: u64) -> CodecError {
    let scan = fc05_branch_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    fc05_cylinder_branch_witnesses(&ctx, &scan)
        .err()
        .expect("FC05 branch witnesses exceed collection limit")
}

#[test]
fn fc05_cylinder_frame_node_refuses_collection_limit() {
    let error = fc05_branch_limit_error(4);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo FC05 cylinder frame nodes"));
}

#[test]
fn fc05_witness_plane_node_refuses_collection_limit() {
    let error = fc05_branch_limit_error(5);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo FC05 witness plane nodes"));
}

#[test]
fn fc05_cylinder_witness_vector_refuses_collection_limit() {
    let error = fc05_branch_limit_error(6);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo FC05 cylinder witnesses"));
}

#[test]
fn fc05_branch_witnesses_keep_plane_and_cylinder_identity() {
    let scan = fc05_branch_scan();
    let witnesses =
        crate::decode::with_test_decode_ctx(|ctx| fc05_cylinder_branch_witnesses(ctx, &scan))
            .expect("service FC05 branch witnesses admitted");
    assert_eq!(witnesses.len(), 1);
    assert_eq!(witnesses.get(&1).map(Vec::len), Some(1));
    assert_eq!(witnesses[&1][0].radius, 1.0);
}

fn fc05_branch_selection_limit_error(limit: u64) -> CodecError {
    let scan = fc05_branch_scan();
    let mut candidates = std::collections::BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    select_stored_frame_branches(&ctx, &scan, &mut candidates)
        .expect_err("FC05 branch selection exceeds collection limit")
}

#[test]
fn plane_origin_domain_node_refuses_collection_limit() {
    let error = fc05_branch_selection_limit_error(7);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo plane origin domain nodes"));
}

#[test]
fn plane_origin_domain_candidates_refuse_collection_limit() {
    let error = fc05_branch_selection_limit_error(8);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo plane origin domain candidates"));
}

#[test]
fn fc05_origin_plane_branch_refuses_collection_limit() {
    let error = fc05_branch_selection_limit_error(13);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo FC05 origin plane branch"));
}

#[test]
fn fc05_tangent_plane_branch_refuses_collection_limit() {
    let error = fc05_branch_selection_limit_error(20);
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo FC05 tangent plane branch"),
        "{error:?}"
    );
}

#[test]
fn fc05_model_witness_uses_a_unique_reference_when_tangency_improves() {
    let scan = fc05_witness_scan();
    let witness = crate::decode::with_test_decode_ctx(|ctx| {
        fc05_cylinder_model_witness(
            ctx,
            &scan,
            2,
            CylinderEquation {
                origin: [0.0, 0.0, 0.0],
                axis: [0.0, 1.0, 0.0],
                ref_direction: [1.0, 0.0, 0.0],
                radius: 1.0,
            },
        )
    })
    .expect("service FC05 witness admitted");

    assert_eq!(witness.origin, [1.0, 0.0, 0.5]);
    assert_eq!(witness.axis, [0.0, 1.0, 0.0]);
    assert_eq!(witness.radius, 1.0);
}

fn stored_frame_branch_scan(with_pcurve: bool) -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    for id in [1, 2] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 4,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code01,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    scan.planes.local_systems.extend([
        crate::surface::PlaneLocalSystem {
            surface_id: 1,
            body: Vec::new(),
            slots: [0.6, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0, -0.6, 0.0, 0.0, 0.0].map(Some),
            layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
            classification: LocalSystemClassification::Unclassified,
            row_offset: 1,
            offset: 10,
        },
        crate::surface::PlaneLocalSystem {
            surface_id: 2,
            body: Vec::new(),
            slots: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0].map(Some),
            layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
            classification: LocalSystemClassification::Simple,
            row_offset: 2,
            offset: 20,
        },
    ]);
    if with_pcurve {
        scan.curves.pcurves.push(crate::curve::PcurveEndpoints {
            curve_id: 7,
            faces: [1, 2].map(std::num::NonZeroU32::new),
            face_0_endpoints: [[1.0, 1.0], [2.0, 1.0]],
            face_1_endpoints: [[0.6, 0.8], [1.2, 1.6]],
            offset: 30,
        });
    }
    scan
}

fn stored_branch_limit_error(limit: u64, with_pcurve: bool) -> CodecError {
    let scan = stored_frame_branch_scan(with_pcurve);
    let mut candidates = std::collections::BTreeMap::new();
    if with_pcurve {
        let frame = scan.planes.local_systems[1].frame();
        let origin = frame.origin.expect("complete fixed frame origin");
        let normal = frame.normal().expect("complete fixed frame normal");
        let u_axis = frame.u_axis().expect("complete fixed frame U axis");
        candidates.insert(
            2,
            vec![PlaneCandidate {
                equation: PlaneEquation { origin, normal },
                chart: Some(PlaneChart {
                    origin,
                    normal,
                    u_axis,
                }),
                offset: 20,
            }],
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    select_stored_frame_branches(&ctx, &scan, &mut candidates)
        .expect_err("stored branch selection exceeds collection limit")
}

#[test]
fn plane_variable_domain_node_refuses_collection_limit() {
    let error = stored_branch_limit_error(0, false);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo plane variable domain nodes"));
}

#[test]
fn plane_variable_domain_candidate_refuses_collection_limit() {
    let error = stored_branch_limit_error(1, false);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo plane variable domain candidates"));
}

#[test]
fn copied_plane_domain_candidates_refuse_collection_limit() {
    let error = stored_branch_limit_error(3, false);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo copied plane domain candidates"));
}

#[test]
fn copied_plane_domain_node_refuses_collection_limit() {
    let error = stored_branch_limit_error(5, false);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo copied plane domain nodes"));
}

#[test]
fn plane_branch_surface_count_node_refuses_collection_limit() {
    let error = stored_branch_limit_error(6, false);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo unique-row count nodes"));
}

#[test]
fn plane_branch_surface_projection_refuses_collection_limit() {
    let error = stored_branch_limit_error(8, false);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo unique-row projection"));
}

#[test]
fn fixed_plane_domain_candidate_refuses_collection_limit() {
    let error = stored_branch_limit_error(10, true);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo fixed plane domain candidates"));
}

#[test]
fn fixed_plane_domain_node_refuses_collection_limit() {
    let error = stored_branch_limit_error(11, true);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo fixed plane domain nodes"));
}

#[test]
fn plane_branch_constraint_refuses_collection_limit() {
    let error = stored_branch_limit_error(12, true);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo plane branch constraints"));
}

#[test]
fn filtered_first_plane_candidate_refuses_collection_limit() {
    let error = stored_branch_limit_error(13, true);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo filtered first plane candidates"));
}

#[test]
fn selected_plane_branch_refuses_collection_limit() {
    let error = stored_branch_limit_error(15, true);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo selected plane branch"));
}

#[test]
fn selected_plane_branch_node_refuses_collection_limit() {
    let error = stored_branch_limit_error(16, true);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo selected plane branch nodes"));
}

#[test]
fn plane_branch_constraint_work_refuses_work_limit() {
    let scan = stored_frame_branch_scan(true);
    // The work boundary includes the complete constraint-source traversal admission.
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo plane branch constraints",
        |ctx| plane_candidates(ctx, &scan),
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo plane branch constraints"));
}

#[test]
fn plane_branch_propagation_round_refuses_work_limit() {
    let scan = stored_frame_branch_scan(true);
    let candidates = crate::test_support::assert_work_boundaries(
        &["creo plane branch propagation rounds"],
        |ctx| plane_candidates(ctx, &scan),
    );
    assert!(candidates.contains_key(&1));
}

fn carrier_pcurve_branch_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = stored_frame_branch_scan(false);
    scan.surfaces
        .rows
        .edit(|rows| rows[1].kind = crate::surface::SurfaceKind::Cylinder);
    let frame = crate::surface::PositionalCylinderFrame::new(
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        1.0,
        None,
    )
    .expect("valid cylinder frame");
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 2,
            body: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            carrier: crate::surface::SurfaceParameterCarrier::Resolved(
                crate::surface::InlineSurfaceCarrier::Cylinder {
                    frame,
                    split_bounds: None,
                },
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 2,
            body_offset: 2,
        });
    scan.curves.pcurves.push(crate::curve::PcurveEndpoints {
        curve_id: 7,
        faces: [1, 2].map(std::num::NonZeroU32::new),
        face_0_endpoints: [[1.0, 0.0], [0.0, 1.0]],
        face_1_endpoints: [[0.0, 0.0], [0.0, 1.0]],
        offset: 7,
    });
    scan
}

fn carrier_branch_domains() -> std::collections::BTreeMap<u32, Vec<PlaneCandidate>> {
    let candidate = |origin| PlaneCandidate {
        equation: PlaneEquation {
            origin,
            normal: [0.0, 1.0, 0.0],
        },
        chart: Some(PlaneChart {
            origin,
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
        }),
        offset: 1,
    };
    std::collections::BTreeMap::from([(
        1,
        vec![candidate([0.0, 0.0, 0.0]), candidate([10.0, 0.0, 0.0])],
    )])
}

#[test]
fn plane_branch_cylinder_carrier_node_refuses_collection_limit() {
    let scan = carrier_pcurve_branch_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = native_positional_cylinder_carriers(&ctx, &scan)
        .err()
        .expect("cylinder carrier node exceeds collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo plane branch cylinder carrier nodes"));
}

#[test]
fn carrier_pcurve_plane_branch_refuses_collection_limit() {
    let scan = carrier_pcurve_branch_scan();
    let domains = carrier_branch_domains();
    let mut selected = domains.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 5;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = select_stored_frame_carrier_pcurve_branches(&ctx, &scan, &domains, &mut selected)
        .expect_err("carrier pcurve branch exceeds collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo carrier pcurve plane branch"));
}

#[test]
fn carrier_pcurve_branch_selects_incident_cylinder() {
    let scan = carrier_pcurve_branch_scan();
    let domains = carrier_branch_domains();
    let mut selected = domains.clone();
    crate::decode::with_test_decode_ctx(|ctx| {
        select_stored_frame_carrier_pcurve_branches(ctx, &scan, &domains, &mut selected)
    })
    .expect("service carrier branch admitted");
    assert_eq!(selected[&1].len(), 1);
    assert_eq!(selected[&1][0].equation.origin, [0.0, 0.0, 0.0]);
}

fn two_variable_plane_branch_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = stored_frame_branch_scan(true);
    scan.planes.local_systems[1].slots =
        [0.0, 0.6, 0.8, 0.0, 0.0, 0.0, 0.0, 0.8, -0.6, 0.0, 0.0, 0.0].map(Some);
    scan.planes.local_systems[1].classification = LocalSystemClassification::Unclassified;
    scan.curves.pcurves[0].face_0_endpoints = [[0.0, 0.0], [0.8, -0.48]];
    scan.curves.pcurves[0].face_1_endpoints = [[0.0, 0.0], [0.8, 0.48]];
    scan
}

#[test]
fn two_variable_plane_branches_keep_mirror_ambiguity() {
    let scan = two_variable_plane_branch_scan();
    let mut candidates = std::collections::BTreeMap::new();
    crate::decode::with_test_decode_ctx(|ctx| {
        select_stored_frame_branches(ctx, &scan, &mut candidates)
    })
    .expect("service two-plane branches admitted");
    assert!(candidates.is_empty());
}

#[test]
fn filtered_second_plane_candidate_refuses_collection_limit() {
    let scan = two_variable_plane_branch_scan();
    let mut candidates = std::collections::BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 19;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = select_stored_frame_branches(&ctx, &scan, &mut candidates)
        .expect_err("second plane filter exceeds collection limit");
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo filtered second plane candidates"),
        "{error:?}"
    );
}

#[test]
fn stored_parameter_normal_frame_exposes_both_mirror_branches() {
    let scan = stored_frame_branch_scan(false);
    let frame = &scan.planes.local_systems[0];
    let (candidates, count) =
        crate::decode::with_test_decode_ctx(|ctx| stored_parameter_normal_candidates(ctx, frame))
            .expect("service stored plane branch scan admitted")
            .expect("ambiguous frame");
    assert_eq!(count, 2);
    assert!(candidates[..count].iter().any(|candidate| {
        candidate.equation.normal == [0.8, 0.0, 0.6]
            && candidate.chart.expect("chart").u_axis == [0.6, 0.0, -0.8]
    }));
    assert!(candidates[..count].iter().any(|candidate| {
        candidate.equation.normal == [0.8, 0.0, -0.6]
            && candidate.chart.expect("chart").u_axis == [0.6, 0.0, 0.8]
    }));

    let mut nonzero_origin = frame.clone();
    nonzero_origin.slots[11] = Some(2.0);
    let (candidates, count) = crate::decode::with_test_decode_ctx(|ctx| {
        stored_parameter_normal_candidates(ctx, &nonzero_origin)
    })
    .expect("service stored plane branch scan admitted")
    .expect("ambiguous frame");
    assert_eq!(count, 2);
    assert!(candidates[..count]
        .iter()
        .all(|candidate| candidate.equation.origin[2] == 2.0));

    let mut invalid = frame.clone();
    invalid.slots[4] = Some(1.0);
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        stored_parameter_normal_candidates(ctx, &invalid)
    })
    .expect("service stored plane branch scan admitted")
    .is_none());

    let mut compact = frame.clone();
    compact.classification = LocalSystemClassification::Simple;
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        stored_parameter_normal_candidates(ctx, &compact)
    })
    .expect("service stored plane branch scan admitted")
    .is_none());
}

#[test]
fn plane_pcurve_discriminates_a_feature_frame_against_an_analytic_carrier() {
    let candidate = PlaneCandidate {
        equation: PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        },
        chart: Some(PlaneChart {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
        }),
        offset: 0,
    };
    let cylinder = crate::decode::analytic::equations::CarrierEquation::Cylinder(
        crate::decode::analytic::equations::CylinderEquation {
            origin: [0.0, 0.0, 0.0],
            axis: [1.0, 0.0, 0.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 1.0,
        },
    );

    assert!(plane_candidate_pcurve_lies_on_carrier(
        candidate,
        [[0.0, 1.0], [2.0, 1.0]],
        cylinder
    ));
    assert!(!plane_candidate_pcurve_lies_on_carrier(
        candidate,
        [[0.0, 2.0], [2.0, 2.0]],
        cylinder
    ));
}

#[test]
fn stored_parameter_normal_branch_uses_unique_pcurve_endpoint_witness() {
    let scan = stored_frame_branch_scan(true);
    let candidates = crate::decode::with_test_decode_ctx(|ctx| plane_candidates(ctx, &scan))
        .expect("service plane candidates admitted");
    let candidates = candidates.get(&1).expect("selected plane");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].equation.normal, [0.8, 0.0, 0.6]);
    assert_eq!(candidates[0].chart.expect("chart").u_axis, [0.6, 0.0, -0.8]);
}

#[test]
fn stored_parameter_normal_branch_considers_every_bounded_frame_candidate() {
    let mut scan = stored_frame_branch_scan(true);
    let mut later = scan.planes.local_systems[0].clone();
    later.offset += 1;
    later.slots[9] = Some(5.0);
    later.slots[10] = Some(5.0);
    scan.planes.local_systems.push(later);

    let candidates = crate::decode::with_test_decode_ctx(|ctx| plane_candidates(ctx, &scan))
        .expect("service plane candidates admitted");
    let candidates = candidates.get(&1).expect("selected plane");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].equation.origin, [0.0, 0.0, 0.0]);
    assert_eq!(candidates[0].equation.normal, [0.8, 0.0, 0.6]);
}

#[test]
fn stored_parameter_normal_branch_uses_unique_two_chart_endpoint_witness() {
    let mut scan = stored_frame_branch_scan(false);
    scan.curves
        .two_chart_pcurves
        .push(crate::curve::TwoChartPcurveSamples {
            curve_id: 7,
            faces: [1, 2],
            samples: vec![[[1.0, 1.0], [0.6, 0.8]], [[2.0, 1.0], [1.2, 1.6]]],
            offset: 30,
        });
    let candidates = crate::decode::with_test_decode_ctx(|ctx| plane_candidates(ctx, &scan))
        .expect("service plane candidates admitted");
    let candidates = candidates.get(&1).expect("selected plane");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].equation.normal, [0.8, 0.0, 0.6]);
    assert_eq!(candidates[0].chart.expect("chart").u_axis, [0.6, 0.0, -0.8]);
}

#[test]
fn stored_parameter_normal_branch_keeps_existing_frame_without_witness() {
    let scan = stored_frame_branch_scan(false);
    let candidates = crate::decode::with_test_decode_ctx(|ctx| plane_candidates(ctx, &scan))
        .expect("service plane candidates admitted");
    let candidates = candidates.get(&1).expect("existing plane");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].equation.normal, [0.8, 0.0, -0.6]);
    assert_eq!(candidates[0].chart.expect("chart").u_axis, [0.6, 0.0, 0.8]);
}

#[test]
fn round_edge_origin_witness_selects_the_plane_with_an_incident_endpoint() {
    let positive = PlaneCandidate {
        equation: PlaneEquation {
            origin: [0.0, 5.5, 0.0],
            normal: [0.0, 1.0, 0.0],
        },
        chart: Some(PlaneChart {
            origin: [0.0, 5.5, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
        }),
        offset: 0,
    };
    let negative = PlaneCandidate {
        equation: PlaneEquation {
            origin: [0.0, -5.5, 0.0],
            normal: [0.0, 1.0, 0.0],
        },
        chart: Some(PlaneChart {
            origin: [0.0, -5.5, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
        }),
        offset: 0,
    };
    let envelope = crate::surface::Type24RoundEdgeEnvelope {
        parameter_interval: [0.0, 1.0],
        vertices: [[-30.0, -5.7, 0.0], [-29.8, -5.5, 1.0]],
        generated_entity_reference: None,
    };

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            unique_round_edge_origin_candidate(ctx, &[positive, negative], &[envelope])
        })
        .expect("service round-edge origin scan admitted")
        .expect("incident plane candidate")
        .equation
        .origin,
        [0.0, -5.5, 0.0]
    );
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        unique_round_edge_origin_candidate(ctx, &[positive, negative], &[])
    })
    .expect("service round-edge origin scan admitted")
    .is_none());
}

fn round_edge_envelope_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    for (id, kind) in [
        (1, crate::surface::SurfaceKind::Plane),
        (2, crate::surface::SurfaceKind::Cylinder),
    ] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind,
            feature_id: 4,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code01,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    scan.features
        .legacy_rounds
        .push(crate::legacy_feature::LegacyRoundFeature {
            feature_id: 4,
            radius: crate::legacy_feature::LegacyRoundRadius::NotPresent,
            edge_ids: None,
            offset: 3,
        });
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 7,
            type_byte: 5,
            feature_id: 4,
            directions: [0; 2],
            faces: [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)],
            next_edges: [0; 2],
            offset: 7,
        });
    let mut body = vec![0x34, 0xe0, 0x00];
    body.extend_from_slice(&[0x56, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x00, 0x12, 0x68]);
    body.extend_from_slice(&[0x6b, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x0f, 0xe4, 0x2f, 0x00, 0x00]);
    body.extend_from_slice(&[0x0d, 0x2f, 0x00, 0x00, 0x0f]);
    body.extend_from_slice(&[0xf7, 0x17]);
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 2,
            body,
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
                crate::surface::SurfaceKind::Cylinder,
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 2,
            body_offset: 2,
        });
    scan
}

fn round_edge_envelope_limit_error(limit: u64) -> CodecError {
    let scan = round_edge_envelope_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    round_edge_envelopes_for_plane(&ctx, &scan, 1)
        .expect_err("round-edge envelopes exceed collection limit")
}

#[test]
fn round_edge_surface_row_node_refuses_collection_limit() {
    let error = round_edge_envelope_limit_error(4);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo round-edge surface row nodes"));
}

#[test]
fn round_edge_surface_count_node_refuses_collection_limit() {
    let error = round_edge_envelope_limit_error(0);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo unique-row count nodes"));
}

#[test]
fn round_edge_surface_projection_refuses_collection_limit() {
    let error = round_edge_envelope_limit_error(2);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo unique-row projection"));
}

#[test]
fn round_edge_topology_count_node_refuses_collection_limit() {
    let error = round_edge_envelope_limit_error(6);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo unique-row count nodes"));
}

#[test]
fn round_edge_topology_projection_refuses_collection_limit() {
    let error = round_edge_envelope_limit_error(7);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo unique-row projection"));
}

#[test]
fn round_edge_plane_envelope_refuses_collection_limit() {
    let error = round_edge_envelope_limit_error(8);
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo round-edge plane envelopes"));
}

#[test]
fn round_edge_plane_envelope_preserves_type24_geometry() {
    let scan = round_edge_envelope_scan();
    let envelopes =
        crate::decode::with_test_decode_ctx(|ctx| round_edge_envelopes_for_plane(ctx, &scan, 1))
            .expect("service round-edge envelopes admitted");
    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].vertices, [[0.0, 1.0, 2.0], [-1.0, 2.0, 0.0]]);
}

const SMALL_TANGENT_SPHERE_RADIUS: f64 = 1.0e-10;

#[test]
fn numerical_seventh_sphere_tangency_and_membership_preserve_scale() {
    use super::super::{point_on_carrier, tangent_plane_sphere_point, tangent_sphere_point};
    use crate::decode::analytic::equations::{CarrierEquation, SphereEquation};
    for radius in [SMALL_TANGENT_SPHERE_RADIUS, 1.0, 1.0e200] {
        let sphere = |x| SphereEquation {
            center: [x, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius,
        };
        assert!(point_on_carrier(
            [radius, 0.0, 0.0],
            CarrierEquation::Sphere(sphere(0.0))
        ));
        assert!(!point_on_carrier(
            [1.5 * radius, 0.0, 0.0],
            CarrierEquation::Sphere(sphere(0.0))
        ));
        assert!(tangent_sphere_point(sphere(0.0), sphere(3.0 * radius)).is_none());
        let tangent = tangent_sphere_point(sphere(0.0), sphere(2.0 * radius))
            .expect("externally tangent spheres share one point");
        assert!((tangent[0] / radius - 1.0).abs() <= 8.0 * f64::EPSILON);
        let plane = PlaneEquation {
            origin: [0.0; 3],
            normal: [1.0, 0.0, 0.0],
        };
        assert!(tangent_plane_sphere_point(plane, sphere(3.0 * radius)).is_none());
        assert_eq!(
            tangent_plane_sphere_point(plane, sphere(radius)),
            Some([0.0; 3])
        );
    }
}

#[test]
fn numerical_followup_fc05_tangency_is_relative_to_radius() {
    for radius in [1e-200, 1e-10, 1.0, 1e200] {
        let cylinder = CylinderEquation {
            origin: [0.; 3],
            axis: [0., 0., 1.],
            ref_direction: [1., 0., 0.],
            radius,
        };
        for offset in [0., radius] {
            let plane = PlaneCandidate {
                equation: PlaneEquation {
                    origin: [offset, 0., 0.],
                    normal: [1., 0., 0.],
                },
                chart: None,
                offset: 0,
            };
            assert_eq!(
                super::super::plane_candidate_is_fc05_tangent(plane, cylinder),
                offset == radius
            );
        }
    }
}

#[test]
fn fc05_tangent_plane_score_refuses_bounded_face_scan() {
    let scan = fc05_witness_scan();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "creo FC05 tangent bounded faces",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
            fc05_cylinder_model_witness(
                &ctx,
                &scan,
                2,
                CylinderEquation {
                    origin: [0.0, 0.0, 0.0],
                    axis: [0.0, 1.0, 0.0],
                    ref_direction: [1.0, 0.0, 0.0],
                    radius: 1.0,
                },
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo FC05 tangent bounded faces"));
}

#[test]
fn round_edge_envelope_refuses_bounded_face_scan() {
    let scan = round_edge_envelope_scan();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "creo round-edge bounded topology faces",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
            round_edge_envelopes_for_plane(&ctx, &scan, 1)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo round-edge bounded topology faces"));
}

#[test]
fn plane_candidates_refuse_surface_identity_child_scan() {
    let scan = stored_frame_branch_scan(true);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "creo plane candidate surface identity count",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
            plane_candidates(&ctx, &scan)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo plane candidate surface identity count"));
}

#[test]
fn stored_plane_origin_sign_mask_traversal_refuses_work_and_preserves_candidates() {
    let base = PlaneCandidate {
        equation: PlaneEquation {
            origin: [1.0, 2.0, 0.0],
            normal: [1.0, 1.0, 0.0],
        },
        chart: None,
        offset: 0,
    };
    let (candidates, count) = crate::test_support::assert_work_boundaries(
        &["creo stored plane origin sign mask traversal"],
        |ctx| super::super::stored_parameter_origin_sign_candidates(ctx, base),
    );
    assert_eq!(count, 4);
    let origins = candidates[..count]
        .iter()
        .map(|candidate| candidate.equation.origin)
        .collect::<Vec<_>>();
    assert_eq!(
        origins,
        vec![
            [1.0, 2.0, 0.0],
            [-1.0, 2.0, 0.0],
            [1.0, -2.0, 0.0],
            [-1.0, -2.0, 0.0],
        ]
    );
}
