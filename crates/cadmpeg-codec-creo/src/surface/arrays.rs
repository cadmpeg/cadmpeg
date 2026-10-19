// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::scalar::FiniteReal;

/// Scalar slots whose present coordinates are finite.
#[derive(Debug, Clone, PartialEq)]
struct FiniteScalarSlots(Vec<Option<f64>>);
impl FiniteScalarSlots {
    fn new(values: Vec<Option<f64>>) -> Option<Self> {
        values
            .iter()
            .all(|value| value.is_none_or(|value| FiniteReal::new(value).is_some()))
            .then_some(Self(values))
    }
}

/// A checked scalar extent, before any slot storage is constructed.
#[derive(Debug, Clone, Copy)]
pub(super) struct ScalarExtent<Shape> {
    shape: Shape,
    len: usize,
}
impl<Shape> ScalarExtent<Shape> {
    pub(super) fn len(&self) -> usize {
        self.len
    }
}

/// Scalar slots and optional source tokens with a checked count or shape.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Scalars<Shape> {
    shape: Shape,
    values: FiniteScalarSlots,
    tokens: Option<Vec<cadmpeg_ir::native::bytes::NativeBytes>>,
}
/// An outer dimension and a scalar count per dimension.
pub(crate) type DimensionedScalars = Scalars<[u32; 2]>;
/// A scalar count without an outer dimension.
pub(crate) type CountedScalars = Scalars<u32>;

impl DimensionedScalars {
    pub(super) fn extent(dimensions: u32, count: u32) -> Option<ScalarExtent<[u32; 2]>> {
        let len = usize::try_from(dimensions)
            .ok()?
            .checked_mul(usize::try_from(count).ok()?)?;
        Some(ScalarExtent {
            shape: [dimensions, count],
            len,
        })
    }
    #[cfg(test)]
    pub(crate) fn empty(dimensions: u32, count: u32) -> Result<Self, cadmpeg_core::CodecError> {
        let extent = Self::extent(dimensions, count)
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("test scalar grid extent"))?;
        let values = crate::decode::with_service_context(|ctx| {
            ctx.alloc_filled(extent.len, None, "creo scalar slots")
        })?;
        Ok(Self {
            shape: extent.shape,
            values: FiniteScalarSlots(values),
            tokens: None,
        })
    }
    pub(crate) fn dimensions(&self) -> u32 {
        self.shape[0]
    }
    pub(crate) fn count(&self) -> u32 {
        self.shape[1]
    }
}
impl CountedScalars {
    pub(super) fn extent(count: u32) -> Option<ScalarExtent<u32>> {
        Some(ScalarExtent {
            shape: count,
            len: usize::try_from(count).ok()?,
        })
    }
    #[cfg(test)]
    pub(crate) fn empty(count: u32) -> Result<Self, cadmpeg_core::CodecError> {
        let extent = Self::extent(count)
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("test scalar array count"))?;
        let values = crate::decode::with_service_context(|ctx| {
            ctx.alloc_filled(extent.len, None, "creo scalar slots")
        })?;
        Ok(Self {
            shape: extent.shape,
            values: FiniteScalarSlots(values),
            tokens: None,
        })
    }
    pub(crate) fn count(&self) -> u32 {
        self.shape
    }
}
impl<Shape> Scalars<Shape> {
    pub(super) fn from_values(
        extent: ScalarExtent<Shape>,
        values: Vec<Option<f64>>,
    ) -> Option<Self> {
        if values.len() != extent.len {
            return None;
        }
        FiniteScalarSlots::new(values).map(|values| Self {
            shape: extent.shape,
            values,
            tokens: None,
        })
    }
    pub(super) fn from_tokens(
        extent: ScalarExtent<Shape>,
        slots: Vec<(Option<f64>, Vec<u8>)>,
    ) -> Option<Self> {
        if slots.len() != extent.len {
            return None;
        }
        if slots
            .iter()
            .any(|(value, _)| value.is_some_and(|value| FiniteReal::new(value).is_none()))
        {
            return None;
        }
        let mut values = Vec::new();
        let mut tokens = Vec::new();
        for (value, token) in slots {
            values.push(value);
            tokens.push(token.into());
        }
        Some(Self {
            shape: extent.shape,
            values: FiniteScalarSlots(values),
            tokens: Some(tokens),
        })
    }
    #[cfg(test)]
    pub(crate) fn fill_values(&mut self, values: Vec<Option<f64>>) -> Option<()> {
        if values.len() != self.values.0.len() {
            return None;
        }
        self.values = FiniteScalarSlots::new(values)?;
        self.tokens = None;
        Some(())
    }
    pub(crate) fn values(&self) -> &[Option<f64>] {
        &self.values.0
    }
    pub(crate) fn tokens(&self) -> Option<&[cadmpeg_ir::native::bytes::NativeBytes]> {
        self.tokens.as_deref()
    }
}
#[cfg(test)]
impl<Shape: Copy> Scalars<Shape> {
    pub(super) fn fill_tokens(&mut self, slots: Vec<(Option<f64>, Vec<u8>)>) -> Option<()> {
        let extent = ScalarExtent {
            shape: self.shape,
            len: self.values.0.len(),
        };
        *self = Self::from_tokens(extent, slots)?;
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CountedScalars, DimensionedScalars};

