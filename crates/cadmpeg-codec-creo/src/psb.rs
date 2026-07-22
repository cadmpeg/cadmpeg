// SPDX-License-Identifier: Apache-2.0
//! Context-independent PSB tokens and primitive numeric encodings.
//!
//! [`tokens`] walks forms whose lengths are known without a parent record
//! grammar. [`compact_int`], [`reference_id`], and [`short_form_float`] decode
//! primitive values. Unknown and truncated input remains explicit in the token
//! stream.

/// Structural token bytes ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#22-structural-tokens)).
pub mod token {
    /// Named-record header: `e0 <type> <name>\0`.
    pub const NAMED_RECORD: u8 = 0xe0;
    /// Array opener: `f8 <count>`.
    pub const ARRAY_OPEN: u8 = 0xf8;
    /// Count-bounded scalar body: `f9 <ndim> <count>`.
    pub const SCALAR_BODY: u8 = 0xf9;
    /// Entity reference: `f7 <id>`.
    pub const ENTITY_REF: u8 = 0xf7;
    /// Array close.
    pub const ARRAY_CLOSE: u8 = 0xfb;
    /// Compound-record close.
    pub const COMPOUND_CLOSE: u8 = 0xe3;
}

/// One structurally framed PSB token.
///
/// `offset` and `length` refer to the input slice. Unknown bytes remain
/// explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Byte offset of the token's first byte in the original stream.
    pub offset: usize,
    /// Total byte length of the token, including its prefix byte(s).
    pub length: usize,
    /// The token's structural classification.
    pub kind: TokenKind,
}

/// Structural token kinds whose byte extent is known independent of the
/// parent record grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// A PSB compact integer ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#21-compact-integers)): `0x00..=0x7f` one byte, or
    /// `0x80..=0xbf XX` two bytes big-endian.
    CompactInt,
    /// A 3-byte short-form float token ([spec §3.3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#23-scalar-tokens)): `<prefix> XX YY`.
    ShortFloat,
    /// A seven-byte subunit float token beginning with `0x5e`.
    SevenByteFloat,
    /// An 8-byte world-coordinate token whose leading byte is `0x46`
    /// (positive) or `0x2d` (negative).
    WorldCoordinate,
    /// A named-record header: `e0 <type> <name>\0`.
    NamedRecord,
    /// An entity reference: `f7 <id>`, where `<id>` is a reference-id token.
    EntityReference,
    /// An array opener: `f8 <count>`.
    ArrayOpen,
    /// A count-bounded scalar body header: `f9 <ndim> <count>`.
    ScalarBody,
    /// An array close byte, `0xfb`.
    ArrayClose,
    /// A nested compound-body opener or continuation byte, `0xe2`.
    CompoundOpen,
    /// A compound close or row terminator byte, `0xe3`, whose exact role
    /// depends on the enclosing record grammar.
    CompoundClose,
    /// A recognized single-byte structural marker outside the primary token
    /// set ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#22-structural-tokens)), such as `0xe1`, `0xe4`, `0xe5`, `0xe6`, `0xe8`,
    /// `0xf1`, `0xf2`, `0xf3`, `0xf5`, or `0xf6`. The wrapped byte is the raw
    /// marker value.
    OtherStructural(u8),
    /// A byte that does not match any recognized structural or numeric
    /// prefix. The wrapped byte is the raw value.
    Unknown(u8),
    /// A structural token whose prefix was recognized but whose required
    /// trailing bytes were not available before end-of-buffer. The wrapped
    /// byte is the token's prefix byte.
    Truncated(u8),
}

