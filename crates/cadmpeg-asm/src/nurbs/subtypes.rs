// SPDX-License-Identifier: Apache-2.0
//! Byte-scope ownership, intcurve subtype classification, and token walkers.

use crate::kernel_header::RefWidth;
use crate::sab::int_le_at;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

/// Modern and legacy spellings of the same intcurve construction.
pub(super) const INTCURVE_ALIASES: &[(&str, &str)] = &[
    ("blend_int_cur", "bldcur"),
    ("spring_int_cur", "blndsprngcur"),
    ("exact_int_cur", "exactcur"),
    ("law_int_cur", "lawintcur"),
    ("off_int_cur", "offintcur"),
    ("offset_int_cur", "offsetintcur"),
    ("off_surf_int_cur", "offsurfintcur"),
    ("para_silh_int_cur", "parasil"),
    ("par_int_cur", "parcur"),
    ("proj_int_cur", "projcur"),
    ("surf_int_cur", "surfcur"),
    ("int_int_cur", "surfintcur"),
    ("skin_int_cur", "d5c2_cur"),
    ("subset_int_cur", "subsetintcur"),
];

/// Ordered byte offsets and borrowed names of owned subtype definitions.
pub(super) type OwnedSubtypeDefinitions<'bytes> = Vec<(usize, &'bytes [u8])>;

/// Byte offsets and names of the subtype definitions `bytes` itself owns: the
/// `0x0f` openings at the outermost nesting level, in stream order, `ref`
/// included. A definition inside a nested scope belongs to that scope's
/// construction, not to `bytes`.
///
/// A `0x10` with no open scope is a malformed stream and is refused.
pub(super) fn owned_subtype_defs<'bytes>(
    ctx: &DecodeContext<'_>,
    bytes: &'bytes [u8],
    int_width: RefWidth,
) -> Result<Option<OwnedSubtypeDefinitions<'bytes>>, CodecError> {
    let mut owned = Vec::new();
    let mut depth = 0usize;
    let mut pos = 0usize;
    while pos < bytes.len() {
        ctx.charge_work(1, "scan ASM owned subtype token")?;
        match bytes[pos] {
            0x0f => {
                if depth == 0 && matches!(bytes.get(pos + 1), Some(0x0d | 0x0e)) {
                    // A stream that ends at the name-length byte states no
                    // name at all, which is not a name of zero bytes.
                    if let Some(&len) = bytes.get(pos + 2) {
                        if let Some(name) = bytes.get(pos + 3..pos + 3 + usize::from(len)) {
                            ctx.push_vec(&mut owned, (pos, name), "ASM owned subtype definitions")?;
                        }
                    }
                }
                depth += 1;
            }
            0x10 => {
                let Some(next) = depth.checked_sub(1) else {
                    return Ok(None);
                };
                depth = next;
            }
            _ => {}
        }
        match next_token(bytes, pos, int_width) {
            Some(next) => pos = next,
            None => break,
        }
    }
    Ok(Some(owned))
}

/// Byte offset of the first subtype definition `bytes` owns whose name matches
/// one of `names`, together with the matched name. Names are tried in order;
/// the first name with a hit wins.
///
/// A construction claims a record only through the definition the record owns.
/// Records nest complete constructions as supports — a rolling-ball blend
/// embeds a variable blend, a variable blend embeds an extrusion — so a decoder
/// that accepted any matching marker anywhere in the record would claim records
/// belonging to the construction that encloses it.
pub(super) fn find_owned_subtype_marker<'n>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    names: &[&'n [u8]],
    int_width: RefWidth,
) -> Result<Option<(usize, &'n [u8])>, CodecError> {
    let (owned, _storage) = ctx.with_scoped_storage("ASM owned subtype search", || {
        owned_subtype_defs(ctx, bytes, int_width)
    })?;
    let Some(owned) = owned else {
        return Ok(None);
    };
    for &name in names {
        if let Some((start, _)) = ctx.find_by(
            &owned,
            |(_, owned_name)| Ok(*owned_name == name),
            "ASM owned subtype name search",
        )? {
            return Ok(Some((*start, name)));
        }
    }
    Ok(None)
}

