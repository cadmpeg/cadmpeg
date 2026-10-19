// SPDX-License-Identifier: Apache-2.0
//! Exact ENLARGE construction prefix, independent of authored surface roles.

use super::operation_record::OperationPayload;
use super::reference_index::{FeatureReferenceToken, PayloadIndexToken};
use super::scalar::{PayloadScalarAtom, PayloadScalarEncoding};
use std::num::NonZeroU8;

pub(crate) const PARAMETER_CLASS: &str = "UGS::MODIFYSRF_isotrim_parms";
const SCALAR_PREFIX: [u8; 15] = [0, 0, 0, 0, 0, 0, 0xff, 0, 5, 2, 0xa0, 0x4c, 0x4a, 1, 5];
const REFERENCE_PREFIX: [u8; 4] = [1, 3, 8, 2];
const SUFFIX: [u8; 13] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnlargeEnvelope {
    pub(crate) parameter_class_index: NonZeroU8,
    pub(crate) references: [PayloadIndexToken; 6],
    pub(crate) middle: FeatureReferenceToken,
    pub(crate) scalars: [PayloadScalarAtom; 4],
}

impl EnlargeEnvelope {
    pub(crate) fn byte_len(&self) -> usize {
        5 + self.references.iter().map(|r| r.raw().len()).sum::<usize>()
            + self.middle.raw().len()
            + SCALAR_PREFIX.len()
            + self.scalars.iter().map(|s| s.raw().len()).sum::<usize>()
            + REFERENCE_PREFIX.len()
            + SUFFIX.len()
    }

    pub(crate) fn read(record: OperationPayload<'_>) -> Option<Self> {
        if record.name() != "ENLARGE" {
            return None;
        }
        let bytes = record.payload();
        let parameter_class_index = NonZeroU8::new(*bytes.first()?)?;
        if bytes.get(1..5) != Some(&[0, 0, 1, 0]) {
            return None;
        }
        let mut at = 5;
        let first = PayloadIndexToken::read(bytes.get(at..)?)?;
        let mut references = [first; 6];
        at += first.raw().len();
        for reference in &mut references[1..2] {
            *reference = PayloadIndexToken::read(bytes.get(at..)?)?;
            at += reference.raw().len();
        }
        let middle = FeatureReferenceToken::read(bytes.get(at..)?)?;
        at += middle.raw().len();
        if bytes.get(at..at + SCALAR_PREFIX.len()) != Some(&SCALAR_PREFIX) {
            return None;
        }
        at += SCALAR_PREFIX.len();
        let mut scalars = [PayloadScalarAtom::Zero; 4];
        for scalar in &mut scalars {
            *scalar = PayloadScalarAtom::read(bytes.get(at..)?)?;
            if scalar.encoding() == PayloadScalarEncoding::Binary32 {
                return None;
            }
            at += scalar.raw().len();
        }
        if bytes.get(at..at + REFERENCE_PREFIX.len()) != Some(&REFERENCE_PREFIX) {
            return None;
        }
        at += REFERENCE_PREFIX.len();
        for reference in &mut references[2..] {
            *reference = PayloadIndexToken::read(bytes.get(at..)?)?;
            at += reference.raw().len();
        }
        if bytes.get(at..at + SUFFIX.len()) != Some(&SUFFIX) {
            return None;
        }
        Some(Self {
            parameter_class_index,
            references,
            middle,
            scalars,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{EnlargeEnvelope, REFERENCE_PREFIX, SCALAR_PREFIX, SUFFIX};
    use crate::om::operation_record::OperationPayload;

    fn prefix() -> Vec<u8> {
        let mut bytes = vec![57, 0, 0, 1, 0, 0xf0, 0, 0xf1, 1, 0, 0x80, 0];
        bytes.extend(SCALAR_PREFIX);
        bytes.extend([0x2f, 0xf0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend(REFERENCE_PREFIX);
        bytes.extend([0xf0, 2, 0xf0, 3, 0xf0, 4, 0xf0, 5]);
        bytes.extend(SUFFIX);
        bytes
    }

    #[test]
    fn enlarge_prefix_preserves_zero_and_exact_token_widths() {
        let bytes = prefix();
        let field =
            EnlargeEnvelope::read(OperationPayload::new(&bytes, 10, "ENLARGE").unwrap()).unwrap();
        assert_eq!(field.byte_len(), bytes.len());
        assert_eq!(
            field
                .references
                .map(crate::om::reference_index::PayloadIndexToken::value),
            [0, 256, 2, 3, 4, 5]
        );
        assert_eq!(field.middle.raw(), [0x80, 0]);
        assert_eq!(field.scalars.map(|s| s.value().get()), [1.0, 0.0, 0.0, 0.0]);
        for end in 0..bytes.len() {
            assert!(EnlargeEnvelope::read(
                OperationPayload::new(&bytes[..end], 0, "ENLARGE").unwrap()
            )
            .is_none());
        }
        let mut zero = bytes.clone();
        zero[0] = 0;
        assert!(
            EnlargeEnvelope::read(OperationPayload::new(&zero, 0, "ENLARGE").unwrap()).is_none()
        );
        assert!(
            EnlargeEnvelope::read(OperationPayload::new(&bytes, 0, "EXTRACT_FACE").unwrap())
                .is_none()
        );
    }
}
