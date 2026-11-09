// SPDX-License-Identifier: Apache-2.0
//! Parameter frames authored from semantic values, without retained envelopes.

use crate::records::parameters::DesignParameter;

use super::records::{native_lp_ascii, native_lp_utf16};

pub(super) fn encode_document_parameter(
    out: &mut Vec<u8>,
    parameter: &DesignParameter,
    collection_index: u32,
) -> Result<(), crate::writer::WriteError> {
    let owner = parameter.owner_record_index();
    let discriminator = parameter
        .family_discriminator()
        .map(|value| value.value.code());
    let legacy = parameter.class_tag.as_str() == "287";
    if legacy && (owner.is_none() || discriminator.is_some()) {
        return Err(crate::writer::WriteError::Unwritable(format!(
            "class-287 parameter {} requires its owned compact family",
            parameter.id,
        )));
    }
    if owner.is_none() && discriminator.is_none() {
        return Err(crate::writer::WriteError::Unwritable(format!(
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
    } else if let Some(owner) = owner {
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&parameter.source_ordinal.to_le_bytes());
        out.push(1);
        out.extend_from_slice(&owner.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
    } else {
        debug_assert!(owner.is_some(), "compact parameters have an owner");
    }
    native_lp_utf16(out, parameter.expression())?;
    if discriminator.is_some() {
        out.extend_from_slice(&[0; 8]);
        out.push(u8::from(
            parameter.kind() == crate::records::parameters::DesignParameterKind::User,
        ));
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
    let mut tail = [0u8; 12];
    tail[1] = 1;
    tail[2..6].copy_from_slice(&collection_index.to_le_bytes());
    out.extend_from_slice(&tail);
    Ok(())
}

pub(super) fn encode_collection(
    out: &mut Vec<u8>,
    class_tag: &str,
    index: u32,
    parameter_indices: &[u32],
) -> Result<(), crate::writer::WriteError> {
    native_lp_ascii(out, class_tag)?;
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&[0; 15]);
    out.extend_from_slice(&0u32.to_le_bytes());
    collection_reference(out, 0);
    out.push(0);
    out.extend_from_slice(
        &u32::try_from(parameter_indices.len())
            .map_err(|_| {
                crate::writer::WriteError::Unwritable("F3D parameter collection exceeds u32".into())
            })?
            .to_le_bytes(),
    );
    for parameter in parameter_indices {
        collection_reference(out, *parameter);
    }
    collection_reference(out, 0);
    Ok(())
}

fn collection_reference(out: &mut Vec<u8>, index: u32) {
    out.push(1);
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
}

#[cfg(test)]
mod tests;
