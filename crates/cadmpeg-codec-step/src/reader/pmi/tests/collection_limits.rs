// SPDX-License-Identifier: Apache-2.0
//! Collection admissions in the STEP PMI reader.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::num::NonZeroU32;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";

fn pmi_refuses(records: &str, operation: &str) {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) = crate::parse::parse(source.as_bytes()).expect("valid PMI exchange");
    let arena = DecodeArena::new();
    let (setup_ctx, _) =
        DecodeContext::from_root_bytes(source.as_bytes(), &arena, &DecodePolicy::default())
            .expect("source fits setup policy");
    let mut setup_ir = cadmpeg_ir::document::CadIr::empty();
    let geometry = crate::reader::geometry::decode(&exchange, &mut setup_ir, &setup_ctx)
        .expect("geometry setup");
    let index = crate::reader::index::CarrierIndex::from_ir(&setup_ir, &setup_ctx)
        .expect("carrier setup");
    let topology = crate::reader::topology::decode(&exchange, &mut setup_ir, &index, &setup_ctx)
        .expect("topology setup");
    let refused = (0..=32).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
            .expect("root fits collection policy");
        matches!(
            super::super::decode(
                &exchange,
                &geometry.value,
                &topology.value,
                &mut setup_ir.clone(),
                Some(&ctx),
            ),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        )
    });
    assert!(refused, "no collection limit refused {operation}");
}

#[test]
fn pmi_base_aspects_refuse_collection_limit() {
    pmi_refuses("#1=DATUM('D');", "step_pmi_base_aspects");
}

#[test]
fn pmi_shape_aspects_refuse_collection_limit() {
    pmi_refuses("#1=DATUM('D');", "step_pmi_shape_aspects");
}

#[test]
fn pmi_targeted_aspects_refuse_collection_limit() {
    pmi_refuses("#1=DATUM('D');", "step_pmi_targeted_aspects");
}

#[test]
fn pmi_typed_claims_refuse_collection_limit() {
    pmi_refuses("#1=DATUM('D');", "step_pmi_typed_claims");
}

fn target_refusal(limit: u64) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits collection policy");
    super::super::targets([1, 2], Some(&ctx)).expect_err("two target IDs exceed the limit")
}

#[test]
fn pmi_target_ids_refuse_collection_limit() {
    assert!(matches!(
        target_refusal(0),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_target_ids"
    ));
}

#[test]
fn pmi_target_items_refuse_collection_limit() {
    assert!(matches!(
        target_refusal(1),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_target_items"
    ));
}

fn source_index_refusal(limit: u64, group_operation: &'static str, item_operation: &'static str) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits collection policy");
    let mut groups = BTreeMap::new();
    super::super::push_source_id(
        &mut groups,
        1,
        &1u64,
        Some(&ctx),
        group_operation,
        item_operation,
    )
    .expect_err("source group exceeds the limit")
}

#[test]
fn pmi_point_source_groups_refuse_collection_limit() {
    assert!(matches!(
        source_index_refusal(0, "step_pmi_point_source_groups", "step_pmi_point_source_items"),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_point_source_groups"
    ));
}

#[test]
fn pmi_point_source_items_refuse_collection_limit() {
    assert!(matches!(
        source_index_refusal(1, "step_pmi_point_source_groups", "step_pmi_point_source_items"),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_point_source_items"
    ));
}

#[test]
fn pmi_curve_source_groups_refuse_collection_limit() {
    assert!(matches!(
        source_index_refusal(0, "step_pmi_curve_source_groups", "step_pmi_curve_source_items"),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_curve_source_groups"
    ));
}

#[test]
fn pmi_curve_source_items_refuse_collection_limit() {
    assert!(matches!(
        source_index_refusal(1, "step_pmi_curve_source_groups", "step_pmi_curve_source_items"),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_curve_source_items"
    ));
}

#[test]
fn pmi_presentation_semantic_groups_refuse_collection_limit() {
    assert!(matches!(
        source_index_refusal(
            0,
            "step_pmi_presentation_semantic_groups",
            "step_pmi_presentation_semantic_members",
        ),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_presentation_semantic_groups"
    ));
}

#[test]
fn pmi_presentation_semantic_members_refuse_collection_limit() {
    assert!(matches!(
        source_index_refusal(
            1,
            "step_pmi_presentation_semantic_groups",
            "step_pmi_presentation_semantic_members",
        ),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_presentation_semantic_members"
    ));
}

#[test]
fn pmi_presentation_semantics_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits collection policy");
    assert!(matches!(
        super::super::push_pmi_vec(
            &mut Vec::new(),
            1u64,
            Some(&ctx),
            "step_pmi_presentation_semantics",
        ),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_presentation_semantics"
    ));
}

