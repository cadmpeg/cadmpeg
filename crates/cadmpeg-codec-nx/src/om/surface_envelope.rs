// SPDX-License-Identifier: Apache-2.0
//! Surface construction envelopes and derived token positions.

use super::operation_record::OperationPayload;
use super::reference_index::PayloadIndexToken;
use super::thru_curve_controls::ThruCurveControls;
use std::num::{NonZeroU8, NonZeroUsize};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

const TRAILING_PREFIX: [u8; 10] = [0x03, 0x03, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b];
const TRAILING_SUFFIX: [u8; 17] = [
    0x01, 0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x01,
    0x02,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceFeaturePayloadReferenceField {
    origin: u64,
    trailing_start: usize,
    tokens: [PayloadIndexToken; 14],
}

impl SurfaceFeaturePayloadReferenceField {
    pub(crate) fn relocate(mut self, ctx: &DecodeContext<'_>, base: u64) -> Result<Option<Self>, CodecError> {
        let Some(origin) = base.checked_add(self.origin) else { return Ok(None); };
        let leading_len = self.tokens[..11].iter().try_fold(17u64, |length, token| {
            length.checked_add(cadmpeg_core::decode::u64_from_index(token.raw().len()))
        });
        let trailing_len = ctx.admit_iter(&self.tokens[11..], "NX surface trailing token widths")?
            .try_fold(cadmpeg_core::decode::u64_from_index(TRAILING_SUFFIX.len()), |length, token| {
                length.checked_add(cadmpeg_core::decode::u64_from_index(token.raw().len()))
            });
        Ok((|| {
            origin.checked_add(leading_len?)?;
            origin.checked_add(cadmpeg_core::decode::u64_from_index(self.trailing_start))?
                .checked_add(trailing_len?)?;
            self.origin = origin;
            Some(self)
        })())
    }
    pub(crate) fn references(&self) -> [(PayloadIndexToken, u64); 14] {
        let mut at = self.origin + 5;
        std::array::from_fn(|slot| {
            if slot == 3 {
                at += 12;
            }
            if slot == 11 {
                at = self.origin + cadmpeg_core::decode::u64_from_index(self.trailing_start);
            }
            let token = self.tokens[slot];
            let offset = at;
            at += cadmpeg_core::decode::u64_from_index(token.raw().len());
            (token, offset)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThruCurvePayloadReferenceField {
    pub(crate) discriminator: NonZeroU8,
    pub(crate) controls: ThruCurveControls,
    pub(crate) trailing_control: NonZeroU8,
    pub(crate) trailing_value: [u8; 2],
    origin: u64,
    tokens: [PayloadIndexToken; 9],
}

impl ThruCurvePayloadReferenceField {
    pub(crate) fn origin(&self) -> u64 {
        self.origin
    }
    pub(super) fn byte_len(&self) -> usize {
        23 + self
            .tokens
            .iter()
            .map(|token| token.raw().len())
            .sum::<usize>()
    }
    pub(crate) fn relocate(mut self, base: u64) -> Option<Self> {
        let origin = base.checked_add(self.origin)?;
        origin.checked_add(cadmpeg_core::decode::u64_from_index(self.byte_len()))?;
        self.origin = origin;
        Some(self)
    }
    pub(crate) fn references(&self) -> [(PayloadIndexToken, u64); 9] {
        let mut at = self.origin + 5;
        std::array::from_fn(|slot| {
            if slot == 3 {
                at += 11;
            }
            let token = self.tokens[slot];
            let offset = at;
            at += cadmpeg_core::decode::u64_from_index(token.raw().len());
            (token, offset)
        })
    }
}

pub(crate) fn surface_feature_payload_references(
    ctx: &DecodeContext<'_>,
    record: OperationPayload<'_>,
) -> Result<Option<SurfaceFeaturePayloadReferenceField>, CodecError> {
    (|| {
    let discriminator = *record.payload().first()?;
    match record.name() {
        "SKIN" if matches!(discriminator, 0x3e | 0x3f) => {}
        "Studio Surface" if discriminator == 0x14 => {}
        _ => return None,
    }
    if record.payload().get(1..5) != Some(&[0, 0, 1, 0]) {
        return None;
    }
    let mut at = 5;
    let first = PayloadIndexToken::read(record.payload().get(at..)?)?;
    at += first.raw().len();
    let mut tokens = [first; 14];
    for (slot, token) in tokens[..11].iter_mut().enumerate().skip(1) {
        if slot == 3 {
            if record.payload().get(at..at + 2) != Some(&[1, 9])
                || record.payload().get(at + 10..at + 12) != Some(&[1, 9])
            {
                return None;
            }
            at += 12;
        }
        *token = PayloadIndexToken::read(record.payload().get(at..)?)?;
        at += token.raw().len();
    }
    let width = NonZeroUsize::new(TRAILING_PREFIX.len())?;
    let mut trailing_start = None;
    for (start, bytes) in propagate_resource!(ctx.admit_iter(record.payload(), "NX surface trailing witness windows").map_err(CodecError::from))
        .windows(width).enumerate() {
        if bytes == TRAILING_PREFIX {
            if trailing_start.is_some() { return None; }
            trailing_start = Some(start);
        }
    }
    let trailing_start = trailing_start? + TRAILING_PREFIX.len();
    at = trailing_start;
    for token in &mut tokens[11..] {
        *token = PayloadIndexToken::read(record.payload().get(at..)?)?;
        at += token.raw().len();
    }
    if record.payload().get(at..at + TRAILING_SUFFIX.len()) != Some(&TRAILING_SUFFIX) {
        return None;
    }
    Some(Ok(SurfaceFeaturePayloadReferenceField {
        origin: cadmpeg_core::decode::u64_from_index(record.payload_offset()),
        trailing_start,
        tokens,
    }))
    })().transpose()
}

pub(crate) fn thru_curve_payload_references(
    record: OperationPayload<'_>,
) -> Option<ThruCurvePayloadReferenceField> {
    if record.name() != "THRU_CURVE" || record.payload().get(1..5) != Some(&[0, 0, 1, 0]) {
        return None;
    }
    let discriminator = NonZeroU8::new(*record.payload().first()?)?;
    let read = |at: &mut usize| {
        let token = PayloadIndexToken::read(record.payload().get(*at..)?)?;
        *at += token.raw().len();
        Some(token)
    };
    let mut at = 5;
    let leading = [read(&mut at)?, read(&mut at)?, read(&mut at)?];
    if record.payload().get(at..at + 2) != Some(&[1, 8]) {
        return None;
    }
    let controls = ThruCurveControls::try_from(
        <[u8; 9]>::try_from(record.payload().get(at + 2..at + 11)?).ok()?,
    )
    .ok()?;
    at += 11;
    let tokens = [
        leading[0],
        leading[1],
        leading[2],
        read(&mut at)?,
        read(&mut at)?,
        read(&mut at)?,
        read(&mut at)?,
        read(&mut at)?,
        read(&mut at)?,
    ];
    if record.payload().get(at) != Some(&4)
        || record.payload().get(at + 2) != Some(&0xa0)
        || record.payload().get(at + 5..at + 7) != Some(&[0x13, 1])
    {
        return None;
    }
    let trailing_control = NonZeroU8::new(*record.payload().get(at + 1)?)?;
    let trailing_value = record.payload().get(at + 3..at + 5)?.try_into().ok()?;
    Some(ThruCurvePayloadReferenceField {
        discriminator,
        controls,
        trailing_control,
        trailing_value,
        origin: cadmpeg_core::decode::u64_from_index(record.payload_offset()),
        tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::super::operation_record::OperationPayload;
    use super::{
        thru_curve_payload_references, TRAILING_PREFIX,
        TRAILING_SUFFIX,
    };

    fn surface_feature_payload_references_test(record: OperationPayload<'_>) -> Option<super::SurfaceFeaturePayloadReferenceField> {
        crate::test_support::with_decode_context(|ctx| super::surface_feature_payload_references(ctx, record)).unwrap()
    }

    #[test]
    fn surface_positions_keep_the_independent_trailing_group_and_both_span_bounds() {
        let mut payload = vec![0x3e, 0, 0, 1, 0];
        for slot in 0..11 {
            if slot == 3 {
                payload.extend([1, 9, 0, 0, 0, 0, 0, 0, 0, 0, 1, 9]);
            }
            if slot % 2 == 0 {
                payload.extend([0xf0, 0]);
            } else {
                payload.extend([0xf1, 1, 0]);
            }
        }
        payload.extend([0x55; 5]);
        payload.extend(TRAILING_PREFIX);
        payload.extend([0xf0, 0, 0xf1, 1, 0, 0xf0, 1]);
        payload.extend(TRAILING_SUFFIX);
        let frame = surface_feature_payload_references_test(
            OperationPayload::new(&payload, 100, "SKIN").unwrap(),
        )
        .unwrap();
        let rows = frame.references();
        assert_eq!(
            [rows[0].1, rows[1].1, rows[2].1, rows[3].1],
            [105, 107, 110, 124]
        );
        assert_eq!([rows[11].1, rows[12].1, rows[13].1], [159, 161, 164]);
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| frame.clone().relocate(ctx, 1000)).unwrap().unwrap().references()[13].1,
            1164
        );
        assert!(crate::test_support::with_decode_context(|ctx| frame.clone().relocate(ctx, u64::MAX - 183)).unwrap().is_some());
        assert!(crate::test_support::with_decode_context(|ctx| frame.relocate(ctx, u64::MAX - 182)).unwrap().is_none());
        payload.extend(TRAILING_PREFIX);
        assert!(surface_feature_payload_references_test(
            OperationPayload::new(&payload, 100, "SKIN").unwrap()
        )
        .is_none());
    }

    #[test]
    fn thru_curve_end_and_reference_positions_follow_the_contiguous_envelope() {
        let mut payload = vec![1, 0, 0, 1, 0, 0xf0, 0, 0xf1, 1, 0, 0xf0, 1];
        payload.extend([1, 8, 0, 0, 0, 0, 0, 0, 0, 0, 7]);
        for value in 0..6 {
            payload.extend([0xf0, value]);
        }
        payload.extend([4, 1, 0xa0, 0, 0, 0x13, 1]);
        let frame = thru_curve_payload_references(
            OperationPayload::new(&payload, 100, "THRU_CURVE").unwrap(),
        )
        .unwrap();
        assert_eq!(frame.byte_len(), 42);
        assert_eq!(
            frame.references().map(|(_, offset)| offset),
            [105, 107, 110, 123, 125, 127, 129, 131, 133]
        );
        assert_eq!(frame.clone().relocate(1000).unwrap().origin(), 1100);
        assert!(frame.clone().relocate(u64::MAX - 142).is_some());
        assert!(frame.relocate(u64::MAX - 141).is_none());
        payload.pop();
        assert!(thru_curve_payload_references(
            OperationPayload::new(&payload, 100, "THRU_CURVE").unwrap()
        )
        .is_none());
    }
}
