// SPDX-License-Identifier: Apache-2.0
//! Product and occurrence resource boundaries.

use crate::reader::product;

const DRAWING_OWNED_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DRAUGHTING_MODEL('',(#2),$);#2=REPRESENTATION('',(#3),$);#3=MAPPED_ITEM();ENDSEC;END-ISO-10303-21;";

fn drawing_owned_refuses(operation: &str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, _) =
        crate::test_support::with_service_context(DRAWING_OWNED_SOURCE, crate::parse::parse_inner)
            .expect("valid drawing owned source");
    let refused = (0..=16).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(DRAWING_OWNED_SOURCE, &arena, &policy)
            .expect("root fits selected policy");
        matches!(
            product::drawing_owned_items(&exchange, &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        )
    });
    assert!(refused, "no collection limit refused {operation}");
}

#[test]
fn drawing_owned_pending_refuses_collection_limit() {
    drawing_owned_refuses("step_drawing_owned_pending");
}

#[test]
fn drawing_owned_visited_refuses_collection_limit() {
    drawing_owned_refuses("step_drawing_owned_visited");
}

#[test]
fn drawing_owned_items_refuse_collection_limit() {
    drawing_owned_refuses("step_drawing_owned_items");
}

#[test]
fn drawing_reference_walk_refuses_depth_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, _) =
        crate::test_support::with_service_context(DRAWING_OWNED_SOURCE, crate::parse::parse_inner)
            .expect("valid drawing owned source");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(DRAWING_OWNED_SOURCE, &arena, &policy)
        .expect("root fits selected policy");
    assert!(matches!(
        product::drawing_owned_items(&exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_drawing_reference_walk"
    ));
}

#[test]
fn pending_occurrence_refuses_caller_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(b"occurrence", &arena, &policy)
            .expect("root fits selected policy");
    let id =
        cadmpeg_ir::ids::OccurrenceId::mint("step:data:occurrence#1").expect("valid occurrence id");
    let mut pending = std::collections::VecDeque::new();
    let error = ctx
        .push_back(&mut pending, (1, id), "step_pending_occurrence")
        .expect_err("one pending occurrence exceeds zero collection items");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "step_pending_occurrence"
    ));
    assert!(pending.is_empty());
}

pub(super) const PRODUCT_STRING_LIMIT_SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=APPLICATION_CONTEXT('mechanical design');#2=PRODUCT_CONTEXT('',#1,'mechanical');#3=PRODUCT('P','Part name','',(#2));#4=PRODUCT_DEFINITION_FORMATION('','',#3);#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');#6=PRODUCT_DEFINITION('part','Description',#4,#5);#7=PRODUCT('C','Child name','',(#2));#8=PRODUCT_DEFINITION_FORMATION('','',#7);#9=PRODUCT_DEFINITION('child','',#8,#5);#10=NEXT_ASSEMBLY_USAGE_OCCURRENCE('u','Child instance','',#6,#9,$);ENDSEC;END-ISO-10303-21;";

pub(super) fn product_collection_refuses_source(source: &[u8], operation: &str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid product exchange");
    let refused = (0..=1024).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits collection policy");
        matches!(
            crate::reader::decode_exchange(
                source,
                exchange.clone(),
                &diagnostics,
                &ctx,
                crate::reader::Packaging::Bare,
            ),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        )
    });
    assert!(refused, "no collection limit refused {operation}");
}

pub(super) fn product_collection_refuses(operation: &str) {
    product_collection_refuses_source(PRODUCT_STRING_LIMIT_SOURCE, operation);
}

#[test]
fn product_formation_map_refuses_collection_limit() {
    product_collection_refuses("step_product_formations");
}

#[test]
fn product_definition_map_refuses_collection_limit() {
    product_collection_refuses("step_product_definitions");
}

#[test]
fn product_definition_group_map_refuses_collection_limit() {
    product_collection_refuses("step_product_definition_groups");
}

#[test]
fn product_definition_group_members_refuse_collection_limit() {
    product_collection_refuses("step_product_definition_group_members");
}

#[test]
fn product_child_definitions_refuse_collection_limit() {
    product_collection_refuses("step_product_child_definitions");
}

#[test]
fn product_usage_parent_groups_refuse_collection_limit() {
    product_collection_refuses("step_product_usage_parent_groups");
}

#[test]
fn product_usage_parent_members_refuse_collection_limit() {
    product_collection_refuses("step_product_usage_parent_members");
}

fn product_representation_collection_refuses(operation: &str) {
    let source = String::from_utf8_lossy(PRODUCT_STRING_LIMIT_SOURCE).replace(
        "ENDSEC;END-ISO-10303-21;",
        "#11=PRODUCT_DEFINITION_SHAPE('','',#9);#12=SHAPE_DEFINITION_REPRESENTATION(#11,#13);#13=SHAPE_REPRESENTATION('',(),$);ENDSEC;END-ISO-10303-21;",
    );
    product_collection_refuses_source(source.as_bytes(), operation);
}

