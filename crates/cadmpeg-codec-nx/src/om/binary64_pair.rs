// SPDX-License-Identifier: Apache-2.0
//! Fixed binary64 pairs with derived payload positions.

use std::borrow::Cow;
use std::num::NonZeroU8;
use super::scalar::{LocatedBinary64, ShiftedBinary64};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectPairForm { Short, Extended }

impl ObjectPairForm {
    pub(crate) const ALL: [Self; 2] = [Self::Short, Self::Extended];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DatumPlanePairForm;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchBinary64PairForm { Object(ObjectPairForm), Repeated(NonZeroU8) }

pub(crate) trait Binary64PairForm: Copy {
    fn discriminator(self) -> Cow<'static, [u8]>;
    fn separator_width(self) -> usize;
}

impl Binary64PairForm for ObjectPairForm {
    fn discriminator(self) -> Cow<'static, [u8]> {
        Cow::Borrowed(match self {
            Self::Short => &[8, 2, 3, 1, 3, 1, 0xc0, 0x45, 4, 0, 0x80, 0x86, 2, 0, 3],
            Self::Extended => &[8, 2, 3, 1, 0x81, 2, 1, 0xc0, 0x45, 4, 0, 0x80, 0x86, 2, 0, 3],
        })
    }
    fn separator_width(self) -> usize { 1 }
}

impl Binary64PairForm for DatumPlanePairForm {
    fn discriminator(self) -> Cow<'static, [u8]> {
        Cow::Borrowed(&[0x6d, 0, 0xf0, 8, 2, 3, 1, 3, 1, 0xc0, 0x45, 4, 0, 0x80, 0x86, 2, 0, 3])
    }
    fn separator_width(self) -> usize { 1 }
}

