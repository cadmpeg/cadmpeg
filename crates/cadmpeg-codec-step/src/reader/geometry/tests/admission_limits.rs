// SPDX-License-Identifier: Apache-2.0
//! Collection refusals in the STEP geometry reader.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

macro_rules! map_refusal_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
                .expect("empty root fits policy");
            assert!(matches!(
                super::super::insert_geometry_map(
                    &mut BTreeMap::<u64, u64>::new(),
                    1,
                    1,
                    &ctx,
                    $operation,
                ),
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == $operation
            ));
        }
    };
}

map_refusal_test!(geometry_points_refuse_collection_limit, "step_geometry_points");
map_refusal_test!(geometry_points2_refuse_collection_limit, "step_geometry_points2");
map_refusal_test!(geometry_apll_point_names_refuse_collection_limit, "step_geometry_apll_point_names");
map_refusal_test!(geometry_directions_refuse_collection_limit, "step_geometry_directions");
map_refusal_test!(geometry_directions2_refuse_collection_limit, "step_geometry_directions2");
map_refusal_test!(geometry_vectors_refuse_collection_limit, "step_geometry_vectors");
map_refusal_test!(geometry_vectors2_refuse_collection_limit, "step_geometry_vectors2");
map_refusal_test!(geometry_placements_refuse_collection_limit, "step_geometry_placements");
map_refusal_test!(geometry_placements2_refuse_collection_limit, "step_geometry_placements2");
map_refusal_test!(geometry_transformation_operators_refuse_collection_limit, "step_geometry_transformation_operators");
map_refusal_test!(geometry_transformation_operators2_refuse_collection_limit, "step_geometry_transformation_operators2");
map_refusal_test!(geometry_curve_parameter_offsets_refuse_collection_limit, "step_geometry_curve_parameter_offsets");

macro_rules! hash_refusal_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
                .expect("empty root fits policy");
            assert!(matches!(
                super::super::insert_geometry_hash(
                    &mut HashMap::<u64, u64>::new(),
                    1,
                    1,
                    &ctx,
                    $operation,
                ),
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == $operation
            ));
        }
    };
}

hash_refusal_test!(geometry_curve_index_refuses_collection_limit, "step_geometry_curve_index");
hash_refusal_test!(geometry_surface_index_refuses_collection_limit, "step_geometry_surface_index");

#[test]
fn geometry_typed_ids_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::claim_geometry_typed(&mut HashSet::new(), 1, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_geometry_typed_ids"
    ));
}

#[test]
fn geometry_point_carriers_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::insert_geometry_set(&mut BTreeSet::new(), 1, &ctx, "step_geometry_point_carriers"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_geometry_point_carriers"
    ));
}

#[test]
fn geometry_ir_points_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::push_geometry_vec(&mut Vec::new(), 1u64, &ctx, "step_geometry_ir_points"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_geometry_ir_points"
    ));
}

#[test]
fn geometry_losses_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    let note = crate::loss::StepLossCode::DecodeWarning.note("invalid geometry");
    assert!(matches!(
        super::super::push_geometry_vec(&mut Vec::new(), note, &ctx, "step_geometry_losses"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_geometry_losses"
    ));
}

#[test]
fn uncertainty_values_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        crate::decode_alloc::charged_join(&ctx, "step_uncertainty_values_text", ["0.1", "0.2"], ", "),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_uncertainty_values_text"
    ));
}

#[test]
fn uncertainty_note_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        crate::decode_alloc::charged_format(
            &ctx,
            "step_uncertainty_note_text",
            format_args!("ambiguous uncertainty values ({})", "0.1, 0.2"),
        ),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_uncertainty_note_text"
    ));
}

macro_rules! deferred_ids_refusal_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
                .expect("empty root fits policy");
            assert!(matches!(
                super::super::push_geometry_vec(&mut Vec::new(), 1u64, &ctx, $operation),
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == $operation
            ));
        }
    };
}

