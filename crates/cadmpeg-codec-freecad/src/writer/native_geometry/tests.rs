// SPDX-License-Identifier: Apache-2.0
use super::project;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn inherited_caller_refusal_precedes_projection_scope_shortcuts() {
    let ir = CadIr::empty();
    let before = serde_json::to_value(&ir).unwrap();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_encode(&arena, &DecodePolicy::service());
    let mut working = Vec::<u64>::new();
    let CodecError::ResourceLimit(refusal) = ctx
        .reserve_working(&mut working, usize::MAX, "native projection prior refusal")
        .unwrap_err()
    else {
        panic!("working capacity overflow must latch a resource refusal")
    };
    assert!(matches!(
        project(&ctx, &ir),
        Err(CodecError::ResourceLimit(actual)) if actual == refusal
    ));
    assert!(working.is_empty());
    assert_eq!(serde_json::to_value(&ir).unwrap(), before);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == refusal
    ));
}

#[test]
fn typed_native_geometry_projects_under_the_actual_source_free_encode_context() {
    let input = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd"
    ));
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(input), &DecodeOptions::default())
        .unwrap();
    assert!(!decoded.ir().model.bodies.is_empty());
    let expected = decoded.ir().model.bodies.len();
    let mut wire = serde_json::to_value(decoded.ir()).unwrap();
    for arena in cadmpeg_ir::document::ArenaName::ALL {
        wire["model"][arena.as_str()] = serde_json::json!([]);
    }
    wire["scope"] = serde_json::to_value(cadmpeg_ir::ContentScope::container()).unwrap();
    let ir = serde_json::from_value(wire).unwrap();
    let before = serde_json::to_value(&ir).unwrap();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_encode(&arena, &DecodePolicy::service());
    assert!(ctx.is_encoding());
    let (projected, _) = project(&ctx, &ir).unwrap().unwrap();
    assert_eq!(projected.model.bodies.len(), expected);
    assert_eq!(serde_json::to_value(&ir).unwrap(), before);
    assert!(ctx.is_encoding());
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}
