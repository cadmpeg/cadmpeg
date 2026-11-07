// SPDX-License-Identifier: Apache-2.0
//! Recover XML elements and map their spans to the original encoding.

use std::ops::Range;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(crate) struct XmlDamage {
    pub(crate) range: Range<usize>,
    pub(crate) name: String,
}

pub(crate) struct RecoveredXml {
    pub(crate) text: String,
    pub(crate) damaged: Vec<XmlDamage>,
}

pub(crate) fn recover(
    ctx: &DecodeContext<'_>,
    text: &str,
    source_offset: usize,
    utf16: bool,
) -> Result<RecoveredXml, CodecError> {
    let mut damaged = Vec::new();
    let (admitted, _) = cadmpeg_core::decode::parse_xml_recovering(
        ctx,
        View::over_retained(text.as_bytes()),
        "Rhino XML userdata",
        "scan Rhino XML elements",
        "Rhino XML userdata tree",
        &CodecError::malformed,
        |extent, name, _, _| {
            let position = |offset| -> Option<usize> {
                let offset = if utf16 {
                    text.get(..offset)?.encode_utf16().count().checked_mul(2)?
                } else {
                    offset
                };
                source_offset.checked_add(offset)
            };
            let (Some(start), Some(end)) = (position(extent.start()), position(extent.end()))
            else {
                return Err(CodecError::malformed("Rhino XML element offset overflows"));
            };
            damaged.push(XmlDamage {
                range: start..end,
                name: String::from_utf8_lossy(name).into_owned(),
            });
            Ok(())
        },
    )?;
    let text = std::str::from_utf8(admitted.window())
        .map_err(|_| CodecError::malformed("recovered Rhino XML is not UTF-8"))?
        .to_owned();
    Ok(RecoveredXml { text, damaged })
}