deferred_ids_refusal_test!(deferred_curve_ids_refuse_collection_limit, "step_deferred_curve_ids");
deferred_ids_refusal_test!(deferred_surface_ids_refuse_collection_limit, "step_deferred_surface_ids");
deferred_ids_refusal_test!(geometry_ir_curves_refuse_collection_limit, "step_geometry_ir_curves");
deferred_ids_refusal_test!(geometry_ir_surfaces_refuse_collection_limit, "step_geometry_ir_surfaces");
deferred_ids_refusal_test!(geometry_ir_pcurves_refuse_collection_limit, "step_geometry_ir_pcurves");

fn deferred_dependency_refusal(limit: u64, group: &'static str, member: &'static str) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    super::super::defer_geometry_dependency(&mut HashMap::new(), 1, 2, &ctx, group, member)
        .expect_err("dependency exceeds the limit")
}

#[test]
fn deferred_curve_groups_refuse_collection_limit() {
    assert!(matches!(deferred_dependency_refusal(0, "step_deferred_curve_groups", "step_deferred_curve_members"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_curve_groups"));
}

#[test]
fn deferred_curve_members_refuse_collection_limit() {
    assert!(matches!(deferred_dependency_refusal(1, "step_deferred_curve_groups", "step_deferred_curve_members"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_curve_members"));
}

#[test]
fn deferred_surface_groups_refuse_collection_limit() {
    assert!(matches!(deferred_dependency_refusal(0, "step_deferred_surface_groups", "step_deferred_surface_members"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_surface_groups"));
}

#[test]
fn deferred_surface_members_refuse_collection_limit() {
    assert!(matches!(deferred_dependency_refusal(1, "step_deferred_surface_groups", "step_deferred_surface_members"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_surface_members"));
}

fn deferred_wake_refusal(operation: &'static str) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    let mut waiting = HashMap::from([(1, vec![2])]);
    super::super::wake_deferred_dependents(1, &mut waiting, &mut VecDeque::new(), &ctx, operation)
        .expect_err("wake queue exceeds the limit")
}

#[test]
fn deferred_curve_queue_refuses_collection_limit() {
    assert!(matches!(deferred_wake_refusal("step_deferred_curve_queue"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_curve_queue"));
}

#[test]
fn deferred_surface_queue_refuses_collection_limit() {
    assert!(matches!(deferred_wake_refusal("step_deferred_surface_queue"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_surface_queue"));
}

#[test]
fn curve_coordinate_rows_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.)));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid coordinate list");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    assert!(matches!(
        super::super::coordinate_rows(&exchange.records()[&1], 1.0, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_curve_coordinate_rows"
    ));
}

fn strip_refusal(collection_limit: u64) -> CodecError {
    use crate::parse::Value;

    let strips = Value::List(vec![Value::List(vec![Value::Integer(1), Value::Integer(2)])]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    super::super::tessellated_line_strips(Some(&strips), 2, &ctx)
        .expect_err("strip exceeds collection limit")
}

#[test]
fn curve_strip_indices_refuse_collection_limit() {
    assert!(matches!(strip_refusal(0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_curve_strip_indices"));
}

#[test]
fn curve_strips_refuse_collection_limit() {
    assert!(matches!(strip_refusal(2), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_curve_strips"));
}

deferred_ids_refusal_test!(curve_strip_points_refuse_collection_limit, "step_curve_strip_points");

#[test]
fn curve_strip_source_name_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        crate::decode_alloc::charged_format(&ctx, "step_curve_strip_source_name", format_args!("{}", "curve")),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_curve_strip_source_name"
    ));
}

#[test]
fn retained_surface_curve_ids_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    assert!(matches!(
        super::super::insert_geometry_set(&mut BTreeSet::new(), 1, &ctx, "step_retained_surface_curve_ids"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_retained_surface_curve_ids"
    ));
}
