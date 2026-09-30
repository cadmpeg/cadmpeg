// SPDX-License-Identifier: Apache-2.0
//! Sketch reference counts and positions with a distinguished terminal reference.

use super::operation_record::OperationPayload;
use super::reference_index::ReferenceIndexToken;
use super::PayloadObjectReference;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::num::NonZeroU8;

#[derive(Debug, Clone, PartialEq, Eq)]
enum References {
    Implicit(PayloadObjectReference),
    Explicit {
        count: NonZeroU8,
        references: Vec<PayloadObjectReference>,
    },
}

/// The reference count a sketch field states.
///
/// The field's third payload byte selects the form. Value `0` states no count
/// and the field carries one implicit terminal reference. Value `1` is followed
/// by the count byte, which the source states and which is never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchReferenceCount {
    /// No count byte; one implicit terminal reference.
    Implicit,
    /// The count byte the source states.
    Declared(NonZeroU8),
}

impl SketchReferenceCount {
    /// References the field carries. The implicit form carries its terminal alone.
    pub(crate) fn effective(self) -> NonZeroU8 {
        match self {
            Self::Implicit => NonZeroU8::MIN,
            Self::Declared(count) => count,
        }
    }

    /// The count byte: `0` for the implicit form, the stated count otherwise.
    fn count_byte(self) -> u8 {
        match self {
            Self::Implicit => 0,
            Self::Declared(count) => count.get(),
        }
    }

    /// Read a count byte. `0` is the implicit form, never a declared zero.
    pub(crate) fn from_count_byte(value: u8) -> Self {
        match NonZeroU8::new(value) {
            None => Self::Implicit,
            Some(count) => Self::Declared(count),
        }
    }
}

/// One implicit terminal or one through 255 explicitly counted references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SketchReferenceField(References);

impl SketchReferenceField {
    pub(super) fn read(
        ctx: &DecodeContext<'_>,
        record: OperationPayload<'_>,
        start: usize,
    ) -> Result<Option<Self>, CodecError> {
        let mut failure = None;
        let field = (|| {
            let bytes = record.payload().get(start..)?;
            let source_offset = record.payload_offset() + start;
            let (count, mut at) = match *bytes.get(2)? {
                0 => (None, 3),
                1 => (Some(NonZeroU8::new(*bytes.get(3)?)?), 4),
                _ => return None,
            };
            let leading_count = count.map_or(0, |count| usize::from(count.get() - 1));
            let mut references = Vec::new();
            for _ in 0..leading_count {
                let token = ReferenceIndexToken::read_payload(bytes.get(at..)?)?;
                let width = token.raw().len();
                if let Err(error) =
                    ctx.reserve_retained_vec(&mut references, 1, "nx sketch references")
                {
                    failure = Some(error);
                    return None;
                }
                references.push(PayloadObjectReference {
                    offset: source_offset + at,
                    token,
                });
                at += width;
            }
            (bytes.get(at..at + 2) == Some(&[0, 0])).then_some(())?;
            at += 2;
            let token = ReferenceIndexToken::read_payload(bytes.get(at..)?)?;
            let width = token.raw().len();
            let terminal = PayloadObjectReference {
                offset: source_offset + at,
                token,
            };
            at += width;
            (bytes.get(at..at + 4) == Some(&[1, 0, 0, 0])).then_some(())?;
            Some(Self(match count {
                None => References::Implicit(terminal),
                Some(count) => {
                    if let Err(error) =
                        ctx.reserve_retained_vec(&mut references, 1, "nx sketch references")
                    {
                        failure = Some(error);
                        return None;
                    }
                    references.push(terminal);
                    References::Explicit { count, references }
                }
            }))
        })();
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(field)
    }

    pub(super) fn declared_count(&self) -> SketchReferenceCount {
        match &self.0 {
            References::Implicit(_) => SketchReferenceCount::Implicit,
            References::Explicit { count, .. } => SketchReferenceCount::Declared(*count),
        }
    }

    #[cfg(test)]
    pub(super) fn references(&self) -> &[PayloadObjectReference] {
        match &self.0 {
            References::Implicit(terminal) => std::slice::from_ref(terminal),
            References::Explicit { references, .. } => references,
        }
    }

    pub(crate) fn into_positioned(
        self,
    ) -> impl Iterator<Item = (SketchReferencePosition, PayloadObjectReference)> {
        let declared_count = self.declared_count();
        let references = match self.0 {
            References::Implicit(terminal) => vec![terminal],
            References::Explicit { references, .. } => references,
        };
        references
            .into_iter()
            .zip(0u8..=u8::MAX)
            .map(move |(reference, ordinal)| {
                (
                    SketchReferencePosition {
                        declared_count,
                        ordinal,
                    },
                    reference,
                )
            })
    }
}

