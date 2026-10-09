// SPDX-License-Identifier: Apache-2.0

use crate::brep::graph::test_support::with_test_context;

#[test]
fn shared_edge_coedge_parity_orients_connected_faces() {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId, LoopId, ShellId, SurfaceId};
    use cadmpeg_ir::topology::{Coedge, Face, Loop, Sense};

    let face = |id: &str, lp: &str| Face {
        id: FaceId::mint(format!("test:model:entity#{id}")).expect("identity grammar"),
        shell: ShellId::mint("test:model:entity#shell").expect("identity grammar"),
        surface: SurfaceId::mint(format!("test:model:entity#surface-{id}"))
            .expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![LoopId::mint(format!(
            "test:model:entity#{lp}"
        ))
        .expect("identity grammar")]),
        name: None,
        color: None,
        tolerance: None,
    };
    let lp = |id: &str, face: &str, coedge: &str| Loop {
        id: LoopId::mint(format!("test:model:entity#{id}")).expect("identity grammar"),
        face: FaceId::mint(format!("test:model:entity#{face}")).expect("identity grammar"),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(
                &cadmpeg_test_support::service_decode_context(),
                vec![CoedgeId::mint(format!("test:model:entity#{coedge}"))
                    .expect("identity grammar")],
                Vec::new(),
            )
            .expect("fixture ring admission")
            .expect("valid loop ring"),
        ),
    };
    let coedge = |id: &str, lp: &str, radial: &str, sense| Coedge {
        id: CoedgeId::mint(format!("test:model:entity#{id}")).expect("identity grammar"),
        owner_loop: LoopId::mint(format!("test:model:entity#{lp}")).expect("identity grammar"),
        edge: EdgeId::mint("test:model:entity#edge").expect("identity grammar"),
        radial_next: CoedgeId::mint(format!("test:model:entity#{radial}"))
            .expect("identity grammar"),
        sense,
        use_curve: None,
        pcurves: Vec::new(),
    };
    let mut brep = crate::brep::graph::Brep {
        faces: vec![face("face-a", "loop-a"), face("face-b", "loop-b")],
        loops: vec![
            lp("loop-a", "face-a", "coedge-a"),
            lp("loop-b", "face-b", "coedge-b"),
        ],
        coedges: vec![
            coedge("coedge-a", "loop-a", "coedge-b", Sense::Forward),
            coedge("coedge-b", "loop-b", "coedge-a", Sense::Forward),
        ],
        ..Default::default()
    };

    with_test_context(|ctx| crate::brep::graph::orientation::solve_face_orientation(ctx, &mut brep))
        .expect("orient first face pair");
    assert_eq!(brep.faces[0].sense, Sense::Forward);
    assert_eq!(brep.faces[1].sense, Sense::Reversed);

    brep.faces[1].sense = Sense::Reversed;
    brep.coedges[1].sense = Sense::Reversed;
    with_test_context(|ctx| crate::brep::graph::orientation::solve_face_orientation(ctx, &mut brep))
        .expect("orient reversed face pair");
    assert_eq!(brep.faces[0].sense, Sense::Forward);
    assert_eq!(brep.faces[1].sense, Sense::Forward);
}

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
use cadmpeg_ir::topology::{Face, FaceLoops, Sense};
use crate::brep::graph::Brep;
use super::FaceSenses;

fn one_face() -> Brep {
    Brep {
        faces: vec![Face {
            id: FaceId::mint("test:model:face#orientation").unwrap(),
            shell: ShellId::mint("test:model:shell#orientation").unwrap(),
            surface: SurfaceId::mint("test:model:surface#orientation").unwrap(),
            sense: Sense::Reversed,
            loops: FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        }],
        ..Brep::default()
    }
}

fn orientation_peak() -> u64 {
    // Each one-entry initial/solved HashMap has four buckets, fifteen
    // alignment bytes, four controls and sixteen trailing controls. Core
    // uses capacity4 for pointer vectors and capacity8 for one-byte bools.
    let map = 4 * std::mem::size_of::<(&FaceId, bool)>()
        + std::mem::align_of::<(&FaceId, bool)>().max(16) - 1 + 4 + 16;
    u64::try_from(2 * map + 4 * std::mem::size_of::<&FaceId>() + 8 * std::mem::size_of::<bool>()).unwrap()
}

fn orientation_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = orientation_peak();
    policy.limits.max_retained_bytes = 0;
    policy
}

#[test]
fn face_senses_keep_only_the_copied_boolean_backing_live() {
    let out = one_face();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &orientation_policy()).unwrap();
    let senses = FaceSenses::new(&ctx, &out).unwrap();
    assert_eq!(senses.values, [true]);
    let live = u64::try_from(8 * std::mem::size_of::<bool>()).unwrap();
    let additional = orientation_peak() - live + 1;
    let Err(CodecError::ResourceLimit(limit)) = ctx.reserve_scoped(additional, "test live face senses") else { panic!("live boolean backing"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((limit.used, limit.additional, limit.limit), (live, additional, orientation_peak()));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(senses);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn face_senses_drop_and_application_release_all_scratch_with_zero_retention() {
    let mut out = one_face();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &orientation_policy()).unwrap();
    drop(FaceSenses::new(&ctx, &out).unwrap());
    drop(ctx.reserve_scoped(orientation_peak(), "test dropped face senses").unwrap());
    let senses = FaceSenses::new(&ctx, &out).unwrap();
    out.faces[0].sense = Sense::Forward;
    senses.apply(&ctx, &mut out.faces).unwrap();
    assert_eq!(out.faces[0].sense, Sense::Reversed);
    drop(ctx.reserve_scoped(orientation_peak(), "test applied face senses").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn face_sense_copy_refuses_at_the_source_derived_peak_without_mutation() {
    let mut out = one_face();
    let original = out.faces[0].sense;
    let arena = DecodeArena::new();
    let mut policy = orientation_policy();
    policy.limits.max_materialized_bytes -= 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = super::solve_face_orientation(&ctx, &mut out) else { panic!("boolean copy peak"); };
    let live = u64::try_from(8 * std::mem::size_of::<bool>()).unwrap();
    assert_eq!(limit.operation, "assign Parasolid face senses");
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((limit.used, limit.additional, limit.limit), (orientation_peak() - live, live, orientation_peak() - 1));
    assert_eq!(out.faces[0].sense, original);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn refused_face_sense_application_preserves_the_original_fuse_and_value() {
    let mut out = one_face();
    let arena = DecodeArena::new();
    let mut policy = orientation_policy();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let senses = FaceSenses::new(&ctx, &out).unwrap();
    out.faces[0].sense = Sense::Forward;
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "test original face sense refusal", None);
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test original face sense refusal") else { panic!("original refusal"); };
    drop(probe);
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| senses.apply(&ctx, &mut out.faces));
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert_eq!(out.faces[0].sense, Sense::Forward);
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn empty_orientation_build_preserves_prior_refusal_before_allocation() {
    let out = Brep::default();
    let arena = DecodeArena::new();
    let mut policy = orientation_policy();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_collection_items(1, "test original orientation refusal") else { panic!("original refusal"); };
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| FaceSenses::new(&ctx, &out));
    assert!(matches!(&result, Err(CodecError::ResourceLimit(actual)) if *actual == limit));
    drop(result);
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
