// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, LimitScope};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::ParserPartOutput;

use super::{admit_field, FieldValue};
use crate::global::GlobalTable;

#[test]
fn optional_directory_scalar_uses_actual_nonzero_field_and_refuses_foreign_source() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"pre       7post", &arena, &DecodePolicy::service())
            .expect("source session");
    let source = root.child(3, 11).expect("exact nonzero field");
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    assert_eq!(
        admit_field(&ctx, source, 12, GlobalTable::Legacy, &mut output).expect("actual scalar"),
        Some(FieldValue::Integer(7))
    );
    let (_, native, fidelity, losses) = output
        .into_parts(&ctx)
        .expect("parser closure")
        .into_parts();
    assert!(native
        .namespace("iges")
        .is_none_or(|namespace| namespace.arenas().is_empty()));
    assert!(losses.is_empty());
    assert_eq!(fidelity.parts().len(), 1);
    let part = fidelity.parts().values().next().expect("reached field");
    assert_eq!(part.offset(), 3);
    assert_eq!(part.byte_len(), 8);
    let foreign_arena = DecodeArena::new();
    let (foreign, _) =
        DecodeContext::from_root_bytes(b"       7", &foreign_arena, &DecodePolicy::service())
            .expect("foreign session");
    let mut rejected = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    assert!(admit_field(&foreign, source, 12, GlobalTable::Legacy, &mut rejected).is_err());
    let (_, native, fidelity, losses) = rejected
        .into_parts(&foreign)
        .expect("unchanged foreign output")
        .into_parts();
    assert!(native
        .namespace("iges")
        .is_none_or(|namespace| namespace.arenas().is_empty()));
    assert!(fidelity.parts().is_empty());
    assert!(fidelity.retained_records().is_empty());
    assert!(losses.is_empty());
}

#[test]
fn optional_directory_field_session_limit_escapes_without_a_given_up_substitute() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"       x", &arena, &policy).expect("source session");
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let error = admit_field(&ctx, source, 12, GlobalTable::Legacy, &mut output)
        .expect_err("session refusal escapes");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.scope == LimitScope::Session)
    );
}
