// SPDX-License-Identifier: Apache-2.0

use crate::reader::RecordExt;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

/// A successful partial lookup pays for each visited partial.
#[test]
fn partial_record_search_charges_each_visited_partial() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(ALPHA() BETA());ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner).unwrap();
    let record = exchange.records().get(&1).unwrap();
    let lookup = |limit: u64, name: &'static str| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).unwrap();
        record
            .partial(&ctx, name)
            .map(|partial| partial.map(|partial| partial.name.clone()))
    };
    for (name, visits, found) in [("ALPHA", 1, true), ("BETA", 2, true)] {
        let Err(CodecError::ResourceLimit(refusal)) = lookup(visits - 1, name) else {
            panic!("{name} needs {visits} visits");
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "STEP partial record search");
        assert_eq!(
            lookup(visits, name).unwrap(),
            found.then(|| name.to_owned())
        );
    }
    assert_eq!(lookup(256, "GAMMA").unwrap(), None);
}

#[test]
fn named_parameter_rejects_long_unrelated_names_with_bounded_comparison() {
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=({}() LINE(1));ENDSEC;END-ISO-10303-21;", "A".repeat(8193));
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    assert!(matches!(super::super::named_parameter(&ctx, &exchange.records()[&1], "LINE", 0).unwrap(), Some(crate::parse::Value::Integer(1))));
    ctx.finish_session().unwrap();
}

#[test]
fn identity_suffix_search_does_not_bill_unvisited_kind_or_tail() {
    let kind = format!("step:data:{}#7", "a".repeat(8193));
    let member = format!("{kind}-member");
    let tail = format!("step:data:point#7-{}", "a".repeat(8193));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 16;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    assert_eq!(super::super::step_instance_id(&ctx, &kind).unwrap(), Some(7));
    assert_eq!(super::super::source_record_id(&ctx, &member).unwrap(), Some(7));
    assert_eq!(super::super::source_numeric_id(&ctx, &tail, "point").unwrap(), Some(7));
    ctx.finish_session().unwrap();
}
