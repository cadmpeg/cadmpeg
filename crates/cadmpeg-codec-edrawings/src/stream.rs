// SPDX-License-Identifier: Apache-2.0
//! The `eModel` member: the wrapper that precedes the model stream, and the
//! zlib body that follows it.
//!
//! Layout (docs/formats/edrawings.md section 2):
//!
//! 1. a comment line `;; HSF V<major>.<minor> ` ended by `\n`;
//! 2. wrapper records, each opened by one tag byte: `I` (a `u32`), `B` (one
//!    flag byte and six `f32` bounds). Every record occurs at most once;
//! 3. the body record: the tag `Z`, one zlib stream, and the tail bytes after
//!    the stream's end.
//!
//! The reader consumes records in file order until the `Z` tag. A tag outside
//! the three is malformed; no search for the zlib header takes place. The
//! zlib stream is expanded under the session decompression caps into a new
//! decode space.

use cadmpeg_container::compression::inflate_zlib_member;
use cadmpeg_core::decode::{DecodeContext, ExpandSpec, View};
use cadmpeg_core::CodecError;

/// Bytes that open the comment line, up to the version number.
pub(crate) const COMMENT_PREFIX: &[u8] = b";; HSF V";
/// The byte that ends the comment line.
const COMMENT_END: u8 = b'\n';
/// The longest comment line the reader accepts, version number included.
const MAX_COMMENT_LEN: usize = 64;
/// Tag of the info record.
const TAG_INFO: u8 = b'I';
/// Tag of the bounds record.
const TAG_BOUNDS: u8 = b'B';
/// Tag of the body record.
const TAG_BODY: u8 = b'Z';
/// The most wrapper records read before the body record.
const MAX_WRAPPER_RECORDS: usize = 8;
/// Tail the observed files carry after the zlib stream: one zero byte.
const OBSERVED_TAIL: &[u8] = &[0];

/// The stream version the comment line declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamVersion {
    /// The version number as written, for example `19.10`.
    text: String,
}

impl StreamVersion {
    /// The version number as written.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }
}

/// The bounding box of the `B` record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Bounds {
    /// The byte after the tag. The observed files hold zero.
    pub(crate) flag: u8,
    /// Minimum corner, in the stream's length unit.
    pub(crate) min: [f32; 3],
    /// Maximum corner.
    pub(crate) max: [f32; 3],
}

/// The records before the body record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Wrapper {
    pub(crate) version: StreamVersion,
    /// The `u32` of the `I` record. The observed files hold 4.
    pub(crate) info: Option<u32>,
    pub(crate) bounds: Option<Bounds>,
    /// Offset in the member of the first byte after the `Z` tag.
    pub(crate) body_offset: usize,
}

/// A parsed `eModel` member with its model stream expanded.
pub(crate) struct EModel<'a> {
    /// The inflated model stream.
    pub(crate) body: View<'a>,
    /// Bytes of the zlib stream in the member.
    pub(crate) compressed_len: usize,
    /// Bytes after the zlib stream.
    pub(crate) tail: &'a [u8],
}

impl EModel<'_> {
    /// Whether the tail is the one zero byte the observed files carry.
    pub(crate) fn tail_is_observed(&self) -> bool {
        self.tail == OBSERVED_TAIL
    }
}

/// Reads the comment line from the front of `member`.
fn read_comment(member: &mut View<'_>) -> Result<StreamVersion, CodecError> {
    if member.take(COMMENT_PREFIX.len()) != Some(COMMENT_PREFIX) {
        return Err(CodecError::WrongFormat(
            "the eModel member does not open with the HSF comment".into(),
        ));
    }
    let mut line = Vec::new();
    loop {
        let byte = member.req_u8()?;
        if byte == COMMENT_END {
            break;
        }
        line.push(byte);
        if line.len() > MAX_COMMENT_LEN {
            return Err(CodecError::malformed(format_args!(
                "the HSF comment line exceeds {MAX_COMMENT_LEN} bytes"
            )));
        }
    }
    let text = std::str::from_utf8(&line)
        .map_err(|_| CodecError::Malformed("the HSF comment line is not ASCII".into()))?
        .trim_end_matches(' ');
    let is_number = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    match text.split_once('.') {
        Some((major, minor)) if is_number(major) && is_number(minor) => Ok(StreamVersion {
            text: text.to_owned(),
        }),
        _ => Err(CodecError::malformed(format_args!(
            "the HSF comment line names no version number: {text:?}"
        ))),
    }
}

