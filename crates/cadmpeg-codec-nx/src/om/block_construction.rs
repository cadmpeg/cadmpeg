// SPDX-License-Identifier: Apache-2.0
//! Fixed block-construction payload frame.

use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;

const TRAILER: [u8; 15] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockConstructionReferenceField {
    control: u8,
    origin: u64,
    tokens: [PayloadIndexToken; 19],
}

impl BlockConstructionReferenceField {
    pub(crate) fn control(&self) -> u8 {
        self.control
    }

    pub(crate) fn relocate(mut self, base: u64) -> Option<Self> {
        let origin = base.checked_add(self.origin)?;
        let width: u64 = self
            .tokens
            .iter()
            .map(|token| token.raw().len() as u64)
            .sum();
        origin.checked_add(6 + width + 1 + TRAILER.len() as u64)?;
        self.origin = origin;
        Some(self)
    }

    pub(crate) fn references(&self) -> [(PayloadIndexToken, u64); 19] {
        let mut at = self.origin + 6;
        std::array::from_fn(|slot| {
            if slot == 18 {
                at += 1;
            }
            let offset = at;
            let token = self.tokens[slot];
            at += token.raw().len() as u64;
            (token, offset)
        })
    }
}

pub(crate) fn block_construction_references(
    record: OperationPayload<'_>,
) -> Option<BlockConstructionReferenceField> {
    if record.name() != "BLOCK"
        || record.payload().get(1..6) != Some(&[0x00, 0x00, 0x01, 0x00, 0x00])
    {
        return None;
    }
    let first = PayloadIndexToken::read(record.payload().get(6..)?)?;
    let mut at = 6 + first.raw().len();
    let mut tokens = [first; 19];
    for (slot, token) in tokens.iter_mut().enumerate().skip(1) {
        if slot == 18 {
            if record.payload().get(at) != Some(&0x01) {
                return None;
            }
            at += 1;
        }
        *token = PayloadIndexToken::read(record.payload().get(at..)?)?;
        at += token.raw().len();
    }
    if record.payload().get(at..at + TRAILER.len()) != Some(&TRAILER) {
        return None;
    }
    Some(BlockConstructionReferenceField {
        control: record.payload()[0],
        origin: record.payload_offset() as u64,
        tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_frame_positions_include_token_widths_and_terminal_separator() {
        let mut payload = vec![0, 0, 0, 1, 0, 0];
        for slot in 0..18 {
            if slot % 2 == 0 {
                payload.extend([0xf0, 0]);
            } else {
                payload.extend([0xf1, 1, 0]);
            }
        }
        payload.extend([1, 0xf1, 2, 0]);
        payload.extend(TRAILER);
        let frame =
            block_construction_references(OperationPayload::new(&payload, 100, "BLOCK").unwrap())
                .unwrap();
        assert_eq!(frame.control(), 0);
        assert_eq!(frame.references()[0].1, 106);
        assert_eq!(frame.references()[1].1, 108);
        assert_eq!(frame.references()[2].1, 111);
        assert_eq!(frame.references()[17].1, 148);
        assert_eq!(frame.references()[18].1, 152);
        assert_eq!(frame.references()[18].0.value(), 512);
        assert_eq!(
            frame.clone().relocate(1000).unwrap().references()[18].1,
            1152
        );
        assert!(frame.clone().relocate(u64::MAX - 170).is_some());
        assert!(frame.relocate(u64::MAX - 169).is_none());
        payload[51] = 0;
        assert!(block_construction_references(
            OperationPayload::new(&payload, 100, "BLOCK").unwrap()
        )
        .is_none());
        payload[51] = 1;
        payload.pop();
        assert!(block_construction_references(
            OperationPayload::new(&payload, 100, "BLOCK").unwrap()
        )
        .is_none());
    }
}
