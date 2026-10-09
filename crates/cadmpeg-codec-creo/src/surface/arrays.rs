// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
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
    tokens: Option<Vec<Vec<u8>>>,
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
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        if values.len() != extent.len {
            return Ok(None);
        }
        ctx.charge_work(u64_from_index(values.len()), "creo scalar array validation")?;
        Ok(FiniteScalarSlots::new(values).map(|values| Self {
            shape: extent.shape,
            values,
            tokens: None,
        }))
    }
    pub(super) fn from_tokens(
        ctx: &DecodeContext<'_>,
        extent: ScalarExtent<Shape>,
        slots: Vec<(Option<f64>, Vec<u8>)>,
    ) -> Result<Option<Self>, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        if slots.len() != extent.len {
            return Ok(None);
        }
        ctx.charge_work(u64_from_index(slots.len()), "creo scalar array validation")?;
        if slots
            .iter()
            .any(|(value, _)| value.is_some_and(|value| FiniteReal::new(value).is_none()))
        {
            return Ok(None);
        }
        let mut values = ctx.collection_vec(slots.len(), "creo scalar array values")?;
        let mut tokens = ctx.collection_vec(slots.len(), "creo scalar array tokens")?;
        ctx.charge_work(u64_from_index(slots.len()), "creo scalar array filling")?;
        for (value, token) in slots {
            values.push(value);
            tokens.push(token);
        }
        Ok(Some(Self {
            shape: extent.shape,
            values: FiniteScalarSlots(values),
            tokens: Some(tokens),
        }))
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
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
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
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0; 4], &arena, &policy).expect("root");
        let error = DimensionedScalars::from_tokens(&ctx, extent, vec![(None, Vec::new()); 4])
            .expect_err("final buffer needs four slots");
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

    #[test]
    fn scalar_extent_mismatch_is_free_and_keeps_original_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        for refused in [false, true] {
            if refused {
                ctx.charge_work_limit(1, "scalar mismatch seed").expect_err("zero work cap");
            }
            for len in [0, 2] {
                let extent = CountedScalars::extent(1).expect("extent");
                let values = CountedScalars::from_values(&ctx, extent, vec![Some(f64::INFINITY); len]);
                let tokens = CountedScalars::from_tokens(&ctx, extent, vec![(Some(f64::INFINITY), vec![0xe4]); len]);
                if refused {
                    let original = ctx.resource_refusal().expect("original refusal");
                    assert!(matches!(values, Err(CodecError::ResourceLimit(actual)) if actual == original));
                    assert!(matches!(tokens, Err(CodecError::ResourceLimit(actual)) if actual == original));
                } else {
                    assert_eq!(values.expect("fixed length comparison"), None);
                    assert_eq!(tokens.expect("fixed length comparison"), None);
                    assert_eq!(ctx.resource_refusal(), None);
                }
            }
        }
    }

    #[test]
    fn empty_scalar_extent_outputs_are_free_and_keep_original_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let counted = CountedScalars::extent(0).expect("count");
        let dimensioned = DimensionedScalars::extent(0, 2).expect("shape");
        let values = CountedScalars::from_values(&ctx, counted, Vec::new()).expect("zero work").expect("extent");
        assert_eq!(values.count(), 0);
        assert_eq!(values.values(), &[]);
        assert_eq!(values.tokens(), None);
        let tokens = CountedScalars::from_tokens(&ctx, counted, Vec::new()).expect("zero work").expect("extent");
        assert_eq!(tokens.count(), 0);
        assert_eq!(tokens.values(), &[]);
        assert_eq!(tokens.tokens(), Some([].as_slice()));
        let values = DimensionedScalars::from_values(&ctx, dimensioned, Vec::new()).expect("zero work").expect("extent");
        assert_eq!((values.dimensions(), values.count()), (0, 2));
        assert_eq!(values.values(), &[]);
        assert_eq!(values.tokens(), None);
        let tokens = DimensionedScalars::from_tokens(&ctx, dimensioned, Vec::new()).expect("zero work").expect("extent");
        assert_eq!((tokens.dimensions(), tokens.count()), (0, 2));
        assert_eq!(tokens.values(), &[]);
        assert_eq!(tokens.tokens(), Some([].as_slice()));
        assert_eq!(ctx.resource_refusal(), None);
        let original = ctx.charge_work_limit(1, "empty scalar seed").expect_err("zero work cap");
        assert!(matches!(CountedScalars::from_values(&ctx, counted, Vec::new()), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert!(matches!(CountedScalars::from_tokens(&ctx, counted, Vec::new()), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert!(matches!(DimensionedScalars::from_values(&ctx, dimensioned, Vec::new()), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert!(matches!(DimensionedScalars::from_tokens(&ctx, dimensioned, Vec::new()), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }

    #[test]
    fn scalar_token_mismatch_keeps_target_and_original_refusal() {
        let mut array = CountedScalars::empty(1).expect("fixture extent");
        let original_array = array.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(array.fill_tokens(&ctx, Vec::new()).expect("fixed length comparison"), None);
        assert_eq!(array, original_array);
        assert_eq!(ctx.resource_refusal(), None);
        let original = ctx.charge_work_limit(1, "scalar fill seed").expect_err("zero work cap");
        for slots in [Vec::new(), vec![(Some(1.0), vec![0xe4])]] {
            assert!(matches!(array.fill_tokens(&ctx, slots), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!(array, original_array);
            assert_eq!(ctx.resource_refusal(), Some(original));
        }
    }
}
