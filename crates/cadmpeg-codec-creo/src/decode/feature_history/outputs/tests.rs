// SPDX-License-Identifier: Apache-2.0

use super::{
    bodies_containing_edges, copy_body_id, decoded_feature_reference_name,
    evaluated_sweep_body_kind, evaluated_sweep_output_bodies, feature_output_bodies,
    feature_reference_name, generated_edge_output_bodies, generated_input_output_bodies,
};

#[test]
fn invalid_feature_reference_name_refuses_before_lossy_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo decoded feature reference name"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    decoded_feature_reference_name(&ctx, b"A\xff").map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = decoded_feature_reference_name(&ctx, b"A\xff")
        .expect_err("replacement needs four retained bytes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo decoded feature reference name")
    );

    crate::decode::with_test_decode_ctx(|ctx| {
        assert_eq!(decoded_feature_reference_name(ctx, b"A\xff")?, "A\u{fffd}");
        assert_eq!(decoded_feature_reference_name(ctx, b"ASCII")?, "ASCII");
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("service-profile reference names");
}

#[test]
fn feature_reference_name_admits_byte_comparison() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.reference_names.extend([
        crate::feature::operations::FeatureReferenceName {
            feature_id: 42,
            name_bytes: b"feature-reference".to_vec(),
            own_reference_id: 7,
            reference_type: 1,
            offset: 0,
        },
        crate::feature::operations::FeatureReferenceName {
            feature_id: 42,
            name_bytes: b"feature-reference".to_vec(),
            own_reference_id: 8,
            reference_type: 1,
            offset: 10,
        },
    ]);

    let name = crate::test_support::assert_work_boundaries(
        &[
            "creo feature reference names",
            "creo feature reference name agreement",
        ],
        |ctx| feature_reference_name(ctx, &scan, 42),
    );
    assert_eq!(name, Some(b"feature-reference".as_slice()));
}
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    FaceSelection, Feature, FeatureDefinition, FeatureOperation, GeneratedEdgeRef, GeneratedFaceRef,
};
use cadmpeg_ir::ids::{BodyId, CoedgeId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId};
use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Face, Loop as IrLoop, Region, Sense, Shell};
use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use super::{insert_feature_parameter, insert_feature_source_property, replace_feature_parameter};

fn sweep_output_ir() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#40:body").expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir
}

#[test]
fn feature_output_history_refuses_before_visiting_node() {
    let scan = crate::test_support::empty_container_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo feature output visiting nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    feature_output_bodies(&ctx, &scan, &CadIr::empty(), 40).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_output_bodies(&ctx, &scan, &CadIr::empty(), 40)
        .expect_err("one history node exceeds the limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature output visiting nodes")
    );
}

#[test]
fn feature_output_history_refuses_before_recursive_step() {
    let scan = crate::test_support::empty_container_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_output_bodies(&ctx, &scan, &CadIr::empty(), 40)
        .expect_err("the first history step exceeds zero recursion depth");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RecursionDepth
            && resource.operation == "creo feature output history")
    );
}

#[test]
fn evaluated_sweep_candidate_refuses_before_scoped_text() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes, Some("creo evaluated sweep body candidate"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    evaluated_sweep_output_bodies(&ctx, &CadIr::empty(), 40).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = evaluated_sweep_output_bodies(&ctx, &CadIr::empty(), 40)
        .expect_err("one candidate needs scoped text");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo evaluated sweep body candidate")
    );
}

#[test]
fn evaluated_sweep_body_refuses_before_retained_id() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo evaluated sweep body IDs"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    evaluated_sweep_output_bodies(&ctx, &sweep_output_ir(), 40).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = evaluated_sweep_output_bodies(&ctx, &sweep_output_ir(), 40)
        .expect_err("one output needs a retained ID");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo evaluated sweep body IDs")
    );
}

#[test]
fn evaluated_sweep_body_refuses_before_output_row() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo evaluated sweep output bodies"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    evaluated_sweep_output_bodies(&ctx, &sweep_output_ir(), 40).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = evaluated_sweep_output_bodies(&ctx, &sweep_output_ir(), 40)
        .expect_err("one output needs a Vec row");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo evaluated sweep output bodies")
    );
}

#[test]
fn selected_edge_refuses_before_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo selected edge nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let edge = EdgeId::mint("creo:test:edge#1").expect("identity grammar");
    bodies_containing_edges(&ctx, &CadIr::empty(), &[edge]).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let edge = EdgeId::mint("creo:test:edge#1").expect("identity grammar");
    let error = bodies_containing_edges(&ctx, &CadIr::empty(), &[edge])
        .expect_err("one selected edge needs a BTreeSet node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo selected edge nodes")
    );
}

