// SPDX-License-Identifier: Apache-2.0
//! Datum-plane common headers and exact construction forms.

use super::compact::LocatedCompactIndex;
use super::operation_record::OperationPayload;
use super::{reference_index, PayloadObjectReference};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SingleForm {
    Tag1b,
    Tag23,
    Tag28,
}

impl SingleForm {
    pub(crate) fn from_header(count: u8, tag: u8) -> Option<Self> {
        match (count, tag) {
            (2, 0x1b) => Some(Self::Tag1b),
            (2, 0x23) => Some(Self::Tag23),
            (3, 0x28) => Some(Self::Tag28),
            _ => None,
        }
    }

    pub(crate) fn header(self) -> (u8, u8) {
        match self {
            Self::Tag1b => (2, 0x1b),
            Self::Tag23 => (2, 0x23),
            Self::Tag28 => (3, 0x28),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DoubleForm {
    CountTwo,
    CountThree,
}

impl DoubleForm {
    pub(crate) fn from_header(count: u8, tag: u8) -> Option<Self> {
        match (count, tag) {
            (2, 0x29) => Some(Self::CountTwo),
            (3, 0x29) => Some(Self::CountThree),
            _ => None,
        }
    }

    pub(crate) fn count(self) -> u8 {
        match self {
            Self::CountTwo => 2,
            Self::CountThree => 3,
        }
    }
}

/// Common typed header preceding tag-specific datum-plane construction data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DatumPlanePayloadHeader {
    /// Payload control byte.
    pub control: u8,
    /// Declared construction count.
    pub declared_count: u8,
    /// Tag selecting the following construction branch.
    pub branch_tag: u8,
}

/// Datum-plane branch carrying one compact descriptor and one payload reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DatumPlaneSingleReferenceBranch {
    pub form: SingleForm,
    /// Non-null compact descriptor with its absolute source offset.
    pub descriptor: LocatedCompactIndex,
    /// Canonical payload object reference with its absolute source offset.
    pub object: PayloadObjectReference<reference_index::PayloadIndexToken>,
}

/// Two canonical references carried by a tag-`29` datum-plane branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DatumPlaneDoubleReferenceBranch {
    pub form: DoubleForm,
    /// Canonical payload object indices in branch order.
    pub references: [PayloadObjectReference<reference_index::PayloadIndexToken>; 2],
}

/// Decode the common header of a bounded `DATUM_PLANE` payload.
pub(crate) fn datum_plane_payload_header(
    record: OperationPayload<'_>,
) -> Option<DatumPlanePayloadHeader> {
    const PREFIX: [u8; 5] = [0x00, 0x00, 0x01, 0x00, 0x01];
    if record.name() != "DATUM_PLANE"
        || record.payload().get(1..6) != Some(&PREFIX)
        || record.payload().get(8..10) != Some(&[0x01, 0x02])
    {
        return None;
    }
    let declared_count = *record.payload().get(6)?;
    (declared_count >= 2).then_some(DatumPlanePayloadHeader {
        control: record.payload()[0],
        declared_count,
        branch_tag: record.payload()[7],
    })
}

/// Decode any datum-plane branch carrying one descriptor and one object reference.
pub(crate) fn datum_plane_descriptor_reference_branch(
    record: OperationPayload<'_>,
) -> Option<DatumPlaneSingleReferenceBranch> {
    const COUNT_TWO_SUFFIX: [u8; 12] = [
        0x00, 0x14, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00,
    ];
    const SEPARATOR: [u8; 4] = [0x01, 0x29, 0x01, 0x02];
    const SUFFIX: [u8; 35] = [
        0x01, 0x01, 0x07, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let header = datum_plane_payload_header(record)?;
    let form = SingleForm::from_header(header.declared_count, header.branch_tag)?;
    let (separator, suffix) = match form {
        SingleForm::Tag1b | SingleForm::Tag23 => ([0x01].as_slice(), COUNT_TWO_SUFFIX.as_slice()),
        SingleForm::Tag28 => (SEPARATOR.as_slice(), SUFFIX.as_slice()),
    };
    let mut at = 10;
    let mut descriptor = LocatedCompactIndex::read(record.payload(), at)?;
    at += descriptor.atom.raw().len();
    descriptor.offset += record.payload_offset();
    (record.payload().get(at..at + separator.len()) == Some(separator)).then_some(())?;
    at += separator.len();
    let object_offset = record.payload_offset() + at;
    let object_index = reference_index::PayloadIndexToken::read(record.payload().get(at..)?)?;
    at += object_index.raw().len();
    (record.payload().get(at..at + suffix.len()) == Some(suffix)).then_some(())?;
    Some(DatumPlaneSingleReferenceBranch {
        form,
        descriptor,
        object: PayloadObjectReference {
            token: object_index,
            offset: object_offset,
        },
    })
}

/// Decode either exact tag-`29` two-reference branch form.
pub(crate) fn datum_plane_double_reference_branch(
    record: OperationPayload<'_>,
) -> Option<DatumPlaneDoubleReferenceBranch> {
    const COUNT_TWO_MIDDLE: [u8; 11] = [
        0x01, 0x01, 0x18, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff,
    ];
    const COUNT_TWO_SUFFIX: [u8; 23] = [
        0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    const COUNT_THREE_MIDDLE: [u8; 5] = [0x01, 0x01, 0x3a, 0x01, 0x02];
    const COUNT_THREE_SUFFIX: [u8; 34] = [
        0x01, 0x17, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x0d,
    ];
    let header = datum_plane_payload_header(record)?;
    let form = DoubleForm::from_header(header.declared_count, header.branch_tag)?;
    let mut at = 10;
    let first_index = reference_index::PayloadIndexToken::read(record.payload().get(at..)?)?;
    let first = PayloadObjectReference {
        offset: record.payload_offset() + at,
        token: first_index,
    };
    at += first_index.raw().len();
    let middle = if form == DoubleForm::CountTwo {
        COUNT_TWO_MIDDLE.as_slice()
    } else {
        COUNT_THREE_MIDDLE.as_slice()
    };
    (record.payload().get(at..at + middle.len()) == Some(middle)).then_some(())?;
    at += middle.len();
    let second_index = reference_index::PayloadIndexToken::read(record.payload().get(at..)?)?;
    let second = PayloadObjectReference {
        offset: record.payload_offset() + at,
        token: second_index,
    };
    at += second_index.raw().len();
    let suffix = if form == DoubleForm::CountTwo {
        COUNT_TWO_SUFFIX.as_slice()
    } else {
        COUNT_THREE_SUFFIX.as_slice()
    };
    (record.payload().get(at..at + suffix.len()) == Some(suffix)).then_some(())?;
    Some(DatumPlaneDoubleReferenceBranch {
        form,
        references: [first, second],
    })
}

#[cfg(test)]
mod tests {
    use super::{DoubleForm, SingleForm};

    #[test]
    fn construction_forms_admit_only_their_header_pairs() {
        for count in u8::MIN..=u8::MAX {
            for tag in u8::MIN..=u8::MAX {
                let single = SingleForm::from_header(count, tag);
                assert_eq!(
                    single.is_some(),
                    matches!((count, tag), (2, 0x1b | 0x23) | (3, 0x28))
                );
                if let Some(form) = single {
                    assert_eq!(form.header(), (count, tag));
                }
                let double = DoubleForm::from_header(count, tag);
                assert_eq!(double.is_some(), matches!((count, tag), (2 | 3, 0x29)));
                if let Some(form) = double {
                    assert_eq!(form.count(), count);
                }
            }
        }
    }
}