#[test]
fn shape_binding_shapes_refuse_collection_limit() {
    product_representation_collection_refuses("step_shape_binding_shapes");
}

#[test]
fn body_placement_shapes_refuse_collection_limit() {
    product_representation_collection_refuses("step_body_placement_shapes");
}

#[test]
fn occurrence_placement_shapes_refuse_collection_limit() {
    product_representation_collection_refuses("step_occurrence_placement_shapes");
}

#[test]
fn definition_representation_groups_refuse_collection_limit() {
    product_representation_collection_refuses("step_definition_representation_groups");
}

#[test]
fn definition_representation_members_refuse_collection_limit() {
    product_representation_collection_refuses("step_definition_representation_members");
}

#[test]
fn assembly_representations_refuse_collection_limit() {
    product_representation_collection_refuses("step_assembly_representations");
}

#[test]
fn represented_definition_groups_refuse_collection_limit() {
    product_representation_collection_refuses("step_represented_definition_groups");
}

#[test]
fn represented_definition_members_refuse_collection_limit() {
    product_representation_collection_refuses("step_represented_definition_members");
}

#[test]
fn body_placement_indices_refuse_collection_limit() {
    product_collection_refuses_source(
        include_bytes!("../../../../tests/fixtures/ap214_sheet.p21"),
        "step_body_placement_indices",
    );
}

#[test]
fn product_definition_ir_items_refuse_collection_limit() {
    product_collection_refuses("step_product_definition_ir_items");
}

#[test]
fn product_source_groups_refuse_collection_limit() {
    product_collection_refuses("step_product_source_groups");
}

#[test]
fn product_source_group_members_refuse_collection_limit() {
    product_collection_refuses("step_product_source_group_members");
}

#[test]
fn product_losses_refuse_collection_limit() {
    product_collection_refuses("step_product_losses");
}

fn shape_binding_collection_refuses(operation: &str) {
    let source = String::from_utf8_lossy(include_bytes!(
        "../../../../tests/fixtures/ap214_sheet.p21"
    ))
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=APPLICATION_CONTEXT('mechanical design');\n#81=PRODUCT_CONTEXT('',#80,'mechanical');\n#82=PRODUCT('P','Shape part','',(#81));\n#83=PRODUCT_DEFINITION_FORMATION('','',#82);\n#84=PRODUCT_DEFINITION_CONTEXT('part definition',#80,'design');\n#85=PRODUCT_DEFINITION('part','',#83,#84);\n#86=PRODUCT_DEFINITION_SHAPE('','',#85);\n#87=SHAPE_DEFINITION_REPRESENTATION(#86,#32);\nENDSEC;\nEND-ISO-10303-21;",
    );
    product_collection_refuses_source(source.as_bytes(), operation);
}

#[test]
fn shape_binding_groups_refuse_collection_limit() {
    shape_binding_collection_refuses("step_shape_binding_groups");
}

#[test]
fn shape_binding_bodies_refuse_collection_limit() {
    shape_binding_collection_refuses("step_shape_binding_bodies");
}

pub(super) fn mapped_body_placement_source() -> String {
    String::from_utf8_lossy(include_bytes!(
        "../../../../tests/fixtures/ap214_sheet.p21"
    ))
    .replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#70=CARTESIAN_POINT('',(20.,0.,0.));\n#71=CARTESIAN_POINT('',(40.,0.,0.));\n#72=AXIS2_PLACEMENT_3D('',#70,#9,#10);\n#73=AXIS2_PLACEMENT_3D('',#71,#9,#10);\n#74=REPRESENTATION_MAP(#27,#32);\n#75=MAPPED_ITEM('first',#74,#72);\n#76=MAPPED_ITEM('second',#74,#73);\nENDSEC;\nEND-ISO-10303-21;",
    )
}

fn mapped_body_placement_collection_refuses(operation: &str) {
    let source = mapped_body_placement_source();
    product_collection_refuses_source(source.as_bytes(), operation);
}

#[test]
fn body_placement_groups_refuse_collection_limit() {
    mapped_body_placement_collection_refuses("step_body_placement_groups");
}

#[test]
fn body_placement_group_members_refuse_collection_limit() {
    mapped_body_placement_collection_refuses("step_body_placement_group_members");
}

#[test]
fn unique_body_placements_refuse_collection_limit() {
    mapped_body_placement_collection_refuses("step_unique_body_placements");
}

#[test]
fn product_definition_descriptions_refuse_collection_limit() {
    product_collection_refuses("step_product_definition_descriptions");
}

#[test]
fn product_definition_prototypes_refuse_collection_limit() {
    product_collection_refuses("step_product_definition_prototypes");
}