fn selected_edge_ir() -> (CadIr, EdgeId) {
    let mut ir = CadIr::empty();
    let body_id = BodyId::mint("creo:test:body#1").expect("identity grammar");
    let region_id = RegionId::mint("creo:test:region#1").expect("identity grammar");
    let shell_id = ShellId::mint("creo:test:shell#1").expect("identity grammar");
    let face_id = FaceId::mint("creo:test:face#1").expect("identity grammar");
    let loop_id = LoopId::mint("creo:test:loop#1").expect("identity grammar");
    let coedge_id = CoedgeId::mint("creo:test:coedge#1").expect("identity grammar");
    let edge_id = EdgeId::mint("creo:test:edge#1").expect("identity grammar");
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: BodyKind::Solid,
        regions: vec![region_id.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id,
        shells: vec![shell_id.clone()],
    });
    ir.model.shells.push(Shell::with_face(
        shell_id.clone(),
        region_id,
        face_id.clone(),
    ));
    ir.model.faces.push(Face {
        id: face_id.clone(),
        shell: shell_id,
        surface: SurfaceId::mint("creo:test:surface#1").expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id.clone()]),
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.loops.push(IrLoop {
        id: loop_id.clone(),
        face: face_id,
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(
                &cadmpeg_test_support::service_decode_context(),
                vec![coedge_id.clone()],
                Vec::new(),
            )
            .expect("fixture ring admission")
            .expect("valid loop ring"),
        ),
    });
    ir.model.coedges.push(Coedge {
        id: coedge_id.clone(),
        owner_loop: loop_id,
        edge: edge_id.clone(),
        radial_next: coedge_id,
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    });
    (ir, edge_id)
}

#[test]
fn selected_shell_refuses_before_btree_node() {
    let (ir, edge) = selected_edge_ir();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo selected shell nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let edge = edge.clone();
let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    bodies_containing_edges(&ctx, &ir, &[edge]).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = bodies_containing_edges(&ctx, &ir, &[edge])
        .expect_err("selected edge and shell need separate nodes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo selected shell nodes")
    );
}

#[test]
fn selected_coedge_membership_refuses_work_and_preserves_body() {
    let (ir, edge) = selected_edge_ir();
    let body = ir.model.bodies[0].id.clone();
    let bodies =
        crate::test_support::assert_work_boundaries(&["creo selected coedge lookup"], |ctx| {
            bodies_containing_edges(ctx, &ir, std::slice::from_ref(&edge))
        });
    assert_eq!(bodies, vec![body]);
}

#[test]
fn unmatched_selected_coedge_refuses_work_and_skips_empty_shell_scan() {
    let (mut ir, _) = selected_edge_ir();
    ir.model.shells.clear();
    let unmatched = EdgeId::mint("creo:test:edge#unmatched").expect("identity grammar");
    let refusal = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo selected coedge lookup",
        |ctx| bodies_containing_edges(ctx, &ir, std::slice::from_ref(&unmatched)),
    );
    let limit = match refusal {
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "creo selected coedge lookup" =>
        {
            limit
        }
        error => panic!("expected selected-coedge lookup refusal, got {error:?}"),
    };
    let cap = limit.used.checked_add(limit.additional).expect("work cap");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cap;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    assert!(
        bodies_containing_edges(&ctx, &ir, std::slice::from_ref(&unmatched))
            .expect("an unmatched coedge skips loop joins and the empty shell scan")
            .is_empty()
    );
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        bodies_containing_edges(ctx, &ir, std::slice::from_ref(&unmatched))
    })
    .expect("service profile preserves an unmatched edge result")
    .is_empty());
}

#[test]
fn selected_wire_shell_membership_short_circuits_after_first_match() {
    let (mut ir, _) = selected_edge_ir();
    let selected = EdgeId::mint("creo:test:wire-edge#selected").expect("identity grammar");
    let later = EdgeId::mint("creo:test:wire-edge#later").expect("identity grammar");
    let region = ir.model.regions[0].id.clone();
    let shell = ShellId::mint("creo:test:wire-shell#1").expect("identity grammar");
    ir.model.coedges.clear();
    ir.model.shells.clear();
    ir.model.regions[0].shells = vec![shell.clone()];
    ir.model.shells.push(
        Shell::new(
            shell,
            region,
            Vec::new(),
            vec![selected.clone(), later],
            Vec::new(),
        )
        .expect("wire shell fixture"),
    );
    let body = ir.model.bodies[0].id.clone();
    let named_needs = std::cell::RefCell::new(std::collections::BTreeSet::new());
    let bounded_result = crate::test_support::assert_work_boundaries(
        &["creo selected shell wire edge lookup"],
        |ctx| {
            let result = bodies_containing_edges(ctx, &ir, std::slice::from_ref(&selected));
            if let Some(resource) = ctx.resource_refusal() {
                if resource.operation == "creo selected shell wire edge lookup" {
                    named_needs.borrow_mut().insert(resource.used.checked_add(resource.additional).expect("work need"));
                }
            }
            result
        },
    );
    let named_refusals = named_needs.borrow().len();
    assert_eq!(named_refusals, 1, "the matched wire edge stops the scan");
    assert_eq!(
        named_refusals, 1,
        "work route reaches one wire membership query"
    );
    assert_eq!(
        bounded_result,
        vec![body.clone()]
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            bodies_containing_edges(ctx, &ir, std::slice::from_ref(&selected))
        })
        .expect("service profile admits wire shell output"),
        vec![body]
    );
}

