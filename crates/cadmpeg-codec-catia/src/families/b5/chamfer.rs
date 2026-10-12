// SPDX-License-Identifier: Apache-2.0
//! Object-id class-`3b` chamfer syntax and native source records.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::families::chamfer::ChamferParameters;
use crate::wire::bytes::read_f64_array;
use crate::wire::object_reference::ObjectReference;

pub(crate) struct B5Chamfer {
    pub(crate) pos: usize,
    pub(crate) end: usize,
    pub(crate) object_id: u32,
    pub(crate) references: [ObjectReference; 5],
    pub(crate) parameters: ChamferParameters,
}

pub(crate) fn payload(bytes: &[u8]) -> Option<([ObjectReference; 5], ChamferParameters)> {
    if bytes.first() != Some(&0x85) {
        return None;
    }
    let mut at = 1;
    let references = [
        ObjectReference::read(bytes, &mut at)?,
        ObjectReference::read(bytes, &mut at)?,
        ObjectReference::read(bytes, &mut at)?,
        ObjectReference::read(bytes, &mut at)?,
        ObjectReference::read(bytes, &mut at)?,
    ];
    if bytes.len().checked_sub(at)? != 22 {
        return None;
    }
    let controls = bytes.get(at..at + 6)?.try_into().ok()?;
    let parameters = ChamferParameters::decode(controls, read_f64_array::<2>(bytes, at + 6)?)?;
    Some((references, parameters))
}

pub(crate) fn records(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<B5Chamfer>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "catia_b5_chamfer_scan",
    )?;
    let mut records = Vec::new();
    let Some(last) = bytes.len().checked_sub(7) else {
        return Ok(records);
    };
    for pos in 0..last {
        if bytes.get(pos..pos + 3) != Some(&[0xb5, 0x03, 0x3b]) {
            continue;
        }
        let start = pos + 8;
        let end = start + usize::from(bytes[pos + 3]);
        let Some((references, parameters)) = bytes.get(start..end).and_then(payload) else {
            continue;
        };
        let Some(object_id) = View::u32_le_at(bytes, pos + 4) else {
            continue;
        };
        ctx.push_vec(
            &mut records,
            B5Chamfer {
                pos,
                end,
                object_id,
                references,
                parameters,
            },
            "catia_b5_chamfer_records",
        )?;
    }
    Ok(records)
}
