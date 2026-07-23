// SPDX-License-Identifier: Apache-2.0
//! Locating embedded Parasolid neutral-binary streams in a container payload.
//!
//! A Parasolid stream begins with the four-byte prologue `PS\0\0` and, a short
//! distance in, carries a `SCH_<modeller>_<schema>_<format>` schema token. Two
//! codecs embed these streams and both must find them: `cadmpeg-codec-sldprt`
//! carries direct and zlib-wrapped streams inside its outer blocks, and
//! `cadmpeg-codec-nx` packs zlib members inside its canonical part span. Both
//! re-implement the same scan: find the members, inflate them, test the
//! prologue, read the schema token.
//!
//! [`locate_streams`] is that scan. It first collects direct `PS\0\0` streams;
//! only when none are present does it scan for zlib members (`0x78 01/9c/da`),
//! inflate each, and keep those that inflate to a `PS\0\0` prologue. Every
//! located stream carries its payload offset, the prologue-leading bytes, and
//! the schema token when one is present.
//!
//! # Inflate strategy
//!
//! The inflater is a parameter, not a fixed choice. [`Inflate::Bounded`] uses
//! [`crate::wire::compression::inflate_zlib_prefix`], which accepts a decoded
//! prefix when a member's trailing input is truncated or belongs to a following
//! packed stream. [`Inflate::With`] takes a caller-supplied inflater for a codec
//! whose tolerance on truncated or garbage members differs — nx currently
//! inflates through `flate2` directly. The parameter keeps both semantics
//! expressible so a later seed-replay comparison can decide whether nx's
//! extraction reproduces under the bounded default.
//!
//! # Scope and divergences
//!
//! This is a sniff, not a decoder. Deep record decoding (attribute-class and
//! entity records) stays in the owning codecs, as does classification
//! (`StreamKind`, `is_body_stream`, preview detection): the sniff returns the
//! located streams and the codec labels them. Reproducing the two codecs
//! exactly requires accounting for these deltas:
//!
//! - **sldprt gates wrapped scanning on a transmit-container magic.** sldprt
//!   scans zlib members only when a 16-byte `SolidWorks` transmit magic is
//!   present; this sniff scans whenever no direct stream is found. That magic is
//!   a codec-specific constant kept out of the platform, so sldprt keeps it as a
//!   pre-check or accepts the (low) false-positive exposure of a zlib member
//!   that happens to inflate to a `PS\0\0` prologue.
//! - **Direct streams are admitted through the description-framed header;
//!   wrapped streams on the prologue alone.** A direct scan runs over raw bytes,
//!   where a `PS\0\0` can occur coincidentally inside adjacent compressed data,
//!   so a direct stream is kept only when its `PS\0\0` + description-length +
//!   `SCH_` framing validates (the check sldprt's `stream_header` performs).
//!   A wrapped stream is kept on the prologue alone, since compressed noise does
//!   not inflate to `PS\0\0`; nx's schema-less inflated streams are therefore
//!   still returned. sldprt further filters direct streams through its own
//!   header parse.
//! - **nx accepts a broader zlib header.** nx admits any standards-conforming
//!   `CMF`/`FLG` pair; this sniff matches only the canonical `0x78 01/9c/da`.
//!   Members outside those three are found by nx but not here.
//! - **nx charges a decode budget and tracks consumed input.** nx inflates
//!   through the decode platform (`begin_expand`/`ExpandWriter`) and records each
//!   member's consumed compressed extent to keep packed members disjoint.
//!   Neither is expressible through a pure `&[u8]` sniff; nx either adopts the
//!   bounded strategy and loses per-member budget granularity or keeps its
//!   platform path and reuses only [`has_prologue`] and [`schema_token`].
//!
//! The schema read here is run-based (`SCH_` followed by alphanumerics and
//! underscores), matching nx. sldprt reads a length-prefixed schema; the two
//! agree whenever the length prefix equals the token's run length, which holds
//! for well-formed tokens.
#![deny(clippy::disallowed_methods)]