#[test]
fn selected_body_refuses_before_output_row() {
    let (ir, edge) = selected_edge_ir();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo bodies containing selected edges"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let edge = edge.clone();
let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    bodies_containing_edges(&ctx, &ir, &[edge]).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = bodies_containing_edges(&ctx, &ir, &[edge])
        .expect_err("body row exceeds the two node allowance");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo bodies containing selected edges")
    );
}

#[test]
fn copied_output_body_id_refuses_before_retained_bytes() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo feature output body IDs"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let body = BodyId::mint("creo:test:body#1").expect("identity grammar");
    copy_body_id(&ctx, &body).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let body = BodyId::mint("creo:test:body#1").expect("identity grammar");
    let error = copy_body_id(&ctx, &body).expect_err("body ID needs retained bytes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo feature output body IDs")
    );
}

#[test]
fn copied_unicode_output_body_id_charges_only_retained_copy_work() {
    let arena = DecodeArena::new();
    let body = BodyId::mint("creo:test:body#é").expect("identity grammar");
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cadmpeg_core::decode::u64_from_index(body.as_str().len());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");

    assert_eq!(
        copy_body_id(&ctx, &body).expect("typed identity copy fits its byte-work limit"),
        body
    );
}

#[test]
fn generated_input_lookup_refuses_before_scoped_text() {
    let ir = CadIr::empty();
    let scan = crate::test_support::empty_container_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes, Some("creo generated input feature lookup"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut history = super::FeatureOutputHistory::new(&ctx, &ir)?;
    generated_input_output_bodies(
        &ctx,
        &scan,
        &ir,
        40,
        &mut history,
    ).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = generated_input_output_bodies(
        &ctx,
        &scan,
        &ir,
        40,
        &mut super::FeatureOutputHistory::new(&ctx, &ir).expect("history storage"),
    )
    .expect_err("lookup needs scoped text");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo generated input feature lookup")
    );
}

#[test]
fn generated_surface_body_refuses_before_feature_output_row() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 10,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    let mut ir = CadIr::empty();
    let shell_id = ShellId::mint("creo:test:shell#1").expect("identity grammar");
    let region_id = RegionId::mint("creo:test:region#1").expect("identity grammar");
    let face_id = FaceId::mint("creo:test:face#1").expect("identity grammar");
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: BodyId::mint("creo:test:body#1").expect("identity grammar"),
        shells: vec![shell_id.clone()],
    });
    ir.model.shells.push(Shell::with_face(
        shell_id.clone(),
        region_id,
        face_id.clone(),
    ));
    ir.model.faces.push(Face {
        id: face_id,
        shell: shell_id,
        surface: SurfaceId::mint("creo:visibgeom:surface#7").expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo feature output bodies"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    feature_output_bodies(&ctx, &scan, &ir, 10).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = feature_output_bodies(&ctx, &scan, &ir, 10)
        .expect_err("visited node and body row need two collection items");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature output bodies")
    );
}

#[test]
fn generated_edge_body_refuses_before_merge_row() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#50:body").expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    let edges = [GeneratedEdgeRef::new(
        cadmpeg_ir::features::FeatureId::mint("creo:model:feature#50").expect("identity grammar"),
        "curve#7".to_string(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("selection reference admission")
    .expect("valid generated edge")];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo generated edge output bodies"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut history = super::FeatureOutputHistory::new(&ctx, &ir)?;
    generated_edge_output_bodies(
        &ctx,
        &scan,
        &ir,
        &edges,
        &mut history,
    ).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = generated_edge_output_bodies(
        &ctx,
        &scan,
        &ir,
        &edges,
        &mut super::FeatureOutputHistory::new(&ctx, &ir).expect("history storage"),
    )
    .expect_err("visited producer and its body use the two admitted rows");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo generated edge output bodies")
    );
}

#[test]
fn generated_input_body_refuses_before_merge_row() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#50:body").expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#10")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Thicken {
                faces: FaceSelection::generated(
                    vec![GeneratedFaceRef::new(
                        cadmpeg_ir::features::FeatureId::mint("creo:model:feature#50")
                            .expect("identity grammar"),
                        "surface#7".to_string(),
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .expect("selection reference admission")
                    .expect("valid generated face")],
                    "creo:test:face#7".to_string(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("selection reference admission")
                .expect("valid face selection"),
                thickness: None,
                side: None,
            }),
        ),
        native_ref: None,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo generated input output bodies"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut history = super::FeatureOutputHistory::new(&ctx, &ir)?;
    generated_input_output_bodies(
        &ctx,
        &scan,
        &ir,
        10,
        &mut history,
    ).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = generated_input_output_bodies(
        &ctx,
        &scan,
        &ir,
        10,
        &mut super::FeatureOutputHistory::new(&ctx, &ir).expect("history storage"),
    )
    .expect_err("generated dependency, visited producer, and its body use three rows");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo generated input output bodies")
    );
}

