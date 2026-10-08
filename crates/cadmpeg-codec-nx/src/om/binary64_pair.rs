// SPDX-License-Identifier: Apache-2.0
//! Fixed binary64 pairs with derived payload positions.

use super::scalar::ShiftedBinary64;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::borrow::Cow;
use std::num::NonZeroU8;
use std::ops::Add;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectPairForm {
    Short,
    Extended,
}

impl ObjectPairForm {
    const ALL: [Self; 2] = [Self::Short, Self::Extended];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DatumPlanePairForm;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchBinary64PairForm {
    Object(ObjectPairForm),
    Repeated(NonZeroU8),
}

pub(crate) trait Binary64PairForm: Copy {
    fn discriminator(self) -> Cow<'static, [u8]>;
    fn separator_width(self) -> usize;
}

impl Binary64PairForm for ObjectPairForm {
    fn discriminator(self) -> Cow<'static, [u8]> {
        Cow::Borrowed(match self {
            Self::Short => &[8, 2, 3, 1, 3, 1, 0xc0, 0x45, 4, 0, 0x80, 0x86, 2, 0, 3],
            Self::Extended => &[
                8, 2, 3, 1, 0x81, 2, 1, 0xc0, 0x45, 4, 0, 0x80, 0x86, 2, 0, 3,
            ],
        })
    }
    fn separator_width(self) -> usize {
        1
    }
}

impl Binary64PairForm for DatumPlanePairForm {
    fn discriminator(self) -> Cow<'static, [u8]> {
        Cow::Borrowed(&[
            0x6d, 0, 0xf0, 8, 2, 3, 1, 3, 1, 0xc0, 0x45, 4, 0, 0x80, 0x86, 2, 0, 3,
        ])
    }
    fn separator_width(self) -> usize {
        1
    }
}

static REPEATED_DISCRIMINATORS: [[u8; 17]; 256] = {
    let mut rows = [[
        0, 0, 0x41, 0, 3, 1, 3, 1, 0xc0, 0x45, 4, 0, 0x80, 0x86, 2, 0, 3,
    ]; 256];
    let mut index = 0;
    let mut code = 0_u8;
    while index < 256 {
        rows[index][0] = code;
        rows[index][1] = code;
        index += 1;
        if index < 256 {
            code += 1;
        }
    }
    rows
};

impl Binary64PairForm for SketchBinary64PairForm {
    fn discriminator(self) -> Cow<'static, [u8]> {
        match self {
            Self::Object(form) => form.discriminator(),
            Self::Repeated(code) => {
                Cow::Borrowed(&REPEATED_DISCRIMINATORS[usize::from(code.get())])
            }
        }
    }
    fn separator_width(self) -> usize {
        match self {
            Self::Object(form) => form.separator_width(),
            Self::Repeated(_) => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Binary64Pair<F, O = usize> {
    form: F,
    offset: O,
    values: [ShiftedBinary64; 2],
    discriminator_byte_len: u16,
    separator_width: u16,
}

impl<F: Binary64PairForm> Binary64Pair<F> {
    fn read(bytes: &[u8], offset: usize, form: F) -> Option<Self> {
        let discriminator = form.discriminator();
        let discriminator_byte_len = u16::try_from(discriminator.len()).ok()?;
        let separator_width = u16::try_from(form.separator_width()).ok()?;
        let first = offset.checked_add(discriminator.len())?;
        if bytes.get(offset..first)? != discriminator.as_ref() {
            return None;
        }
        let separator = first.checked_add(8)?;
        let second = separator.checked_add(form.separator_width())?;
        if form.separator_width() == 1 && bytes.get(separator) != Some(&0) {
            return None;
        }
        let end = second.checked_add(8)?;
        let values = [
            ShiftedBinary64::read(bytes.get(first..separator)?)?,
            ShiftedBinary64::read(bytes.get(second..end)?)?,
        ];
        Some(Self {
            form,
            offset,
            values,
            discriminator_byte_len,
            separator_width,
        })
    }

    pub(crate) fn into_wire_frame(self) -> Option<Binary64Pair<F, u64>> {
        Binary64Pair::new(self.form, u64::try_from(self.offset).ok()?, self.values)
    }
}

impl<F: Binary64PairForm, O: Copy + Add<Output = O> + From<u16>> Binary64Pair<F, O> {
    pub(crate) fn offset(&self) -> O {
        self.offset
    }
    pub(crate) fn discriminator(&self) -> Cow<'static, [u8]> {
        self.form.discriminator()
    }
    pub(crate) fn atoms(&self) -> [ShiftedBinary64; 2] {
        self.values
    }
    pub(crate) fn value_offsets(&self) -> [O; 2] {
        let first = self.offset + O::from(self.discriminator_byte_len);
        [first, first + O::from(8 + self.separator_width)]
    }
}

impl<F: Binary64PairForm> Binary64Pair<F, u64> {
    pub(crate) fn new(form: F, offset: u64, values: [ShiftedBinary64; 2]) -> Option<Self> {
        let discriminator_byte_len = u16::try_from(form.discriminator().len()).ok()?;
        let separator_width = u16::try_from(form.separator_width()).ok()?;
        offset.checked_add(
            cadmpeg_core::decode::u64_from_index(form.discriminator().len())
                + 16
                + cadmpeg_core::decode::u64_from_index(form.separator_width()),
        )?;
        Some(Self {
            form,
            offset,
            values,
            discriminator_byte_len,
            separator_width,
        })
    }
}

impl TryFrom<&[u8]> for ObjectPairForm {
    type Error = &'static str;
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|form| form.discriminator().as_ref() == bytes)
            .ok_or("discriminator must name an object binary64 pair form")
    }
}

