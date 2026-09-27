// SPDX-License-Identifier: Apache-2.0
//! Collection and nesting limits for drawing discovery.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";

fn drawing_refuses(records: &str, operation: &str) {
    let source = format!("{HEADER}{records}{TAIL}");
    drawing_refuses_source(source.as_bytes(), operation);
}

fn drawing_refuses_source(source: &[u8], operation: &str) {
    let (exchange, _) = crate::parse::parse(source).expect("valid drawing exchange");
    let refused = (0..=256).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
            .expect("root fits collection policy");
        matches!(
            super::super::decode(
                &exchange,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &HashSet::new(),
                &BTreeMap::new(),
                &ctx,
            ),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        )
    });
    assert!(refused, "no collection limit refused {operation}");
}

#[test]
fn drawing_candidates_refuse_collection_limit() {
    drawing_refuses("#1=DRAWING_DEFINITION('Main','detail');", "step_drawing_candidates");
}

#[test]
fn drawing_discovery_losses_refuse_collection_limit() {
    drawing_refuses("#1=DRAWING_DEFINITION('Only one');", "step_drawing_losses");
}

#[test]
fn drawing_ids_refuse_collection_limit() {
    drawing_refuses("#1=DRAWING_DEFINITION('Main','detail');", "step_drawing_ids");
}

#[test]
fn hidden_drawing_ids_refuse_collection_limit() {
    drawing_refuses("#1=DRAWING_DEFINITION('Main','detail');#2=INVISIBILITY((#1));", "step_hidden_drawing_ids");
}

#[test]
fn drawing_reference_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits depth policy");
    let value = crate::parse::Value::List(vec![crate::parse::Value::Reference(1)]);
    assert!(matches!(
        super::super::visit_drawing_references(&value, &ctx, &mut |_| Ok(())),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_drawing_reference_walk"
    ));
}

#[test]
fn drawing_target_groups_refuse_collection_limit() {
    drawing_refuses("#1=DRAWING_DEFINITION('Main','detail');", "step_drawing_target_groups");
}

#[test]
fn drawing_target_members_refuse_collection_limit() {
    drawing_refuses("#1=DRAWING_DEFINITION('Main','detail');", "step_drawing_target_members");
}

#[test]
fn drawing_external_documents_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#100=<part.step#root>;ENDSEC;DATA;#1=DRAWING_DEFINITION('Main','detail');ENDSEC;END-ISO-10303-21;";
    drawing_refuses_source(source, "step_drawing_external_documents");
}

#[test]
fn drawing_stored_parameters_refuse_collection_limit() {
    drawing_refuses("#1=DRAWING_DEFINITION('Main','detail');", "step_drawing_stored_parameters");
}

#[test]
fn drawing_entries_refuse_collection_limit() {
    drawing_refuses("#1=DRAWING_DEFINITION('Main','detail');", "step_drawing_entries");
}

#[test]
fn drawing_typed_claims_refuse_collection_limit() {
    drawing_refuses("#1=DRAWING_DEFINITION('Main','detail');", "step_drawing_typed_claims");
}

#[test]
fn drawing_ir_items_refuse_collection_limit() {
    drawing_refuses("#1=DRAWING_DEFINITION('Main','detail');", "step_drawing_ir_items");
}

#[test]
fn drawing_referenced_targets_refuse_collection_limit() {
    drawing_refuses(
        "#1=DRAWING_DEFINITION('Main','detail');#2=DRAWING_REVISION('A',#1,'revision');",
        "step_drawing_referenced_targets",
    );
}
