// SPDX-License-Identifier: Apache-2.0

fn limited_context(
    max_items: u64,
    max_retained: u64,
) -> cadmpeg_core::decode::DecodeContext<'static> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = Box::leak(Box::new(DecodeArena::new()));
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let policy = Box::leak(Box::new(policy));
    DecodeContext::from_root_bytes(&[], arena, policy)
        .unwrap()
        .0
}

fn body_brep(with_region: bool) -> cadmpeg_asm::brep::AsmBrep {
    use cadmpeg_ir::ids::{BodyId, RegionId};
    use cadmpeg_ir::topology::{Body, BodyKind};
    let regions = if with_region {
        vec![RegionId::mint("f3d:brep:entity#2").unwrap()]
    } else {
        Vec::new()
    };
    let body = Body {
        id: BodyId::mint("f3d:brep:entity#1").unwrap(),
        kind: BodyKind::Solid,
        regions,
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    cadmpeg_asm::brep::AsmBrep {
        bodies: vec![body],
        ..Default::default()
    }
}

#[test]
fn historical_topology_references_refuse_collection_limit() {
    let ctx = limited_context(0, u64::MAX);
    let error = super::super::historical_topology(&ctx, &body_brep(false)).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical topology references")
    );
}

#[test]
fn historical_topology_relation_members_refuse_collection_limit() {
    let ctx = limited_context(1, u64::MAX);
    let error = super::super::historical_topology(&ctx, &body_brep(true)).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical topology references")
    );
}

#[test]
fn historical_topology_relation_rows_refuse_collection_limit() {
    let ctx = limited_context(1, u64::MAX);
    let error = super::super::historical_topology(&ctx, &body_brep(false)).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical topology relations")
    );
}

fn tagged_brep() -> crate::brep::Brep {
    use cadmpeg_ir::attributes::AttributeTarget;
    use cadmpeg_ir::ids::FaceId;
    crate::brep::Brep {
        persistent_subentity_tags: vec![crate::records::sketch_links::PersistentSubentityTag {
            id: "native:tag".into(),
            target: AttributeTarget::Face(FaceId::mint("f3d:brep:entity#4").unwrap()),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::try_from("tag").unwrap(),
            design_references: vec![2],
            ordinal: 0,
        }],
        ..Default::default()
    }
}

#[test]
fn historical_tag_references_refuse_collection_limit() {
    let ctx = limited_context(0, u64::MAX);
    let error = super::super::historical_topology_with_tags(&ctx, &tagged_brep()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical tag design references")
    );
}

#[test]
fn historical_tag_token_refuses_retained_limit() {
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D historical tag token",
        0,
        |ctx| super::super::historical_topology_with_tags(ctx, &tagged_brep()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D historical tag token")
    );
}

#[test]
fn historical_tags_refuse_collection_limit() {
    let ctx = limited_context(1, u64::MAX);
    let error = super::super::historical_topology_with_tags(&ctx, &tagged_brep()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical persistent tags")
    );
}

fn relation_brep(classified_face_loops: bool) -> cadmpeg_asm::brep::AsmBrep {
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId,
    };
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, Face, FaceLoops, Loop, LoopBoundary, LoopRing, Region, Sense,
        Shell,
    };

    let id = |slot| format!("f3d:brep:entity#{slot}");
    let body = BodyId::mint(id(1)).expect("identity grammar");
    let region = RegionId::mint(id(2)).expect("identity grammar");
    let shell = ShellId::mint(id(3)).expect("identity grammar");
    let face = FaceId::mint(id(4)).expect("identity grammar");
    let outer_loop = LoopId::mint(id(5)).expect("identity grammar");
    let outer_coedge = CoedgeId::mint(id(6)).expect("identity grammar");
    let mut brep = cadmpeg_asm::brep::AsmBrep {
        bodies: vec![Body {
            id: body.clone(),
            kind: BodyKind::Solid,
            regions: vec![region.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        }],
        regions: vec![Region {
            id: region.clone(),
            body,
            shells: vec![shell.clone()],
        }],
        shells: vec![Shell::with_face(shell, region, face.clone())],
        faces: vec![Face {
            id: face.clone(),
            shell: ShellId::mint(id(3)).expect("identity grammar"),
            surface: SurfaceId::mint(id(20)).expect("identity grammar"),
            sense: Sense::Forward,
            loops: if classified_face_loops {
                FaceLoops::classified(
                    outer_loop.clone(),
                    vec![LoopId::mint(id(9)).expect("identity grammar")],
                )
            } else {
                FaceLoops::unspecified(vec![outer_loop.clone()])
            },
            name: None,
            color: None,
            tolerance: None,
        }],
        ..Default::default()
    };
    brep.loops.push(Loop {
        id: outer_loop.clone(),
        face: face.clone(),
        boundary: LoopBoundary::Ring(LoopRing::single(outer_coedge.clone())),
    });
    brep.coedges.push(Coedge {
        id: outer_coedge.clone(),
        owner_loop: outer_loop,
        edge: EdgeId::mint(id(7)).expect("identity grammar"),
        radial_next: outer_coedge,
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    });
    if classified_face_loops {
        let inner_loop = LoopId::mint(id(9)).expect("identity grammar");
        let inner_coedge = CoedgeId::mint(id(10)).expect("identity grammar");
        brep.loops.push(Loop {
            id: inner_loop.clone(),
            face,
            boundary: LoopBoundary::Ring(LoopRing::single(inner_coedge.clone())),
        });
        brep.coedges.push(Coedge {
            id: inner_coedge.clone(),
            owner_loop: inner_loop,
            edge: EdgeId::mint(id(11)).expect("identity grammar"),
            radial_next: inner_coedge,
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
        });
    }
    brep
}

#[test]
fn historical_topology_relation_owner_slices_refuse_work() {
    let brep = relation_brep(false);
    let operation = "scan F3D historical topology relation owners";
    for skip in 0..7 {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            skip,
            |ctx| super::super::historical_topology(ctx, &brep).map(|_| ()),
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
        ));
    }
}

#[test]
fn historical_topology_relation_member_slices_refuse_work() {
    let brep = relation_brep(false);
    let operation = "scan F3D historical topology relation members";
    for skip in 0..5 {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            skip,
            |ctx| super::super::historical_topology(ctx, &brep).map(|_| ()),
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
        ));
    }
}

#[test]
fn historical_topology_classified_face_loop_refuses_work() {
    let brep = relation_brep(true);
    let operation = "visit F3D historical classified face loop";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| super::super::historical_topology(ctx, &brep).map(|_| ()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn historical_topology_classified_face_loop_members_refuse_work() {
    let brep = relation_brep(true);
    let operation = "scan F3D historical topology relation members";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        3,
        |ctx| super::super::historical_topology(ctx, &brep).map(|_| ()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}