    #[test]
    fn scalar_token_fill_stores_values_and_tokens() {
        let mut array = CountedScalars::empty(1).expect("valid extent");
        assert_eq!(array.fill_tokens(vec![(Some(1.0), vec![0xe4])]), Some(()));
        assert_eq!(array.values(), &[Some(1.0)]);
        assert_eq!(array.tokens(), Some([vec![0xe4].into()].as_slice()));
    }

    #[test]
    fn scalar_extent_counts_every_slot() {
        let extent = DimensionedScalars::extent(2, 2).expect("shape");
        assert_eq!(extent.len(), 4);
    }

    #[test]
    fn dimensioned_fills_reject_mismatched_extents_without_mutation() {
        let mut array = DimensionedScalars::empty(2, 2).expect("valid extent");
        let slots = vec![(Some(2.0), vec![0xe4]); 4];
        assert_eq!(array.fill_tokens(slots), Some(()));
        let original = array.clone();
        for len in [0, 3, 5] {
            assert_eq!(array.fill_values(vec![Some(3.0); len]), None);
            assert_eq!(array, original);
            assert_eq!(array.fill_tokens(vec![(None, vec![0x0f]); len]), None);
            assert_eq!(array, original);
        }
        assert_eq!(array.fill_values(vec![None; 4]), Some(()));
        assert_eq!(array.values(), &[None; 4]);
        assert_eq!(array.tokens(), None);
    }

    #[test]
    fn counted_fills_reject_mismatched_extents_without_mutation() {
        let mut array = CountedScalars::empty(2).expect("valid extent");
        assert_eq!(array.tokens(), None);
        assert_eq!(array.values(), &[None; 2]);
        assert_eq!(
            array.fill_tokens(vec![(Some(2.0), vec![0xe4]); 2]),
            Some(())
        );
        let original = array.clone();
        for len in [0, 1, 3] {
            assert_eq!(array.fill_tokens(vec![(None, vec![0x0f]); len]), None);
            assert_eq!(array, original);
        }
        assert_eq!(array.fill_tokens(vec![(None, vec![0x0f]); 2]), Some(()));
        assert_eq!(array.values(), &[None; 2]);
        assert_eq!(
            array.tokens(),
            Some([vec![0x0f].into(), vec![0x0f].into()].as_slice())
        );
    }

    #[test]
    fn empty_arrays_accept_only_empty_fills() {
        let mut dimensioned = DimensionedScalars::empty(0, 2).expect("valid extent");
        assert_eq!(dimensioned.fill_values(Vec::new()), Some(()));
        assert_eq!(dimensioned.fill_tokens(Vec::new()), Some(()));
        assert_eq!(dimensioned.fill_values(vec![None]), None);
        assert_eq!(dimensioned.fill_tokens(vec![(None, Vec::new())]), None);
        let mut counted = CountedScalars::empty(0).expect("valid extent");
        assert_eq!(counted.fill_tokens(Vec::new()), Some(()));
        assert_eq!(counted.fill_tokens(vec![(None, Vec::new())]), None);
    }
    #[test]
    fn scalar_arrays_reject_nonfinite_slots_without_mutation() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut array = CountedScalars::empty(1).expect("extent");
            let before = array.clone();
            assert_eq!(array.fill_values(vec![Some(value)]), None);
            assert_eq!(array, before);
            assert_eq!(array.fill_tokens(vec![(Some(value), vec![0xe4])]), None);
            assert_eq!(array, before);
        }
    }
    #[test]
    fn value_only_fill_clears_source_tokens() {
        let mut array = CountedScalars::empty(1).expect("extent");
        array
            .fill_tokens(vec![(Some(1.0), vec![0xe4])])
            .expect("extent");
        assert_eq!(array.fill_values(vec![Some(2.0)]), Some(()));
        assert_eq!(array.values(), &[Some(2.0)]);
        assert_eq!(array.tokens(), None);
    }
}