#[test]
fn product_shape_prototypes_refuse_collection_limit() {
    let source = String::from_utf8_lossy(PRODUCT_STRING_LIMIT_SOURCE).replace(
        "ENDSEC;END-ISO-10303-21;",
        "#11=PRODUCT_DEFINITION_SHAPE('','',#6);ENDSEC;END-ISO-10303-21;",
    );
    product_collection_refuses_source(source.as_bytes(), "step_product_shape_prototypes");
}

#[test]
fn product_typed_claims_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits collection policy");
    assert!(matches!(
        ctx.insert_hash_set(&mut std::collections::HashSet::new(), 1, "step_product_typed_claims").map(|_| ()),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_product_typed_claims"
    ));
}

#[test]
fn product_string_text_refuses_materialized_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, diagnostics) = crate::test_support::with_service_context(
        PRODUCT_STRING_LIMIT_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid product exchange");
    let arena = DecodeArena::new();
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "step_string_text",
            |limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = limit;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(PRODUCT_STRING_LIMIT_SOURCE, &arena, &policy)
                        .expect("root fits materialized policy");

                (crate::reader::decode_exchange(
                    PRODUCT_STRING_LIMIT_SOURCE,
                    exchange.clone(),
                    &diagnostics,
                    &ctx,
                    crate::reader::Packaging::Bare,
                ))
                .map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::MaterializedBytes
                    && refusal.operation == "step_string_text")
    };
    assert!(refused, "no materialized limit refused a product string");
}

pub(super) fn product_retained_refuses_source(source: &[u8], operation: &str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid product exchange");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        operation,
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).expect("root");
            crate::reader::decode_exchange(
                source,
                exchange.clone(),
                &diagnostics,
                &ctx,
                crate::reader::Packaging::Bare,
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RetainedBytes && refusal.operation == operation));
}

pub(super) fn product_text_work_refuses_source(source: &[u8], operation: &str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid product exchange");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).expect("root");
            crate::reader::decode_exchange(
                source,
                exchange.clone(),
                &diagnostics,
                &ctx,
                crate::reader::Packaging::Bare,
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::WorkUnits && refusal.operation == operation));
}

fn product_copy_refuses_retained_limit(operation: &str) {
    let source = String::from_utf8_lossy(PRODUCT_STRING_LIMIT_SOURCE).replace(
        "PRODUCT('P','Part name',''",
        "PRODUCT('P','Part name','Summary'",
    );
    product_retained_refuses_source(source.as_bytes(), operation);
}

#[test]
fn product_description_copy_refuses_retained_limit() {
    product_copy_refuses_retained_limit("step_product_description_copy");
}

#[test]
fn product_source_name_copy_refuses_retained_limit() {
    product_copy_refuses_retained_limit("step_product_source_name_copy");
}

#[test]
fn product_label_copy_refuses_retained_limit() {
    product_copy_refuses_retained_limit("step_product_label_copy");
}

#[test]
fn product_part_number_copy_refuses_retained_limit() {
    product_copy_refuses_retained_limit("step_product_part_number_copy");
}

#[test]
fn product_occurrence_name_copy_refuses_retained_limit() {
    product_copy_refuses_retained_limit("step_product_occurrence_name_copy");
}

#[test]
fn product_usage_entries_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let (exchange, diagnostics) = crate::test_support::with_service_context(
        PRODUCT_STRING_LIMIT_SOURCE,
        crate::parse::parse_inner,
    )
    .expect("valid product exchange");
    let arena = DecodeArena::new();
    let refused = (0..512).any(|limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(PRODUCT_STRING_LIMIT_SOURCE, &arena, &policy)
            .expect("root fits collection policy");
        matches!(
            crate::reader::decode_exchange(
                PRODUCT_STRING_LIMIT_SOURCE,
                exchange.clone(),
                &diagnostics,
                &ctx,
                crate::reader::Packaging::Bare,
            ),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "step_product_usage_entries"
        )
    });
    assert!(refused, "no collection limit refused product usage entries");
}

#[test]
fn standalone_mapped_body_resolution_retains_no_scratch_nodes() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::BTreeMap;
    let source = mapped_body_placement_source().replace("#76=MAPPED_ITEM('second',#74,#73);", "");
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).expect("mapped body exchange");
    let arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).expect("setup");
    let mut ir = cadmpeg_ir::CadIr::empty();
    let geometry = crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("geometry");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carriers");
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup).expect("topology");
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut losses = Vec::new();
        let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
        product::apply_body_placements(&exchange, product::BodyPlacementSources { geometry: &geometry.value, topology: &topology.value, usages: &BTreeMap::new() }, &mut ir, (&mut losses, &reports), ctx).expect("resolver nodes are scratch");
        assert!(losses.is_empty());
        assert_eq!(ir.model.bodies.len(), 1);
        assert_eq!(ir.model.bodies[0].transform.expect("mapped transform").rows()[0][3], 20.0);
    });
}
