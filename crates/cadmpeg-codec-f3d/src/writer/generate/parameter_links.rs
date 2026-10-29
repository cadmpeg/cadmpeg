// SPDX-License-Identifier: Apache-2.0
//! Parameter-owner and companion frames authored from their typed links.

use super::records::native_lp_ascii;
use crate::records::parameters::{DesignParameterCompanion, DesignParameterOwner};
use cadmpeg_core::CodecError;

pub(crate) fn encode(
    out: &mut Vec<u8>,
    owners: &[DesignParameterOwner],
    companions: &[DesignParameterCompanion],
) -> Result<(), CodecError> {
    for owner in owners {
        encode_owner(out, owner)?;
        // A repeated indexed header terminates the owner frame. It is a
        // generated framing choice, independent of the source byte positions.
        header(out, owner.class_tag().as_str(), owner.record_index())?;
    }
    for companion in companions {
        header(
            out,
            companion.class_tag().as_str(),
            companion.record_index(),
        )?;
        out.extend_from_slice(&[0; 20]);
        reference(out, companion.owner_record_index(), 6);
        out.extend_from_slice(&companion.timestamp_micros().get().to_le_bytes());
        out.extend_from_slice(&[0; 8]);
    }
    Ok(())
}

fn header(out: &mut Vec<u8>, tag: &str, index: u32) -> Result<(), CodecError> {
    native_lp_ascii(out, tag)?;
    out.extend_from_slice(&index.to_le_bytes());
    Ok(())
}

fn reference(out: &mut Vec<u8>, index: u32, tail: usize) {
    out.push(1);
    out.extend_from_slice(&index.to_le_bytes());
    out.resize(out.len() + tail, 0);
}

fn encode_owner(out: &mut Vec<u8>, owner: &DesignParameterOwner) -> Result<(), CodecError> {
    header(out, owner.class_tag().as_str(), owner.record_index())?;
    out.extend_from_slice(&[0; 8]);
    let Some(scope) = owner.scope_record_index() else {
        out.push(1);
        out.extend_from_slice(&[0; 13]);
        reference(out, owner.parameter_record_index(), 6);
        out.extend_from_slice(&owner.owned_ordinal().to_le_bytes());
        out.extend_from_slice(&[0; 7]);
        reference(out, owner.companion_record_index(), 8);
        return Ok(());
    };
    let Some(local) = owner.local_ordinal() else {
        out.push(1);
        out.extend_from_slice(&[0; 13]);
        reference(out, owner.parameter_record_index(), 6);
        out.extend_from_slice(&owner.owned_ordinal().to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        reference(out, scope, 8);
        reference(out, owner.companion_record_index(), 7);
        reference(out, scope, 6);
        return Ok(());
    };
    out.extend_from_slice(&[1, 1, 0, 0, 0]);
    reference(out, scope, 6);
    out.extend_from_slice(&local.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&owner.evaluated_value().get().to_le_bytes());
    reference(out, owner.parameter_record_index(), 6);
    out.extend_from_slice(&owner.owned_ordinal().to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    match owner.variant() {
        None => reference(out, scope, 8),
        Some(variant) => {
            reference(out, scope, 6);
            out.extend_from_slice(&[1, variant, 0]);
        }
    }
    reference(out, owner.companion_record_index(), 7);
    reference(out, scope, 6);
    Ok(())
}