use crate::wire::compression::inflate_zlib_prefix;

/// The four-byte Parasolid neutral-binary prologue.
pub const PROLOGUE: [u8; 4] = [b'P', b'S', 0, 0];

/// The window, in leading bytes, searched for the `SCH_` schema token.
const SCHEMA_SEARCH_WINDOW: usize = 512;

/// A Parasolid stream located inside a container payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParasolidStream {
    /// Offset in the scanned payload where the stream begins: the `PS\0\0`
    /// signature for a direct stream, or the `0x78` zlib header for a wrapped one.
    pub offset: usize,
    /// The `PS\0\0`-leading stream bytes: inflated for a wrapped member, the
    /// direct slice otherwise.
    pub bytes: Vec<u8>,
    /// The `SCH_<...>` schema token, when the stream carries one.
    pub schema: Option<String>,
}

/// How a located zlib member is inflated.
#[derive(Debug, Clone, Copy)]
pub enum Inflate {
    /// Bounded prefix inflate via [`crate::wire::compression::inflate_zlib_prefix`],
    /// which accepts a decoded prefix on truncated or packed trailing input.
    Bounded,
    /// A caller-supplied inflater, for a codec whose tolerance on truncated or
    /// garbage members differs from the bounded prefix inflater.
    With(fn(&[u8]) -> Option<Vec<u8>>),
}

impl Inflate {
    /// Applies the strategy to one zlib member.
    fn inflate(self, member: &[u8]) -> Option<Vec<u8>> {
        match self {
            Inflate::Bounded => inflate_zlib_prefix(member),
            Inflate::With(inflater) => inflater(member),
        }
    }
}

/// Returns whether `bytes` begin with the `PS\0\0` Parasolid prologue.
pub fn has_prologue(bytes: &[u8]) -> bool {
    bytes.starts_with(&PROLOGUE)
}

/// Reads the `SCH_<schema>` token from a `PS\0\0`-leading stream.
///
/// Searches the leading [`SCHEMA_SEARCH_WINDOW`] bytes for `SCH_` and returns
/// the marker plus the following run of alphanumeric and `_` characters.
/// Returns `None` when no `SCH_` marker appears in the window.
pub fn schema_token(stream: &[u8]) -> Option<String> {
    let window = stream.get(..stream.len().min(SCHEMA_SEARCH_WINDOW))?;
    let start = window.windows(4).position(|four| four == b"SCH_")?;
    let mut end = start;
    while end < window.len()
        && window
            .get(end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        end = end.saturating_add(1);
    }
    window
        .get(start..end)
        .map(|token| String::from_utf8_lossy(token).into_owned())
}

/// The maximum bytes past the description scanned for the `SCH_` marker when
/// validating the Parasolid header framing.
const HEADER_SCHEMA_WINDOW: usize = 64;

/// Validates the description-framed Parasolid header of a `PS\0\0`-leading
/// stream.
///
/// The header is `PS\0\0`, a big-endian `u16` description length, the
/// description, padding, a one-byte schema length, and the `SCH_` token. This
/// is the framing sldprt parses; it is used only to admit a *direct* stream,
/// where a coincidental `PS\0\0` in adjacent compressed bytes must be rejected.
/// A wrapped stream is admitted on the prologue alone, since compressed noise
/// does not inflate to `PS\0\0`.
fn header_frames_a_stream(stream: &[u8]) -> bool {
    if !has_prologue(stream) {
        return false;
    }
    let Some(len_bytes) = stream.get(4..6) else {
        return false;
    };
    let desc_len = usize::from(u16::from_be_bytes([len_bytes[0], len_bytes[1]]));
    let Some(desc_end) = 6usize.checked_add(desc_len) else {
        return false;
    };
    // The description must lie within the stream.
    if stream.get(6..desc_end).is_none() {
        return false;
    }
    let window_end = desc_end
        .saturating_add(HEADER_SCHEMA_WINDOW)
        .min(stream.len());
    let Some(search) = stream.get(desc_end..window_end) else {
        return false;
    };
    let Some(rel) = search.windows(4).position(|four| four == b"SCH_") else {
        return false;
    };
    // A schema-length byte must precede the marker and frame it within bounds.
    let Some(schema_at) = desc_end.checked_add(rel) else {
        return false;
    };
    let Some(len_at) = schema_at.checked_sub(1) else {
        return false;
    };
    let Some(&schema_len) = stream.get(len_at) else {
        return false;
    };
    schema_at
        .checked_add(usize::from(schema_len))
        .is_some_and(|schema_end| stream.get(schema_at..schema_end).is_some())
}

