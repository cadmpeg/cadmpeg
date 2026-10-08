// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

/// Scalar slots whose present coordinates are finite.
#[derive(Debug, Clone, PartialEq)]
struct FiniteScalarSlots(Vec<Option<f64>>);

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
    tokens: Option<Vec<Vec<u8>>>,
    complete: bool,
    increasing: bool,
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
    pub(crate) fn empty(dimensions: u32, count: u32) -> Result<Self, CodecError> {
        let extent = Self::extent(dimensions, count)
            .ok_or_else(|| CodecError::malformed("test scalar grid extent"))?;
        let values = crate::decode::with_test_decode_ctx(|ctx| {
            ctx.alloc_filled(extent.len, None, "creo scalar slots")
        })?;
        Ok(Self {
            shape: extent.shape,
            values: FiniteScalarSlots(values),
            tokens: None,
            complete: extent.len == 0,
            increasing: extent.len == 0,
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
    pub(crate) fn empty(count: u32) -> Result<Self, CodecError> {
        let extent =
            Self::extent(count).ok_or_else(|| CodecError::malformed("test scalar array count"))?;
        let values = crate::decode::with_test_decode_ctx(|ctx| {
            ctx.alloc_filled(extent.len, None, "creo scalar slots")
        })?;
        Ok(Self {
            shape: extent.shape,
            values: FiniteScalarSlots(values),
            tokens: None,
            complete: extent.len == 0,
            increasing: extent.len == 0,
        })
    }
    pub(crate) fn count(&self) -> u32 {
        self.shape
    }
}
impl<Shape> Scalars<Shape> {
    pub(super) fn from_values(
        ctx: &DecodeContext<'_>,
        extent: ScalarExtent<Shape>,
        values: Vec<Option<f64>>,
    ) -> Result<Option<Self>, CodecError> {
        if values.len() != extent.len {
            return Ok(None);
        }
        let mut complete = true;
        let mut increasing = true;
        let mut previous = None;
        if !ctx.all_by(
            &values,
            |value| {
                complete &= value.is_some();
                increasing &= match (previous, *value) {
                    (Some(previous), Some(value)) => previous < value,
                    (None, Some(_)) => true,
                    (_, None) => false,
                };
                previous = *value;
                Ok(value.is_none_or(|value| FiniteReal::new(value).is_some()))
            },
            "creo scalar array validation",
        )? {
            return Ok(None);
        }
        Ok(Some(Self {
            shape: extent.shape,
            values: FiniteScalarSlots(values),
            tokens: None,
            complete,
            increasing,
        }))
    }
    pub(super) fn from_tokens(
        ctx: &DecodeContext<'_>,
        extent: ScalarExtent<Shape>,
        slots: Vec<(Option<f64>, Vec<u8>)>,
    ) -> Result<Option<Self>, CodecError> {
        if slots.len() != extent.len {
            return Ok(None);
        }
        let mut complete = true;
        let mut increasing = true;
        let mut previous = None;
        if ctx.any_by(
            &slots,
            |(value, _)| {
                complete &= value.is_some();
                increasing &= match (previous, *value) {
                    (Some(previous), Some(value)) => previous < value,
                    (None, Some(_)) => true,
                    (_, None) => false,
                };
                previous = *value;
                Ok(value.is_some_and(|value| FiniteReal::new(value).is_none()))
            },
            "creo scalar array validation",
        )? {
            return Ok(None);
        }
        let mut values = ctx.collection_vec(slots.len(), "creo scalar array values")?;
        let mut tokens = ctx.collection_vec(slots.len(), "creo scalar array tokens")?;
        for (value, token) in ctx.admit_iter(slots, "creo scalar array filling")? {
            values.push(value);
            tokens.push(token);
        }
        Ok(Some(Self {
            shape: extent.shape,
            values: FiniteScalarSlots(values),
            tokens: Some(tokens),
            complete,
            increasing,
        }))
    }
    #[cfg(test)]
    pub(crate) fn fill_values(&mut self, values: Vec<Option<f64>>) -> Option<()> {
        if values.len() != self.values.0.len() {
            return None;
        }
        let extent = ScalarExtent {
            shape: (),
            len: values.len(),
        };
        let array =
            crate::decode::with_test_decode_ctx(|ctx| Scalars::from_values(ctx, extent, values))
                .ok()??;
        self.values = array.values;
        self.complete = array.complete;
        self.increasing = array.increasing;
        self.tokens = None;
        Some(())
    }
    pub(super) fn copy_retained(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError>
    where
        Shape: Copy,
    {
        let values = ctx.collect_retained_vec(
            self.values.0.iter().copied(),
            "creo retained scalar array values",
        )?;
        let tokens = match &self.tokens {
            Some(source) => {
                let mut tokens =
                    ctx.collection_vec(source.len(), "creo retained scalar array tokens")?;
                for token in ctx.admit_iter(source, "creo retained scalar token traversal")? {
                    tokens.push(ctx.copy_retained(token, "creo retained scalar token bytes")?);
                }
                Some(tokens)
            }
            None => None,
        };
        Ok(Self {
            shape: self.shape,
            values: FiniteScalarSlots(values),
            tokens,
            complete: self.complete,
            increasing: self.increasing,
        })
    }
    pub(crate) fn is_complete(&self) -> bool {
        self.complete
    }
    pub(crate) fn is_strictly_increasing(&self) -> bool {
        self.complete && self.increasing
    }
    pub(crate) fn values(&self) -> &[Option<f64>] {
        &self.values.0
    }
    pub(crate) fn tokens(&self) -> Option<&[Vec<u8>]> {
        self.tokens.as_deref()
    }
}
#[cfg(test)]
impl<Shape: Copy> Scalars<Shape> {
    pub(super) fn fill_tokens(
        &mut self,
        ctx: &DecodeContext<'_>,
        slots: Vec<(Option<f64>, Vec<u8>)>,
    ) -> Result<Option<()>, CodecError> {
        let extent = ScalarExtent {
            shape: self.shape,
            len: self.values.0.len(),
        };
        let Some(array) = Self::from_tokens(ctx, extent, slots)? else {
            return Ok(None);
        };
        *self = array;
        Ok(Some(()))
    }
}

#[cfg(test)]
mod tests {
    use super::{CountedScalars, DimensionedScalars};
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    #[test]
    fn scalar_token_filling_propagates_storage_refusal_without_mutation() {
        let array = crate::test_support::assert_retained_boundaries(
            &["creo scalar array values", "creo scalar array tokens"],
            |ctx| {
                let mut array = CountedScalars::empty(1).expect("valid extent");
                let original = array.clone();
                match array.fill_tokens(ctx, vec![(Some(1.0), vec![0xe4])]) {
                    Ok(Some(())) => Ok(array),
                    Ok(None) => panic!("matching extent"),
                    Err(error) => {
                        assert_eq!(array, original);
                        assert_eq!(
                            ctx.resource_refusal(),
                            match &error {
                                CodecError::ResourceLimit(limit) => Some(*limit),
                                _ => None,
                            }
                        );
                        Err(error)
                    }
                }
            },
        );
        assert_eq!(array.values(), &[Some(1.0)]);
        assert_eq!(array.tokens(), Some([vec![0xe4]].as_slice()));
    }

    #[test]
    fn scalar_extent_does_not_allocate_placeholder_slots() {
        let extent = DimensionedScalars::extent(2, 2).expect("shape");
        assert_eq!(extent.len(), 4);
        let error = crate::test_support::last_refusal_at(
            &[0; 4],
            ResourceDimension::CollectionItems,
            "creo scalar array values",
            |ctx| DimensionedScalars::from_tokens(ctx, extent, vec![(None, Vec::new()); 4]),
        );
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "creo scalar array values")
        );
    }

    #[test]
    fn dimensioned_fills_reject_mismatched_extents_without_mutation() {
        let mut array = DimensionedScalars::empty(2, 2).expect("valid extent");
        let slots = vec![(Some(2.0), vec![0xe4]); 4];
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| array.fill_tokens(ctx, slots))
                .expect("admitted scalar fill"),
            Some(())
        );
        let original = array.clone();
        for len in [0, 3, 5] {
            assert_eq!(array.fill_values(vec![Some(3.0); len]), None);
            assert_eq!(array, original);
            assert_eq!(
                crate::decode::with_test_decode_ctx(
                    |ctx| array.fill_tokens(ctx, vec![(None, vec![0x0f]); len])
                )
                .expect("admitted scalar fill"),
                None
            );
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
            crate::decode::with_test_decode_ctx(
                |ctx| array.fill_tokens(ctx, vec![(Some(2.0), vec![0xe4]); 2])
            )
            .expect("admitted scalar fill"),
            Some(())
        );
        let original = array.clone();
        for len in [0, 1, 3] {
            assert_eq!(
                crate::decode::with_test_decode_ctx(
                    |ctx| array.fill_tokens(ctx, vec![(None, vec![0x0f]); len])
                )
                .expect("admitted scalar fill"),
                None
            );
            assert_eq!(array, original);
        }
        assert_eq!(
            crate::decode::with_test_decode_ctx(
                |ctx| array.fill_tokens(ctx, vec![(None, vec![0x0f]); 2])
            )
            .expect("admitted scalar fill"),
            Some(())
        );
        assert_eq!(array.values(), &[None; 2]);
        assert_eq!(array.tokens(), Some([vec![0x0f], vec![0x0f]].as_slice()));
    }

    #[test]
    fn empty_arrays_accept_only_empty_fills() {
        let mut dimensioned = DimensionedScalars::empty(0, 2).expect("valid extent");
        assert_eq!(dimensioned.fill_values(Vec::new()), Some(()));
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| dimensioned.fill_tokens(ctx, Vec::new()))
                .expect("admitted scalar fill"),
            Some(())
        );
        assert_eq!(dimensioned.fill_values(vec![None]), None);
        assert_eq!(
            crate::decode::with_test_decode_ctx(
                |ctx| dimensioned.fill_tokens(ctx, vec![(None, Vec::new())])
            )
            .expect("admitted scalar fill"),
            None
        );
        let mut counted = CountedScalars::empty(0).expect("valid extent");
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| counted.fill_tokens(ctx, Vec::new()))
                .expect("admitted scalar fill"),
            Some(())
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(
                |ctx| counted.fill_tokens(ctx, vec![(None, Vec::new())])
            )
            .expect("admitted scalar fill"),
            None
        );
    }
    #[test]
    fn scalar_arrays_reject_nonfinite_slots_without_mutation() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut array = CountedScalars::empty(1).expect("extent");
            let before = array.clone();
            assert_eq!(array.fill_values(vec![Some(value)]), None);
            assert_eq!(array, before);
            assert_eq!(
                crate::decode::with_test_decode_ctx(
                    |ctx| array.fill_tokens(ctx, vec![(Some(value), vec![0xe4])])
                )
                .expect("service"),
                None
            );
            assert_eq!(array, before);
        }
    }
    #[test]
    fn value_only_fill_clears_source_tokens() {
        let mut array = CountedScalars::empty(1).expect("extent");
        crate::decode::with_test_decode_ctx(|ctx| {
            array.fill_tokens(ctx, vec![(Some(1.0), vec![0xe4])])
        })
        .expect("service")
        .expect("extent");
        assert_eq!(array.fill_values(vec![Some(2.0)]), Some(()));
        assert_eq!(array.values(), &[Some(2.0)]);
        assert_eq!(array.tokens(), None);
    }
}
