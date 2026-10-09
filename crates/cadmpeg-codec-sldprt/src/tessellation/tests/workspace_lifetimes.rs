// SPDX-License-Identifier: Apache-2.0
//! Local indexes and selection backing expire before their callers' output.

use super::{add_square_face, model_with_body, persistent_identity, persistent_mesh, set_shell_faces};
use crate::brep::PersistentFaceIdentity;
use crate::tessellation::{assign_persistent_owners, assign_unique_surface_owners, shell_bodies,
    PersistentFaceBinding, SurfaceCandidate};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::{BodyId, FaceId, ShellId};
use cadmpeg_ir::transform::Transform;

// Core's one-entry SwissTable bound: four buckets, alignment padding,
// four control bytes and sixteen trailing control bytes.
fn one_entry_table<T>() -> u64 {
    u64::try_from(4 * std::mem::size_of::<T>()
        + std::mem::align_of::<T>().max(16) - 1 + 4 + 16).unwrap()
}

fn pointer_table() -> u64 {
    one_entry_table::<(&ShellId, &BodyId)>()
}

fn policy(bytes: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = bytes;
    policy.limits.max_retained_bytes = 0;
    policy
}

fn shell_model() -> Model {
    let mut model = model_with_body();
    let face = add_square_face(&mut model, "shell-workspace", 0.);
    set_shell_faces(&mut model, vec![face]);
    model
}

#[test]
fn shell_body_lookup_keeps_only_the_returned_table_live() {
    let model = shell_model();
    let table = pointer_table();
    let peak = 2 * table;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(peak)).unwrap();
    let mut output_storage = ctx.reserve_scoped(0, "test shell table owner").unwrap();
    let bodies = output_storage.with_storage(|| shell_bodies(&ctx, &model.regions, &model.shells)).unwrap();
    assert_eq!(bodies.get(&model.shells[0].id), Some(&&model.bodies[0].id));
    // Region backing has died; exactly the returned shell table remains.
    drop(ctx.reserve_scoped(table, "test region table expiry").unwrap());
    drop(bodies);
    drop(output_storage);
    drop(ctx.reserve_scoped(peak, "test returned shell table expiry").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn shell_body_lookup_refuses_the_second_table_before_allocation() {
    let model = shell_model();
    let table = pointer_table();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(2 * table - 1)).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test shell table refusal").unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(||
        storage.with_storage(|| shell_bodies(&ctx, &model.regions, &model.shells)));
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("second table refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "index SLDPRT tessellation shells");
    assert_eq!((limit.used, limit.additional, limit.limit), (table, table, 2 * table - 1));
    assert_eq!(allocations, 1); // Only the admitted region table.
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn missing_region_discards_only_the_local_region_table() {
    let mut model = shell_model();
    model.shells[0].region = cadmpeg_ir::ids::RegionId::mint("synthetic:test:region#missing").unwrap();
    let table = pointer_table();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(table)).unwrap();
    let bodies = shell_bodies(&ctx, &model.regions, &model.shells).unwrap();
    assert!(bodies.is_empty());
    drop(ctx.reserve_scoped(table, "test missing region expiry").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn shell_body_lookup_preserves_an_original_refusal_without_allocation() {
    let model = shell_model();
    let mut policy = policy(2 * pointer_table());
    policy.limits.max_collection_items = 2;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = ctx.charge_collection_items(3, "test original shell refusal")
    else { panic!("original refusal"); };
    let (result, allocations) = crate::test_support::allocation::count_allocations(||
        shell_bodies(&ctx, &model.regions, &model.shells));
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == first));
    assert_eq!(allocations, 0);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == first));
}

fn assignment_model() -> Model {
    let mut model = model_with_body();
    let face = add_square_face(&mut model, "local-workspace", 0.);
    set_shell_faces(&mut model, vec![face]);
    model.tessellations.push(persistent_mesh("synthetic:test:tessellation#local-workspace"));
    model
}

fn persistent_inputs(model: &Model) -> (Vec<(FaceId, PersistentFaceIdentity)>, Vec<PersistentFaceBinding>) {
    (vec![(model.faces[0].id.clone(), persistent_identity(7, 3, &[]))],
        vec![PersistentFaceBinding {
            tessellation: model.tessellations[0].id.as_str().into(),
            identity: persistent_identity(7, 3, &[]),
        }])
}

fn persistent_index_peak() -> u64 {
    one_entry_table::<(&PersistentFaceIdentity, Option<&FaceId>)>()
        + one_entry_table::<(&FaceId, &BodyId)>()
        + one_entry_table::<(&str, Option<&PersistentFaceIdentity>)>()
}