#[test]
fn reconciled_output_refuses_before_update_row() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#40")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
        native_ref: None,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo reconciled output update rows"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let mut ir = ir.clone();
let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    super::super::dependencies::reconcile_feature_links(&ctx, &scan, &mut ir, &BTreeMap::new()).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error =
        super::super::dependencies::reconcile_feature_links(&ctx, &scan, &mut ir, &BTreeMap::new())
            .expect_err("visiting node and update row need two collection items");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo reconciled output update rows")
    );
}

#[test]
fn section_feature_lookups_keep_unique_source_selection() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Section),
        stream_offset: 0,
        body: vec![0; 8].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(17),
                owner_feature_id: Some(40),
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 4,
        });
    assert_eq!(crate::decode::with_test_decode_ctx(|ctx| super::owned_section_feature_id(ctx, &scan, 17)).expect("admitted section owner"), Some(40));
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::section_definition_for_history_feature(ctx, &scan, 40)).expect("admitted section definition").map(|value| value.offset),
        Some(4)
    );
    scan.features
        .definitions
        .push(scan.features.definitions[0].clone());
    assert_eq!(crate::decode::with_test_decode_ctx(|ctx| super::owned_section_feature_id(ctx, &scan, 17)).expect("admitted section owner"), None);
    assert!(crate::decode::with_test_decode_ctx(|ctx| super::section_definition_for_history_feature(ctx, &scan, 40)).expect("admitted section definition").is_none());
}

#[test]
fn feature_parameter_refuses_before_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo feature parameter nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    ).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    let error = insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    )
    .expect_err("one parameter needs one BTreeMap node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature parameter nodes")
    );
}

#[test]
fn feature_parameter_staging_node_refuses_materialized_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes, Some("creo feature parameter nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    ).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    let error = insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    )
    .expect_err("one source node exceeds materialized storage");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo feature parameter nodes"
            && resource.additional > 0)
    );
}

#[test]
fn feature_parameter_refuses_before_staging_value() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes, Some("creo feature parameter value"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    ).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    let error = insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    )
    .expect_err("staging value exceeds materialized allowance");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo feature parameter value")
    );
}

#[test]
fn feature_parameter_refuses_before_scoped_key_candidate() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes, Some("creo feature parameter key candidate"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    ).map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    let error = insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    )
    .expect_err("candidate exceeds materialized allowance");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo feature parameter key candidate")
    );
}

#[test]
fn feature_parameter_native_text_transfer_refuses_retained_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo feature parameter text"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    )
    .expect("staging text fits materialized storage");
    text_storage
        .commit().map(|_| ())
            });
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "x",
    )
    .expect("staging text fits materialized storage");
    let error = text_storage
        .commit()
        .expect_err("Native parameter text exceeds retained allowance");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo feature parameter text")
    );
}

#[test]
fn feature_parameter_keeps_duplicate_suffix_and_direct_replacement() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "a",
    )
    .expect("first fits");
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "b",
    )
    .expect("second fits");
    insert_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "c",
    )
    .expect("third fits");
    replace_feature_parameter(
        &ctx,
        &mut text_storage,
        &mut node_storage,
        &mut parameters,
        "choice.value",
        "z",
    )
    .expect("replacement fits");
    assert_eq!(
        parameters.into_iter().collect::<Vec<_>>(),
        vec![
            ("choice.value".into(), "z".into()),
            ("choice.value#2".into(), "b".into()),
            ("choice.value#3".into(), "c".into()),
        ]
    );
}

#[test]
fn feature_parameter_named_entry_service_preserves_suffix_order() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut text_storage = ctx
        .reserve_scoped(0, "creo feature parameter text")
        .expect("parameter text lease");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature parameter nodes")
        .expect("parameter node lease");
    let mut parameters = BTreeMap::new();
    for value in ["a", "b", "c"] {
        insert_feature_parameter(
            &ctx,
            &mut text_storage,
            &mut node_storage,
            &mut parameters,
            "choice.value",
            value,
        )
        .expect("parameter collision suffix is admitted");
    }
    text_storage
        .commit()
        .expect("Native output retains staged parameter text");
    let entries = cadmpeg_core::text::named_entries_for_decode(&ctx, "feature", parameters)
        .expect("parameter names are nonblank and unique");
    drop(node_storage);
    assert_eq!(
        entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("choice.value", "a"),
            ("choice.value#2", "b"),
            ("choice.value#3", "c"),
        ]
    );
}

#[test]
fn feature_source_property_refuses_before_btree_node() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo feature source property nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    ).map(|_| ())
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    let error = insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    )
    .expect_err("one property needs one map node");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo feature source property nodes")
    );
    assert!(properties.is_empty());
}