/// Byte offset and name length of the `intcurve` subtype definition `bytes`
/// owns, given the subtype's modern name. The legacy spelling of the same
/// construction is accepted as a second candidate.
pub(super) fn find_owned_intcurve_subtype(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    modern: &[u8],
    int_width: RefWidth,
) -> Result<Option<(usize, usize)>, CodecError> {
    if modern.is_empty() {
        return Ok(None);
    }
    let legacy = INTCURVE_ALIASES
        .iter()
        .find_map(|(name, alias)| (name.as_bytes() == modern).then_some(alias.as_bytes()));
    let found = match legacy {
        Some(legacy) => find_owned_subtype_marker(ctx, bytes, &[modern, legacy], int_width)?,
        None => find_owned_subtype_marker(ctx, bytes, &[modern], int_width)?,
    };
    Ok(found.map(|(marker, name)| (marker, name.len())))
}

/// Read the next compact or named subtype reference in token order.
/// The cursor advances only over tokens the search actually visits.
pub(super) fn next_subtype_reference(
    ctx: &DecodeContext<'_>,
    tokens: &[crate::sab::Token],
    position: &mut usize,
) -> Result<Option<usize>, CodecError> {
    use crate::sab::Token;
    let start = *position;
    let Some(remaining) = tokens.get(start..) else {
        return Ok(None);
    };
    ctx.find_map(
        remaining.iter().enumerate(),
        |(offset, token)| {
            let pos = start + offset;
            *position = pos + 1;
            if !matches!(token, Token::SubtypeOpen) {
                return Ok(None);
            }
            let index = match (tokens.get(pos + 1), tokens.get(pos + 2)) {
                (Some(Token::Ident(name)), Some(Token::Long(index)))
                    if name == "ref" && *index >= 0 =>
                {
                    usize::try_from(*index).ok()
                }
                (Some(Token::Long(index)), Some(Token::SubtypeClose)) if *index >= 0 => {
                    usize::try_from(*index).ok()
                }
                _ => None,
            };
            Ok(index)
        },
        "ASM subtype reference search",
    )
}

/// Whether an outer subtype definition names a construction other than `ref`.
/// A close without an opening invalidates the complete ownership walk.
pub(super) fn has_owned_construction(
    ctx: &DecodeContext<'_>,
    tokens: &[crate::sab::Token],
) -> Result<bool, CodecError> {
    use crate::sab::Token;
    let mut depth = 0usize;
    let mut owns = false;
    let valid = ctx.all_by(
        tokens.iter().enumerate(),
        |(position, token)| {
            match token {
                Token::SubtypeOpen => {
                    if depth == 0 {
                        if let Some(Token::Ident(name) | Token::SubIdent(name)) =
                            tokens.get(position + 1)
                        {
                            owns |= name != "ref";
                        }
                    }
                    depth += 1;
                }
                Token::SubtypeClose => {
                    let Some(next) = depth.checked_sub(1) else {
                        return Ok(false);
                    };
                    depth = next;
                }
                _ => {}
            }
            Ok(true)
        },
        "ASM construction ownership tokens",
    )?;
    Ok(valid && owns)
}

/// A balanced subtype scope in byte space.
///
/// [`subtype_span`] is the only constructor: the field is private and the type
/// has no `From` and no `Deref`. Every value therefore states one scope that
/// tokenizes end to end at the width it was walked at, whose every `0x10` has a
/// matching `0x0f` within the span, and whose final token is the close that
/// balances it. The owned-marker walk is total over this type.
///
/// The field is not reachable from another module:
///
/// ```compile_fail
/// use cadmpeg_asm::nurbs::subtypes::SubtypeScope;
///
/// let bytes = [0x0fu8, 0x10];
/// let scope = SubtypeScope(&bytes[..]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubtypeScope<'a>(&'a [u8]);

impl<'a> SubtypeScope<'a> {
    /// The scope's bytes, both delimiters included.
    pub fn bytes(&self) -> &'a [u8] {
        self.0
    }
}

