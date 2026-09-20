// SPDX-License-Identifier: Apache-2.0
//! Byte-scope ownership, intcurve subtype classification, and token walkers.

use crate::kernel_header::RefWidth;
use crate::sab::int_le_at;
use cadmpeg_core::decode::View;

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

/// Byte offsets and names of the subtype definitions `bytes` itself owns: the
/// `0x0f` openings at the outermost nesting level, in stream order, `ref`
/// included. A definition inside a nested scope belongs to that scope's
/// construction, not to `bytes`.
///
/// A `0x10` with no open scope is a malformed stream and is refused.
pub(super) fn owned_subtype_defs(bytes: &[u8], int_width: RefWidth) -> Option<Vec<(usize, &[u8])>> {
    let mut owned = Vec::new();
    let mut depth = 0usize;
    let mut pos = 0usize;
    while pos < bytes.len() {
        match bytes[pos] {
            0x0f => {
                if depth == 0 && matches!(bytes.get(pos + 1), Some(0x0d | 0x0e)) {
                    // A stream that ends at the name-length byte states no
                    // name at all, which is not a name of zero bytes.
                    if let Some(&len) = bytes.get(pos + 2) {
                        if let Some(name) = bytes.get(pos + 3..pos + 3 + usize::from(len)) {
                            owned.push((pos, name));
                        }
                    }
                }
                depth += 1;
            }
            0x10 => depth = depth.checked_sub(1)?,
            _ => {}
        }
        match next_token(bytes, pos, int_width) {
            Some(next) => pos = next,
            None => break,
        }
    }
    Some(owned)
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
    bytes: &[u8],
    names: &[&'n [u8]],
    int_width: RefWidth,
) -> Option<(usize, &'n [u8])> {
    let owned = owned_subtype_defs(bytes, int_width)?;
    names.iter().copied().find_map(|name| {
        owned
            .iter()
            .find(|(_, owned_name)| *owned_name == name)
            .map(|(start, _)| (*start, name))
    })
}

/// Byte offset and name length of the `intcurve` subtype definition `bytes`
/// owns, given the subtype's modern name. The legacy spelling of the same
/// construction is accepted as a second candidate.
pub(super) fn find_owned_intcurve_subtype(
    bytes: &[u8],
    modern: &[u8],
    int_width: RefWidth,
) -> Option<(usize, usize)> {
    if modern.is_empty() {
        return None;
    }
    let legacy = INTCURVE_ALIASES
        .iter()
        .find_map(|(name, alias)| (name.as_bytes() == modern).then_some(alias.as_bytes()));
    let found = match legacy {
        Some(legacy) => find_owned_subtype_marker(bytes, &[modern, legacy], int_width),
        None => find_owned_subtype_marker(bytes, &[modern], int_width),
    };
    found.map(|(marker, name)| (marker, name.len()))
}

/// A balanced subtype scope in byte space.
///
/// [`subtype_span`] is the only constructor: the field is private and the type
/// has no `From` and no `Deref`. Every value therefore states one scope that
/// tokenizes end to end at the width it was walked at, whose every `0x10` has a
/// matching `0x0f` within the span, and whose final token is the close that
/// balances it. The marker walk that refuses an unbalanced stream
/// ([`crate::nurbs::reader::owned_marker_positions`]) is total over this type.
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
pub fn subtype_span(bytes: &[u8], start: usize, int_width: RefWidth) -> Option<SubtypeScope<'_>> {
    (bytes.get(start) == Some(&0x0f)).then_some(())?;
    let mut depth = 0usize;
    let mut pos = start;
    while pos < bytes.len() {
        match bytes[pos] {
            0x0f => depth += 1,
            0x10 => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return bytes.get(start..=pos).map(SubtypeScope);
                }
            }
            _ => {}
        }
        pos = next_token(bytes, pos, int_width)?;
    }
    None
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
    use super::{find_owned_intcurve_subtype, subtype_span};
    use crate::kernel_header::RefWidth;

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
        for int_width in [RefWidth::Four, RefWidth::Eight] {
            let mut bytes = Vec::new();
            open(&mut bytes, b"defm_int_cur");
            bytes.push(0x04);
            bytes.extend_from_slice(&vec![0u8; int_width.bytes()]);
            open(&mut bytes, b"int_int_cur");
            bytes.push(0x10);
            bytes.push(0x10);

            assert_eq!(
                find_owned_intcurve_subtype(&bytes, b"defm_int_cur", int_width),
                Some((0, b"defm_int_cur".len()))
            );
            assert_eq!(
                find_owned_intcurve_subtype(&bytes, b"int_int_cur", int_width),
                None
            );
            assert_eq!(
                crate::nurbs::toks::owned_construction_subtype(
                    &crate::nurbs::toks::lex_test_span(&bytes, int_width)
                        .expect("valid single-record byte fixture")
                )
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
        for int_width in [RefWidth::Four, RefWidth::Eight] {
            let mut bytes = Vec::new();
            open(&mut bytes, b"exact_int_cur");
            let owned_marker = bytes.len();
            bytes.extend_from_slice(b"\x0d\x04nubs");
            open(&mut bytes, b"ref");
            bytes.extend_from_slice(b"\x0d\x05nurbs");
            bytes.push(0x10);
            bytes.push(0x10);

            let scope = subtype_span(&bytes, 0, int_width).expect("balanced scope");
            let owned: Vec<usize> = scope.owned_marker_positions(int_width);
            assert_eq!(owned, vec![owned_marker]);
            assert_eq!(scope.bytes(), bytes.as_slice());
        }
    }

    /// A definition opens at its own `0x0f`. A start that names another byte
    /// names no definition.
    #[test]
    fn a_byte_span_that_does_not_open_at_start_is_not_a_scope() {
        for int_width in [RefWidth::Four, RefWidth::Eight] {
            let mut bytes = vec![0x0du8, 0x01, b'x'];
            let open_at = bytes.len();
            open(&mut bytes, b"exact_int_cur");
            bytes.push(0x10);

            assert_eq!(subtype_span(&bytes, 0, int_width), None);
            assert_eq!(subtype_span(&bytes, 1, int_width), None);
            assert_eq!(subtype_span(&bytes, bytes.len(), int_width), None);
            let scope = subtype_span(&bytes, open_at, int_width).expect("balanced scope");
            assert_eq!(scope.bytes(), &bytes[open_at..]);
        }
    }
}