#[test]
fn feature_source_property_staging_node_refuses_materialized_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes, Some("creo feature source property nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    ).map(|_| ())
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    let error = insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    )
    .expect_err("one staging node exceeds materialized storage");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo feature source property nodes")
    );
}

#[test]
fn feature_source_property_refuses_before_key_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo feature source property key"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    ).map(|_| ())
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    let error = insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    )
    .expect_err("key bytes exceed the retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo feature source property key")
    );
}

#[test]
fn feature_source_property_refuses_before_value_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo feature source property value"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    ).map(|_| ())
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    let error = insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    )
    .expect_err("value bytes exceed the retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo feature source property value")
    );
}

#[test]
fn feature_source_property_keeps_key_order_and_replacement() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(&ctx, &mut node_storage, &mut properties, "z", 1)
        .expect("first property fits");
    insert_feature_source_property(&ctx, &mut node_storage, &mut properties, "a", 2)
        .expect("second property fits");
    insert_feature_source_property(&ctx, &mut node_storage, &mut properties, "z", 3)
        .expect("replacement fits");
    assert_eq!(
        properties.into_iter().collect::<Vec<_>>(),
        vec![("a".into(), "2".into()), ("z".into(), "3".into())]
    );
}

#[test]
fn feature_source_property_named_output_node_remains_retained() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("named entry map nodes"), |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    )
    ?;
    cadmpeg_core::text::named_entries_for_decode(&ctx, "feature", properties).map(|_| ()).map_err(|error| match error {
        cadmpeg_core::text::NamedEntryError::ResourceRefusal(resource) => cadmpeg_core::CodecError::ResourceLimit(resource),
        error => panic!("unexpected named entry refusal: {error:?}"),
    })
            });
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(
        &ctx,
        &mut node_storage,
        &mut properties,
        "recipe",
        "Extrude",
    )
    .expect("staging node uses scoped storage");
    let error = cadmpeg_core::text::named_entries_for_decode(&ctx, "feature", properties)
        .expect_err("the decoded output node needs retained storage");
    assert!(
        matches!(error, cadmpeg_core::text::NamedEntryError::ResourceRefusal(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "named entry map nodes")
    );
}

#[test]
fn feature_source_property_named_entry_service_preserves_replacement() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let mut node_storage = ctx
        .reserve_scoped(0, "creo feature source property nodes")
        .expect("source property node lease");
    let mut properties = BTreeMap::new();
    insert_feature_source_property(&ctx, &mut node_storage, &mut properties, "z", 1)
        .expect("first property fits");
    insert_feature_source_property(&ctx, &mut node_storage, &mut properties, "a", 2)
        .expect("second property fits");
    insert_feature_source_property(&ctx, &mut node_storage, &mut properties, "z", 3)
        .expect("replacement fits");
    let entries = cadmpeg_core::text::named_entries_for_decode(&ctx, "feature", properties)
        .expect("property names are nonblank and unique");
    drop(node_storage);
    assert_eq!(
        entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        vec![("a", "2"), ("z", "3")]
    );
}

#[test]
fn generated_edge_outputs_follow_producer_history_before_ir_feature_insertion() {
    let feature_row = |feature_id| crate::feature::rows::FeatureRow {
        feature_id,
        root_schema_class: None,
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    };
    let curve_row = |id, feature_id| crate::curve::CurveTopologyRow {
        id,
        type_byte: 8,
        feature_id,
        directions: [1, 0xf6],
        faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(11)],
        next_edges: [id, id],
        offset: 0,
    };
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .rows
        .extend([feature_row(50), feature_row(70)]);
    scan.features.affected_ids.extend([
        crate::feature::rows::FeatureAffectedIds {
            feature_id: 10,
            kind: crate::feature::rows::AffectedIdKind::Edges,
            ids: vec![45],
            offset: 0,
        },
        crate::feature::rows::FeatureAffectedIds {
            feature_id: 50,
            kind: crate::feature::rows::AffectedIdKind::Edges,
            ids: vec![60],
            offset: 0,
        },
    ]);
    scan.curves
        .topology_rows
        .extend([curve_row(45, 50), curve_row(60, 70)]);

    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#70:body".to_string()).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(ctx, &scan, &ir, 10))
            .expect("service profile admits output bodies"),
        vec![BodyId::mint("creo:feature:extrusion#70:body".to_string()).expect("identity grammar")]
    );
}

#[test]
fn generated_face_outputs_follow_producer_history_after_feature_insertion() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#10")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Thicken {
                faces: FaceSelection::generated(
                    vec![GeneratedFaceRef::new(
                        cadmpeg_ir::features::FeatureId::mint("creo:model:feature#50")
                            .expect("identity grammar"),
                        "surface#7".to_string(),
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .expect("selection reference admission")
                    .expect("valid test fixture")],
                    "creo:generated-face#7".to_string(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("selection reference admission")
                .expect("valid test fixture"),
                thickness: None,
                side: None,
            }),
        ),
        native_ref: None,
    });

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(ctx, &scan, &ir, 10))
            .expect("service profile admits output bodies"),
        vec![BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar")]
    );

    crate::decode::with_test_decode_ctx(|ctx| {
        super::super::dependencies::reconcile_feature_links(ctx, &scan, &mut ir, &BTreeMap::new())
    })
    .expect("the fixture feature links reconcile");
    assert_eq!(
        *ir.model.features[0].evaluation.outputs(),
        vec![BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar")]
    );
}