/// The byte span of the subtype definition that opens at `start`: from its
/// `0x0f` opening through the matching `0x10` close, nested definitions
/// included.
///
/// `None` unless the byte at `start` is `0x0f`. A definition opens at its own
/// `0x0f`, so a `start` that names another byte names no definition.
pub fn subtype_span<'bytes>(
    ctx: &DecodeContext<'_>,
    bytes: &'bytes [u8],
    start: usize,
    int_width: RefWidth,
) -> Result<Option<SubtypeScope<'bytes>>, CodecError> {
    if bytes.get(start) != Some(&0x0f) {
        return Ok(None);
    }
    let mut depth = 0usize;
    let mut pos = start;
    while pos < bytes.len() {
        ctx.charge_work(1, "scan ASM subtype scope token")?;
        match bytes[pos] {
            0x0f => depth += 1,
            0x10 => {
                let Some(next) = depth.checked_sub(1) else {
                    return Ok(None);
                };
                depth = next;
                if depth == 0 {
                    return Ok(bytes.get(start..=pos).map(SubtypeScope));
                }
            }
            _ => {}
        }
        let Some(next) = next_token(bytes, pos, int_width) else {
            return Ok(None);
        };
        pos = next;
    }
    Ok(None)
}

/// Offset of the token after the one at `pos`, or `None` when the tag is
/// unrecognized or its payload runs past the end. `0x04`, `0x0c` and `0x15`
/// carry an `int_width` payload; `0x09` and `0x12` carry an `int_width` string
/// length prefix, unlike the one- and two-byte prefixes of `0x07` and `0x08`.
pub(super) fn next_token(bytes: &[u8], pos: usize, int_width: RefWidth) -> Option<usize> {
    let tag = *bytes.get(pos)?;
    let fixed = match tag {
        0x02 => 2,
        0x03 => 3,
        0x04 | 0x0c | 0x15 => 1 + int_width.bytes(),
        0x06 | 0x17 => 9,
        0x05 => 5,
        0x0a | 0x0b | 0x0f | 0x10 | 0x11 => 1,
        0x13 | 0x14 => 25,
        0x16 => 17,
        0x07 | 0x0d | 0x0e => 2 + usize::from(*bytes.get(pos + 1)?),
        0x08 => 3 + usize::from(View::u16_le_at(bytes, pos + 1)?),
        0x09 | 0x12 => {
            let length = int_le_at(bytes, pos + 1, int_width)?;
            1 + int_width.bytes() + usize::try_from(length).ok()?
        }
        _ => return None,
    };
    let next = pos.checked_add(fixed)?;
    (next <= bytes.len()).then_some(next)
}

#[cfg(test)]
mod ownership_tests {
    use super::{find_owned_intcurve_subtype, owned_subtype_defs, subtype_span};
    use crate::kernel_header::RefWidth;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn subtype_reference_scanner_preserves_named_and_compact_stream_order() {
        use crate::sab::Token;
        let ctx = cadmpeg_test_support::service_decode_context();
        let tokens = [
            Token::Double(9.0),
            Token::SubtypeOpen,
            Token::Ident("ref".into()),
            Token::Long(7),
            Token::SubtypeClose,
            Token::Double(8.0),
            Token::SubtypeOpen,
            Token::Long(3),
            Token::SubtypeClose,
        ];
        let mut position = 0;
        assert_eq!(
            super::next_subtype_reference(&ctx, &tokens, &mut position).unwrap(),
            Some(7)
        );
        assert_eq!(
            super::next_subtype_reference(&ctx, &tokens, &mut position).unwrap(),
            Some(3)
        );
        assert_eq!(
            super::next_subtype_reference(&ctx, &tokens, &mut position).unwrap(),
            None
        );
        assert_eq!(position, tokens.len());
    }

    #[test]
    fn construction_presence_has_no_owned_storage_and_rejects_unmatched_closes() {
        use crate::sab::Token;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let tokens = [
            Token::SubtypeOpen,
            Token::Ident("construction".into()),
            Token::SubtypeClose,
        ];
        assert!(super::has_owned_construction(&ctx, &tokens).unwrap());
        assert!(!super::has_owned_construction(&ctx, &[Token::SubtypeClose]).unwrap());
        ctx.finish_session().unwrap();
    }

    /// A subtype definition opening: `0x0f`, name token, length, name bytes.
    fn open(bytes: &mut Vec<u8>, name: &[u8]) {
        bytes.push(0x0f);
        bytes.push(0x0d);
        bytes.push(u8::try_from(name.len()).expect("short name"));
        bytes.extend_from_slice(name);
    }

