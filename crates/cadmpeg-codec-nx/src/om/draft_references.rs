// SPDX-License-Identifier: Apache-2.0
//! Exact draft construction graph with derived reference positions.

use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;

const PAYLOAD_PREFIX: [u8; 14] = [
    0x67, 0x00, 0x00, 0x01, 0x00, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0x03,
];
const GRAPH_PREFIX: [u8; 2] = [0x01, 0x02];
const MIDDLE: [u8; 35] = [
    0x68, 0x2f, 0x70, 0x62, 0x4d, 0xd2, 0xf1, 0xa9, 0xfc, 0x03, 0x50, 0x44, 0x00, 0x00, 0x01, 0x46,
    0x8a, 0x2a, 0x01, 0xa3, 0x60, 0x10, 0x01, 0x01, 0x01, 0x04, 0x02, 0x01, 0x02, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x01,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DraftFeaturePayloadReferenceField {
    origin: u64,
    tokens: [PayloadIndexToken; 4],
}

impl DraftFeaturePayloadReferenceField {
    pub(crate) fn relocate(mut self, base: u64) -> Option<Self> {
        let origin = base.checked_add(self.origin)?;
        let width = self
            .tokens
            .iter()
            .map(|token| token.raw().len() as u64)
            .sum::<u64>();
        origin.checked_add(2 * GRAPH_PREFIX.len() as u64 + MIDDLE.len() as u64 + 5 + width)?;
        self.origin = origin;
        Some(self)
    }
    pub(crate) fn references(&self) -> [(PayloadIndexToken, u64); 4] {
        let mut at = self.origin;
        std::array::from_fn(|slot| {
            at += [GRAPH_PREFIX.len(), GRAPH_PREFIX.len(), MIDDLE.len(), 4][slot] as u64;
            let offset = at;
            let token = self.tokens[slot];
            at += token.raw().len() as u64;
            (token, offset)
        })
    }
}

pub(crate) fn draft_feature_payload_references(
    record: OperationPayload<'_>,
) -> Option<DraftFeaturePayloadReferenceField> {
    if record.name() != "DRAFT"
        || record.payload().get(..PAYLOAD_PREFIX.len()) != Some(&PAYLOAD_PREFIX)
    {
        return None;
    }
    let decode = |start: usize| {
        let mut at = start + GRAPH_PREFIX.len();
        let decode_reference = |at: &mut usize| {
            let token = PayloadIndexToken::read(record.payload().get(*at..)?)?;
            *at += token.raw().len();
            Some(token)
        };
        let first = decode_reference(&mut at)?;
        (record.payload().get(at..at + GRAPH_PREFIX.len()) == Some(&GRAPH_PREFIX)).then_some(())?;
        at += GRAPH_PREFIX.len();
        let second = decode_reference(&mut at)?;
        (record.payload().get(at..at + MIDDLE.len()) == Some(&MIDDLE)).then_some(())?;
        at += MIDDLE.len();
        let third = decode_reference(&mut at)?;
        (record.payload().get(at..at + 4) == Some(&[0xff, 0x00, 0x00, 0x00])).then_some(())?;
        at += 4;
        let fourth = decode_reference(&mut at)?;
        (record.payload().get(at) == Some(&0xff)).then_some(())?;
        Some(DraftFeaturePayloadReferenceField {
            tokens: [first, second, third, fourth],
            origin: (record.payload_offset() + start) as u64,
        })
    };
    super::unique_candidate(
        (PAYLOAD_PREFIX.len()..=record.payload().len().saturating_sub(GRAPH_PREFIX.len()))
            .filter(|&start| {
                record.payload().get(start..start + GRAPH_PREFIX.len()) == Some(&GRAPH_PREFIX)
            })
            .filter_map(decode),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_graph_positions_follow_fixed_separators_and_token_widths() {
        let mut graph = GRAPH_PREFIX.to_vec();
        graph.extend([0xf0, 0]);
        graph.extend(GRAPH_PREFIX);
        graph.extend([0xf1, 1, 0]);
        graph.extend(MIDDLE);
        graph.extend([0xf0, 1, 0xff, 0, 0, 0, 0xf1, 2, 0, 0xff]);
        let mut payload = PAYLOAD_PREFIX.to_vec();
        payload.extend([0x55; 2]);
        payload.extend_from_slice(&graph);
        let field = draft_feature_payload_references(
            OperationPayload::new(&payload, 100, "DRAFT").unwrap(),
        )
        .unwrap();
        assert_eq!(
            field.references().map(|(token, _)| token.value()),
            [0, 256, 1, 512]
        );
        assert_eq!(
            field.references().map(|(_, offset)| offset),
            [118, 122, 160, 166]
        );
        assert_eq!(
            field.clone().relocate(1000).unwrap().references()[3].1,
            1166
        );
        assert!(field.clone().relocate(u64::MAX - 170).is_some());
        assert!(field.relocate(u64::MAX - 169).is_none());
        payload.extend(graph);
        assert!(draft_feature_payload_references(
            OperationPayload::new(&payload, 100, "DRAFT").unwrap()
        )
        .is_none());
    }
}