#[test]
fn persistent_assignment_expires_region_and_shell_tables_before_bindings() {
    let mut model = assignment_model();
    let (identities, bindings) = persistent_inputs(&model);
    let peak = persistent_index_peak();
    let mut policy = policy(peak);
    // Actual escaping face slots, identity strings and assigned-ID slots stay retained.
    policy.limits.max_retained_bytes = DecodePolicy::service().limits.max_retained_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let assigned = assign_persistent_owners(&ctx, &mut model, &identities, &bindings).unwrap();
    assert_eq!(assigned, vec![bindings[0].tessellation.as_str()]);
    assert_eq!(model.tessellations[0].faces, vec![model.faces[0].id.clone()]);
    assert_eq!(model.tessellations[0].body, Some(model.bodies[0].id.clone()));
    drop(ctx.reserve_scoped(peak, "test persistent index expiry").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn persistent_assignment_refuses_binding_growth_before_mutating_the_mesh() {
    let mut model = assignment_model();
    let (identities, bindings) = persistent_inputs(&model);
    let peak = persistent_index_peak();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(peak - 1)).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(||
        assign_persistent_owners(&ctx, &mut model, &identities, &bindings));
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("binding table refusal"); };
    let binding_bytes = one_entry_table::<(&str, Option<&PersistentFaceIdentity>)>();
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "index SLDPRT persistent tessellation bindings");
    assert_eq!((limit.used, limit.additional, limit.limit), (peak - binding_bytes, binding_bytes, peak - 1));
    assert_eq!(allocations, 4); // Identity, region, shell and face-body tables.
    assert!(model.tessellations[0].faces.is_empty());
    assert!(model.tessellations[0].body.is_none());
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

fn unbounded_assignment_model() -> Model {
    let mut model = assignment_model();
    // The carrier alone is a valid ownership witness. No trim is asserted.
    model.faces[0].loops = cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new());
    model.loops.clear();
    model.coedges.clear();
    model.edges.clear();
    model.vertices.clear();
    model.points.clear();
    model.curves.clear();
    model
}

fn geometric_index_bytes() -> u64 {
    // One surface, shell/body, transform and candidate; all other indexes empty.
    one_entry_table::<(&cadmpeg_ir::ids::SurfaceId, &SurfaceGeometry)>()
        + pointer_table()
        + one_entry_table::<(&BodyId, Option<Transform>)>()
        + u64::try_from(4 * std::mem::size_of::<SurfaceCandidate<'_>>()).unwrap()
}

fn assignment_output_bytes(model: &Model) -> u64 {
    u64::try_from(4 * std::mem::size_of::<FaceId>()
        + model.faces[0].id.as_str().len() + model.bodies[0].id.as_str().len()
        + model.tessellations[0].id.as_str().len()
        + 4 * std::mem::size_of::<String>()).unwrap()
}

#[test]
fn scalar_owner_selection_expires_before_scoped_output_publication() {
    let model = unbounded_assignment_model();
    let indexes = geometric_index_bytes();
    let output = assignment_output_bytes(&model);
    // Selection needs four usize slots. Publication's final live output is
    // larger, and the scalar result must not carry that dead selection backing.
    assert!(output > u64::try_from(4 * std::mem::size_of::<usize>()).unwrap());
    let peak = indexes + output;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(peak)).unwrap();
    let mut output_storage = ctx.reserve_scoped(0, "test assignment output owner").unwrap();
    // Actual output fields live in model plus assigned; both die before this lease.
    let mut model = model;
    let assigned = output_storage.with_storage(|| assign_unique_surface_owners(&ctx, &mut model)).unwrap();
    assert_eq!(assigned, vec![model.tessellations[0].id.as_str()]);
    assert_eq!(model.tessellations[0].faces, vec![model.faces[0].id.clone()]);
    assert_eq!(model.tessellations[0].body, Some(model.bodies[0].id.clone()));
    drop(ctx.reserve_scoped(indexes, "test geometric index expiry").unwrap());
    drop(assigned);
    drop(model);
    drop(output_storage);
    drop(ctx.reserve_scoped(peak, "test assignment output expiry").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn scoped_owner_publication_refuses_the_final_output_slots_at_need_minus_one() {
    let model = unbounded_assignment_model();
    let peak = geometric_index_bytes() + assignment_output_bytes(&model);
    let slots = u64::try_from(4 * std::mem::size_of::<String>()).unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(peak - 1)).unwrap();
    let mut output_storage = ctx.reserve_scoped(0, "test output slot refusal").unwrap();
    let mut model = model;
    let Err(CodecError::ResourceLimit(limit)) = output_storage.with_storage(||
        assign_unique_surface_owners(&ctx, &mut model)) else { panic!("final slots refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect SLDPRT assigned tessellations");
    assert_eq!((limit.used, limit.additional, limit.limit), (peak - slots, slots, peak - 1));
    assert!(model.tessellations[0].faces.is_empty());
    assert!(model.tessellations[0].body.is_none());
    drop(model);
    drop(output_storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