#[test]
fn generated_result_faces_are_outputs_alongside_generated_input_bodies() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 10,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:generated:result#10".to_string()).expect("identity grammar"),
        kind: BodyKind::Sheet,
        regions: vec![
            RegionId::mint("creo:generated:region#10".to_string()).expect("identity grammar")
        ],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: RegionId::mint("creo:generated:region#10".to_string()).expect("identity grammar"),
        body: BodyId::mint("creo:generated:result#10".to_string()).expect("identity grammar"),
        shells: vec![
            ShellId::mint("creo:generated:shell#10".to_string()).expect("identity grammar")
        ],
    });
    ir.model.shells.push(Shell::with_face(
        ShellId::mint("creo:generated:shell#10".to_string()).expect("identity grammar"),
        RegionId::mint("creo:generated:region#10".to_string()).expect("identity grammar"),
        FaceId::mint("creo:generated:face#7".to_string()).expect("identity grammar"),
    ));
    ir.model.faces.push(Face {
        id: FaceId::mint("creo:generated:face#7".to_string()).expect("identity grammar"),
        shell: ShellId::mint("creo:generated:shell#10".to_string()).expect("identity grammar"),
        surface: SurfaceId::mint("creo:visibgeom:surface#7".to_string()).expect("identity grammar"),
        sense: cadmpeg_ir::topology::Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![LoopId::mint(
            "creo:generated:loop#7".to_string(),
        )
        .expect("identity grammar")]),
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#10")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Thicken {
                faces: FaceSelection::generated(
                    vec![GeneratedFaceRef::new(
                        cadmpeg_ir::features::FeatureId::mint("creo:model:feature#50")
                            .expect("identity grammar"),
                        "surface#7".to_string(),
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .expect("selection reference admission")
                    .expect("valid test fixture")],
                    "creo:generated-face#7".to_string(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("selection reference admission")
                .expect("valid test fixture"),
                thickness: None,
                side: None,
            }),
        ),
        native_ref: None,
    });

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(ctx, &scan, &ir, 10))
            .expect("service profile admits output bodies"),
        vec![
            BodyId::mint("creo:generated:result#10".to_string()).expect("identity grammar"),
            BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar"),
        ]
    );

    let mut existing_surface_output = ir.clone();
    let sweep_body = BodyId::mint("creo:feature:extrusion#10:body").expect("identity grammar");
    existing_surface_output.model.bodies[1].id = sweep_body.clone();
    existing_surface_output.model.regions[0].body = sweep_body.clone();
    let output_bodies =
        crate::test_support::assert_work_boundaries(&["creo feature output body lookup"], |ctx| {
            feature_output_bodies(ctx, &scan, &existing_surface_output, 10)
        });
    assert_eq!(
        output_bodies,
        vec![
            sweep_body,
            BodyId::mint("creo:feature:extrusion#50:body").expect("identity grammar"),
        ]
    );

    let mut duplicate_shell = ir.clone();
    duplicate_shell.model.shells.push(
        Shell::new(
            ShellId::mint("creo:generated:shell#10".to_string()).expect("identity grammar"),
            RegionId::mint("creo:ambiguous:region#10".to_string()).expect("identity grammar"),
            ir.model.shells[0].faces().to_vec(),
            Vec::new(),
            Vec::new(),
        )
        .expect("valid test fixture"),
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(
            ctx,
            &scan,
            &duplicate_shell,
            10
        ))
        .expect("service profile admits output bodies"),
        vec![BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar")]
    );

    let mut duplicate_region = ir.clone();
    duplicate_region.model.regions.push(Region {
        id: RegionId::mint("creo:generated:region#10".to_string()).expect("identity grammar"),
        body: BodyId::mint("creo:ambiguous:body#10".to_string()).expect("identity grammar"),
        shells: Vec::new(),
    });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(
            ctx,
            &scan,
            &duplicate_region,
            10
        ))
        .expect("service profile admits output bodies"),
        vec![BodyId::mint("creo:feature:extrusion#50:body".to_string()).expect("identity grammar")]
    );

    let mut duplicate_feature = ir.clone();
    let feature = duplicate_feature.model.features[0].clone();
    duplicate_feature.model.features.push(feature);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| feature_output_bodies(
            ctx,
            &scan,
            &duplicate_feature,
            10
        ))
        .expect("service profile admits output bodies"),
        vec![BodyId::mint("creo:generated:result#10".to_string()).expect("identity grammar")]
    );
}