/// Tokenize byte-self-delimiting PSB forms.
///
/// Numeric forms that depend on a parent grammar remain compact or unknown
/// tokens.
pub fn tokens(data: &[u8]) -> Vec<Token> {
    let mut result = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        let head = data[offset];
        let (length, kind) = match head {
            token::NAMED_RECORD => match data
                .get(offset + 2..)
                .and_then(|rest| rest.iter().position(|&b| b == 0))
            {
                Some(name_len) => (name_len + 3, TokenKind::NamedRecord),
                None => (data.len() - offset, TokenKind::Truncated(head)),
            },
            token::ENTITY_REF => match reference_id(data, offset + 1) {
                Ok((_, end)) => (end - offset, TokenKind::EntityReference),
                Err(_) => (data.len() - offset, TokenKind::Truncated(head)),
            },
            token::ARRAY_OPEN => {
                let (_, end) = compact_int(data, offset + 1);
                if end == offset + 1 {
                    (1, TokenKind::Truncated(head))
                } else {
                    (end - offset, TokenKind::ArrayOpen)
                }
            }
            token::SCALAR_BODY => {
                let (_, dimensions_end) = compact_int(data, offset + 1);
                let (_, count_end) = compact_int(data, dimensions_end);
                if dimensions_end == offset + 1 || count_end == dimensions_end {
                    (data.len() - offset, TokenKind::Truncated(head))
                } else {
                    (count_end - offset, TokenKind::ScalarBody)
                }
            }
            token::ARRAY_CLOSE => (1, TokenKind::ArrayClose),
            0xe2 => (1, TokenKind::CompoundOpen),
            token::COMPOUND_CLOSE => (1, TokenKind::CompoundClose),
            0x29 | 0x2a | 0x2e | 0x2f | 0x42 | 0x43 | 0x47 | 0x48 => {
                if offset + 3 <= data.len() {
                    (3, TokenKind::ShortFloat)
                } else {
                    (data.len() - offset, TokenKind::Truncated(head))
                }
            }
            0x5e => {
                if offset + 7 <= data.len() {
                    (7, TokenKind::SevenByteFloat)
                } else {
                    (data.len() - offset, TokenKind::Truncated(head))
                }
            }
            0x46 | 0x2d if offset + 8 <= data.len() => (8, TokenKind::WorldCoordinate),
            0x46 | 0x2d => (data.len() - offset, TokenKind::Truncated(head)),
            0..=0xbf => {
                let (_, end) = compact_int(data, offset);
                (end - offset, TokenKind::CompactInt)
            }
            0xe1 | 0xe4 | 0xe5 | 0xe6 | 0xe8 | 0xf1 | 0xf2 | 0xf3 | 0xf5 | 0xf6 => {
                (1, TokenKind::OtherStructural(head))
            }
            _ => (1, TokenKind::Unknown(head)),
        };
        result.push(Token {
            offset,
            length,
            kind,
        });
        offset += length;
    }
    result
}

/// Decode a generic PSB compact integer at `offset` ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#21-compact-integers)).
///
/// - `0x00..=0x7f`: one-byte direct value.
/// - `0x80..=0xbf XX`: two-byte big-endian `((head - 0x80) << 8) | XX`.
/// - `0xc0..=0xff`: control/special range; on the generic lane this returns the
///   raw byte as a one-byte value (callers that need the stricter reference-id
///   grammar must reject this range themselves).
///
/// Returns `(value, new_offset)`; `new_offset == offset` signals end-of-buffer.
pub fn compact_int(data: &[u8], offset: usize) -> (u32, usize) {
    let Some(&b) = data.get(offset) else {
        return (0, offset);
    };
    if b <= 0x7f {
        (b as u32, offset + 1)
    } else if (0x80..=0xbf).contains(&b) {
        match data.get(offset + 1) {
            Some(&lo) => ((((b - 0x80) as u32) << 8) | lo as u32, offset + 2),
            None => (b as u32, offset + 1),
        }
    } else {
        (b as u32, offset + 1)
    }
}

/// Decode a canonical PSB entity-reference identifier. Unlike
/// [`compact_int`], typed reference lanes reject control bytes and reject a
/// two-byte representation for values that fit in one byte.
pub fn reference_id(data: &[u8], offset: usize) -> Result<(u32, usize), &'static str> {
    let Some(&head) = data.get(offset) else {
        return Err("reference id is truncated");
    };
    match head {
        0..=0x7f => Ok((head as u32, offset + 1)),
        0x80..=0xbf => {
            let Some(&tail) = data.get(offset + 1) else {
                return Err("two-byte reference id is truncated");
            };
            let value = (((head - 0x80) as u32) << 8) | tail as u32;
            if value < 0x80 {
                return Err("reference id uses a non-canonical two-byte form");
            }
            Ok((value, offset + 2))
        }
        _ => Err("control byte cannot start a reference id"),
    }
}

