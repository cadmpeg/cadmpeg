// SPDX-License-Identifier: Apache-2.0
//! Counted extrusion profile tokens with a shared duplicate-list witness.

use super::branch_items::BranchItems;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use super::unique_candidate;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtrudeProfileReferenceField {
    field_tag: u8,
    references: BranchItems<PayloadIndexToken>,
    primary_offset: u64,
    witness_offset: Option<u64>,
}

impl ExtrudeProfileReferenceField {
    pub(crate) fn field_tag(&self) -> u8 {
        self.field_tag
    }

    pub(crate) fn relocate(mut self, base: u64) -> Option<Self> {
        let width: u64 = self
            .references
            .as_slice()
            .iter()
            .map(|token| token.raw().len() as u64)
            .sum();
        self.primary_offset = base.checked_add(self.primary_offset)?;
        self.primary_offset.checked_add(width)?.checked_add(3)?;
        if let Some(offset) = self.witness_offset {
            let offset = base.checked_add(offset)?;
            offset.checked_add(width)?.checked_add(2)?;
            self.witness_offset = Some(offset);
        }
        Some(self)
    }

    pub(crate) fn references(
        &self,
    ) -> impl Iterator<Item = (PayloadIndexToken, u64, Option<u64>)> + '_ {
        let mut relative = 0;
        self.references.as_slice().iter().map(move |token| {
            let position = relative;
            relative += token.raw().len() as u64;
            (
                *token,
                self.primary_offset + position,
                self.witness_offset.map(|offset| offset + position),
            )
        })
    }
}

/// Decode the unique witnessed profile-reference field in an `EXTRUDE` payload.
pub(crate) fn extrude_profile_references(
    record: OperationPayload<'_>,
) -> Option<ExtrudeProfileReferenceField> {
    if record.name() != "EXTRUDE" {
        return None;
    }
    unique_candidate(
        (0..record.payload().len().saturating_sub(6)).filter_map(|start| {
            if record.payload().get(start..start + 2) != Some(&[0x01, 0x02])
                || record.payload().get(start + 3) != Some(&0x01)
            {
                return None;
            }
            extrude_profile_reference_field(record, start)
        }),
    )
}

fn extrude_profile_reference_field(
    record: OperationPayload<'_>,
    start: usize,
) -> Option<ExtrudeProfileReferenceField> {
    let count = *record.payload().get(start + 4)?;
    if count < 2 {
        return None;
    }
    let references_start = start + 5;
    let mut at = references_start;
    let mut references = Vec::with_capacity(usize::from(count - 1));
    for _ in 1..count {
        let token = PayloadIndexToken::read(record.payload().get(at..)?)?;
        at += token.raw().len();
        references.push(token);
    }
    if record.payload().get(at..at + 3) != Some(&[0x01, 0x03, 0x79]) {
        return None;
    }
    let encoded_references = record.payload().get(references_start..at)?;
    let witness_len = 2 + encoded_references.len() + 2;
    let witness_start = unique_candidate(
        record
            .payload()
            .windows(witness_len)
            .enumerate()
            .filter_map(|(witness_start, candidate)| {
                (candidate.starts_with(&[0x01, count])
                    && candidate.get(2..2 + encoded_references.len()) == Some(encoded_references)
                    && candidate.ends_with(&[0x00, 0x00]))
                .then_some(witness_start)
            }),
    );
    Some(ExtrudeProfileReferenceField {
        field_tag: record.payload()[start + 2],
        references: BranchItems::new(references).ok()?,
        primary_offset: (record.payload_offset() + references_start) as u64,
        witness_offset: witness_start.map(|start| (record.payload_offset() + start + 2) as u64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relocation_preserves_the_shared_witness_and_checks_both_complete_spans() {
        let bytes = b"\x01\x02\x00\x01\x03\xf0\x00\xf1\x01\x00\x01\x03\x79\x01\x03\xf0\x00\xf1\x01\x00\x00\x00";
        let field =
            extrude_profile_references(OperationPayload::new(bytes, 100, "EXTRUDE").unwrap())
                .unwrap();
        let relocated = field.clone().relocate(1000).unwrap();
        let rows: Vec<_> = relocated.references().collect();
        assert_eq!((rows[0].1, rows[0].2), (1105, Some(1115)));
        assert_eq!((rows[1].1, rows[1].2), (1107, Some(1117)));
        let maximum_base = u64::MAX - 100 - bytes.len() as u64;
        assert!(field.clone().relocate(maximum_base).is_some());
        assert!(field.relocate(maximum_base + 1).is_none());
        let no_witness = extrude_profile_references(
            OperationPayload::new(&bytes[..13], 100, "EXTRUDE").unwrap(),
        )
        .unwrap();
        assert!(no_witness.references().all(|row| row.2.is_none()));
        assert!(no_witness.clone().relocate(u64::MAX - 113).is_some());
        assert!(no_witness.relocate(u64::MAX - 112).is_none());
    }
}
