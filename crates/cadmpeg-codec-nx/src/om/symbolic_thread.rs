// SPDX-License-Identifier: Apache-2.0
//! Checked duplicate coordinate witness adjoining a symbolic-thread mode frame.

use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use super::scalar::{PayloadScalarAtom, PayloadScalarEncoding};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU8;

pub(crate) const PARAMETER_CLASS: &str = "UGS::SYMB_THD_PARMS";

/// A one-byte zero or finite shifted binary64, retaining its exact encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "CoordinateWire")]
pub(crate) struct ThreadCoordinate(PayloadScalarAtom);

#[derive(Deserialize)]
struct CoordinateWire {
    value: f64,
    raw: NativeBytes<Vec<u8>>,
}

impl TryFrom<CoordinateWire> for ThreadCoordinate {
    type Error = &'static str;

    fn try_from(wire: CoordinateWire) -> Result<Self, Self::Error> {
        let encoding = if wire.raw.as_slice() == [0] {
            PayloadScalarEncoding::Zero
        } else {
            PayloadScalarEncoding::Binary64
        };
        PayloadScalarAtom::from_wire(wire.value, encoding, &wire.raw).map(Self)
    }
}

impl Serialize for ThreadCoordinate {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct CoordinateRef<'a> {
            value: f64,
            raw: NativeBytes<&'a [u8]>,
        }
        CoordinateRef {
            value: self.0.value().get(),
            raw: self.0.raw().into(),
        }
        .serialize(serializer)
    }
}

impl ThreadCoordinate {
    fn read(bytes: &[u8]) -> Option<Self> {
        let scalar = PayloadScalarAtom::read(bytes)?;
        (!matches!(scalar, PayloadScalarAtom::Binary32(_))).then_some(Self(scalar))
    }

    fn byte_len(self) -> usize {
        self.0.raw().len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SymbolicThreadWitness {
    pub(crate) parameter_class_index: NonZeroU8,
    coordinates: [ThreadCoordinate; 3],
    tail_references: [PayloadIndexToken; 3],
    tail_sentinel: bool,
}

impl SymbolicThreadWitness {
    pub(crate) fn point_mm(&self) -> [f64; 3] {
        self.coordinates
            .map(|coordinate| coordinate.0.value().get() * 250.0)
    }

    pub(crate) fn byte_len(&self) -> usize {
        2 * self.coordinates.iter().map(|c| c.byte_len()).sum::<usize>()
            + 2
            + self
                .tail_references
                .iter()
                .map(|t| t.raw().len())
                .sum::<usize>()
            + usize::from(self.tail_sentinel)
    }

    /// The returned position is relative to the enclosing section's bytes.
    pub(crate) fn read(
        ctx: &DecodeContext<'_>,
        record: OperationPayload<'_>,
        cut_offset: usize,
    ) -> Result<Option<(Self, usize)>, CodecError> {
        if record.name() != "SYMBOLIC_THREAD" {
            return Ok(None);
        }
        let bytes = record.payload();
        let Some(parameter_class_index) = bytes.first().copied().and_then(NonZeroU8::new) else {
            return Ok(None);
        };
        let Some(cut) = cut_offset.checked_sub(record.payload_offset()) else {
            return Ok(None);
        };
        let mode_end = cut
            .checked_add(6)
            .ok_or_else(|| ctx.refuse_codec_limit("NX symbolic thread mode span", 0, 1))?;
        if !matches!(
            bytes.get(cut..mode_end),
            Some(b"\x03\x05CUT\0" | b"\x03\x05Cut\0")
        ) {
            return Ok(None);
        }
        // Twelve tail positions, 22 possible half widths, and at most 72 bytes
        // of exact-byte/scalar work per candidate; no unbounded payload scan.
        ctx.charge_work(12 * 22 * 72, "NX symbolic thread duplicate witness")?;
        let mut found = None;
        let Some(first_tail) = cut.checked_sub(12) else {
            return Ok(None);
        };
        for tail in first_tail..cut {
            if bytes.get(tail..tail + 2) != Some(&[1, 3]) {
                continue;
            }
            let mut cursor = tail + 2;
            let mut read_reference = || {
                let token = PayloadIndexToken::read(bytes.get(cursor..)?)?;
                cursor += token.raw().len();
                Some(token)
            };
            let [Some(first), Some(second), Some(third)] =
                std::array::from_fn::<_, 3, _>(|_| read_reference())
            else {
                continue;
            };
            let tail_sentinel = bytes.get(cursor) == Some(&255);
            cursor += usize::from(tail_sentinel);
            if cursor != cut {
                continue;
            }
            for width in 3..=24 {
                let Some(start) = tail.checked_sub(2 * width).filter(|start| *start > 0) else {
                    continue;
                };
                let middle = start + width;
                if bytes[start..middle] != bytes[middle..tail] {
                    continue;
                }
                let mut at = middle;
                let mut read_coordinate = || {
                    let coordinate = ThreadCoordinate::read(bytes.get(at..tail)?)?;
                    at += coordinate.byte_len();
                    Some(coordinate)
                };
                let [Some(x), Some(y), Some(z)] =
                    std::array::from_fn::<_, 3, _>(|_| read_coordinate())
                else {
                    continue;
                };
                if at != tail {
                    continue;
                }
                if found.is_some() {
                    return Ok(None);
                }
                found = Some((
                    Self {
                        parameter_class_index,
                        coordinates: [x, y, z],
                        tail_references: [first, second, third],
                        tail_sentinel,
                    },
                    record.payload_offset() + start,
                ));
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests;