/// 3-byte short-form float prefix table ([spec §3.3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#23-scalar-tokens)): `prefix -> (ieee_byte0,
/// repeat_fill)`. Repeat-fill tokens fill the 6-byte IEEE tail with the third
/// byte repeated; the others place the third byte then five zero bytes.
///
/// The pairs are `0x19` apart and encode sign/exponent mirror: `(0x29,0x42)`
/// and `(0x2a,0x43)` are `3F/BF`; `(0x2e,0x47)` and `(0x2f,0x48)` are `40/C0`.
const fn short_form_spec(prefix: u8) -> Option<(u8, bool)> {
    match prefix {
        0x29 => Some((0x3f, true)),
        0x2a => Some((0x3f, false)),
        0x2e => Some((0x40, true)),
        0x2f => Some((0x40, false)),
        0x42 => Some((0xbf, true)),
        0x43 => Some((0xbf, false)),
        0x47 => Some((0xc0, true)),
        0x48 => Some((0xc0, false)),
        _ => None,
    }
}

/// True when `prefix` opens a 3-byte short-form float token.
pub fn is_short_form_float(prefix: u8) -> bool {
    short_form_spec(prefix).is_some()
}

/// Decode a 3-byte short-form float at `offset` ([spec §3.3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#23-scalar-tokens)). Returns
/// `Some((value, new_offset))` when the prefix is a known short-form opener and
/// three bytes are available; `None` otherwise.
///
/// The token `<prefix> XX YY` reconstructs the IEEE-754 `double` whose bytes are
/// `(byte0, XX, fill…)`, where `byte0` and the fill mode come from the prefix.
/// Byte-exact against the spec's worked examples (e.g. `2f 43 00 = 38.0`,
/// `29 eb 33 = 0.85`, `48 22 00 = -9.0`).
pub fn short_form_float(data: &[u8], offset: usize) -> Option<(f64, usize)> {
    let (byte0, repeat) = short_form_spec(*data.get(offset)?)?;
    let xx = *data.get(offset + 1)?;
    let yy = *data.get(offset + 2)?;
    let mut ieee = [0u8; 8];
    ieee[0] = byte0;
    ieee[1] = xx;
    if repeat {
        for b in ieee.iter_mut().skip(2) {
            *b = yy;
        }
    } else {
        ieee[2] = yy;
    }
    Some((f64::from_be_bytes(ieee), offset + 3))
}

