// SPDX-License-Identifier: Apache-2.0
//! Datum-plane common headers and exact construction forms.

use super::compact::CompactIndexAtom;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;

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

    fn separator(self) -> &'static [u8] {
        match self {
            Self::Tag1b | Self::Tag23 => &[0x01],
            Self::Tag28 => &[0x01, 0x29, 0x01, 0x02],
        }
    }

    fn suffix(self) -> &'static [u8] {
        const COUNT_TWO_SUFFIX: [u8; 12] = [
            0x00, 0x14, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00,
        ];
        const SUFFIX: [u8; 35] = [
            0x01, 0x01, 0x07, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0xff,
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
        ];
        match self {
            Self::Tag1b | Self::Tag23 => &COUNT_TWO_SUFFIX,
            Self::Tag28 => &SUFFIX,
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

    fn separator(self) -> &'static [u8] {
        const COUNT_TWO_MIDDLE: [u8; 11] = [
            0x01, 0x01, 0x18, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff,
        ];
        const COUNT_THREE_MIDDLE: [u8; 5] = [0x01, 0x01, 0x3a, 0x01, 0x02];
        match self {
            Self::CountTwo => &COUNT_TWO_MIDDLE,
            Self::CountThree => &COUNT_THREE_MIDDLE,
        }
    }

    fn suffix(self) -> &'static [u8] {
        const COUNT_TWO_SUFFIX: [u8; 23] = [
            0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
        ];
        const COUNT_THREE_SUFFIX: [u8; 34] = [
            0x01, 0x17, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
        ];
        match self {
            Self::CountTwo => &COUNT_TWO_SUFFIX,
            Self::CountThree => &COUNT_THREE_SUFFIX,
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

/// Exact construction references without redundant source positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DatumPlaneBranch<B> {
    Single {
        form: SingleForm,
        descriptor: (CompactIndexAtom, B),
        object: (PayloadIndexToken, B),
    },
    Double {
        form: DoubleForm,
        objects: [(PayloadIndexToken, B); 2],
    },
}

impl<B> DatumPlaneBranch<B> {
    fn byte_len(&self) -> u64 {
        let length = match self {
            Self::Single {
                form,
                descriptor,
                object,
            } => {
                descriptor.0.raw().len()
                    + form.separator().len()
                    + object.0.raw().len()
                    + form.suffix().len()
            }
            Self::Double {
                form,
                objects: [first, second],
            } => {
                first.0.raw().len()
                    + form.separator().len()
                    + second.0.raw().len()
                    + form.suffix().len()
            }
        };
        10 + length as u64
    }
}

/// A complete construction prefix whose reference locations derive from one origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DatumPlaneFrame<B> {
    origin: u64,
    branch: DatumPlaneBranch<B>,
}

impl<B> DatumPlaneFrame<B> {
    pub(crate) fn new(origin: u64, branch: DatumPlaneBranch<B>) -> Option<Self> {
        origin.checked_add(branch.byte_len())?;
        Some(Self { origin, branch })
    }

    pub(crate) fn origin(&self) -> u64 {
        self.origin
    }

    pub(crate) fn header(&self) -> (u8, u8) {
        match &self.branch {
            DatumPlaneBranch::Single { form, .. } => form.header(),
            DatumPlaneBranch::Double { form, .. } => (form.count(), 0x29),
        }
    }

    pub(crate) fn descriptor(&self) -> Option<(&CompactIndexAtom, &B, u64)> {
        match &self.branch {
            DatumPlaneBranch::Single { descriptor, .. } => {
                Some((&descriptor.0, &descriptor.1, self.origin + 10))
            }
            DatumPlaneBranch::Double { .. } => None,
        }
    }

    pub(crate) fn objects(&self) -> impl Iterator<Item = (&PayloadIndexToken, &B, u64)> {
        let references = match &self.branch {
            DatumPlaneBranch::Single {
                form,
                descriptor,
                object,
            } => [
                Some((
                    &object.0,
                    &object.1,
                    self.origin
                        + 10
                        + descriptor.0.raw().len() as u64
                        + form.separator().len() as u64,
                )),
                None,
            ],
            DatumPlaneBranch::Double {
                form,
                objects: [first, second],
            } => [
                Some((&first.0, &first.1, self.origin + 10)),
                Some((
                    &second.0,
                    &second.1,
                    self.origin + 10 + first.0.raw().len() as u64 + form.separator().len() as u64,
                )),
            ],
        };
        references.into_iter().flatten()
    }

    pub(crate) fn relocate(self, base: u64) -> Option<Self> {
        Self::new(self.origin.checked_add(base)?, self.branch)
    }
}

