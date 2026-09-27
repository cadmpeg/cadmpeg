// SPDX-License-Identifier: Apache-2.0
//! Collection admissions in the STEP PMI reader.

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