/// Scans `payload` for embedded Parasolid streams.
///
/// Returns direct `PS\0\0` streams when any are present; otherwise returns the
/// zlib members (`0x78 01/9c/da`) that inflate, under `inflate`, to a `PS\0\0`
/// prologue. See the [module docs](self) for the scan order and the divergences
/// from the codecs this consolidates.
pub fn locate_streams(payload: &[u8], inflate: Inflate) -> Vec<ParasolidStream> {
    let direct = direct_streams(payload);
    if direct.is_empty() {
        wrapped_streams(payload, inflate)
    } else {
        direct
    }
}

/// Collects direct (uncompressed) `PS\0\0` streams, each sliced up to the next
/// prologue.
///
/// A direct stream is kept only when its description-framed header validates.
/// Raw payloads can hold a coincidental `PS\0\0` inside adjacent compressed
/// data; the framing check rejects those the way sldprt's header parse does,
/// without which the direct scan would mask the wrapped scan on such a payload.
fn direct_streams(payload: &[u8]) -> Vec<ParasolidStream> {
    let signatures: Vec<usize> = payload
        .windows(4)
        .enumerate()
        .filter_map(|(at, four)| (four == PROLOGUE).then_some(at))
        .collect();
    let mut out = Vec::new();
    for (index, &start) in signatures.iter().enumerate() {
        let end = signatures
            .get(index.saturating_add(1))
            .copied()
            .unwrap_or(payload.len());
        let Some(bytes) = payload.get(start..end).map(<[u8]>::to_vec) else {
            continue;
        };
        if !header_frames_a_stream(&bytes) {
            continue;
        }
        out.push(ParasolidStream {
            offset: start,
            schema: schema_token(&bytes),
            bytes,
        });
    }
    out
}