#[test]
fn pmi_other_datum_target_form_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 10;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits retained policy");
    assert!(matches!(
        super::super::datum_target_form("custom form", Some(&ctx)),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_pmi_datum_target_form_copy"
    ));
}

#[test]
fn pmi_other_dimension_name_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 10;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits retained policy");
    assert!(matches!(
        super::super::dimension_kind("CUSTOM_SIZE", Some(&ctx)),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_pmi_other_dimension_name"
    ));
}

#[test]
fn pmi_modifier_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits retained policy");
    let value = crate::parse::Value::Enumeration("ABC".into());
    assert!(matches!(
        super::super::modifier_values(&value, &mut Vec::new(), Some(&ctx)),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_pmi_modifier_text"
    ));
}

#[test]
fn pmi_modifier_items_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits collection policy");
    let value = crate::parse::Value::Enumeration("ABC".into());
    assert!(matches!(
        super::super::modifier_values(&value, &mut Vec::new(), Some(&ctx)),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_modifier_items"
    ));
}

#[test]
fn pmi_modifier_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits depth policy");
    let value = crate::parse::Value::List(vec![crate::parse::Value::Enumeration("ABC".into())]);
    assert!(matches!(
        super::super::modifier_values(&value, &mut Vec::new(), Some(&ctx)),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_modifier_walk"
    ));
}

#[test]
fn pmi_datum_id_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits depth policy");
    let value = crate::parse::Value::List(vec![crate::parse::Value::Reference(1)]);
    assert!(matches!(
        super::super::visit_datum_ids(&value, Some(&ctx), &mut |_| Ok(())),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_datum_id_walk"
    ));
}

#[test]
fn pmi_datum_modifier_copy_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits retained policy");
    assert!(matches!(
        super::super::clone_pmi_modifiers(&["ABC".into()], Some(&ctx)),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_pmi_datum_modifier_copy"
    ));
}

#[test]
fn pmi_datum_modifier_text_refuses_retained_limit() {
    let source = format!("{HEADER}#1=ITEM();{TAIL}");
    let (exchange, _) = crate::parse::parse(source.as_bytes()).expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits retained policy");
    let mut losses = Vec::new();
    let mut measurements = super::super::MeasureContext {
        length_scale: 1.0,
        angle_scale: 1.0,
        graph_limit: 64,
        losses: &mut losses,
    };
    let value = crate::parse::Value::Enumeration("ABC".into());
    assert!(matches!(
        super::super::modifier_text(
            &value,
            &exchange,
            &mut HashSet::new(),
            &mut measurements,
            Some(&ctx),
        ),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_pmi_datum_modifier_text"
    ));
}

fn datum_reference_refuses(records: &str, operation: &str) {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) = crate::parse::parse(source.as_bytes()).expect("valid datum exchange");
    let mut annotations = super::super::Annotations::default();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    annotations
        .push(
            None,
            &mut ir,
            2,
            None,
            Vec::new(),
            None,
            cadmpeg_ir::pmi::PmiDefinition::Datum {
                identification: "A".into(),
            },
        )
        .expect("datum annotation setup");
    let refused = (0..=8).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits collection policy");
        let mut losses = Vec::new();
        let mut measurements = super::super::MeasureContext {
            length_scale: 1.0,
            angle_scale: 1.0,
            graph_limit: 64,
            losses: &mut losses,
        };
        matches!(
            super::super::datum_references_for_compartment(
                &crate::parse::Value::Reference(1),
                NonZeroU32::new(1).expect("positive precedence"),
                &exchange,
                &annotations,
                &mut HashSet::new(),
                &mut measurements,
                Some(&ctx),
            ),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        )
    });
    assert!(refused, "datum reference never refused {operation}");
}

#[test]
fn pmi_datum_reference_items_refuse_collection_limit() {
    datum_reference_refuses(
        "#1=DATUM_REFERENCE_COMPARTMENT($,$,$,$,#2,$);#2=DATUM('A');",
        "step_pmi_datum_reference_items",
    );
}

#[test]
fn pmi_datum_modifier_items_refuse_collection_limit() {
    datum_reference_refuses(
        "#1=DATUM_REFERENCE_COMPARTMENT($,$,$,$,#2,(.ABC.));#2=DATUM('A');",
        "step_pmi_datum_modifier_items",
    );
}

#[test]
fn pmi_datum_system_references_refuse_collection_limit() {
    pmi_refuses(
        "#1=DATUM('A');#2=DATUM_REFERENCE_COMPARTMENT($,$,$,$,#1,$);#3=DATUM_SYSTEM('S','',#4,.F.,(#2));#4=ITEM();",
        "step_pmi_datum_system_references",
    );
}
