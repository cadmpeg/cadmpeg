// SPDX-License-Identifier: Apache-2.0
//! Complete compact class-0x5e records with four unassigned operands.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use crate::layout::b_four_reference_edge_payload as body;
use crate::wire::records::{ConsolidatedFamily, ConsolidatedRawFrame, ConsolidatedRecord};

use super::CatiaAllocationReferenceEncoding as Encoding;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FourReferenceOperand {
    pub(super) value: u32,
    pub(super) encoding: Encoding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FourReferenceEdgeWire")]
pub(crate) struct CatiaConsolidatedFourReferenceEdge {
    pub(super) id: String,
    pub(super) source_index: u64,
    pub(super) source_offset: u64,
    pub(super) frame: ConsolidatedRawFrame<u64>,
    pub(super) operands: [FourReferenceOperand; 4],
    pub(super) terminal: u8,
    pub(super) terminal_value: u32,
    pub(super) terminal_encoding: Encoding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FourReferenceEdgeWire {
    id: String,
    source_index: u64,
    source_offset: u64,
    frame: ConsolidatedRawFrame<u64>,
    operands: [FourReferenceOperand; 4],
    terminal: u8,
    terminal_value: u32,
    terminal_encoding: Encoding,
}

struct Fields {
    operands: [FourReferenceOperand; 4],
    terminal: u8,
    terminal_value: u32,
    terminal_encoding: Encoding,
}

fn operand(view: &mut View<'_>) -> Option<FourReferenceOperand> {
    let raw = view.u8()?;
    let encoding = match raw & 3 {
        1 => Encoding::BackwardDistance,
        3 => Encoding::OwnedChild,
        _ => return None,
    };
    Some(FourReferenceOperand {
        value: u32::from(raw >> 2),
        encoding,
    })
}

fn fields(payload: &[u8]) -> Option<Fields> {
    if payload.len() != body::LEN {
        return None;
    }
    let mut view = View::over_retained(payload);
    view.seek(body::OPERANDS)?;
    let operands = [
        operand(&mut view)?,
        operand(&mut view)?,
        operand(&mut view)?,
        operand(&mut view)?,
    ];
    view.seek(body::TERMINAL)?;
    let terminal = view.u8()?;
    if !matches!(terminal, 0x01 | 0x02 | 0x21 | 0x22 | 0x25 | 0x29 | 0x2a) || view.remaining() != 0
    {
        return None;
    }
    Some(Fields {
        operands,
        terminal,
        terminal_value: u32::from(terminal >> 2),
        terminal_encoding: if terminal & 3 == 1 {
            Encoding::BackwardDistance
        } else {
            Encoding::Selector2
        },
    })
}

fn valid_payload(record: &CatiaConsolidatedFourReferenceEdge) -> bool {
    fields(&record.frame.payload).is_some_and(|parsed| {
        parsed.operands == record.operands
            && parsed.terminal == record.terminal
            && parsed.terminal_value == record.terminal_value
            && parsed.terminal_encoding == record.terminal_encoding
    })
}

impl TryFrom<FourReferenceEdgeWire> for CatiaConsolidatedFourReferenceEdge {
    type Error = &'static str;

    fn try_from(wire: FourReferenceEdgeWire) -> Result<Self, Self::Error> {
        let record = Self {
            id: wire.id,
            source_index: wire.source_index,
            source_offset: wire.source_offset,
            frame: wire.frame,
            operands: wire.operands,
            terminal: wire.terminal,
            terminal_value: wire.terminal_value,
            terminal_encoding: wire.terminal_encoding,
        };
        if !valid_payload(&record) {
            return Err("four-reference edge fields disagree with the complete frame");
        }
        Ok(record)
    }
}

fn candidates<'a>(
    bytes: &'a [u8],
    records: &'a [ConsolidatedRecord],
) -> impl Iterator<Item = (&'a ConsolidatedRecord, &'a [u8], Fields)> + 'a {
    records
        .iter()
        .filter(|record| record.family() == ConsolidatedFamily::B && record.class() == 0x5e)
        .filter_map(|record| {
            let payload = bytes.get(record.payload()?)?;
            Some((record, payload, fields(payload)?))
        })
}

fn admit(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    record: &ConsolidatedRecord,
    payload: &[u8],
    parsed: Fields,
) -> Result<CatiaConsolidatedFourReferenceEdge, CodecError> {
    let mut retained = ctx.alloc_filled(body::LEN, 0u8, "CATIA four-reference edge bytes")?;
    retained.copy_from_slice(payload);
    Ok(CatiaConsolidatedFourReferenceEdge {
        id: format!("catia:consolidated:four-reference-edge#{ordinal}"),
        source_index: u64_from_index(record.source_index()),
        source_offset: u64_from_index(record.source_range().start),
        frame: ConsolidatedRawFrame::from_record(record, retained)?.into(),
        operands: parsed.operands,
        terminal: parsed.terminal,
        terminal_value: parsed.terminal_value,
        terminal_encoding: parsed.terminal_encoding,
    })
}

pub(super) fn derive(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedFourReferenceEdge>, CodecError> {
    let Some((record, payload, parsed)) = candidates(bytes, records).next() else {
        return Ok(Vec::new());
    };
    let count = candidates(bytes, records).count();
    ctx.charge_entities(u64_from_index(count), "admit CATIA four-reference edges")?;
    let first = admit(ctx, 0, record, payload, parsed)?;
    let mut output = ctx.alloc_filled(count, first, "CATIA four-reference edge records")?;
    for (ordinal, (target, (record, payload, parsed))) in output
        .iter_mut()
        .zip(candidates(bytes, records))
        .enumerate()
    {
        *target = admit(ctx, ordinal, record, payload, parsed)?;
    }
    Ok(output)
}

#[cfg(test)]
pub(super) fn validate(
    records: &[CatiaConsolidatedFourReferenceEdge],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (ordinal, record) in records.iter().enumerate() {
        let order_valid = ordinal == 0
            || (
                records[ordinal - 1].source_index,
                records[ordinal - 1].source_offset,
            ) < (record.source_index, record.source_offset);
        if record.id != format!("catia:consolidated:four-reference-edge#{ordinal}")
            || !valid_payload(record)
            || !order_valid
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "invalid four-reference edge source identity or payload".to_owned(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
