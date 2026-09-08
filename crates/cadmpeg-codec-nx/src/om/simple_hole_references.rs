// SPDX-License-Identifier: Apache-2.0
//! Tagged reference pairs after the two simple-hole scalar witnesses.

use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;

pub const FIRST_PREFIX: [u8; 8] = [0x50, 0x10, 0x00, 0x04, 0x50, 0x49, 0x66, 0x2e];
pub const SECOND_PREFIX: [u8; 8] = [0x50, 0x21, 0x66, 0x62, 0x50, 0x49, 0x66, 0x2e];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferencePair {
    tokens: [PayloadIndexToken; 2],
    offset: usize,
    wrapped: bool,
}

impl ReferencePair {
    pub fn references(self) -> [(PayloadIndexToken, usize); 2] {
        [
            (self.tokens[0], self.offset),
            (self.tokens[1], self.offset + self.tokens[0].raw().len()),
        ]
    }

    pub fn wrapped(self) -> bool {
        self.wrapped
    }

    fn read(
        record: OperationPayload<'_>,
        coordinate_offset: usize,
        admitted_prefix: [u8; 8],
    ) -> Option<Self> {
        let relative = coordinate_offset.checked_sub(record.payload_offset())?;
        let mut at = relative.checked_add(8)?;
        let wrapped = if PayloadIndexToken::read(record.payload().get(at..)?).is_some() {
            false
        } else {
            (record.payload().get(at..at.checked_add(8)?)? == admitted_prefix).then_some(())?;
            at += 8;
            true
        };
        let first = PayloadIndexToken::read(record.payload().get(at..)?)?;
        let second =
            PayloadIndexToken::read(record.payload().get(at.checked_add(first.raw().len())?..)?)?;
        let offset = record.payload_offset().checked_add(at)?;
        offset
            .checked_add(first.raw().len())?
            .checked_add(second.raw().len())?;
        Some(Self {
            tokens: [first, second],
            offset,
            wrapped,
        })
    }
}

/// Decode both pairs without discarding their tagged token encodings.
pub fn simple_hole_repeated_scalar_lane_block_references(
    record: OperationPayload<'_>,
) -> Option<[ReferencePair; 2]> {
    let scalars = super::simple_hole_repeated_scalar_lane(record)?;
    let [first, second] = scalars.last().witness_offsets;
    Some([
        ReferencePair::read(record, first, FIRST_PREFIX)?,
        ReferencePair::read(record, second, SECOND_PREFIX)?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_pairs_derive_positions_from_exact_tagged_widths() {
        for (bytes, values, second_offset) in [
            (vec![0xf0, 0, 0xf1, 1, 0], [0, 256], 110),
            (vec![0xf1, 1, 0, 0xf0, 0], [256, 0], 111),
        ] {
            let mut payload = vec![0; 8];
            payload.extend(bytes);
            let record = OperationPayload::new(&payload, 100, "SIMPLE HOLE").unwrap();
            let pair = ReferencePair::read(record, 100, FIRST_PREFIX).unwrap();
            assert_eq!(pair.references().map(|(token, _)| token.value()), values);
            assert_eq!(
                pair.references().map(|(_, offset)| offset),
                [108, second_offset]
            );
            assert!(!pair.wrapped());
            let short =
                OperationPayload::new(&payload[..payload.len() - 1], 100, "SIMPLE HOLE").unwrap();
            assert!(ReferencePair::read(short, 100, FIRST_PREFIX).is_none());
        }
    }
}
