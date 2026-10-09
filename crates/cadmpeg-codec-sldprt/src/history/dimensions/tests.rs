// SPDX-License-Identifier: Apache-2.0
//! Ordinary repeated Dimension grammar and exact surviving output backing.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;

use super::{DimensionCandidates, Properties};
use crate::history::attributes::BorrowedProperties;

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;
const REPLACED: &str = r#"<Feature><Dimension Name="D1" Unit="old">123456789</Dimension><Dimension Name="D1" Unit="mm">2</Dimension></Feature>"#;

fn node_bytes<K, V>() -> usize {
    let alignment = std::mem::align_of::<K>().max(std::mem::align_of::<V>()).max(std::mem::align_of::<usize>());
    11 * (std::mem::size_of::<K>() + std::mem::size_of::<V>()) + 16 * std::mem::size_of::<usize>() + 2 * alignment
}

fn output_bytes() -> u64 {
    // One outer property node, one Unit/mm node and one D1/2 parameter node.
    // Both distinct output maps own their D1 key; no earlier text survives.
    u64::try_from(node_bytes::<String, Properties>() + 2 * node_bytes::<NonBlankString, String>()
        + 2 * "D1".len() + "Unit".len() + "mm".len() + "2".len()).unwrap()
}

fn collect<'xml, 'ctx, 'arena>(ctx: &'ctx DecodeContext<'arena>, document: &'xml roxmltree::Document<'_>) -> DimensionCandidates<'xml, 'ctx, 'arena> {
    // Parsed fixture nodes are input to this owner. Production reaches the
    // same source grammar through the context's admitted XML document.
    let mut candidates = DimensionCandidates::new(ctx).unwrap();
    let mut losses = Vec::new();
    for node in document.root_element().children().filter(roxmltree::Node::is_element) {
        candidates.record(&mut losses, node, node.attribute("Name").unwrap()).unwrap();
    }
    assert!(losses.is_empty());
    candidates
}

#[test]
fn repeated_dimension_publishes_only_the_last_nonempty_property_and_value() {
    let document = roxmltree::Document::parse(REPLACED).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = output_bytes();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (properties, parameters) = collect(&ctx, &document).into_retained().unwrap();
    assert_eq!(properties.len(), 1);
    assert_eq!(properties["D1"].len(), 1);
    assert_eq!(properties["D1"]["Unit"], "mm");
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters["D1"], "2");
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test dimension source teardown").unwrap());
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(1, "test dimension survivor bytes") else { panic!("exact output boundary"); };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((limit.used, limit.additional), (output_bytes(), 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn repeated_dimension_one_below_survivor_storage_preserves_the_final_node_refusal() {
    let document = roxmltree::Document::parse(REPLACED).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = output_bytes() - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = collect(&ctx, &document).into_retained() else { panic!("final parameter node refused"); };
    let node = u64::try_from(node_bytes::<NonBlankString, String>()).unwrap();
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "index SLDPRT parameters");
    assert_eq!((limit.used, limit.additional), (output_bytes() - node, node));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn repeated_dimension_replacement_releases_the_previous_property_shell() {
    let document = roxmltree::Document::parse(REPLACED).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let candidates = collect(&ctx, &document);
    let live = u64::try_from(node_bytes::<&str, BorrowedProperties<'_, '_>>() + 2 * node_bytes::<&str, &str>()).unwrap();
    let additional = SCRATCH_BYTES - live + 1;
    let Err(CodecError::ResourceLimit(limit)) = ctx.reserve_scoped(additional, "test live dimension source") else { panic!("exact surviving source shells"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((limit.used, limit.additional), (live, additional));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(candidates);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn unpublished_dimension_drop_releases_all_source_backing_with_zero_retained_bytes() {
    let document = roxmltree::Document::parse(REPLACED).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    drop(collect(&ctx, &document));
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test complete dimension candidate teardown").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn later_empty_properties_keep_the_previous_properties_and_replace_with_empty_text() {
    let document = roxmltree::Document::parse(r#"<Feature><Dimension Name="D1" Unit="mm">1</Dimension><Dimension Name="D1"/></Feature>"#).unwrap();
    let ctx = cadmpeg_test_support::service_decode_context();
    let (properties, parameters) = collect(&ctx, &document).into_retained().unwrap();
    assert_eq!(properties.len(), 1);
    assert_eq!(properties["D1"].len(), 1);
    assert_eq!(properties["D1"]["Unit"], "mm");
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters["D1"], "");
}

#[test]
fn blank_dimension_name_keeps_properties_without_a_parameter() {
    let document = roxmltree::Document::parse(r#"<Feature><Dimension Name=" " Unit="mm">1</Dimension></Feature>"#).unwrap();
    let ctx = cadmpeg_test_support::service_decode_context();
    let (properties, parameters) = collect(&ctx, &document).into_retained().unwrap();
    assert_eq!(properties.len(), 1);
    assert_eq!(properties[" "]["Unit"], "mm");
    assert!(parameters.is_empty());
}

#[test]
fn dimension_source_preserves_the_original_refusal_before_any_new_allocation() {
    let document = roxmltree::Document::parse(REPLACED).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut candidates = DimensionCandidates::new(&ctx).unwrap();
    let node = document.root_element().first_element_child().unwrap();
    let name = node.attribute("Name").unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test original dimension refusal") else { panic!("original refusal"); };
    let mut losses = Vec::new();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| candidates.record(&mut losses, node, name));
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert_eq!(allocations, 0);
    assert!(losses.is_empty());
    assert!(candidates.properties.is_empty());
    assert!(candidates.parameters.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(candidates);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