impl DatumPlaneFrame<()> {
    pub(crate) fn resolve<B>(
        &self,
        mut block: impl FnMut(u32) -> Option<B>,
    ) -> Option<DatumPlaneFrame<B>> {
        let branch = match &self.branch {
            DatumPlaneBranch::Single {
                form,
                descriptor,
                object,
            } => DatumPlaneBranch::Single {
                form: *form,
                descriptor: (descriptor.0, block(descriptor.0.value())?),
                object: (object.0, block(object.0.value())?),
            },
            DatumPlaneBranch::Double {
                form,
                objects: [first, second],
            } => DatumPlaneBranch::Double {
                form: *form,
                objects: [
                    (first.0, block(first.0.value())?),
                    (second.0, block(second.0.value())?),
                ],
            },
        };
        Some(DatumPlaneFrame {
            origin: self.origin,
            branch,
        })
    }
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
) -> Option<DatumPlaneFrame<()>> {
    let header = datum_plane_payload_header(record)?;
    let form = SingleForm::from_header(header.declared_count, header.branch_tag)?;
    let separator = form.separator();
    let suffix = form.suffix();
    let mut at = 10;
    let descriptor = CompactIndexAtom::read(record.payload().get(at..)?)?;
    at += descriptor.raw().len();
    (record.payload().get(at..at + separator.len()) == Some(separator)).then_some(())?;
    at += separator.len();
    let object_index = PayloadIndexToken::read(record.payload().get(at..)?)?;
    at += object_index.raw().len();
    (record.payload().get(at..at + suffix.len()) == Some(suffix)).then_some(())?;
    DatumPlaneFrame::new(
        record.payload_offset() as u64,
        DatumPlaneBranch::Single {
            form,
            descriptor: (descriptor, ()),
            object: (object_index, ()),
        },
    )
}

/// Decode either exact tag-`29` two-reference branch form.
pub(crate) fn datum_plane_double_reference_branch(
    record: OperationPayload<'_>,
) -> Option<DatumPlaneFrame<()>> {
    let header = datum_plane_payload_header(record)?;
    let form = DoubleForm::from_header(header.declared_count, header.branch_tag)?;
    let mut at = 10;
    let first_index = PayloadIndexToken::read(record.payload().get(at..)?)?;
    at += first_index.raw().len();
    let middle = form.separator();
    (record.payload().get(at..at + middle.len()) == Some(middle)).then_some(())?;
    at += middle.len();
    let second_index = PayloadIndexToken::read(record.payload().get(at..)?)?;
    at += second_index.raw().len();
    let suffix = form.suffix();
    (record.payload().get(at..at + suffix.len()) == Some(suffix)).then_some(())?;
    DatumPlaneFrame::new(
        record.payload_offset() as u64,
        DatumPlaneBranch::Double {
            form,
            objects: [(first_index, ()), (second_index, ())],
        },
    )
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

    #[test]
    fn frame_positions_and_end_bounds_follow_each_form() {
        use super::{CompactIndexAtom, DatumPlaneBranch, DatumPlaneFrame, PayloadIndexToken};
        let descriptor = CompactIndexAtom::from_wire(4096, &[0x90, 0]).unwrap();
        let first = PayloadIndexToken::from_wire(0, &[0xf0, 0]).unwrap();
        let second = PayloadIndexToken::from_wire(256, &[0xf1, 1, 0]).unwrap();
        for (form, object_offset, byte_len) in [
            (SingleForm::Tag1b, 113, 28),
            (SingleForm::Tag23, 113, 28),
            (SingleForm::Tag28, 116, 54),
        ] {
            let branch = DatumPlaneBranch::Single {
                form,
                descriptor: (descriptor, ()),
                object: (second, ()),
            };
            let frame = DatumPlaneFrame::new(100, branch.clone()).unwrap();
            assert_eq!(frame.descriptor().unwrap().2, 110);
            assert_eq!(
                frame
                    .objects()
                    .map(|(_, (), offset)| offset)
                    .collect::<Vec<_>>(),
                [object_offset]
            );
            let moved = frame.relocate(1000).unwrap();
            assert_eq!(moved.descriptor().unwrap().2, 1110);
            assert_eq!(moved.objects().next().unwrap().2, 1000 + object_offset);
            assert!(DatumPlaneFrame::new(u64::MAX - byte_len + 1, branch.clone()).is_none());
            assert!(DatumPlaneFrame::new(u64::MAX - byte_len, branch)
                .unwrap()
                .relocate(1)
                .is_none());
        }
        for (form, second_offset, byte_len) in [
            (DoubleForm::CountTwo, 123, 49),
            (DoubleForm::CountThree, 117, 54),
        ] {
            let branch = DatumPlaneBranch::Double {
                form,
                objects: [(first, ()), (second, ())],
            };
            let frame = DatumPlaneFrame::new(100, branch.clone()).unwrap();
            assert!(frame.descriptor().is_none());
            assert_eq!(
                frame
                    .objects()
                    .map(|(_, (), offset)| offset)
                    .collect::<Vec<_>>(),
                [110, second_offset]
            );
            assert!(frame
                .resolve(|index| (index == 0).then_some("block"))
                .is_none());
            let resolved = frame.resolve(|index| Some(index.to_string())).unwrap();
            assert_eq!(
                resolved
                    .objects()
                    .map(|(_, block, offset)| (block.as_str(), offset))
                    .collect::<Vec<_>>(),
                [("0", 110), ("256", second_offset)]
            );
            assert!(DatumPlaneFrame::new(u64::MAX - byte_len + 1, branch.clone()).is_none());
            assert!(DatumPlaneFrame::new(u64::MAX - byte_len, branch)
                .unwrap()
                .relocate(1)
                .is_none());
        }
    }
}
