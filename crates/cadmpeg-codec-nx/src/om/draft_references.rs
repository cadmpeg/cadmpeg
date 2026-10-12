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
            .map(|token| cadmpeg_core::decode::u64_from_index(token.raw().len()))
            .sum::<u64>();
        origin.checked_add(
            2 * cadmpeg_core::decode::u64_from_index(GRAPH_PREFIX.len())
                + cadmpeg_core::decode::u64_from_index(MIDDLE.len())
                + 5
                + width,
        )?;
        self.origin = origin;
        Some(self)
    }
    pub(crate) fn references(&self) -> [(PayloadIndexToken, u64); 4] {
        let mut at = self.origin;
        std::array::from_fn(|slot| {
            at += cadmpeg_core::decode::u64_from_index(
                [GRAPH_PREFIX.len(), GRAPH_PREFIX.len(), MIDDLE.len(), 4][slot],
            );
            let offset = at;
            let token = self.tokens[slot];
            at += cadmpeg_core::decode::u64_from_index(token.raw().len());
            (token, offset)
        })
    }
}

pub(crate) fn draft_feature_payload_references(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<DraftFeaturePayloadReferenceField>, cadmpeg_core::CodecError> {
    if record.name() != "DRAFT"
        || record.payload().get(..PAYLOAD_PREFIX.len()) != Some(&PAYLOAD_PREFIX)
    {
        return Ok(None);
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
            origin: cadmpeg_core::decode::u64_from_index(record.payload_offset() + start),
        })
    };
    let Some(candidate_end) = record.payload().len().checked_sub(GRAPH_PREFIX.len()) else {
        return Ok(None);
    };
    let mut candidate = None;
    let mut starts = PAYLOAD_PREFIX.len()..=candidate_end;
    while let Some(start) = ctx.next_charged(
        &mut starts,
        "NX draft feature payload references candidate search",
    )? {
        if record.payload().get(start..start + GRAPH_PREFIX.len()) != Some(&GRAPH_PREFIX) {
            continue;
        }
        if let Some(next) = decode(start) {
            if candidate.is_some() {
                return Ok(None);
            }
            candidate = Some(next);
        }
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::super::operation_record::OperationPayload;
    use super::{draft_feature_payload_references, GRAPH_PREFIX, MIDDLE, PAYLOAD_PREFIX};

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
        let field = crate::test_support::with_decode_context(|ctx| {
            draft_feature_payload_references(
                ctx,
                OperationPayload::new(&payload, 100, "DRAFT").unwrap(),
            )
        })
        .unwrap()
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
        assert!(
            crate::test_support::with_decode_context(|ctx| draft_feature_payload_references(
                ctx,
                OperationPayload::new(&payload, 100, "DRAFT").unwrap()
            ))
            .unwrap()
            .is_none()
        );
    }
    #[test]
    fn draft_reference_candidate_range_refusal_precedes_rejection() {
        let mut bytes = PAYLOAD_PREFIX.to_vec();
        bytes.extend([0; 6]);
        crate::test_support::with_decode_context_over(
            &bytes,
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                let record = OperationPayload::new(&bytes, 0, "DRAFT").unwrap();
                let error = draft_feature_payload_references(ctx, record).unwrap_err();
                assert!(
                    matches!(&error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && limit.operation == "NX draft feature payload references candidate search"
                    && limit.additional == 1)
                );
                if let cadmpeg_core::CodecError::ResourceLimit(limit) = error {
                    assert_eq!(ctx.resource_refusal(), Some(limit));
                }
            },
        );
    }
}
