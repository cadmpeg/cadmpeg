// SPDX-License-Identifier: Apache-2.0
//! Parameter frames authored from semantic values, without retained envelopes.

use std::collections::BTreeSet;

use cadmpeg_core::CodecError;

use crate::records::parameters::DesignParameter;

use super::records::{native_lp_ascii, native_lp_utf16};

pub(super) fn encode_document_parameters(
    parameters: &[DesignParameter],
) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    let mut indices = BTreeSet::new();
    let mut ordinals = BTreeSet::new();
    for parameter in parameters {
        if !indices.insert(parameter.record_index) || !ordinals.insert(parameter.source_ordinal) {
            return Err(CodecError::malformed(format_args!(
                "F3D Design parameter {} duplicates a record index or source ordinal",
                parameter.id,
            )));
        }
        encode(&mut out, parameter)?;
    }
    Ok(out)
}

fn encode(out: &mut Vec<u8>, parameter: &DesignParameter) -> Result<(), CodecError> {
    let owner = parameter.owner_record_index();
    let discriminator = parameter
        .family_discriminator()
        .map(|value| value.value.code());
    let legacy = parameter.class_tag.as_str() == "287";
    if legacy && (owner.is_none() || discriminator.is_some()) {
        return Err(CodecError::malformed(format_args!(
            "class-287 parameter {} requires its owned compact family",
            parameter.id,
        )));
    }
    if owner.is_none() && discriminator.is_none() {
        return Err(CodecError::malformed(format_args!(
            "user parameter {} requires its document family discriminator",
            parameter.id,
        )));
    }
    native_lp_ascii(out, parameter.class_tag.as_str())?;
    out.extend_from_slice(&parameter.record_index.to_le_bytes());
    out.extend_from_slice(&[0; 11]);
    if let Some(discriminator) = discriminator {
        out.extend_from_slice(&discriminator.to_le_bytes());
        out.push(0);
        out.extend_from_slice(&parameter.source_ordinal.to_le_bytes());
        match owner {
            Some(owner) => {
                out.push(1);
                out.extend_from_slice(&owner.to_le_bytes());
                out.extend_from_slice(&[0; 6]);
            }
            None => out.push(0),
        }
    } else {
        let owner = owner.ok_or_else(|| CodecError::malformed("compact parameter has no owner"))?;
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&parameter.source_ordinal.to_le_bytes());
        out.push(1);
        out.extend_from_slice(&owner.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
    }
    native_lp_utf16(out, parameter.expression())?;
    if discriminator.is_some() {
        out.extend_from_slice(&[0; 8]);
        out.push(u8::from(owner.is_none()));
    } else {
        out.extend_from_slice(&[0; 5]);
    }
    native_lp_utf16(out, parameter.source_kind_str())?;
    if discriminator.is_some() {
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    match parameter.unit() {
        Some(unit) => native_lp_utf16(out, unit.value.as_str())?,
        None => out.extend_from_slice(&0u32.to_le_bytes()),
    }
    native_lp_utf16(out, parameter.name())?;
    out.extend_from_slice(&parameter.evaluated_value().get().to_le_bytes());
    let family = if legacy {
        crate::layout::design_parameter_legacy_287_tail::FAMILY_MARKER_VALUE
    } else if matches!(discriminator, Some(5 | 6))
        && parameter.source_kind_str() != "TangencyWeight"
    {
        16
    } else {
        19
    };
    out.extend_from_slice(&[0, 1, family, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    Ok(())
}

#[cfg(test)]
mod tests;