#[test]
fn generated_input_chain_membership_refuses_work_without_surface_route() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    // A preexisting evaluated output makes the generated-input chain search nonempty.
    let initial_output = BodyId::mint("creo:feature:extrusion#10:body").expect("identity grammar");
    ir.model.bodies.push(Body {
        id: initial_output.clone(),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    let body = BodyId::mint("creo:feature:extrusion#50:body").expect("identity grammar");
    ir.model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });

    let fixture_ctx = cadmpeg_test_support::service_decode_context();
    let producer = GeneratedFaceRef::new(
        cadmpeg_ir::features::FeatureId::mint("creo:model:feature#50").expect("identity grammar"),
        "surface#7".to_string(),
        &fixture_ctx,
    )
    .expect("selection reference admission")
    .expect("valid generated reference");
    let faces = FaceSelection::generated(
        vec![producer],
        "creo:generated-face#7".to_string(),
        &fixture_ctx,
    )
    .expect("selection reference admission")
    .expect("valid generated face selection");
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#10")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Thicken {
                faces,
                thickness: None,
                side: None,
            }),
        ),
        native_ref: None,
    });

    assert!(scan.surfaces.rows.is_empty());
    assert!(ir.model.faces.is_empty());
    let outputs =
        crate::test_support::assert_work_boundaries(&["creo feature output body lookup"], |ctx| {
            feature_output_bodies(ctx, &scan, &ir, 10)
        });
    assert_eq!(outputs, vec![initial_output, body]);
}

#[test]
fn edge_output_joins_reject_duplicate_topology_owners() {
    let body_id =
        BodyId::mint("test:model:entity#creo:test:body".to_string()).expect("identity grammar");
    let region_id =
        RegionId::mint("test:model:entity#creo:test:region".to_string()).expect("identity grammar");
    let shell_id =
        ShellId::mint("test:model:entity#creo:test:shell".to_string()).expect("identity grammar");
    let face_id =
        FaceId::mint("test:model:entity#creo:test:face".to_string()).expect("identity grammar");
    let loop_id =
        LoopId::mint("test:model:entity#creo:test:loop".to_string()).expect("identity grammar");
    let coedge_id =
        CoedgeId::mint("test:model:entity#creo:test:coedge".to_string()).expect("identity grammar");
    let edge_id =
        EdgeId::mint("test:model:entity#creo:test:edge".to_string()).expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: BodyKind::Solid,
        regions: vec![region_id.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id.clone(),
        shells: vec![shell_id.clone()],
    });
    ir.model.shells.push(Shell::with_face(
        shell_id.clone(),
        region_id.clone(),
        face_id.clone(),
    ));
    ir.model.faces.push(Face {
        id: face_id.clone(),
        shell: shell_id.clone(),
        surface: SurfaceId::mint("test:model:entity#creo:test:surface".to_string())
            .expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id.clone()]),
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.loops.push(IrLoop {
        id: loop_id.clone(),
        face: face_id.clone(),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(
                &cadmpeg_test_support::service_decode_context(),
                vec![coedge_id.clone()],
                Vec::new(),
            )
            .expect("fixture ring admission")
            .expect("valid loop ring"),
        ),
    });
    ir.model.coedges.push(Coedge {
        id: coedge_id.clone(),
        owner_loop: loop_id.clone(),
        edge: edge_id.clone(),
        radial_next: coedge_id.clone(),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| bodies_containing_edges(
            ctx,
            &ir,
            std::slice::from_ref(&edge_id)
        ))
        .expect("service profile admits output bodies"),
        vec![body_id.clone()]
    );

    let mut duplicate_loop = ir.clone();
    duplicate_loop.model.loops.push(IrLoop {
        id: loop_id.clone(),
        face: FaceId::mint("test:model:entity#creo:ambiguous:face".to_string())
            .expect("identity grammar"),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(
                &cadmpeg_test_support::service_decode_context(),
                vec![coedge_id.clone()],
                Vec::new(),
            )
            .expect("fixture ring admission")
            .expect("valid loop ring"),
        ),
    });
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| bodies_containing_edges(
            ctx,
            &duplicate_loop,
            std::slice::from_ref(&edge_id)
        ))
        .expect("service profile admits output bodies")
        .is_empty()
    );

    let mut duplicate_face = ir.clone();
    duplicate_face.model.faces.push(Face {
        id: face_id.clone(),
        shell: ShellId::mint("test:model:entity#creo:ambiguous:shell".to_string())
            .expect("identity grammar"),
        surface: SurfaceId::mint("test:model:entity#creo:test:surface-2".to_string())
            .expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    });
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| bodies_containing_edges(
            ctx,
            &duplicate_face,
            std::slice::from_ref(&edge_id)
        ))
        .expect("service profile admits output bodies")
        .is_empty()
    );

    let mut duplicate_shell = ir.clone();
    duplicate_shell.model.shells.push(
        Shell::new(
            shell_id.clone(),
            RegionId::mint("test:model:entity#creo:ambiguous:region".to_string())
                .expect("identity grammar"),
            ir.model.shells[0].faces().to_vec(),
            Vec::new(),
            Vec::new(),
        )
        .expect("valid test fixture"),
    );
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| bodies_containing_edges(
            ctx,
            &duplicate_shell,
            std::slice::from_ref(&edge_id)
        ))
        .expect("service profile admits output bodies")
        .is_empty()
    );

    let mut duplicate_region = ir.clone();
    duplicate_region.model.regions.push(Region {
        id: region_id.clone(),
        body: BodyId::mint("test:model:entity#creo:ambiguous:body".to_string())
            .expect("identity grammar"),
        shells: Vec::new(),
    });
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| bodies_containing_edges(
            ctx,
            &duplicate_region,
            std::slice::from_ref(&edge_id)
        ))
        .expect("service profile admits output bodies")
        .is_empty()
    );

    let mut duplicate_body = ir;
    duplicate_body.model.bodies.push(Body {
        id: body_id,
        kind: BodyKind::Sheet,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| bodies_containing_edges(
            ctx,
            &duplicate_body,
            std::slice::from_ref(&edge_id)
        ))
        .expect("service profile admits output bodies")
        .is_empty()
    );
}

