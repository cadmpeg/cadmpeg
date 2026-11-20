// SPDX-License-Identifier: Apache-2.0
//! Content detection for `SolidCraft` designs.
//!
//! A JSON object whose top-level `format` is a design format tag is
//! [`Confidence::High`]; an untagged object with top-level `params` and
//! `features` members is [`Confidence::Medium`]. A ZIP whose directory names
//! both `container.json` and `document.json` is [`Confidence::High`], one
//! whose directory or leading local members name either is
//! [`Confidence::Medium`], and any other ZIP is [`Confidence::Low`], as for
//! every ZIP-based format.

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Confidence;

use crate::document::design_version;
use crate::package::{DOCUMENT_MEMBER, MARKER_MEMBER};

/// ZIP local file header signature.
const LOCAL_HEADER_MAGIC: &[u8; 4] = b"PK\x03\x04";
/// Length of the fixed part of a ZIP local file header.
const LOCAL_HEADER_LEN: usize = 30;
/// Local-header offset of the general-purpose flags.
const LOCAL_FLAGS: usize = 6;
/// Local-header offset of the compressed size.
const LOCAL_COMPRESSED_SIZE: usize = 18;
/// Local-header offset of the name length.
const LOCAL_NAME_LEN: usize = 26;
/// Local-header offset of the extra-field length.
const LOCAL_EXTRA_LEN: usize = 28;
/// General-purpose flag bit of a size that follows the payload.
const FLAG_DATA_DESCRIPTOR: u16 = 0x0008;
/// UTF-8 byte order mark.
const BOM: &[u8] = &[0xef, 0xbb, 0xbf];

/// How strongly a byte prefix names a `SolidCraft` design.
pub(crate) fn confidence(ctx: &DecodeContext<'_>, prefix: &[u8]) -> Result<Confidence, CodecError> {
    let directory = if ArchiveSnapshot::has_footer(prefix) {
        match directory_confidence(ctx, prefix) {
            Ok(confidence) => confidence,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => Confidence::No,
        }
    } else {
        Confidence::No
    };
    if prefix.starts_with(LOCAL_HEADER_MAGIC) {
        let local = if leading_members_name_design(prefix) {
            Confidence::Medium
        } else {
            Confidence::Low
        };
        return Ok(directory.max(local));
    }
    if directory != Confidence::No {
        return Ok(directory);
    }
    Ok(json_confidence(prefix))
}

/// High when the indexed directory names both the marker and the document,
/// Medium when it names one of them.
fn directory_confidence(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Confidence, CodecError> {
    let mut marker = false;
    let mut document = false;
    ArchiveSnapshot::contains_matching_name(ctx, View::over_retained(bytes), |name| {
        marker |= name == MARKER_MEMBER;
        document |= name == DOCUMENT_MEMBER;
        marker && document
    })?;
    Ok(match (marker, document) {
        (true, true) => Confidence::High,
        (true, false) | (false, true) => Confidence::Medium,
        (false, false) => Confidence::No,
    })
}

/// Whether a local member that the prefix frames completely, from the first,
/// is the marker or the document.
fn leading_members_name_design(prefix: &[u8]) -> bool {
    let mut offset = 0_usize;
    while prefix
        .get(offset..)
        .is_some_and(|rest| rest.starts_with(LOCAL_HEADER_MAGIC))
    {
        let field = |at: usize| {
            offset
                .checked_add(at)
                .and_then(|at| View::u16_le_at(prefix, at))
        };
        let (Some(flags), Some(name_len), Some(extra_len)) = (
            field(LOCAL_FLAGS),
            field(LOCAL_NAME_LEN),
            field(LOCAL_EXTRA_LEN),
        ) else {
            return false;
        };
        let Some(compressed) = offset
            .checked_add(LOCAL_COMPRESSED_SIZE)
            .and_then(|at| View::u32_le_at(prefix, at))
        else {
            return false;
        };
        let Some(name_start) = offset.checked_add(LOCAL_HEADER_LEN) else {
            return false;
        };
        let Some(name_end) = name_start.checked_add(usize::from(name_len)) else {
            return false;
        };
        match prefix.get(name_start..name_end) {
            Some(name)
                if name == MARKER_MEMBER.as_bytes() || name == DOCUMENT_MEMBER.as_bytes() =>
            {
                return true
            }
            Some(_) => {}
            None => return false,
        }
        if flags & FLAG_DATA_DESCRIPTOR != 0 {
            return false;
        }
        let Some(next) = name_end
            .checked_add(usize::from(extra_len))
            .and_then(|at| at.checked_add(usize::try_from(compressed).ok()?))
        else {
            return false;
        };
        offset = next;
    }
    false
}

/// The confidence of a JSON prefix, from its top-level members.
fn json_confidence(prefix: &[u8]) -> Confidence {
    let prefix = prefix.strip_prefix(BOM).unwrap_or(prefix);
    let mut scan = TopLevel::new(prefix);
    let mut params = false;
    let mut features = false;
    while let Some(member) = scan.next_member() {
        match member {
            Member::Format(Some(text)) if design_version(&text).is_some() => {
                return Confidence::High
            }
            Member::Format(_) => return Confidence::No,
            Member::Key(key) => {
                params |= key == b"params";
                features |= key == b"features";
            }
        }
    }
    if scan.opened && params && features {
        Confidence::Medium
    } else {
        Confidence::No
    }
}

/// One top-level member that the scan reports.
enum Member<'a> {
    /// The `format` member and its string value, when it is a string.
    Format(Option<String>),
    /// Any other member key, unescaped bytes as written.
    Key(&'a [u8]),
}

/// A lexical scan over the top-level members of a JSON object prefix. It
/// tracks strings and nesting only; it stops at the first byte that a JSON
/// object cannot hold at its position.
struct TopLevel<'a> {
    bytes: &'a [u8],
    at: usize,
    opened: bool,
    done: bool,
}

