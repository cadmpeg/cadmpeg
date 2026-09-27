// SPDX-License-Identifier: Apache-2.0
//! Collection refusals in the STEP geometry reader.

use std::collections::{BTreeMap, BTreeSet, HashSet};

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
