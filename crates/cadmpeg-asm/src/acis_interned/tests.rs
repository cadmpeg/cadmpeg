// SPDX-License-Identifier: Apache-2.0
use super::{applies, frame, END_OF_DATA};
use crate::sab::Token;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn name(tag: u8, text: &str, id: u32) -> Vec<u8> {
    let mut out = vec![tag, u8::try_from(text.len() + 5).unwrap()];
    out.extend_from_slice(text.as_bytes());
    out.push(b'%');
    out.extend_from_slice(&id.to_le_bytes());
    out
}

fn reference(value: i32) -> Vec<u8> {
    let mut out = vec![0x0c];
    out.extend_from_slice(&value.to_le_bytes());
    out
}

fn long(value: i32) -> Vec<u8> {
    let mut out = vec![0x04];
    out.extend_from_slice(&value.to_le_bytes());
    out
}

fn prefix() -> Vec<u8> {
    [reference(-1), long(-1), long(-1), reference(-1)].concat()
}

/// A header extension, a body, two lumps (the second repeats the interned
/// name), and the end marker.
fn stream() -> Vec<u8> {
    let mut out = vec![0x0a, 0x07, 3];
    out.extend_from_slice(b"key");
    out.extend(name(0x0d, "body", 1));
    out.extend(prefix());
    out.extend(long(0));
    out.extend([reference(1), reference(-1), reference(-1)].concat());
    out.push(0x11);
    out.extend(name(0x0d, "lump", 2));
    out.extend(prefix());
    out.extend([reference(-1), reference(2), reference(0)].concat());
    out.push(0x11);
    out.extend(name(0x0d, "", 2));
    out.extend(prefix());
    out.extend([reference(-1), reference(-1), reference(0)].concat());
    out.push(0x11);
    out.extend([0x0d, 16]);
    out.extend_from_slice(END_OF_DATA.as_bytes());
    out
}

fn with_ctx<T>(bytes: &[u8], run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    run(&ctx)
}

#[test]
fn interned_names_resolve_and_extended_prefixes_reduce() {
    let bytes = stream();
    assert!(applies(&bytes, 0));
    let framed = with_ctx(&bytes, |ctx| frame(ctx, &bytes, 0, bytes.len(), Some(1))).unwrap();
    assert_eq!(
        framed.header_extension,
        [Token::True, Token::Str("key".into())]
    );
    let names: Vec<_> = framed
        .records
        .iter()
        .map(|record| record.name.as_str())
        .collect();
    assert_eq!(names, ["body", "lump", "lump"]);
    assert_eq!(framed.reduced_records, 3);
    let body = &framed.records[0];
    assert_eq!(body.ref_at(3), Some(1));
    assert_eq!(body.chunk(1), Some(&Token::Long(-1)));
    assert_eq!(framed.records[1].ref_at(4), Some(2));
    assert_eq!(framed.records[1].ref_at(5), Some(0));
}

#[test]
fn classic_identifiers_are_not_interned() {
    let mut bytes = vec![0x0d, 4];
    bytes.extend_from_slice(b"body");
    assert!(!applies(&bytes, 0));
}

#[test]
fn an_undefined_name_id_is_refused() {
    let mut bytes = name(0x0d, "body", 1);
    bytes.extend(name(0x0d, "", 7));
    let error = with_ctx(&bytes, |ctx| frame(ctx, &bytes, 0, bytes.len(), None)).unwrap_err();
    assert!(format!("{error:?}").contains("name id 7 is not defined"));
}

#[test]
fn a_stream_without_the_end_marker_is_refused() {
    let mut bytes = name(0x0d, "body", 1);
    bytes.extend(prefix());
    bytes.push(0x11);
    let error = with_ctx(&bytes, |ctx| frame(ctx, &bytes, 0, bytes.len(), None)).unwrap_err();
    assert!(format!("{error:?}").contains(END_OF_DATA));
}