impl<'a> TopLevel<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            at: 0,
            opened: false,
            done: false,
        }
    }

    fn skip_whitespace(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    /// The raw content of the string that starts at `self.at`, and moves
    /// past it.
    fn string(&mut self) -> Option<&'a [u8]> {
        if self.bytes.get(self.at) != Some(&b'"') {
            return None;
        }
        let start = self.at + 1;
        let mut at = start;
        loop {
            match self.bytes.get(at)? {
                b'\\' => at += 2,
                b'"' => {
                    self.at = at + 1;
                    return self.bytes.get(start..at);
                }
                _ => at += 1,
            }
        }
    }

    /// Moves past one value of any kind.
    fn skip_value(&mut self) -> Option<()> {
        let mut depth = 0_usize;
        loop {
            self.skip_whitespace();
            match *self.bytes.get(self.at)? {
                b'"' => {
                    self.string()?;
                }
                b'{' | b'[' => {
                    depth += 1;
                    self.at += 1;
                }
                b'}' | b']' => {
                    depth = depth.checked_sub(1)?;
                    self.at += 1;
                }
                b',' | b':' if depth > 0 => self.at += 1,
                b',' | b':' => return None,
                _ => {
                    while self.bytes.get(self.at).is_some_and(|byte| {
                        !matches!(byte, b',' | b'}' | b']' | b':') && !byte.is_ascii_whitespace()
                    }) {
                        self.at += 1;
                    }
                }
            }
            if depth == 0 {
                return Some(());
            }
        }
    }

    fn next_member(&mut self) -> Option<Member<'a>> {
        if self.done {
            return None;
        }
        let member = self.member();
        if member.is_none() {
            self.done = true;
        }
        member
    }

    fn member(&mut self) -> Option<Member<'a>> {
        self.skip_whitespace();
        if self.opened {
            match self.bytes.get(self.at)? {
                b',' => {
                    self.at += 1;
                    self.skip_whitespace();
                }
                _ => return None,
            }
        } else {
            if self.bytes.get(self.at) != Some(&b'{') {
                return None;
            }
            self.opened = true;
            self.at += 1;
            self.skip_whitespace();
        }
        let key = self.string()?;
        self.skip_whitespace();
        if self.bytes.get(self.at) != Some(&b':') {
            return None;
        }
        self.at += 1;
        self.skip_whitespace();
        if key == b"format" {
            let value = self
                .string()
                .and_then(|text| std::str::from_utf8(text).ok())
                .map(str::to_owned);
            if value.is_none() {
                self.skip_value()?;
            }
            return Some(Member::Format(value));
        }
        if self.skip_value().is_none() {
            self.done = true;
        }
        Some(Member::Key(key))
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::codec::{Codec, Confidence};
    use cadmpeg_test_support::zip_fixture::{archive, RawEntry};

    use crate::SolidcraftCodec;

    fn detect(bytes: &[u8]) -> Confidence {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default()).unwrap();
        let confidence = SolidcraftCodec.detect(&ctx, root).unwrap();
        ctx.finish_session().unwrap();
        confidence
    }

    #[test]
    fn tagged_json_designs_are_high() {
        for text in [
            r#"{"format":"solidcraft/57","name":"A","features":[]}"#,
            "\u{feff}\n {\n  \"name\": \"A\", \"params\": [{\"format\": 1}],\n  \"format\": \"solvecraft/13\"\n}",
        ] {
            assert_eq!(detect(text.as_bytes()), Confidence::High, "{text}");
        }
    }

    #[test]
    fn untagged_json_with_params_and_features_is_medium() {
        assert_eq!(
            detect(br#"{"name":"A","params":[],"features":[{"type":"box"}]}"#),
            Confidence::Medium
        );
    }

    #[test]
    fn other_json_is_no() {
        for text in [
            r#"{"format":"solidcraft-drawing/3","sheets":[]}"#,
            r#"{"format":"solidcraft/0","features":[]}"#,
            r#"{"ir_version":"11","geometry":{}}"#,
            r#"{"features":[]}"#,
            "[1,2]",
            "",
        ] {
            assert_eq!(detect(text.as_bytes()), Confidence::No, "{text}");
        }
    }

    #[test]
    fn a_zip_naming_marker_and_document_is_high() {
        let bytes = archive(&[
            RawEntry::stored("document.json", br#"{"format":"solidcraft/57"}"#),
            RawEntry::stored("container.json", br#"{"format":"solidcraft-container/1"}"#),
        ]);
        assert_eq!(detect(&bytes), Confidence::High);
    }

    #[test]
    fn a_zip_without_the_design_members_is_low() {
        let bytes = archive(&[RawEntry::stored("readme.txt", b"text")]);
        assert_eq!(detect(&bytes), Confidence::Low);
    }

    #[test]
    fn a_truncated_zip_with_a_leading_document_is_medium() {
        let bytes = archive(&[
            RawEntry::stored("document.json", br#"{"format":"solidcraft/57"}"#),
            RawEntry::stored("container.json", br#"{"format":"solidcraft-container/1"}"#),
        ]);
        assert_eq!(detect(&bytes[..60]), Confidence::Medium);
    }
}