/// Reads the wrapper records up to and including the `Z` tag. Does not touch
/// the zlib stream.
pub(crate) fn read_wrapper(member: View<'_>) -> Result<Wrapper, CodecError> {
    let mut cursor = member;
    let version = read_comment(&mut cursor)?;
    let mut info = None;
    let mut bounds = None;
    for _ in 0..MAX_WRAPPER_RECORDS {
        let tag_offset = cursor.position();
        match cursor.req_u8()? {
            TAG_INFO => {
                if info.replace(cursor.req_u32_le()?).is_some() {
                    return Err(repeated("I", tag_offset));
                }
            }
            TAG_BOUNDS => {
                let flag = cursor.req_u8()?;
                let mut corners = [0.0_f32; 6];
                for value in &mut corners {
                    *value = cursor.req_f32_le()?;
                }
                let [min_x, min_y, min_z, max_x, max_y, max_z] = corners;
                let parsed = Bounds {
                    flag,
                    min: [min_x, min_y, min_z],
                    max: [max_x, max_y, max_z],
                };
                if bounds.replace(parsed).is_some() {
                    return Err(repeated("B", tag_offset));
                }
            }
            TAG_BODY => {
                return Ok(Wrapper {
                    version,
                    info,
                    bounds,
                    body_offset: cursor.position(),
                });
            }
            other => {
                return Err(CodecError::malformed(format_args!(
                    "eModel wrapper record at offset {tag_offset} has the undefined tag {other:#04x}"
                )));
            }
        }
    }
    Err(CodecError::malformed(format_args!(
        "eModel wrapper holds no body record within {MAX_WRAPPER_RECORDS} records"
    )))
}

fn repeated(tag: &str, offset: usize) -> CodecError {
    CodecError::malformed(format_args!(
        "eModel wrapper record {tag} at offset {offset} occurs twice"
    ))
}

