// SPDX-License-Identifier: Apache-2.0
//! Work admission for rejected topology prefixes.

use std::collections::BTreeMap;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn completed_work(ctx: &DecodeContext<'_>) -> u64 {
    let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "measure topology prefix work")
        .expect_err("work counter probe") else {
        panic!("work probe resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

fn root_rejection_work(tail: usize, first: u64) -> u64 {
    let references = std::iter::once(format!("$,#{first}"))
        .chain(std::iter::repeat_n("#2".to_owned(), tail))
        .collect::<Vec<_>>().join(",");
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#999=<outside.step#root>;ENDSEC;DATA;#1=FACE_BASED_SURFACE_MODEL('',({references}));#2=DUMMY();ENDSEC;END-ISO-10303-21;");
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("valid root references");
    assert_eq!(exchange.records().contains_key(&first), first == 2);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let result = super::super::root_shell_steps(exchange.records().get(&1).expect("root"),
        &exchange, &BTreeMap::new(), &ctx).expect("reject invalid set without budget refusal");
    assert!(result.is_none());
    assert_eq!(ctx.resource_refusal(), None);
    completed_work(&ctx)
}

#[test]
fn missing_face_set_does_not_charge_the_abandoned_reference_tail() {
    // Root keyword probes, two raw steps ($ and the first reference), and one record lookup.
    assert_eq!(root_rejection_work(0, 999), root_rejection_work(1024, 999));
}

#[test]
fn wrong_type_face_set_does_not_charge_the_abandoned_reference_tail() {
    // The same prefix plus the first set's type probes; no later reference is visited.
    assert_eq!(root_rejection_work(0, 2), root_rejection_work(1024, 2));
}
