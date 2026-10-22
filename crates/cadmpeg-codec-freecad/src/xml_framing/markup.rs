// SPDX-License-Identifier: Apache-2.0
//! Bounded delimiter navigation inside already parsed element extents.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use super::{relative, xml_space};

pub(super) fn fields<'a>(
    source: View<'a>,
    local_name: &str,
) -> Result<[Option<View<'a>>; 3], CodecError> {
    let raw = source.window();
    if raw.first() != Some(&b'<') {
        return Err(CodecError::malformed(
            "XML element extent lacks its opening marker",
        ));
    }
    let name_end = raw
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, byte)| xml_space(**byte) || matches!(**byte, b'/' | b'>'))
        .map(|(at, _)| at)
        .ok_or_else(|| CodecError::malformed("XML opening name has no boundary"))?;
    let name = &raw[1..name_end];
    if name.rsplit(|byte| *byte == b':').next() != Some(local_name.as_bytes()) {
        return Err(CodecError::malformed(
            "XML tree name differs from its source QName",
        ));
    }
    let mut quote = None;
    let mut close = None;
    for (at, &byte) in raw.iter().enumerate().skip(name_end) {
        if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
        } else if byte == b'>' {
            close = Some(at);
            break;
        }
    }
    let close = close.ok_or_else(|| CodecError::malformed("XML start tag has no unquoted end"))?;
    let after_open = close
        .checked_add(1)
        .ok_or_else(|| CodecError::malformed("XML start-tag end overflows"))?;
    let before = close
        .checked_sub(1)
        .ok_or_else(|| CodecError::malformed("XML start tag is empty"))?;
    let empty = raw.get(before) == Some(&b'/');
    let suffix = if empty { before } else { close };
    let closing = if empty {
        if after_open != raw.len() {
            return Err(CodecError::malformed(
                "empty XML element has trailing source bytes",
            ));
        }
        None
    } else {
        let at = raw
            .iter()
            .rposition(|byte| *byte == b'<')
            .filter(|at| *at >= after_open)
            .ok_or_else(|| CodecError::malformed("XML element lacks its closing tag"))?;
        let name_start = at
            .checked_add(2)
            .ok_or_else(|| CodecError::malformed("XML closing name overflows"))?;
        let name_end = name_start
            .checked_add(name.len())
            .ok_or_else(|| CodecError::malformed("XML closing name overflows"))?;
        let marker_end = at
            .checked_add(2)
            .ok_or_else(|| CodecError::malformed("XML closing marker overflows"))?;
        let end = raw
            .len()
            .checked_sub(1)
            .ok_or_else(|| CodecError::malformed("XML closing tag is empty"))?;
        if raw.get(at..marker_end) != Some(b"</".as_slice())
            || raw.get(name_start..name_end) != Some(name)
            || raw
                .get(name_end..end)
                .is_none_or(|space| !space.iter().all(|byte| xml_space(*byte)))
            || raw.last() != Some(&b'>')
        {
            return Err(CodecError::malformed(
                "XML closing tag differs from its original QName",
            ));
        }
        Some(relative(source, at..raw.len())?)
    };
    Ok([
        Some(relative(source, 0..name_end)?),
        Some(relative(source, suffix..after_open)?),
        closing,
    ])
}