impl TryFrom<&[u8]> for SketchBinary64PairForm {
    type Error = &'static str;
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if let Ok(form) = ObjectPairForm::try_from(bytes) {
            return Ok(Self::Object(form));
        }
        if let Some(code) = bytes.first().copied().and_then(NonZeroU8::new) {
            let form = Self::Repeated(code);
            if form.discriminator().as_ref() == bytes {
                return Ok(form);
            }
        }
        Err("discriminator must name a sketch binary64 pair form")
    }
}

pub(crate) fn datum_plane_pairs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<Binary64Pair<DatumPlanePairForm>>, CodecError> {
    let mut pairs = Vec::new();
    for offset in ctx.admit_iter(0..bytes.len(), "scan NX datum-plane pairs")? {
        if let Some(pair) = Binary64Pair::read(bytes, offset, DatumPlanePairForm) {
            ctx.push_vec(&mut pairs, pair, "NX datum-plane binary64 pairs")?;
        }
    }
    Ok(pairs)
}

fn scan_object_pairs<F: Binary64PairForm>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    mut form: impl FnMut(ObjectPairForm) -> F,
) -> Result<Vec<Binary64Pair<F>>, CodecError> {
    let mut pairs = Vec::new();
    for object_form in ObjectPairForm::ALL {
        let form = form(object_form);
        for offset in ctx.admit_iter(0..bytes.len(), "scan NX object pairs")? {
            if let Some(pair) = Binary64Pair::read(bytes, offset, form) {
                ctx.push_vec(&mut pairs, pair, "NX object binary64 pairs")?;
            }
        }
    }
    Ok(pairs)
}

pub(crate) fn object_pairs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<Binary64Pair<ObjectPairForm>>, CodecError> {
    let mut pairs = scan_object_pairs(ctx, bytes, |form| form)?;
    ctx.stable_sort_by_key(
        &mut pairs,
        Binary64Pair::offset,
        Ord::cmp,
        "sort NX object pairs",
    )?;
    Ok(pairs)
}

