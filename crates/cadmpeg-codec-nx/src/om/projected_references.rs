// SPDX-License-Identifier: Apache-2.0
//! Complete projected-curve storage graphs in serialized reference order.

use super::compact::CompactIndexAtom;
use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use super::PayloadObjectReference;

const CPROJ_SUFFIX: [u8; 5] = [0xff, 0x01, 0x02, 0x02, 0x7d];
const CPROJ_ALTERNATE_SUFFIX: [u8; 5] = [0xff, 1, 1, 1, 0x50];
const CPROJ_ELEMENT_PREFIX: [u8; 23] = [
    0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0, 0, 1, 1, 0xff, 0xff, 1, 1, 1, 0, 0,
];
const CMB_PREFIX: [u8; 10] = [0x3c, 0x32, 0x01, 0x02, 0x32, 0x01, 0x04, 0x36, 0x01, 0x33];
const NETWORK_PREFIX: [u8; 5] = [1, 1, 0x1d, 1, 17];
const NETWORK_TRIPLE: [u8; 10] = [0x1f, 0x20, 1, 2, 0x20, 1, 4, 0x21, 1, 0x22];
const BRANCH_MIDDLE: [u8; 10] = [1, 2, 0, 0, 0, 0, 0, 0xff, 1, 2];
const NETWORK_MIDDLE: [u8; 8] = [0, 0, 0, 0, 0, 0xff, 1, 2];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectedCurveReferences {
    references: Vec<PayloadObjectReference<PayloadIndexToken>>,
}

struct FieldReader<'a> {
    bytes: &'a [u8],
    at: usize,
    origin: usize,
}

impl FieldReader<'_> {
    fn fixed(&mut self, expected: &[u8]) -> Option<()> {
        let end = self.at.checked_add(expected.len())?;
        (self.bytes.get(self.at..end) == Some(expected)).then_some(())?;
        self.at = end;
        Some(())
    }

    fn reference(&mut self) -> Option<PayloadObjectReference<PayloadIndexToken>> {
        let token = PayloadIndexToken::read(self.bytes.get(self.at..)?)?;
        let offset = self.origin.checked_add(self.at)?;
        self.at = self.at.checked_add(token.raw().len())?;
        Some(PayloadObjectReference { offset, token })
    }

    fn compact(&mut self) -> Option<CompactIndexAtom> {
        let atom = CompactIndexAtom::read(self.bytes.get(self.at..)?)?;
        self.at = self.at.checked_add(atom.raw().len())?;
        Some(atom)
    }

    /// Walk the complete lane without allocating. The second pass emits only
    /// after framing, count, repeated tokens and suffix have all been validated.
    fn projected(
        &mut self,
        mut emit: impl FnMut(PayloadObjectReference<PayloadIndexToken>),
    ) -> Option<usize> {
        let leader = *self.bytes.get(self.at)?;
        self.at += 1;
        self.fixed(&[0, 0, 1, 0, 0, 0])?;
        self.bytes.get(self.at..self.at.checked_add(8)?)?;
        self.at += 8;
        self.fixed(&[0, 0, 0, 1, 2])?;
        let header = self.compact()?;
        self.fixed(&[0xff, 1])?;
        let declared = self.compact()?;
        // A direct count includes the final reference slot. In the extended
        // form the encoded value is the number of pair elements.
        let pairs = if declared.raw().len() == 1 {
            declared.value().checked_sub(1)?
        } else {
            declared.value()
        };
        let pairs = usize::try_from(pairs).ok()?;
        (pairs > 0 && pairs <= self.bytes.len().checked_sub(self.at)? / 31).then_some(())?;
        let element_leader = leader.checked_add(1)?;
        for _ in 0..pairs {
            self.fixed(&[element_leader])?;
            self.fixed(&CPROJ_ELEMENT_PREFIX)?;
            self.fixed(&[1, 2])?;
            emit(self.reference()?);
            emit(self.reference()?);
            (self.compact()? == header).then_some(())?;
        }
        self.fixed(&[0, 2, 1])?;
        emit(self.reference()?);
        let suffix = self.bytes.get(self.at..self.at.checked_add(5)?)?;
        (suffix == CPROJ_SUFFIX || suffix == CPROJ_ALTERNATE_SUFFIX).then_some(())?;
        self.at += 5;
        pairs.checked_mul(2)?.checked_add(1)
    }

    fn combined(&mut self) -> Option<ProjectedCurveReferences> {
        self.fixed(&CMB_PREFIX)?;
        let first = self.reference()?;
        self.fixed(&[0x33])?;
        let second = self.reference()?;
        self.fixed(&[0])?;
        let third = self.reference()?;
        self.fixed(&[0; 6])?;
        let fourth = self.reference()?;
        let mut branch = |anchor: PayloadIndexToken| {
            self.fixed(&[0x16, 1, 2])?;
            let repeated = self.reference()?;
            (repeated.token == anchor).then_some(())?;
            self.fixed(&BRANCH_MIDDLE)?;
            let member = self.reference()?;
            self.fixed(&[0, 0x81, 0x5c])?;
            Some((repeated, member))
        };
        let (first_repeat, fifth) = branch(first.token)?;
        let (second_repeat, sixth) = branch(second.token)?;
        self.fixed(&[0xff, 1, 0xff, 1])?;
        let seventh = self.reference()?;
        let eighth = self.reference()?;
        self.fixed(&[4, 2])?;
        Some(ProjectedCurveReferences {
            references: Vec::from([
                first,
                second,
                third,
                fourth,
                first_repeat,
                fifth,
                second_repeat,
                sixth,
                seventh,
                eighth,
            ]),
        })
    }

    fn network(&mut self) -> Option<ProjectedCurveReferences> {
        self.fixed(&NETWORK_PREFIX)?;
        self.fixed(&NETWORK_TRIPLE)?;
        let first = self.reference()?;
        let mut references: [_; 68] = std::array::from_fn(|_| first.clone());
        for (ordinal, reference) in references.iter_mut().enumerate().skip(1) {
            match ordinal {
                1..=47 => match ordinal % 3 {
                    0 => self.fixed(&NETWORK_TRIPLE)?,
                    1 => self.fixed(&[0x22])?,
                    _ => self.fixed(&[0])?,
                },
                48 => self.fixed(&[0; 6])?,
                49 => self.fixed(&[0x1e, 1, 13])?,
                61 => self.fixed(&NETWORK_MIDDLE)?,
                62 => self.fixed(&[0, 9, 0x1e, 1, 4])?,
                65 => self.fixed(&NETWORK_MIDDLE)?,
                66 => self.fixed(&[0, 29, 0xff, 1, 0xff, 1])?,
                _ => {}
            }
            *reference = self.reference()?;
        }
        self.fixed(&[4, 2])?;
        Some(ProjectedCurveReferences {
            references: Vec::from(references),
        })
    }
}