#[test]
fn evaluated_sweep_body_joins_reject_duplicate_ids() {
    let mut ir = CadIr::empty();
    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#40:body".to_string()).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| evaluated_sweep_output_bodies(ctx, &ir, 40))
            .expect("service profile admits output bodies"),
        vec![BodyId::mint("creo:feature:extrusion#40:body".to_string()).expect("identity grammar")]
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| evaluated_sweep_body_kind(
            ctx,
            &ir,
            "extrusion",
            40
        ))
        .expect("service profile admits scalar parsing"),
        Some(BodyKind::Solid)
    );

    ir.model.bodies.push(Body {
        id: BodyId::mint("creo:feature:extrusion#40:body".to_string()).expect("identity grammar"),
        kind: BodyKind::Sheet,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| evaluated_sweep_output_bodies(ctx, &ir, 40))
            .expect("service profile admits output bodies")
            .is_empty()
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| evaluated_sweep_body_kind(
            ctx,
            &ir,
            "extrusion",
            40
        ))
        .expect("service profile admits scalar parsing"),
        None
    );
}

#[test]
fn generated_input_feature_scan_refuses_before_identity_comparison() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.features.push(Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:model:feature#40")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
        native_ref: None,
    });
    let outputs = crate::test_support::assert_work_boundaries(
        &["creo generated input feature lookup traversal"],
        |ctx| {
            generated_input_output_bodies(
                ctx,
                &scan,
                &ir,
                40,
                &mut super::FeatureOutputHistory::new(ctx, &ir)?,
            )
        },
    );
    assert!(outputs.is_empty());
}

#[test]
fn evaluated_sweep_body_scan_refuses_before_identity_comparison() {
    let ir = sweep_output_ir();
    let outputs = crate::test_support::assert_work_boundaries(
        &["creo evaluated sweep body lookup traversal"],
        |ctx| evaluated_sweep_output_bodies(ctx, &ir, 40),
    );
    assert_eq!(
        outputs,
        vec![BodyId::mint("creo:feature:extrusion#40:body").expect("identity grammar")],
    );
}

#[test]
fn evaluated_sweep_body_identity_validation_refuses_at_work_boundary() {
    let ir = sweep_output_ir();
    let bodies = crate::test_support::assert_work_boundaries(
        &["creo evaluated sweep body identity validation"],
        |ctx| evaluated_sweep_output_bodies(ctx, &ir, 40),
    );
    assert_eq!(
        bodies,
        vec![BodyId::mint("creo:feature:extrusion#40:body").expect("fixture body ID")],
    );
}

#[test]
fn feature_parameter_collision_boundary_keeps_first_unused_suffix() {
    let parameters = crate::test_support::assert_work_boundaries(
        &["creo feature parameter key lookup"],
        |ctx| {
            let mut text_storage = ctx.reserve_scoped(0, "creo feature parameter text")?;
            let mut node_storage = ctx.reserve_scoped(0, "creo feature parameter nodes")?;
            let mut parameters = BTreeMap::from([
                ("choice.value".to_owned(), "a".to_owned()),
                ("choice.value#2".to_owned(), "b".to_owned()),
            ]);
            insert_feature_parameter(
                ctx,
                &mut text_storage,
                &mut node_storage,
                &mut parameters,
                "choice.value",
                "c",
            )?;
            Ok::<_, cadmpeg_core::CodecError>(parameters)
        },
    );
    assert_eq!(
        parameters.into_iter().collect::<Vec<_>>(),
        vec![
            ("choice.value".to_owned(), "a".to_owned()),
            ("choice.value#2".to_owned(), "b".to_owned()),
            ("choice.value#3".to_owned(), "c".to_owned()),
        ],
    );
}