/// A position within the effective count; terminal status follows from its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "PositionWire", into = "PositionWire")]
pub(crate) struct SketchReferencePosition {
    declared_count: SketchReferenceCount,
    ordinal: u8,
}

impl SketchReferencePosition {
    pub(crate) fn new(
        declared_count: SketchReferenceCount,
        ordinal: u32,
    ) -> Result<Self, &'static str> {
        if ordinal >= u32::from(declared_count.effective().get()) {
            return Err(
                "ordinal/declared_count: ordinal must be within the effective reference count",
            );
        }
        let ordinal = u8::try_from(ordinal).map_err(|_| {
            "ordinal/declared_count: ordinal must be within the effective reference count"
        })?;
        Ok(Self {
            declared_count,
            ordinal,
        })
    }
    pub(crate) fn ordinal(self) -> u32 {
        u32::from(self.ordinal)
    }
    pub(crate) fn declared_count(self) -> SketchReferenceCount {
        self.declared_count
    }
    fn terminal(self) -> bool {
        self.ordinal == self.declared_count.effective().get() - 1
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PositionWire {
    ordinal: u32,
    declared_count: u8,
    terminal: bool,
}

impl From<SketchReferencePosition> for PositionWire {
    fn from(position: SketchReferencePosition) -> Self {
        Self {
            ordinal: position.ordinal(),
            declared_count: position.declared_count().count_byte(),
            terminal: position.terminal(),
        }
    }
}

impl TryFrom<PositionWire> for SketchReferencePosition {
    type Error = &'static str;
    fn try_from(wire: PositionWire) -> Result<Self, Self::Error> {
        let position = Self::new(
            SketchReferenceCount::from_count_byte(wire.declared_count),
            wire.ordinal,
        )?;
        if wire.terminal != position.terminal() {
            return Err("terminal: must match ordinal within declared_count");
        }
        Ok(position)
    }
}

#[cfg(test)]
mod tests {
    // Source fixtures and wire assertions construct checked positions directly.
    #![allow(clippy::unwrap_used)]
    use super::{
        OperationPayload, SketchReferenceCount, SketchReferenceField, SketchReferencePosition,
    };

    fn read_field(record: OperationPayload<'_>, start: usize) -> Option<SketchReferenceField> {
        crate::test_support::with_decode_context(|ctx| {
            SketchReferenceField::read(ctx, record, start)
        })
        .unwrap()
    }

    #[test]
    fn sketch_reference_field_refuses_collection_limit() {
        let bytes = b"\x01\x00\x01\x02\xf0\x42\x00\x00\xf0\x43\x01\x00\x00\x00";

        crate::test_support::with_decode_context_over(
            bytes,
            |policy| {
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
                let record = OperationPayload::new(bytes, 0, "SKETCH").unwrap();
                let error = crate::om::sketch_payload_references(ctx, record).unwrap_err();
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
                );
            },
        );
    }

    #[test]
    fn sketch_reference_field_refuses_retained_limit() {
        let bytes = b"\x01\x00\x01\x02\xf0\x42\x00\x00\xf0\x43\x01\x00\x00\x00";

        crate::test_support::with_decode_context_over(
            bytes,
            |policy| {
                policy.limits.max_retained_bytes = 0;
            },
            |ctx| {
                let record = OperationPayload::new(bytes, 0, "SKETCH").unwrap();
                let error = crate::om::sketch_payload_references(ctx, record).unwrap_err();
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
                );
            },
        );
    }

    #[test]
    fn sketch_reference_field_refuses_work_limit() {
        let bytes = b"\x01\x00\x01\x02\xf0\x42\x00\x00\xf0\x43\x01\x00\x00\x00";

        crate::test_support::with_decode_context_over(
            bytes,
            |policy| {
                policy.limits.max_work_units = 0;
            },
            |ctx| {
                let record = OperationPayload::new(bytes, 0, "SKETCH").unwrap();
                let error = crate::om::sketch_payload_references(ctx, record).unwrap_err();
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
                );
            },
        );
    }

    #[test]
    fn implicit_and_explicit_single_reference_fields_retain_distinct_counts() {
        for (bytes, count, offset) in [
            (
                &b"\x01\x00\x00\x00\x00\xf0\x42\x01\x00\x00\x00"[..],
                SketchReferenceCount::Implicit,
                105,
            ),
            (
                &b"\x01\x00\x01\x01\x00\x00\xf0\x42\x01\x00\x00\x00"[..],
                SketchReferenceCount::from_count_byte(1),
                106,
            ),
        ] {
            let payload = OperationPayload::new(bytes, 100, "SKETCH").unwrap();
            assert!(read_field(payload, usize::MAX).is_none());
            let field = read_field(payload, 0).unwrap();
            assert_eq!(field.declared_count(), count);
            assert_eq!(field.references().len(), 1);
            let (position, reference) = field.into_positioned().next().unwrap();
            assert_eq!(position.ordinal(), 0);
            assert_eq!(position.declared_count(), count);
            assert!(position.terminal());
            assert_eq!(reference.offset, offset);
            assert_eq!(reference.token.value(), 0x42);
            for end in 0..bytes.len() {
                assert!(read_field(
                    OperationPayload::new(&bytes[..end], 100, "SKETCH").unwrap(),
                    0
                )
                .is_none());
            }
        }
        let zero_explicit_count = b"\x01\x00\x01\x00\x00\x00\xf0\x42\x01\x00\x00\x00";
        assert!(read_field(
            OperationPayload::new(zero_explicit_count, 100, "SKETCH").unwrap(),
            0
        )
        .is_none());
    }

    #[test]
    fn maximum_sketch_field_derives_all_positions_from_its_references() {
        let mut bytes = vec![1, 0, 1, 255];
        for _ in 0..254 {
            bytes.extend_from_slice(&[0xf0, 0x42]);
        }
        bytes.extend_from_slice(&[0, 0, 0xf0, 0x43, 1, 0, 0, 0]);
        let field = read_field(OperationPayload::new(&bytes, 100, "SKETCH").unwrap(), 0).unwrap();
        assert_eq!(
            field.declared_count(),
            SketchReferenceCount::from_count_byte(255)
        );
        assert_eq!(field.references().len(), 255);
        for (ordinal, (position, reference)) in field.into_positioned().enumerate() {
            assert_eq!(
                position.ordinal(),
                u32::try_from(ordinal).expect("fixture value fits u32")
            );
            assert_eq!(
                position.declared_count(),
                SketchReferenceCount::from_count_byte(255)
            );
            assert_eq!(position.terminal(), ordinal == 254);
            assert_eq!(
                reference.offset,
                104 + 2 * ordinal + if ordinal == 254 { 2 } else { 0 }
            );
        }
    }

    #[test]
    fn sketch_position_wire_rejects_out_of_range_ordinals_and_wrong_terminals() {
        for (count, ordinal, terminal) in [
            (0, 0, true),
            (1, 0, true),
            (2, 0, false),
            (2, 1, true),
            (255, 254, true),
        ] {
            let wire = format!(
                r#"{{"ordinal":{ordinal},"declared_count":{count},"terminal":{terminal}}}"#
            );
            let position: SketchReferencePosition = serde_json::from_str(&wire).unwrap();
            assert_eq!(serde_json::to_string(&position).unwrap(), wire);
            let mut invalid = serde_json::to_value(position).unwrap();
            invalid["terminal"] = serde_json::json!(!terminal);
            assert!(serde_json::from_value::<SketchReferencePosition>(invalid)
                .unwrap_err()
                .to_string()
                .contains("terminal"));
            for ordinal in [
                u32::from(
                    SketchReferenceCount::from_count_byte(count)
                        .effective()
                        .get(),
                ),
                u32::MAX,
            ] {
                assert!(SketchReferencePosition::new(
                    SketchReferenceCount::from_count_byte(count),
                    ordinal
                )
                .unwrap_err()
                .contains("ordinal"));
            }
        }
    }
    #[test]
    fn a_stated_zero_count_is_the_implicit_form_and_never_a_declared_one() {
        // Count byte zero states no count. The field carries its terminal
        // alone, so the effective count is one, and that one is the form's own
        // cardinality, not a floored zero: the implicit form and a declared
        // count of one are distinct values.
        let implicit = SketchReferenceCount::from_count_byte(0);
        assert_eq!(implicit, SketchReferenceCount::Implicit);
        assert_eq!(implicit.effective().get(), 1);
        assert_ne!(implicit, SketchReferenceCount::from_count_byte(1));
        assert_eq!(implicit.count_byte(), 0);

        // A second ordinal has no place in an implicit field.
        assert!(SketchReferencePosition::new(implicit, 1)
            .unwrap_err()
            .contains("ordinal"));

        // The explicit form refuses a zero count byte at the source.
        let zero_count_byte = b"\x01\x00\x01\x00\x00\x00\xf0\x42\x01\x00\x00\x00";
        assert!(read_field(
            OperationPayload::new(zero_count_byte, 100, "SKETCH").unwrap(),
            0
        )
        .is_none());
    }
}