impl ProjectedCurveReferences {
    pub(crate) fn read(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        record: OperationPayload<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if !matches!(record.name(), "CPROJ" | "CPROJ_CMB") {
            return Ok(None);
        }
        let bytes = record.payload();
        if record.name() == "CPROJ" {
            let mut candidate = None;
            for start in 0..bytes.len() {
                let mut reader = FieldReader {
                    bytes,
                    at: start,
                    origin: record.payload_offset(),
                };
                let parsed = reader.projected(|_| {});
                ctx.charge_work(
                    cadmpeg_core::decode::u64_from_index(reader.at - start + 1),
                    "NX projected curve lane scan",
                )?;
                if let Some(count) = parsed {
                    if candidate.is_some() {
                        return Ok(None);
                    }
                    candidate = Some((start, count, reader.at));
                }
            }
            let Some((start, count, end)) = candidate else {
                return Ok(None);
            };
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(end - start),
                "NX projected curve lane emission",
            )?;
            let mut references = ctx.collection_vec(count, "NX projected curve lane references")?;
            let mut reader = FieldReader {
                bytes,
                at: start,
                origin: record.payload_offset(),
            };
            if reader.projected(|reference| references.push(reference)) != Some(count)
                || reader.at != end
            {
                return Err(cadmpeg_core::CodecError::malformed(
                    "NX validated projected lane changed",
                ));
            }
            return Ok(Some(Self { references }));
        }
        Ok(super::unique_candidate((0..bytes.len()).filter_map(
            |start| {
                let mut reader = FieldReader {
                    bytes,
                    at: start,
                    origin: record.payload_offset(),
                };
                if bytes
                    .get(start..)
                    .is_some_and(|tail| tail.starts_with(&CMB_PREFIX))
                {
                    reader.combined()
                } else if bytes
                    .get(start..)
                    .is_some_and(|tail| tail.starts_with(&NETWORK_PREFIX))
                {
                    reader.network()
                } else {
                    None
                }
            },
        )))
    }

    pub(crate) fn into_references(self) -> Vec<PayloadObjectReference<PayloadIndexToken>> {
        self.references
    }
}

#[cfg(test)]
mod tests {
    use super::ProjectedCurveReferences;
    use crate::om::operation_record::OperationPayload;

    fn read_for_test(record: OperationPayload<'_>) -> Option<ProjectedCurveReferences> {
        crate::test_support::with_decode_context(|ctx| ProjectedCurveReferences::read(ctx, record))
            .unwrap()
    }

    #[test]
    fn variable_middle_retains_complete_lane_reference_offsets() {
        for middle in [&[10][..], &[0x80, 0x57], &[0xa3, 0xb9]] {
            let mut bytes = vec![4, 0, 0, 1, 0, 0, 0];
            bytes.extend(crate::test_support::test_bytes::shifted_f64_bytes(4.0e-5));
            bytes.extend([0, 0, 0, 1, 2]);
            bytes.extend(middle);
            bytes.extend([0xff, 1, 2, 5]);
            bytes.extend(super::CPROJ_ELEMENT_PREFIX);
            bytes.extend([1, 2, 0xf0, 1, 0xf1, 1, 0]);
            bytes.extend(middle);
            bytes.extend([0, 2, 1, 0xf0, 3, 0xff, 1, 2, 2, 0x7d]);
            let references = read_for_test(OperationPayload::new(&bytes, 100, "CPROJ").unwrap())
                .unwrap()
                .into_references();
            assert_eq!(
                references
                    .iter()
                    .map(|r| r.token.value())
                    .collect::<Vec<_>>(),
                [1, 256, 3]
            );
            assert_eq!(
                references.iter().map(|r| r.offset).collect::<Vec<_>>(),
                [
                    149 + middle.len(),
                    151 + middle.len(),
                    157 + 2 * middle.len()
                ]
            );
            let last = bytes.len() - 1;
            assert!(
                read_for_test(OperationPayload::new(&bytes[..last], 100, "CPROJ").unwrap())
                    .is_none()
            );
            bytes[20] = 0xff;
            assert!(read_for_test(OperationPayload::new(&bytes, 100, "CPROJ").unwrap()).is_none());
        }
    }