/// Collects zlib members that inflate to a `PS\0\0` prologue, deduplicated by
/// inflated bytes.
fn wrapped_streams(payload: &[u8], inflate: Inflate) -> Vec<ParasolidStream> {
    let mut out: Vec<ParasolidStream> = Vec::new();
    let mut at = 0usize;
    while at.saturating_add(2) <= payload.len() {
        let is_member = payload.get(at) == Some(&0x78)
            && matches!(payload.get(at.saturating_add(1)), Some(0x01 | 0x9c | 0xda));
        if is_member {
            if let Some(member) = payload.get(at..) {
                if let Some(inflated) = inflate.inflate(member) {
                    if has_prologue(&inflated) && !out.iter().any(|stream| stream.bytes == inflated)
                    {
                        out.push(ParasolidStream {
                            offset: at,
                            schema: schema_token(&inflated),
                            bytes: inflated,
                        });
                    }
                }
            }
        }
        at = at.saturating_add(1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{has_prologue, locate_streams, schema_token, Inflate, ParasolidStream, PROLOGUE};
    use std::io::Write as _;

    use flate2::{write::ZlibEncoder, Compression};

    /// Builds a validly framed `PS\0\0` stream: prologue, big-endian
    /// description length, description, padding, a one-byte schema length, the
    /// `SCH_` token, and trailing record bytes.
    ///
    /// The trailing bytes are a long repetitive run so the stream compresses to
    /// a real deflate block rather than a stored (verbatim) one; otherwise the
    /// literal prologue would reappear inside the zlib member the tests embed.
    fn ps_stream(description: &str, schema: &str) -> Vec<u8> {
        let mut stream = PROLOGUE.to_vec();
        stream.extend_from_slice(&(description.len() as u16).to_be_bytes());
        stream.extend_from_slice(description.as_bytes());
        stream.push(0x00); // padding between description and the schema length
        stream.push(schema.len() as u8); // schema-length byte immediately before SCH_
        stream.extend_from_slice(schema.as_bytes());
        stream.extend_from_slice(b"\x00\x00 record body: ");
        stream.extend_from_slice(&[b'A'; 512]);
        stream
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).expect("write to in-memory encoder");
        encoder.finish().expect("finish in-memory encoder")
    }

    #[test]
    fn has_prologue_detects_and_rejects() {
        assert!(has_prologue(b"PS\x00\x00rest"));
        assert!(!has_prologue(b"PK\x03\x04"));
        assert!(!has_prologue(b"PS"));
        assert!(!has_prologue(&[]));
    }

    #[test]
    fn schema_token_reads_run_and_rejects_absent() {
        let stream = ps_stream("(partition)", "SCH_1900000_15006");
        assert_eq!(schema_token(&stream).as_deref(), Some("SCH_1900000_15006"));
        // A stream with no SCH_ marker.
        assert_eq!(schema_token(b"PS\x00\x00no schema here"), None);
        // The run stops at the first non-token byte (the space before `padding`).
        assert_eq!(
            schema_token(b"PS\x00\x00 SCH_ABC then more"),
            Some("SCH_ABC".into())
        );
    }

    #[test]
    fn locates_a_single_direct_stream() {
        let stream = ps_stream("(partition)", "SCH_1900000_15006");
        // A direct stream sitting at offset 0.
        let located = locate_streams(&stream, Inflate::Bounded);
        assert_eq!(
            located,
            vec![ParasolidStream {
                offset: 0,
                bytes: stream.clone(),
                schema: Some("SCH_1900000_15006".into()),
            }]
        );
    }

    #[test]
    fn locates_multiple_direct_streams_with_boundaries() {
        let first = ps_stream("(partition)", "SCH_1900000_15006");
        let second = ps_stream("(deltas)", "SCH_1900000_13006");
        let mut payload = first.clone();
        let boundary = payload.len();
        payload.extend_from_slice(&second);

        let located = locate_streams(&payload, Inflate::Bounded);
        assert_eq!(located.len(), 2);
        assert_eq!(located[0].offset, 0);
        assert_eq!(located[0].bytes, first);
        assert_eq!(located[1].offset, boundary);
        assert_eq!(located[1].bytes, second);
        assert_eq!(located[1].schema.as_deref(), Some("SCH_1900000_13006"));
    }

    #[test]
    fn locates_a_wrapped_member_at_offset() {
        let stream = ps_stream("(partition)", "SCH_1900000_15006");
        let member = zlib(&stream);
        // Prefix bytes that never contain 0x78 or a raw PS\0\0 signature.
        let mut payload = vec![0xAAu8, 0xBB, 0xCC, 0xDD];
        let member_at = payload.len();
        payload.extend_from_slice(&member);

        let located = locate_streams(&payload, Inflate::Bounded);
        assert_eq!(located.len(), 1);
        assert_eq!(located[0].offset, member_at);
        assert_eq!(located[0].bytes, stream);
        assert_eq!(located[0].schema.as_deref(), Some("SCH_1900000_15006"));
    }

    #[test]
    fn bounded_recovers_a_member_with_trailing_packed_bytes() {
        // The bounded strategy's reason for existing: a zlib member immediately
        // followed by bytes of the next packed stream still inflates to the
        // member's stream, with the trailing bytes ignored.
        let stream = ps_stream("(partition)", "SCH_1900000_15006");
        let mut member = zlib(&stream);
        member.extend_from_slice(b"bytes belonging to the following packed stream");
        let mut payload = vec![0xAAu8, 0xBB, 0xCC];
        let member_at = payload.len();
        payload.extend_from_slice(&member);

        let located = locate_streams(&payload, Inflate::Bounded);
        assert_eq!(located.len(), 1);
        assert_eq!(located[0].offset, member_at);
        assert_eq!(located[0].bytes, stream);
    }

    #[test]
    fn inflate_strategy_routes_to_the_chosen_inflater() {
        let stream = ps_stream("(partition)", "SCH_1900000_15006");
        let member = zlib(&stream);
        let mut payload = vec![0xAAu8, 0xBB, 0xCC];
        payload.extend_from_slice(&member);

        // Bounded inflates the real member.
        let bounded = locate_streams(&payload, Inflate::Bounded);
        assert_eq!(bounded.len(), 1);
        assert_eq!(bounded[0].bytes, stream);

        // A caller-supplied inflater is used in Bounded's place: it substitutes
        // its own PS\0\0 bytes, which the located stream then carries.
        fn substitute(_member: &[u8]) -> Option<Vec<u8>> {
            Some(b"PS\x00\x00 substituted by the caller".to_vec())
        }
        let custom = locate_streams(&payload, Inflate::With(substitute));
        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0].bytes, b"PS\x00\x00 substituted by the caller");

        // A stricter inflater that rejects the member — the knob a codec whose
        // truncation tolerance differs would supply — locates nothing.
        fn reject(_member: &[u8]) -> Option<Vec<u8>> {
            None
        }
        assert!(locate_streams(&payload, Inflate::With(reject)).is_empty());
    }

    #[test]
    fn wrapped_member_that_does_not_inflate_to_a_prologue_is_dropped() {
        // A zlib member of non-Parasolid bytes: located as a member, inflated,
        // but rejected for lacking the prologue.
        let member = zlib(b"preview thumbnail bytes, no parasolid prologue at all");
        let mut payload = vec![0xAAu8, 0xBB];
        payload.extend_from_slice(&member);
        assert!(locate_streams(&payload, Inflate::Bounded).is_empty());
    }

    #[test]
    fn direct_streams_take_precedence_over_wrapped() {
        // A payload holding both a direct stream and a wrapped member: the direct
        // stream is returned and the wrapped scan is not consulted.
        let direct = ps_stream("(partition)", "SCH_1900000_15006");
        let wrapped_inner = ps_stream("(deltas)", "SCH_1900000_13006");
        let mut payload = direct.clone();
        payload.extend_from_slice(&zlib(&wrapped_inner));

        let located = locate_streams(&payload, Inflate::Bounded);
        assert_eq!(located.len(), 1);
        assert_eq!(located[0].offset, 0);
        assert_eq!(located[0].schema.as_deref(), Some("SCH_1900000_15006"));
        // The last direct stream is sliced to the payload end, so its bytes lead
        // with the direct stream; the wrapped member is never inflated.
        assert!(located[0].bytes.starts_with(&direct));
    }

    #[test]
    fn unframed_prologue_is_not_admitted_as_a_direct_stream() {
        // Carries the prologue and a SCH_ run, but the bytes at offset 4..6 do
        // not frame a valid description length, so the direct gate rejects it.
        let payload = b"PS\x00\x00SCH_FOO and some trailing noise";
        assert!(locate_streams(payload, Inflate::Bounded).is_empty());
    }

    #[test]
    fn no_streams_returns_empty() {
        assert!(locate_streams(
            b"nothing here, no prologue, no zlib member",
            Inflate::Bounded
        )
        .is_empty());
        assert!(locate_streams(&[], Inflate::Bounded).is_empty());
    }
}
