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
    let references = std::iter::once(format!("#{first}"))
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
    // Reference-list validation visits the tail once; carrier traversal visits only the first reference.
    assert_eq!(root_rejection_work(0, 999) + 1024, root_rejection_work(1024, 999));
}

#[test]
fn wrong_type_face_set_does_not_charge_the_abandoned_reference_tail() {
    // List validation costs one unit per tail item; set resolution stops after the first type probes.
    assert_eq!(root_rejection_work(0, 2) + 1024, root_rejection_work(1024, 2));
}

fn rejected_wire_work(tail: usize, shell: bool, nested: bool) -> u64 {
    let references = std::iter::once("#999".to_owned())
        .chain(std::iter::repeat_n("#999".to_owned(), tail))
        .collect::<Vec<_>>().join(",");
    let records = if shell {
        if nested {
            format!("#1=WIRE_SHELL('',(#2));#2=EDGE_LOOP('',({references}));")
        } else {
            format!("#1=WIRE_SHELL('',({references}));#2=DUMMY();")
        }
    } else {
        format!("#1=CONNECTED_EDGE_SET('',({references}));#2=DUMMY();")
    };
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#999=<outside.step#root>;ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("valid wire source");
    let ctx = cadmpeg_test_support::service_decode_context();
    let ir = cadmpeg_ir::CadIr::empty();
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &ctx).expect("empty carriers");
    let sources = super::super::WireSources {
        vdefs: &BTreeMap::new(), edefs: &BTreeMap::new(), point_positions: &carriers,
    };
    let mut storage = ctx.reserve_scoped(0, "test wire losses").expect("empty losses");
    let mut losses = Vec::new();
    let result = if shell {
        super::super::build_shell_wire_set(3, 1, &exchange, sources,
            super::super::WireScope { scoped: false, root: false },
            (&mut losses, &mut storage), &ctx)
    } else {
        super::super::build_wire_set(3, 1, &exchange, sources, false,
            (&mut losses, &mut storage), &ctx)
    }.expect("reject missing wire carrier without budget refusal");
    assert!(result.is_none());
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    completed_work(&ctx)
}

#[test]
fn missing_wire_edge_does_not_charge_the_abandoned_reference_tail() {
    // List validation reads each tail item once; carrier resolution reads none of them.
    assert_eq!(rejected_wire_work(0, false, false) + 1024, rejected_wire_work(1024, false, false));
}

#[test]
fn missing_wire_loop_does_not_charge_the_abandoned_reference_tail() {
    // List validation reads each tail item once; carrier resolution reads none of them.
    assert_eq!(rejected_wire_work(0, true, false) + 1024, rejected_wire_work(1024, true, false));
}

#[test]
fn missing_oriented_wire_edge_does_not_charge_the_abandoned_reference_tail() {
    // List validation reads each tail item once; carrier resolution reads none of them.
    assert_eq!(rejected_wire_work(0, true, true) + 1024, rejected_wire_work(1024, true, true));
}

fn rejected_shell_face_work(tail: usize) -> u64 {
    let references = std::iter::once("#999".to_owned())
        .chain(std::iter::repeat_n("#999".to_owned(), tail))
        .collect::<Vec<_>>().join(",");
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#999=<outside.step#root>;ENDSEC;DATA;#1=SHELL_BASED_SURFACE_MODEL('',(#2));#2=OPEN_SHELL('',({references}));ENDSEC;END-ISO-10303-21;");
    let (exchange, _) = crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("valid shell source");
    let setup_ctx = cadmpeg_test_support::service_decode_context();
    let shells = super::super::shell_defs(&exchange, &setup_ctx).expect("shell definitions");
    let ctx = cadmpeg_test_support::service_decode_context();
    let ir = cadmpeg_ir::CadIr::empty();
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &ctx).expect("empty carriers");
    let mut cache = super::super::FaceAttributeCache::new(&ctx).expect("empty face cache");
    let mut storage = ctx.reserve_scoped(0, "test shell losses").expect("empty losses");
    let outcome = super::super::build(1, exchange.records().get(&1).expect("root"),
        super::super::BuildSources {
            exchange: &exchange, ir: &ir, vdefs: &BTreeMap::new(), edefs: &BTreeMap::new(),
            odefs: &BTreeMap::new(), shell_definitions: &shells,
            decoded_pcurves: &std::collections::BTreeSet::new(), point_positions: &carriers, ctx: &ctx,
        }, &mut cache, false, (&mut Vec::new(), &mut storage))
        .expect("reject missing face without budget refusal");
    let (built, failures, _storage) = outcome.into_parts();
    assert!(built.is_empty());
    assert_eq!(failures.expect("rejected shell").first.expect("missing face").record_id, 999);
    assert_eq!(ctx.resource_refusal(), None);
    completed_work(&ctx)
}

#[test]
fn missing_shell_face_does_not_charge_the_abandoned_reference_tail() {
    // Shell member validation reads each tail item once; face resolution reads none of them.
    assert_eq!(rejected_shell_face_work(0) + 1024, rejected_shell_face_work(1024));
}

fn rejected_implicit_bound_work(tail: usize) -> u64 {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#999=<outside.step#root>;ENDSEC;DATA;ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("valid empty exchange");
    let ctx = cadmpeg_test_support::service_decode_context();
    let ir = cadmpeg_ir::CadIr::empty();
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &ctx).expect("empty carriers");
    let bounds = std::iter::repeat_n(999, tail + 1).collect::<Vec<_>>();
    assert!(super::super::implicit_face_points(&bounds, &exchange, &BTreeMap::new(), &carriers, &ctx)
        .expect("reject missing bound without budget refusal").is_none());
    assert_eq!(ctx.resource_refusal(), None);
    completed_work(&ctx)
}

#[test]
fn missing_implicit_bound_does_not_charge_the_abandoned_bound_tail() {
    assert_eq!(rejected_implicit_bound_work(0), rejected_implicit_bound_work(1024));
}
