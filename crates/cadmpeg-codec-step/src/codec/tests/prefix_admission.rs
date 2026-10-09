// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

#[test]
fn empty_detection_has_no_input_visit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    assert!(!crate::codec::starts_with_step_magic(&ctx, b"").unwrap());
    assert!(!crate::codec::is_part28_xml(&ctx, b"").unwrap());
    assert!(!crate::codec::is_ap242_bo_model_xml(&ctx, b"").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn empty_detection_preserves_the_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "original detection refusal").unwrap_err() else {
        panic!("original work refusal");
    };
    for result in [
        crate::codec::starts_with_step_magic(&ctx, b""),
        crate::codec::is_part28_xml(&ctx, b""),
        crate::codec::is_ap242_bo_model_xml(&ctx, b""),
    ] {
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit)) if limit == original));
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn magic_trivia_has_no_exhaustion_visit() {
    for (input, work) in [(b" ".as_slice(), 2), (b"\\N\\", 1), (b"/*x*/", 3)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // A comment visits the outer cursor and two actual delimiter windows.
        policy.limits.max_work_units = work;
        let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy).unwrap();
        assert!(!crate::codec::starts_with_step_magic(&ctx, input).unwrap());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn xml_preamble_has_no_exhaustion_visit() {
    for input in [b"<!--x-->".as_slice(), b"<?x?>", b"<!X>"] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One actual outer visit and two delimiter positions, ending at input EOF.
        policy.limits.max_work_units = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy).unwrap();
        assert!(!crate::codec::is_part28_xml(&ctx, input).unwrap());
        ctx.finish_session().unwrap();
    }
}
