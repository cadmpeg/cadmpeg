// SPDX-License-Identifier: Apache-2.0
//! Empty known-length topology sources preserve the refusal fuse without work.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn with_empty_bound_source(run: impl FnOnce(&crate::parse::Exchange, &crate::reader::index::CarrierIndex)) {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("valid empty exchange");
    let setup_ctx = cadmpeg_test_support::service_decode_context();
    let ir = cadmpeg_ir::CadIr::empty();
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup_ctx)
        .expect("empty carrier index");
    run(&exchange, &carriers);
}

#[test]
fn empty_implicit_bounds_visit_no_terminal_step() {
    with_empty_bound_source(|exchange, carriers| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(super::super::implicit_face_points(&[], exchange, &BTreeMap::new(), carriers, &ctx)
            .expect("empty source has no work or storage").is_none());
        assert_eq!(ctx.resource_refusal(), None);
        ctx.finish_session().expect("empty source completed without refusal");
    });
}

#[test]
fn empty_implicit_bounds_preserve_original_refusal() {
    with_empty_bound_source(|exchange, carriers| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let CodecError::ResourceLimit(original) = ctx.charge_work(1, "test original topology refusal")
            .expect_err("seed original refusal") else { panic!("resource refusal"); };
        let CodecError::ResourceLimit(actual) = super::super::implicit_face_points(
            &[], exchange, &BTreeMap::new(), carriers, &ctx
        ).expect_err("empty path preserves fuse") else { panic!("resource refusal"); };
        assert_eq!(actual, original);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    });
}