    /// A `defm_int_cur` record whose bend data nests a complete `int_int_cur`
    /// construction is the deformable curve, not the intersection: the
    /// intersection belongs to the construction the record embeds.
    #[test]
    fn a_nested_intcurve_construction_does_not_own_the_record() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for int_width in [RefWidth::Four, RefWidth::Eight] {
            let mut bytes = Vec::new();
            open(&mut bytes, b"defm_int_cur");
            bytes.push(0x04);
            bytes.extend_from_slice(&vec![0u8; int_width.bytes()]);
            open(&mut bytes, b"int_int_cur");
            bytes.push(0x10);
            bytes.push(0x10);

            assert_eq!(
                find_owned_intcurve_subtype(&ctx, &bytes, b"defm_int_cur", int_width).unwrap(),
                Some((0, b"defm_int_cur".len()))
            );
            assert_eq!(
                find_owned_intcurve_subtype(&ctx, &bytes, b"int_int_cur", int_width).unwrap(),
                None
            );
            assert_eq!(
                crate::nurbs::toks::owned_construction_subtype(
                    &ctx,
                    &crate::nurbs::toks::lex_test_span(&bytes, int_width)
                        .expect("valid single-record byte fixture")
                )
                .transpose()
                .unwrap()
                .as_deref(),
                Some("defm_int_cur")
            );
        }
    }

    /// A byte scope answers its owned markers with no refusal to answer: the
    /// call binds a `Vec<usize>` directly, because `subtype_span` has already
    /// proven the balance the raw-stream walk refuses.
    #[test]
    fn a_byte_scope_yields_its_owned_markers_with_no_refusal_to_answer() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty test input fits input limit");
        for int_width in [RefWidth::Four, RefWidth::Eight] {
            let mut bytes = Vec::new();
            open(&mut bytes, b"exact_int_cur");
            let owned_marker = bytes.len();
            bytes.extend_from_slice(b"\x0d\x04nubs");
            open(&mut bytes, b"ref");
            bytes.extend_from_slice(b"\x0d\x05nurbs");
            bytes.push(0x10);
            bytes.push(0x10);

            let scope = subtype_span(&ctx, &bytes, 0, int_width)
                .unwrap()
                .expect("balanced scope");
            let owned: Vec<usize> = scope.owned_marker_positions(&ctx, int_width).unwrap();
            assert_eq!(owned, vec![owned_marker]);
            assert_eq!(scope.bytes(), bytes.as_slice());
        }
    }

    /// A definition opens at its own `0x0f`. A start that names another byte
    /// names no definition.
    #[test]
    fn a_byte_span_that_does_not_open_at_start_is_not_a_scope() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty test input fits input limit");
        for int_width in [RefWidth::Four, RefWidth::Eight] {
            let mut bytes = vec![0x0du8, 0x01, b'x'];
            let open_at = bytes.len();
            open(&mut bytes, b"exact_int_cur");
            bytes.push(0x10);

            assert_eq!(subtype_span(&ctx, &bytes, 0, int_width).unwrap(), None);
            assert_eq!(subtype_span(&ctx, &bytes, 1, int_width).unwrap(), None);
            assert_eq!(
                subtype_span(&ctx, &bytes, bytes.len(), int_width).unwrap(),
                None
            );
            let scope = subtype_span(&ctx, &bytes, open_at, int_width)
                .unwrap()
                .expect("balanced scope");
            assert_eq!(scope.bytes(), &bytes[open_at..]);
        }
    }

    #[test]
    fn owned_subtype_walk_refuses_unadmitted_work_before_first_token() {
        let bytes = [0x0f, 0x10];
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "scan ASM owned subtype token",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

                owned_subtype_defs(&ctx, &bytes, RefWidth::Four)
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected work refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "scan ASM owned subtype token");
    }

    #[test]
    fn subtype_span_refuses_unadmitted_work_before_first_token() {
        let bytes = [0x0f, 0x10];
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "scan ASM subtype scope token",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

                subtype_span(&ctx, &bytes, 0, RefWidth::Four)
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected work refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "scan ASM subtype scope token");
    }
}