pub(crate) fn sketch_pairs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<Binary64Pair<SketchBinary64PairForm>>, CodecError> {
    let mut pairs = scan_object_pairs(ctx, bytes, SketchBinary64PairForm::Object)?;
    let windows = bytes.len().checked_sub(2).map_or(0..0, |end| 0..end);
    for offset in ctx.admit_iter(windows, "scan NX sketch pairs")? {
        let window = &bytes[offset..offset + 3];
        let [code, repeated, 0x41] = window else {
            continue;
        };
        let Some(code) = NonZeroU8::new(*code) else {
            continue;
        };
        if code.get() != *repeated || offset == 0 || bytes.get(offset - 1) != Some(&0) {
            continue;
        }
        if let Some(pair) =
            Binary64Pair::read(bytes, offset, SketchBinary64PairForm::Repeated(code))
        {
            ctx.push_vec(&mut pairs, pair, "NX binary64 pairs")?;
        }
    }
    ctx.stable_sort_by_key(
        &mut pairs,
        Binary64Pair::offset,
        Ord::cmp,
        "sort NX sketch pairs",
    )?;
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    const EPS_SKETCH_SCALAR: f64 = 1e-12;

    fn datum_plane_pairs(bytes: &[u8]) -> Vec<super::Binary64Pair<super::DatumPlanePairForm>> {
        crate::test_support::with_decode_context(|ctx| super::datum_plane_pairs(ctx, bytes))
            .unwrap()
    }
    fn object_pairs(bytes: &[u8]) -> Vec<super::Binary64Pair<super::ObjectPairForm>> {
        crate::test_support::with_decode_context(|ctx| super::object_pairs(ctx, bytes)).unwrap()
    }
    fn sketch_pairs(bytes: &[u8]) -> Vec<super::Binary64Pair<super::SketchBinary64PairForm>> {
        crate::test_support::with_decode_context(|ctx| super::sketch_pairs(ctx, bytes)).unwrap()
    }

    use crate::test_support::test_bytes::shifted_f64_bytes;

    #[test]
    fn binary_pair_fixed_discriminators_need_no_work_budget() {
        fn case<F: super::Binary64PairForm + std::fmt::Debug>(form: F) {
            let mut bytes = form.discriminator().into_owned();
            bytes.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
            bytes.extend(std::iter::repeat_n(0, form.separator_width()));
            bytes.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
            let pair = super::Binary64Pair::read(&bytes, 0, form).unwrap();
            assert_eq!(pair.atoms().map(|atom| atom.value().get()), [10.0, -20.0]);
            for mismatch in [false, true] {
                if mismatch {
                    bytes[0] ^= 1;
                }
                crate::test_support::with_decode_context_over(
                    &[],
                    |policy| policy.limits.max_work_units = 0,
                    |ctx| {
                        let parsed = super::Binary64Pair::read(&bytes, 0, form);
                        assert_eq!(parsed.is_none(), mismatch);
                        if let Some(parsed) = parsed {
                            assert_eq!(
                                parsed.atoms().map(|atom| atom.value().get()),
                                [10.0, -20.0]
                            );
                        }
                        assert_eq!(ctx.resource_refusal(), None);
                    },
                );
            }
        }
        case(super::DatumPlanePairForm);
        case(super::ObjectPairForm::Short);
        case(super::ObjectPairForm::Extended);
        case(super::SketchBinary64PairForm::Repeated(
            std::num::NonZeroU8::new(20).unwrap(),
        ));
    }

    #[test]
    fn binary_pair_scanners_refuse_at_source_traversal() {
        for operation in [
            "scan NX datum-plane pairs",
            "scan NX object pairs",
            "scan NX sketch pairs",
        ] {
            let bytes = [0_u8; 64];
            let error = crate::test_support::resource_refusal_at(
                &[],
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                operation,
                |ctx| match operation {
                    "scan NX datum-plane pairs" => {
                        super::datum_plane_pairs(ctx, &bytes).map(|pairs| pairs.len())
                    }
                    "scan NX object pairs" => {
                        super::object_pairs(ctx, &bytes).map(|pairs| pairs.len())
                    }
                    _ => super::sketch_pairs(ctx, &bytes).map(|pairs| pairs.len()),
                },
            );
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation && limit.additional == if operation == "scan NX sketch pairs" { 62 } else { 64 })
            );
        }
    }

    #[test]
    fn object_binary64_pairs_refuse_collection_limit() {
        let mut bytes = vec![8, 2, 3, 1, 3, 1, 0xc0, 0x45, 4, 0, 0x80, 0x86, 2, 0, 3];
        bytes.extend_from_slice(&shifted_f64_bytes(10.0));
        bytes.push(0);
        bytes.extend_from_slice(&shifted_f64_bytes(20.0));

        crate::test_support::with_decode_context_over(
            &bytes,
            |policy| {
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
                let error = super::object_pairs(ctx, &bytes).unwrap_err();
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
                );
            },
        );
    }

    #[test]
    fn om_datum_plane_object_scalar_pairs_require_the_complete_discriminator() {
        let mut bytes = vec![0x7f, 0x01, 0x01, 0xff];
        bytes.extend_from_slice(&[
            0x6d, 0x00, 0xf0, 0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80,
            0x86, 0x02, 0x00, 0x03,
        ]);
        bytes.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
        bytes.push(0);
        bytes.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
        let pairs = datum_plane_pairs(&bytes);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].offset(), 4);
        assert_eq!(pairs[0].value_offsets(), [22, 31]);
        assert_eq!(
            pairs[0].atoms().map(|scalar| scalar.value().get()),
            [10.0, -20.0]
        );
        assert_eq!(pairs[0].atoms()[0].raw(), [0x30, 0x24, 0, 0, 0, 0, 0, 0]);
        assert_eq!(pairs[0].atoms()[1].raw(), [0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
        bytes[10] ^= 1;
        assert!(datum_plane_pairs(&bytes).is_empty());
    }

    #[test]
    fn om_datum_csys_scalar_pairs_require_discriminator_and_separator() {
        let mut bytes = vec![0x2f, 0x2f, 0x41, 0x6d, 0x00, 0xf0];
        bytes.extend_from_slice(&[
            0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00,
            0x03,
        ]);
        bytes.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
        bytes.push(0);
        bytes.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
        let pairs = object_pairs(&bytes);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].offset(), 6);
        assert_eq!(pairs[0].value_offsets(), [21, 30]);
        assert_eq!(
            pairs[0].atoms().map(|scalar| scalar.value().get()),
            [10.0, -20.0]
        );
        assert_eq!(pairs[0].atoms()[0].raw(), [0x30, 0x24, 0, 0, 0, 0, 0, 0]);
        assert_eq!(pairs[0].atoms()[1].raw(), [0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
        assert_eq!(pairs[0].discriminator().len(), 15);

        let mut extended = vec![
            0x08, 0x02, 0x03, 0x01, 0x81, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02,
            0x00, 0x03,
        ];
        extended.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
        extended.push(0);
        extended.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
        let extended_pairs = object_pairs(&extended);
        assert_eq!(extended_pairs.len(), 1);
        assert_eq!(extended_pairs[0].discriminator().len(), 16);
        assert_eq!(extended_pairs[0].value_offsets(), [16, 25]);
        assert_eq!(
            extended_pairs[0].atoms()[0].raw(),
            [0x30, 0x24, 0, 0, 0, 0, 0, 0]
        );

        bytes[29] = 1;
        assert!(object_pairs(&bytes).is_empty());
    }

    #[test]
    fn om_sketch_scalar_pairs_accept_the_repeated_type_frame() {
        let mut bytes = vec![0xaa, 0x00];
        let discriminator_offset = bytes.len();
        bytes.extend_from_slice(&[
            0x14, 0x14, 0x41, 0x00, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86,
            0x02, 0x00, 0x03,
        ]);
        let first_offset = bytes.len();
        bytes.extend_from_slice(&shifted_f64_bytes(10.0));
        let second_offset = bytes.len();
        bytes.extend_from_slice(&shifted_f64_bytes(-20.0));

        let pairs = sketch_pairs(&bytes);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].offset(), discriminator_offset);
        assert_eq!(pairs[0].value_offsets(), [first_offset, second_offset]);
        assert!((pairs[0].atoms()[0].value().get() - 10.0).abs() < EPS_SKETCH_SCALAR);
        assert!((pairs[0].atoms()[1].value().get() + 20.0).abs() < EPS_SKETCH_SCALAR);
        assert_eq!(
            pairs[0].discriminator(),
            bytes[discriminator_offset..first_offset].to_vec()
        );
        assert!(object_pairs(&bytes).is_empty());

        bytes[discriminator_offset + 1] = 0x15;
        assert!(sketch_pairs(&bytes).is_empty());
        bytes[discriminator_offset + 1] = 0x14;
        bytes.truncate(second_offset + 7);
        assert!(sketch_pairs(&bytes).is_empty());
    }

    #[test]
    fn om_sketch_scalar_pairs_reject_non_binary64_atoms() {
        let mut bytes = vec![0x00, 0x21, 0x21, 0x41, 0x00];
        bytes.extend_from_slice(&[
            0x00, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
        ]);
        bytes.extend_from_slice(&[0x30, 0x42, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0xd0, 0x29, 0x33, 0x32, 0x50, 0x20, 0x00, 0x00]);

        assert!(sketch_pairs(&bytes).is_empty());
    }
}