/// A forward-only byte cursor over a PSB body.
///
/// The cursor owns a borrowed slice and a read position. Typed takes delegate
/// to the module's free decoders ([`compact_int`], [`short_form_float`],
/// [`reference_id`]) and to any caller-supplied decoder of the same
/// `fn(&[u8], usize) -> Option<(T, usize)>` shape, advancing the position only
/// when the decode succeeds. A failed take leaves the position unchanged: the
/// wrapped decoders are pure and report their consumed extent through the
/// returned offset, so the cursor never advances partially. This lets a
/// best-effort walker thread a single `Cursor` instead of hand-carrying a
/// `(value, next)` tuple through every field, while preserving the exact
/// truncation behavior (`break`/`continue` on a `None` take).
pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// Start a cursor at the beginning of `data`.
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Start a cursor at an explicit byte position within `data`.
    pub(crate) fn at(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    /// The current read position, i.e. the offset of the next unread byte.
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    /// Decode one value at the current position with `decode`, advancing to the
    /// decoder's reported offset on success.
    ///
    /// Returns `None` — leaving the position unchanged — exactly when `decode`
    /// returns `None`. Because `decode` reports its consumed extent through the
    /// returned offset rather than by mutating state, the cursor never advances
    /// on failure and never advances partially.
    pub(crate) fn take_with<T>(
        &mut self,
        decode: impl FnOnce(&'a [u8], usize) -> Option<(T, usize)>,
    ) -> Option<T> {
        let (value, next) = decode(self.data, self.pos)?;
        self.pos = next;
        Some(value)
    }

    /// Advance past `prefix` when the bytes at the current position match it.
    ///
    /// Returns `true` and consumes exactly `prefix.len()` bytes on a match;
    /// returns `false` and leaves the position unchanged otherwise.
    pub(crate) fn take_slice_if(&mut self, prefix: &[u8]) -> bool {
        if self
            .data
            .get(self.pos..)
            .is_some_and(|tail| tail.starts_with(prefix))
        {
            self.pos += prefix.len();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_int_one_byte() {
        assert_eq!(compact_int(&[0x00], 0), (0, 1));
        assert_eq!(compact_int(&[0x7f], 0), (127, 1));
    }

    #[test]
    fn compact_int_two_byte() {
        // (0x80 - 0x80) << 8 | 0x80 = 128
        assert_eq!(compact_int(&[0x80, 0x80], 0), (128, 2));
        // (0x81 - 0x80) << 8 | 0x02 = 258
        assert_eq!(compact_int(&[0x81, 0x02], 0), (258, 2));
    }

    #[test]
    fn compact_int_truncated_two_byte_is_raw() {
        assert_eq!(compact_int(&[0x81], 0), (0x81, 1));
    }

    #[test]
    fn compact_int_empty_is_noop() {
        assert_eq!(compact_int(&[], 0), (0, 0));
    }

    #[test]
    fn reference_ids_are_canonical_and_strict() {
        assert_eq!(reference_id(&[0x7f], 0), Ok((127, 1)));
        assert_eq!(reference_id(&[0x80, 0x80], 0), Ok((128, 2)));
        assert!(reference_id(&[0x80, 0x7f], 0).is_err());
        assert!(reference_id(&[0xc0], 0).is_err());
        assert!(reference_id(&[0x81], 0).is_err());
    }

    #[test]
    fn token_walker_preserves_boundaries_and_unknown_bytes() {
        let payload = [
            0xe0, 0x22, b'p', 0, // named record
            0xf8, 0x81, 0x02, // array count 258
            0x2f, 0x43, 0x00, // short float 38
            0xf7, 0x80, 0x80, // canonical reference 128
            0xcc, // unrecognized control byte
        ];
        assert_eq!(
            tokens(&payload),
            vec![
                Token {
                    offset: 0,
                    length: 4,
                    kind: TokenKind::NamedRecord
                },
                Token {
                    offset: 4,
                    length: 3,
                    kind: TokenKind::ArrayOpen
                },
                Token {
                    offset: 7,
                    length: 3,
                    kind: TokenKind::ShortFloat
                },
                Token {
                    offset: 10,
                    length: 3,
                    kind: TokenKind::EntityReference
                },
                Token {
                    offset: 13,
                    length: 1,
                    kind: TokenKind::Unknown(0xcc)
                },
            ]
        );
    }

    #[test]
    fn token_walker_bounds_compact_scalar_body_extents() {
        let tokens = tokens(&[0xf9, 0x80, 0x88, 0x03, 0x0f]);
        assert_eq!(tokens[0].kind, TokenKind::ScalarBody);
        assert_eq!(tokens[0].length, 4);
        assert_eq!(tokens[1].offset, 4);
    }

    #[test]
    fn token_walker_marks_truncated_structural_tokens() {
        assert_eq!(
            tokens(&[token::NAMED_RECORD]),
            vec![Token {
                offset: 0,
                length: 1,
                kind: TokenKind::Truncated(token::NAMED_RECORD),
            }]
        );
    }

    /// Every worked example from [spec §3.3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#23-scalar-tokens), byte-exact.
    #[test]
    fn short_form_float_worked_examples() {
        let cases: &[(&[u8], f64)] = &[
            (&[0x2f, 0x43, 0x00], 38.0),
            (&[0x2f, 0x20, 0x00], 8.0),
            (&[0x48, 0x22, 0x00], -9.0),
            (&[0x29, 0xeb, 0x33], 0.85),
            (&[0x47, 0x25, 0xcc], -10.9),
            (&[0x2a, 0xe8, 0x00], 0.75),
            (&[0x2a, 0xf4, 0x00], 1.25),
            (&[0x2e, 0x08, 0x00], 3.0),
        ];
        for (bytes, expected) in cases {
            let (val, next) = short_form_float(bytes, 0).expect("known short form");
            assert_eq!(next, 3);
            assert!(
                (val - expected).abs() < 1e-9,
                "decoding {bytes:02x?}: got {val}, want {expected}"
            );
        }
    }

    #[test]
    fn short_form_float_rejects_unknown_prefix() {
        assert!(short_form_float(&[0x00, 0x00, 0x00], 0).is_none());
        assert!(!is_short_form_float(0x00));
        assert!(is_short_form_float(0x29));
    }

    #[test]
    fn short_form_float_rejects_truncated() {
        assert!(short_form_float(&[0x2f, 0x43], 0).is_none());
    }
}
