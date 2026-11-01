// SPDX-License-Identifier: Apache-2.0
//! Thread-definition text and exact nominal sizing.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveLength;
use serde::{Deserialize, Serialize};

use super::{PmDcFeature, PmDcFeatureLabel};
use crate::pmdc::Cursor;
use crate::record_identity::{Located, RecordPayload};

pub(crate) const TYPE: [u8; 16] = [
    0x1f, 0x7e, 0x08, 0xa4, 0xd4, 0x11, 0x5f, 0x95, 0x60, 0x00, 0xc6, 0xa4, 0xca, 0xe1, 0xfb, 0xb0,
];
const MILLIMETRES_PER_INCH: f64 = 25.4;

pub(crate) const fn definition_version_supported(version: u8) -> bool {
    matches!(version, 19 | 21)
}

pub(crate) type Definition = Located<DefinitionPayload>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DefinitionPayload {
    save_version_major: u8,
    pub(crate) header: super::PmDcLinkedHeader,
    nominal_size: String,
    designation: String,
    standard: String,
    controls: [u32; 6],
    class: String,
    values: [String; 8],
}

impl RecordPayload for DefinitionPayload {
    const KIND: &'static str = "thread-definition";
}

pub(crate) fn parse(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<DefinitionPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = super::linked_header(&mut cursor, version)?;
    let nominal_size = cursor.utf16(ctx, "thread nominal size")?;
    let designation = cursor.utf16(ctx, "thread designation")?;
    let standard = cursor.utf16(ctx, "thread standard")?;
    let controls = cursor.u32_array("thread definition controls")?;
    let class = cursor.utf16(ctx, "thread class")?;
    let values = [
        cursor.utf16(ctx, "thread table value 0")?,
        cursor.utf16(ctx, "thread table value 1")?,
        cursor.utf16(ctx, "thread table value 2")?,
        cursor.utf16(ctx, "thread table value 3")?,
        cursor.utf16(ctx, "thread table value 4")?,
        cursor.utf16(ctx, "thread table value 5")?,
        cursor.utf16(ctx, "thread table value 6")?,
        cursor.utf16(ctx, "thread pitch or threads per inch")?,
    ];
    cursor.finish("thread definition")?;
    Ok(DefinitionPayload {
        save_version_major: version,
        header,
        nominal_size,
        designation,
        standard,
        controls,
        class,
        values,
    })
}

#[derive(Debug, PartialEq)]
struct Sizing {
    diameter: PositiveLength,
    pitch: PositiveLength,
}

#[derive(Default)]
pub(crate) struct Fields {
    pub(crate) diameter: Option<PositiveLength>,
    pub(crate) pitch: Option<PositiveLength>,
    pub(crate) standard: Option<NonBlankString>,
    pub(crate) designation: Option<NonBlankString>,
}

pub(crate) fn fields(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    definitions: &[Definition],
) -> Fields {
    let Some(definition) = definition(source, label, definitions) else {
        return Fields::default();
    };
    let sizing = definition.sizing();
    Fields {
        diameter: sizing.as_ref().map(|value| value.diameter),
        pitch: sizing.as_ref().map(|value| value.pitch),
        standard: NonBlankString::new(definition.standard.clone()),
        designation: NonBlankString::new(definition.designation.clone()),
    }
}

fn definition<'a>(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    definitions: &'a [Definition],
) -> Option<&'a Definition> {
    let owner = source.identity.record_ordinal.checked_add(1)?;
    let token = source.identity.segment_token.as_str();
    if label.class_id() != super::THREAD_CLASS_ID
        || label.identity.segment_token.as_str() != token
        || label.header.owner.index() != owner
        || !label.header.owner.qualified()
    {
        return None;
    }
    let mut candidates = definitions.iter().filter(|definition| {
        definition.identity.segment_token.as_str() == token
            && definition.header.owner.index() == owner
    });
    let definition = candidates.next()?;
    if candidates.next().is_some()
        || definition.identity.type_id.as_str() != crate::pmdc::type_id_string(TYPE)
        || !definition_version_supported(definition.save_version_major)
        || source.save_version_major != definition.save_version_major
        || label.save_version_major != definition.save_version_major
        || !definition.header.owner.qualified()
        || !definition.header.parent.qualified()
    {
        return None;
    }
    Some(definition)
}

fn decimal(text: &str) -> Option<f64> {
    if text.is_empty()
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        || !text.bytes().any(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let value: f64 = text.parse().ok()?;
    (value.is_finite() && value > 0.0).then_some(value)
}

impl DefinitionPayload {
    fn sizing(&self) -> Option<Sizing> {
        let [zero, form, a, b, c, d] = self.controls;
        if !definition_version_supported(self.save_version_major)
            || [zero, a, b, c, d] != [0; 5]
            || (self.save_version_major == 21
                && (self.standard != "ANSI Metric M Profile" || !matches!(form, 0x10ea | 0x10ee)))
        {
            return None;
        }
        let nominal = decimal(&self.nominal_size)?;
        let pitch = decimal(&self.values[7])?;
        let (diameter, pitch) = match self.standard.as_str() {
            standard
                if (standard == "ANSI Metric M Profile"
                    && matches!(form, 0x00ca | 0x10ca | 0x10ce | 0x10ea | 0x10ee))
                    || (standard == "ISO Metric profile" && matches!(form, 0x00ca | 0x10ee)) =>
            {
                let (diameter, designated_pitch) =
                    self.designation.strip_prefix('M')?.split_once('x')?;
                if decimal(diameter)? != nominal || decimal(designated_pitch)? != pitch {
                    return None;
                }
                (nominal, pitch)
            }
            "ANSI Unified Screw Threads" if form == 0x18f2 => {
                let (size, rest) = self.designation.split_once('-')?;
                let (tpi, series) = rest.split_once(' ')?;
                if !matches!(series, "UNC" | "UNF") || decimal(tpi)? != pitch {
                    return None;
                }
                let diameter = if let Some((numerator, denominator)) = size.split_once('/') {
                    decimal(numerator)? / decimal(denominator)?
                } else {
                    decimal(size)?
                };
                if diameter != nominal {
                    return None;
                }
                (nominal * MILLIMETRES_PER_INCH, MILLIMETRES_PER_INCH / pitch)
            }
            _ => return None,
        };
        Some(Sizing {
            diameter: PositiveLength::new(diameter)?,
            pitch: PositiveLength::new(pitch)?,
        })
    }
}

pub(crate) fn partial_loss(count: usize) -> Option<cadmpeg_ir::report::loss::LossNote> {
    (count != 0).then(|| crate::loss::InventorLossCode::FeatureThreadDefinitionPartial.note(format!(
        "Transferred {count} Inventor thread annotation(s) with native face selections; class, unresolved sizing, handedness, and termination operands remain native."
    )))
}

#[cfg(test)]
mod tests;
