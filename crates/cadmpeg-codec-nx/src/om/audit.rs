// SPDX-License-Identifier: Apache-2.0
//! Audit fields determine the complete framed row and its byte length.

use super::{state_index::StateIndexToken, state_tagged_value::StateTaggedValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AuditRecord {
    pub(crate) ordinal: StateIndexToken,
    pub(crate) frame_selector: Option<u8>,
    pub(crate) timestamp: u32,
    pub(crate) value: StateTaggedValue,
}

impl AuditRecord {
    pub(crate) fn byte_len(self) -> usize {
        7 + self.ordinal.raw().len() + self.value.raw().len() + self.frame_selector.map_or(0, |_| 4)
    }

    pub(crate) fn raw(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.byte_len());
        bytes.push(0x04);
        bytes.extend_from_slice(self.ordinal.raw());
        bytes.push(0x13);
        if let Some(selector) = self.frame_selector {
            bytes.extend_from_slice(&[0x04, 0x05, selector, 0x00]);
        }
        bytes.push(0xe0);
        bytes.extend_from_slice(&self.timestamp.to_be_bytes());
        bytes.extend_from_slice(self.value.raw());
        bytes
    }
}

/// Source positions are derived from the immutable fields and a checked origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AuditTrailRow {
    record: AuditRecord,
    base: usize,
    at: usize,
}

impl AuditTrailRow {
    pub(crate) fn new(base: usize, at: usize, record: AuditRecord) -> Option<Self> {
        base.checked_add(at.checked_add(record.byte_len())?)?;
        Some(Self { record, base, at })
    }

    pub(crate) fn record(self) -> AuditRecord {
        self.record
    }
    pub(crate) fn offset(self) -> usize {
        self.base + self.at
    }
    #[cfg(test)]
    pub(crate) fn end_offset(self) -> usize {
        self.base + self.local_end()
    }
    pub(crate) fn local_end(self) -> usize {
        self.at + self.record.byte_len()
    }
}

#[cfg(test)]
mod tests {
    use super::{AuditRecord, AuditTrailRow};
    use crate::om::{state_index::StateIndexToken, state_tagged_value::StateTaggedValue};

    #[test]
    fn audit_frame_derives_selector_and_source_extent() {
        let record = AuditRecord {
            ordinal: StateIndexToken::from_wire(2, &[2]).unwrap(),
            frame_selector: None,
            timestamp: 0x0102_0304,
            value: StateTaggedValue::read_at(&[0xa0, 0, 0], 0).unwrap(),
        };
        assert_eq!(record.raw(), [4, 2, 19, 224, 1, 2, 3, 4, 160, 0, 0]);
        let selected = AuditRecord {
            frame_selector: Some(7),
            ..record
        };
        assert_eq!(
            selected.raw(),
            [4, 2, 19, 4, 5, 7, 0, 224, 1, 2, 3, 4, 160, 0, 0]
        );
        assert_eq!(selected.byte_len(), 15);
        let row = AuditTrailRow::new(100, 2, selected).unwrap();
        assert_eq!((row.offset(), row.end_offset()), (102, 117));
        assert!(AuditTrailRow::new(usize::MAX - 14, 0, selected).is_none());
    }
}
