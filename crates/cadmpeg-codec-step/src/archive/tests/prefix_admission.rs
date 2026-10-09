// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use zip::CompressionMethod;

use crate::archive::{ReferenceTarget, ROOT_NAME};

const EMPTY_ROOT: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;";

#[test]
fn empty_reference_notes_have_no_exhaustion_visit_and_preserve_fuse() {
    let bytes = super::step_zip(&[(ROOT_NAME, EMPTY_ROOT, CompressionMethod::Stored)]);
    crate::test_support::with_service_context(&bytes, |source, fixture_ctx| {
        let opened = crate::archive::open_root(fixture_ctx, cadmpeg_core::decode::View::over_retained(source)).unwrap();
        let (exchange, _) = crate::parse::parse_retained(EMPTY_ROOT, fixture_ctx).unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let notes = crate::archive::root_reference_notes(&ctx, &opened.archive, &exchange).unwrap();
        assert!(notes.is_empty());
        let CodecError::ResourceLimit(original) = ctx.charge_work(1, "original reference refusal").unwrap_err() else {
            panic!("original work refusal");
        };
        assert!(matches!(crate::archive::root_reference_notes(&ctx, &opened.archive, &exchange), Err(CodecError::ResourceLimit(limit)) if limit == original));
        drop(notes);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
    });
}

fn single_component_work() -> u64 {
    // Three one-byte searches each retain the current core terminal probe.
    // The base reverse search visits ROOT_NAME.len() actual bytes.
    // One component visit, two delimiter-search steps, one length measurement,
    // one output-component visit and one actual output-byte copy follow.
    3 * 2 + ROOT_NAME.len() as u64 + 1 + 2 + 1 + 1 + 1
}

#[test]
fn uri_member_admits_exact_current_search_and_output_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = single_component_work();
    policy.limits.max_materialized_bytes = (4 * std::mem::size_of::<&str>() + 1) as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let mut member_storage = ctx.reserve_scoped(0, "test URI member storage").unwrap();
    let target = crate::archive::resolve_uri(&ctx, &mut member_storage, ROOT_NAME, "a").unwrap();
    assert_eq!(target, ReferenceTarget::Internal { member: "a".into(), query: None, fragment: None });
    let (next, next_storage) = ctx.temporary_vec::<&str>(4, "next actual component buffer")
        .expect("component backing is reusable while the actual member stays live");
    assert!(next.capacity() >= 4);
    drop(next);
    drop(next_storage);
    drop(target);
    drop(member_storage);
    ctx.finish_session().unwrap();
}

#[test]
fn uri_member_copy_keeps_the_original_one_below_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = single_component_work() - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let mut member_storage = ctx.reserve_scoped(0, "test URI member storage").unwrap();
    let CodecError::ResourceLimit(limit) = crate::archive::resolve_uri(&ctx, &mut member_storage, ROOT_NAME, "a").unwrap_err() else {
        panic!("actual member copy refusal");
    };
    assert_eq!(limit.operation, "step_zip_uri_member");
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!((limit.used, limit.additional), (single_component_work() - 1, 1));
    drop(member_storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}
