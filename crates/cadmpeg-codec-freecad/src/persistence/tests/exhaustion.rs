// SPDX-License-Identifier: Apache-2.0
//! Exact attribute visits and empty value populations.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn empty_link_alias_attributes_need_no_work() {
    let xml = roxmltree::Document::parse("<Link/>").unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::reject_link_aliases(xml.root_element(), &["obj"], &ctx).unwrap();
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn link_alias_exhaustion_costs_only_actual_attribute_visits() {
    let xml = roxmltree::Document::parse("<Link obj=\"A\"/>").unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::reject_link_aliases(xml.root_element(), &["obj"], &ctx).unwrap();
    assert_eq!(ctx.resource_refusal(), None);
    let CodecError::ResourceLimit(limit) = ctx.charge_work(1, "after attribute scan").unwrap_err()
    else {
        panic!("one actual attribute exhausts work")
    };
    assert_eq!(limit.used, 1);
}

#[test]
fn first_link_alias_error_does_not_visit_later_attributes() {
    let mut source = "<Link object=\"A\"".to_owned();
    for index in 0..1024 {
        source.push_str(&format!(" z{index}=\"unused\""));
    }
    source.push_str("/>");
    let xml = roxmltree::Document::parse(&source).unwrap();
    let message = "Link has unsupported link carrier object";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One attribute visit and the two admitted diagnostic formatting passes.
    policy.limits.max_work_units = 1 + 2 * cadmpeg_core::decode::u64_from_index(message.len());
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(super::super::reject_link_aliases(xml.root_element(), &["obj"], &ctx),
        Err(CodecError::Malformed(actual)) if actual == message)
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn empty_value_attributes_are_not_advanced_and_keep_retained_value() {
    let source = "<Properties Count=\"1\"><Property name=\"Label\" type=\"App::PropertyString\"><String/></Property></Properties>";
    let xml = roxmltree::Document::parse(source).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "FCStd value attribute records",
        None,
    );
    let mut records = Vec::new();
    super::super::parse_properties(
        source,
        xml.root_element(),
        "fcstd:native:object#Object",
        &mut records,
        &ctx,
    )
    .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].name, "Label");
    assert_eq!(records[0].values().len(), 1);
    assert_eq!(records[0].values()[0].tag, "String");
    assert_eq!(records[0].values()[0].raw_xml, "<String/>");
    assert!(records[0].values()[0].attributes.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
}
