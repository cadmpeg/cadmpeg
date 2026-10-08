// SPDX-License-Identifier: Apache-2.0

use super::{bodies_containing_edges, CadIr, BodyId, CoedgeId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId, Body, BodyKind, Coedge, Face, IrLoop, Region, Sense, Shell, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

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

