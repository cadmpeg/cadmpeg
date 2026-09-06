// SPDX-License-Identifier: Apache-2.0
//! Bounded operation records and their distinct payload and body scan inputs.

use super::OperationLabel;

/// One operation record bounded by consecutive validated operation headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationRecord<'a> {
    bytes: &'a [u8],
    label: OperationLabel<'a>,
}

/// A named payload fragment with its source position, independent of a record header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationPayload<'a> {
    payload: &'a [u8],
    payload_offset: usize,
    name: &'a str,
}

impl<'a> OperationPayload<'a> {
    #[cfg(test)]
    pub(crate) fn new(payload: &'a [u8], payload_offset: usize, name: &'a str) -> Option<Self> {
        payload_offset.checked_add(payload.len())?;
        Some(Self { payload, payload_offset, name })
    }

    pub(crate) fn payload(self) -> &'a [u8] { self.payload }
    pub(crate) fn payload_offset(self) -> usize { self.payload_offset }
    pub(crate) fn name(self) -> &'a str { self.name }
}

/// Byte range scanned for body clauses, with a bounded post-label payload suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationBodyInput<'a> {
    bytes: &'a [u8],
    offset: usize,
    payload_start: usize,
    name: &'a str,
}

impl<'a> OperationBodyInput<'a> {
    #[cfg(test)]
    pub(crate) fn new(bytes: &'a [u8], offset: usize, payload_start: usize, name: &'a str) -> Option<Self> {
        bytes.get(payload_start..)?;
        offset.checked_add(bytes.len())?;
        Some(Self { bytes, offset, payload_start, name })
    }

    pub(crate) fn bytes(self) -> &'a [u8] { self.bytes }
    pub(crate) fn offset(self) -> usize { self.offset }
    pub(crate) fn payload_start(self) -> usize { self.payload_start }
    pub(crate) fn payload(self) -> &'a [u8] { &self.bytes[self.payload_start..] }
    pub(crate) fn payload_offset(self) -> usize { self.offset + self.payload_start }
    pub(crate) fn name(self) -> &'a str { self.name }
    pub(crate) fn payload_view(self) -> OperationPayload<'a> {
        OperationPayload { payload: self.payload(), payload_offset: self.payload_offset(), name: self.name }
    }
}

impl<'a> OperationRecord<'a> {
    pub(super) fn new(bytes: &'a [u8], label: OperationLabel<'a>) -> Option<Self> {
        let label_start = usize::from(label.header.byte_len());
        let label_end = label_start.checked_add(label.value.len())?.checked_add(2)?;
        if bytes.get(label_start) != Some(&0x03)
            || usize::from(*bytes.get(label_start + 1)?) != label.value.len() + 2
            || bytes.get(label_start + 2..label_end)? != label.value.as_bytes()
            || bytes.get(label_end) != Some(&0)
        {
            return None;
        }
        label.header.offset().checked_add(bytes.len())?;
        Some(Self { bytes, label })
    }

    pub(crate) fn label(self) -> OperationLabel<'a> { self.label }
    pub(crate) fn bytes(self) -> &'a [u8] { self.bytes }
    pub(crate) fn offset(self) -> usize { self.label.header.offset() }
    fn payload_start(self) -> usize { usize::from(self.label.header.byte_len()) + self.label.value.len() + 3 }
    pub(crate) fn payload(self) -> &'a [u8] { &self.bytes[self.payload_start()..] }
    pub(crate) fn payload_offset(self) -> usize { self.offset() + self.payload_start() }
    pub(crate) fn body_view(self) -> OperationBodyInput<'a> {
        OperationBodyInput { bytes: self.bytes, offset: self.offset(), payload_start: self.payload_start(), name: self.label.value }
    }
    pub(crate) fn payload_view(self) -> OperationPayload<'a> {
        self.body_view().payload_view()
    }
}

#[cfg(test)]
mod tests {
    // Fixture constructors deliberately require valid checked spans.
    #![allow(clippy::unwrap_used)]

    use super::{OperationBodyInput, OperationPayload, OperationRecord};
    use crate::om::OperationLabel;
    use crate::om::header_references::{HeaderReferences, OperationHeader};

    fn label(offset: usize) -> OperationLabel<'static> {
        OperationLabel {
            header: OperationHeader::<usize>::new(offset, HeaderReferences([None; 4])).unwrap(),
            value: "BLOCK",
        }
    }

    #[test]
    fn record_payload_and_scan_ranges_derive_from_the_label_frame() {
        let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x07BLOCK\0payload";
        let record = OperationRecord::new(bytes, label(100)).unwrap();
        assert_eq!(record.bytes(), bytes);
        assert_eq!(record.label(), label(100));
        assert_eq!(record.offset(), 100);
        assert_eq!(record.payload_offset(), 127);
        assert_eq!(record.payload(), b"payload");
        let payload = record.payload_view();
        assert_eq!(payload.payload(), b"payload");
        assert_eq!(payload.payload_offset(), 127);
        assert_eq!(payload.name(), "BLOCK");
        let body = record.body_view();
        assert_eq!(body.bytes(), bytes);
        assert_eq!(body.offset(), 100);
        assert_eq!(body.payload_start(), 27);
        assert_eq!(body.payload_view(), payload);
        for end in 0..27 {
            assert!(OperationRecord::new(&bytes[..end], label(100)).is_none());
        }
        assert!(OperationRecord::new(&bytes[..27], label(100)).unwrap().payload().is_empty());
        assert!(OperationRecord::new(bytes, label(usize::MAX - bytes.len())).is_some());
        assert!(OperationRecord::new(bytes, label(usize::MAX - bytes.len() + 1)).is_none());
        for (offset, value) in [(19, 4), (20, 6), (21, b'X'), (26, 1)] {
            let mut invalid = *bytes;
            invalid[offset] = value;
            assert!(OperationRecord::new(&invalid, label(100)).is_none());
        }
    }

    #[test]
    fn body_scan_preserves_prefix_references_and_excludes_payload_body_writes() {
        let bytes = b"\x01\x02\x10\x42\xff\x00\x01\x02\x0b\x21\x97\x75\x01\x02\x10\x22\xff";
        let body = OperationBodyInput::new(bytes, 100, 6, "EXTRUDE").unwrap();
        let references = crate::om::operation_body_references(body);
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].offset, 103);
        assert_eq!(references[0].object_index.value(), 0x42);
        assert_eq!(body.payload(), &bytes[6..]);
        assert_eq!(body.payload_offset(), 106);
        assert!(OperationBodyInput::new(bytes, 100, bytes.len() + 1, "EXTRUDE").is_none());
        assert!(OperationBodyInput::new(bytes, usize::MAX - bytes.len(), bytes.len(), "EXTRUDE").is_some());
        assert!(OperationBodyInput::new(bytes, usize::MAX - bytes.len() + 1, 0, "EXTRUDE").is_none());
    }

    #[test]
    fn payload_fragment_bounds_do_not_require_a_record_header() {
        let payload = OperationPayload::new(b"x", usize::MAX - 1, "").unwrap();
        assert_eq!(payload.payload(), b"x");
        assert_eq!(payload.payload_offset(), usize::MAX - 1);
        assert_eq!(payload.name(), "");
        assert!(OperationPayload::new(b"x", usize::MAX, "").is_none());
        assert!(OperationPayload::new(b"", usize::MAX, "").is_some());
    }
}