/// Reads the wrapper and expands the zlib body of an `eModel` member.
pub(crate) fn parse<'a>(
    ctx: &DecodeContext<'a>,
    member: View<'a>,
) -> Result<EModel<'a>, CodecError> {
    let wrapper = read_wrapper(member)?;
    let rest = member
        .child(wrapper.body_offset, member.end())
        .ok_or_else(|| CodecError::Malformed("eModel body lies outside the member".into()))?;
    let (body, compressed_len) = inflate_zlib_member(ctx, rest, ExpandSpec::Unknown)?;
    let tail_start = wrapper
        .body_offset
        .checked_add(compressed_len)
        .ok_or_else(|| CodecError::Malformed("eModel body length overflows".into()))?;
    let tail = member
        .child(tail_start, member.end())
        .map(View::window)
        .ok_or_else(|| CodecError::Malformed("eModel tail lies outside the member".into()))?;
    Ok(EModel {
        body,
        compressed_len,
        tail,
    })
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
    use cadmpeg_core::CodecError;

    use super::{parse, read_wrapper};
    use crate::test_support::{emodel, emodel_with, zlib_stored, DEFAULT_BOUNDS};

    /// Parses `member` as the root of a fresh session.
    fn parsed(member: &[u8], policy: &DecodePolicy) -> Result<(Vec<u8>, Vec<u8>), CodecError> {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(member, &arena, policy).unwrap();
        let model = parse(&ctx, root)?;
        Ok((model.body.window().to_vec(), model.tail.to_vec()))
    }

    #[test]
    fn the_observed_layout_parses() {
        let member = emodel(b"stream body");
        let wrapper = read_wrapper(View::over_retained(&member)).unwrap();
        assert_eq!(wrapper.version.text(), "19.10");
        assert_eq!(wrapper.info, Some(4));
        let bounds = wrapper.bounds.unwrap();
        assert_eq!(bounds.flag, 0);
        assert_eq!(
            bounds.min,
            [DEFAULT_BOUNDS[0], DEFAULT_BOUNDS[1], DEFAULT_BOUNDS[2]]
        );
        assert_eq!(
            bounds.max,
            [DEFAULT_BOUNDS[3], DEFAULT_BOUNDS[4], DEFAULT_BOUNDS[5]]
        );
        let (body, tail) = parsed(&member, &DecodePolicy::default()).unwrap();
        assert_eq!(body, b"stream body");
        assert_eq!(tail, [0]);
    }

    #[test]
    fn a_wrapper_without_optional_records_parses() {
        let member = emodel_with("19.10", &[], &zlib_stored(b"x"), &[]);
        let wrapper = read_wrapper(View::over_retained(&member)).unwrap();
        assert_eq!((wrapper.info, wrapper.bounds), (None, None));
        let (body, tail) = parsed(&member, &DecodePolicy::default()).unwrap();
        assert_eq!(body, b"x");
        assert!(tail.is_empty());
    }

    #[test]
    fn a_damaged_wrapper_is_an_error() {
        let good = emodel(b"x");
        let mut no_comment = good.clone();
        no_comment[0] = b'#';
        let mut bad_version = good.clone();
        bad_version[8] = b'x';
        let mut bad_tag = good.clone();
        bad_tag[15] = b'Q';
        let truncated_info = good[..17].to_vec();
        let truncated_bounds = good[..30].to_vec();
        let long_comment = [b";; HSF V".as_slice(), &[b'1'; 80]].concat();
        let repeated_info = emodel_with("19.10", &[b'I', 4, 0, 0, 0, b'I', 4, 0, 0, 0], &[], &[]);
        for (member, expected_wrong_format) in [
            (no_comment, true),
            (bad_version, false),
            (bad_tag, false),
            (truncated_info, false),
            (truncated_bounds, false),
            (long_comment, false),
            (repeated_info, false),
            (Vec::new(), true),
        ] {
            let error = read_wrapper(View::over_retained(&member)).unwrap_err();
            assert_eq!(
                matches!(error, CodecError::WrongFormat(_)),
                expected_wrong_format,
                "{error}"
            );
        }
    }

    #[test]
    fn a_wrapper_that_never_reaches_the_body_record_is_malformed() {
        let info = [b'I', 4, 0, 0, 0].repeat(9);
        let member = [b";; HSF V19.10 \n".as_slice(), &info].concat();
        assert!(matches!(
            read_wrapper(View::over_retained(&member)),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn a_truncated_or_damaged_zlib_stream_is_an_error() {
        let member = emodel(&[7_u8; 300]);
        let cut = member[..member.len() - 6].to_vec();
        let mut damaged = member.clone();
        let header = damaged.len() - zlib_stored(&[7_u8; 300]).len() - 1;
        damaged[header] = 0xff;
        for bad in [cut, damaged] {
            let error = parsed(&bad, &DecodePolicy::default()).unwrap_err();
            assert!(matches!(error, CodecError::Malformed(_)), "{error}");
        }
    }

    #[test]
    fn an_expansion_past_the_cap_is_a_resource_refusal() {
        let member = emodel(&[1_u8; 4096]);
        let mut policy = DecodePolicy::default();
        policy.limits.max_decompressed_bytes_per_expand = 64;
        let error = parsed(&member, &policy).unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(_)), "{error}");
    }
}