impl Binary64PairForm for SketchBinary64PairForm {
    fn discriminator(self) -> Cow<'static, [u8]> {
        match self {
            Self::Object(form) => form.discriminator(),
            Self::Repeated(code) => Cow::Owned(vec![code.get(), code.get(), 0x41, 0, 3, 1, 3, 1, 0xc0, 0x45, 4, 0, 0x80, 0x86, 2, 0, 3]),
        }
    }
    fn separator_width(self) -> usize {
        match self { Self::Object(form) => form.separator_width(), Self::Repeated(_) => 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Binary64Pair<F> {
    form: F,
    offset: usize,
    values: [ShiftedBinary64; 2],
}

impl<F: Binary64PairForm> Binary64Pair<F> {
    fn read(bytes: &[u8], offset: usize, form: F) -> Option<Self> {
        let discriminator = form.discriminator();
        let first = offset.checked_add(discriminator.len())?;
        (bytes.get(offset..first)? == discriminator.as_ref()).then_some(())?;
        let separator = first.checked_add(8)?;
        let second = separator.checked_add(form.separator_width())?;
        if form.separator_width() == 1 && bytes.get(separator) != Some(&0) { return None; }
        let end = second.checked_add(8)?;
        let values = [ShiftedBinary64::read(bytes.get(first..separator)?)?, ShiftedBinary64::read(bytes.get(second..end)?)?];
        Some(Self { form, offset, values })
    }

    pub(crate) fn offset(&self) -> usize { self.offset }
    pub(crate) fn discriminator(&self) -> Cow<'static, [u8]> { self.form.discriminator() }
    pub(crate) fn values(&self) -> [LocatedBinary64; 2] {
        let first = self.offset + self.form.discriminator().len();
        let positions = [first, first + 8 + self.form.separator_width()];
        std::array::from_fn(|i| LocatedBinary64 { scalar: self.values[i], offset: positions[i] })
    }
}

pub(crate) fn datum_plane_pairs(bytes: &[u8]) -> Vec<Binary64Pair<DatumPlanePairForm>> {
    (0..bytes.len()).filter_map(|offset| Binary64Pair::read(bytes, offset, DatumPlanePairForm)).collect()
}

pub(crate) fn object_pairs(bytes: &[u8]) -> Vec<Binary64Pair<ObjectPairForm>> {
    let mut pairs: Vec<_> = ObjectPairForm::ALL.into_iter().flat_map(|form|
        (0..bytes.len()).filter_map(move |offset| Binary64Pair::read(bytes, offset, form))).collect();
    pairs.sort_by_key(Binary64Pair::offset);
    pairs
}

pub(crate) fn sketch_pairs(bytes: &[u8]) -> Vec<Binary64Pair<SketchBinary64PairForm>> {
    let mut pairs: Vec<_> = object_pairs(bytes).into_iter().map(|pair| Binary64Pair {
        form: SketchBinary64PairForm::Object(pair.form), offset: pair.offset, values: pair.values,
    }).collect();
    for (offset, window) in bytes.windows(3).enumerate() {
        let [code, repeated, 0x41] = window else { continue; };
        let Some(code) = NonZeroU8::new(*code) else { continue; };
        if code.get() != *repeated || offset == 0 || bytes.get(offset - 1) != Some(&0) { continue; }
        if let Some(pair) = Binary64Pair::read(bytes, offset, SketchBinary64PairForm::Repeated(code)) { pairs.push(pair); }
    }
    pairs.sort_by_key(Binary64Pair::offset);
    pairs
}

#[cfg(test)]
mod tests {
    use crate::test_support::shifted_f64_bytes;

    #[test]
    fn om_datum_plane_object_scalar_pairs_require_the_complete_discriminator() {
        let mut bytes = vec![0x7f, 0x01, 0x01, 0xff];
        bytes.extend_from_slice(&[
            0x6d, 0x00, 0xf0, 0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86,
            0x02, 0x00, 0x03,
        ]);
        bytes.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
        bytes.push(0);
        bytes.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
        let pairs = crate::om::binary64_pair::datum_plane_pairs(&bytes);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].offset(), 4);
        assert_eq!(pairs[0].values().map(|value| value.offset), [22, 31]);
        assert_eq!(pairs[0].values().map(|value| value.scalar.value()), [10.0, -20.0]);
        assert_eq!(pairs[0].values()[0].scalar.raw(), [0x30, 0x24, 0, 0, 0, 0, 0, 0]);
        assert_eq!(pairs[0].values()[1].scalar.raw(), [0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
        bytes[10] ^= 1;
        assert!(crate::om::binary64_pair::datum_plane_pairs(&bytes).is_empty());
    }

    #[test]
    fn om_datum_csys_scalar_pairs_require_discriminator_and_separator() {
        let mut bytes = vec![0x2f, 0x2f, 0x41, 0x6d, 0x00, 0xf0];
        bytes.extend_from_slice(&[
            0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
        ]);
        bytes.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
        bytes.push(0);
        bytes.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
        let pairs = crate::om::binary64_pair::object_pairs(&bytes);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].offset(), 6);
        assert_eq!(pairs[0].values().map(|value| value.offset), [21, 30]);
        assert_eq!(pairs[0].values().map(|value| value.scalar.value()), [10.0, -20.0]);
        assert_eq!(pairs[0].values()[0].scalar.raw(), [0x30, 0x24, 0, 0, 0, 0, 0, 0]);
        assert_eq!(pairs[0].values()[1].scalar.raw(), [0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
        assert_eq!(pairs[0].discriminator().len(), 15);

        let mut extended = vec![
            0x08, 0x02, 0x03, 0x01, 0x81, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00,
            0x03,
        ];
        extended.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
        extended.push(0);
        extended.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
        let extended_pairs = crate::om::binary64_pair::object_pairs(&extended);
        assert_eq!(extended_pairs.len(), 1);
        assert_eq!(extended_pairs[0].discriminator().len(), 16);
        assert_eq!(extended_pairs[0].values().map(|value| value.offset), [16, 25]);
        assert_eq!(
            extended_pairs[0].values()[0].scalar.raw(),
            [0x30, 0x24, 0, 0, 0, 0, 0, 0]
        );

        bytes[29] = 1;
        assert!(crate::om::binary64_pair::object_pairs(&bytes).is_empty());
    }

    #[test]
    fn om_sketch_scalar_pairs_accept_the_repeated_type_frame() {
        const EPS_SKETCH_SCALAR: f64 = 1e-12;

        let mut bytes = vec![0xaa, 0x00];
        let discriminator_offset = bytes.len();
        bytes.extend_from_slice(&[
            0x14, 0x14, 0x41, 0x00, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02,
            0x00, 0x03,
        ]);
        let first_offset = bytes.len();
        bytes.extend_from_slice(&shifted_f64_bytes(10.0));
        let second_offset = bytes.len();
        bytes.extend_from_slice(&shifted_f64_bytes(-20.0));

        let pairs = crate::om::binary64_pair::sketch_pairs(&bytes);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].offset(), discriminator_offset);
        assert_eq!(pairs[0].values().map(|value| value.offset), [first_offset, second_offset]);
        assert!((pairs[0].values()[0].scalar.value() - 10.0).abs() < EPS_SKETCH_SCALAR);
        assert!((pairs[0].values()[1].scalar.value() + 20.0).abs() < EPS_SKETCH_SCALAR);
        assert_eq!(
            pairs[0].discriminator(),
            bytes[discriminator_offset..first_offset].to_vec()
        );
        assert!(crate::om::binary64_pair::object_pairs(&bytes).is_empty());

        bytes[discriminator_offset + 1] = 0x15;
        assert!(crate::om::binary64_pair::sketch_pairs(&bytes).is_empty());
        bytes[discriminator_offset + 1] = 0x14;
        bytes.truncate(second_offset + 7);
        assert!(crate::om::binary64_pair::sketch_pairs(&bytes).is_empty());
    }

    #[test]
    fn om_sketch_scalar_pairs_reject_non_binary64_atoms() {
        let mut bytes = vec![0x00, 0x21, 0x21, 0x41, 0x00];
        bytes.extend_from_slice(&[
            0x00, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
        ]);
        bytes.extend_from_slice(&[0x30, 0x42, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0xd0, 0x29, 0x33, 0x32, 0x50, 0x20, 0x00, 0x00]);

        assert!(crate::om::binary64_pair::sketch_pairs(&bytes).is_empty());
    }
}