    #[test]
    fn counted_projected_lane_requires_every_pair_and_repeated_token() {
        let one = crate::test_support::test_om::projected_curve_payload();
        // Header is 25 bytes. One pair occupies 34 bytes, followed by the
        // final reference/suffix. N=3 therefore carries two pairs and five refs.
        let mut bytes = one[..25].to_vec();
        bytes[24] = 3;
        bytes.extend(&one[25..59]);
        bytes.extend(&one[25..]);
        let read = |bytes: &[u8]| read_for_test(OperationPayload::new(bytes, 0, "CPROJ").unwrap());
        let refs = read(&bytes).unwrap().into_references();
        assert_eq!(
            refs.iter().map(|r| r.token.value()).collect::<Vec<_>>(),
            [712, 713, 712, 713, 714]
        );
        let mut mismatch = bytes.clone();
        mismatch[58] = 0x58;
        assert!(read(&mismatch).is_none());
        let mut count = bytes.clone();
        count[24] = 2;
        assert!(read(&count).is_none());
        let mut alternate = bytes.clone();
        let n = alternate.len();
        alternate[n - 5..].copy_from_slice(&super::CPROJ_ALTERNATE_SUFFIX);
        assert_eq!(read(&alternate).unwrap().into_references().len(), 5);
        let mut extended = bytes[..24].to_vec();
        extended.extend([0x80, 2]);
        extended.extend(&bytes[25..]);
        assert_eq!(read(&extended).unwrap().into_references().len(), 5);
        crate::test_support::with_decode_context_over(
            &bytes,
            |policy| policy.limits.max_collection_items = 0,
            |ctx| {
                assert!(matches!(
                    super::ProjectedCurveReferences::read(
                        ctx,
                        OperationPayload::new(&bytes, 0, "CPROJ").unwrap()
                    ),
                    Err(cadmpeg_core::CodecError::ResourceLimit(_))
                ));
            },
        );
    }

    #[test]
    fn combined_network_retains_all_occurrences_and_rejects_partial_graphs() {
        // The field contract supplies 16 triples, one reference, 12+1 and
        // 3+1 branch references, then two tail references: 68 occurrences.
        let mut bytes = vec![1, 1, 0x1d, 1, 17];
        let mut expected = Vec::new();
        for ordinal in 0..16u8 {
            bytes.extend([0x1f, 0x20, 1, 2, 0x20, 1, 4, 0x21, 1, 0x22]);
            for (marker, value) in [
                (None, ordinal),
                (Some(0x22), ordinal + 1),
                (Some(0), ordinal + 2),
            ] {
                if let Some(marker) = marker {
                    bytes.push(marker);
                }
                expected.push((u32::from(value), 100 + bytes.len()));
                bytes.extend([0xf0, value]);
            }
        }
        bytes.extend([0; 6]);
        expected.push((20, 100 + bytes.len()));
        bytes.extend([0xf0, 20]);
        for (count, terminal, first) in [(13, 9, 21), (4, 29, 33)] {
            bytes.extend([0x1e, 1, count]);
            for value in first..first + count - 1 {
                expected.push((u32::from(value), 100 + bytes.len()));
                bytes.extend([0xf0, value]);
            }
            bytes.extend([0, 0, 0, 0, 0, 0xff, 1, 2]);
            expected.push((40, 100 + bytes.len()));
            bytes.extend([0xf0, 40, 0, terminal]);
        }
        bytes.extend([0xff, 1, 0xff, 1]);
        for value in [41, 42] {
            expected.push((u32::from(value), 100 + bytes.len()));
            bytes.extend([0xf0, value]);
        }
        bytes.extend([4, 2]);
        let read =
            |bytes: &[u8]| read_for_test(OperationPayload::new(bytes, 100, "CPROJ_CMB").unwrap());
        let references = read(&bytes).unwrap().into_references();
        assert_eq!(expected.len(), 68);
        assert_eq!(
            references
                .iter()
                .map(|r| (r.token.value(), r.offset))
                .collect::<Vec<_>>(),
            expected
        );
        assert!(read(&bytes[..bytes.len() - 1]).is_none());
        let ambiguous = [bytes.as_slice(), bytes.as_slice()].concat();
        assert!(read(&ambiguous).is_none());
        bytes[4] = 16;
        assert!(read(&bytes).is_none());
    }
}
