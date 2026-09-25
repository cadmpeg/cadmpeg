// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Scalar slots and their source tokens, with a checked count or shape.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Scalars<Shape> {
    shape: Shape,
    values: Vec<Option<f64>>,
    tokens: Option<Vec<Vec<u8>>>,
}

/// An outer dimension and a scalar count per dimension.
pub(crate) type DimensionedScalars = Scalars<[u32; 2]>;
/// A scalar count without an outer dimension.
pub(crate) type CountedScalars = Scalars<u32>;

impl DimensionedScalars {
    /// Admit the declared grid before allocating its value slots.
    pub(crate) fn admit_empty(
        ctx: &DecodeContext<'_>,
        dimensions: u32,
        count: u32,
    ) -> Result<Option<Self>, CodecError> {
        let Some(len) = usize::try_from(dimensions)
            .ok()
            .and_then(|dimensions| usize::try_from(count).ok()?.checked_mul(dimensions))
        else {
            return Ok(None);
        };
        Ok(Some(Self {
            shape: [dimensions, count],
            values: ctx.alloc_filled(len, None, "admit Creo spline scalar grid")?,
            tokens: None,
        }))
    }

    /// Allocates the declared shape with undecoded slots.
    #[cfg(test)]
    pub(crate) fn empty(dimensions: u32, count: u32) -> Option<Self> {
        let len = usize::try_from(dimensions)
            .ok()?
            .checked_mul(usize::try_from(count).ok()?)?;
        Some(Self {
            shape: [dimensions, count],
            values: cadmpeg_core::decode::alloc_filled(len, None, "creo scalar slots").ok()?,
            tokens: None,
        })
    }

    /// Stored outer dimension.
    pub(crate) fn dimensions(&self) -> u32 {
        self.shape[0]
    }
    /// Stored scalar count per dimension.
    pub(crate) fn count(&self) -> u32 {
        self.shape[1]
    }
}

impl CountedScalars {
    /// Allocates the declared count with undecoded slots.
    pub(super) fn empty(count: u32) -> Option<Self> {
        Some(Self {
            shape: count,
            values: cadmpeg_core::decode::alloc_filled(
                usize::try_from(count).ok()?,
                None,
                "creo scalar slots",
            )
            .ok()?,
            tokens: None,
        })
    }

    /// Stored scalar count.
    pub(crate) fn count(&self) -> u32 {
        self.shape
    }
}

impl<Shape> Scalars<Shape> {
    /// Replaces values only when the input matches the declared extent.
    pub(crate) fn fill_values(&mut self, values: Vec<Option<f64>>) -> Option<()> {
        if values.len() != self.values.len() {
            return None;
        }
        self.values = values;
        Some(())
    }

    /// Replaces values and tokens only when the input matches the declared extent.
    pub(super) fn fill_tokens(&mut self, slots: Vec<(Option<f64>, Vec<u8>)>) -> Option<()> {
        if slots.len() != self.values.len() {
            return None;
        }
        let (values, tokens) = slots.into_iter().unzip();
        self.values = values;
        self.tokens = Some(tokens);
        Some(())
    }

    /// Decoded values in slot order.
    pub(crate) fn values(&self) -> &[Option<f64>] {
        &self.values
    }
    /// Source token bytes in slot order.
    pub(crate) fn tokens(&self) -> Option<&[Vec<u8>]> {
        self.tokens.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::{CountedScalars, DimensionedScalars};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn dimensioned_scalar_grid_refuses_before_allocating_value_slots() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0; 4], &arena, &policy)
            .expect("small root is admitted");
        let error = DimensionedScalars::admit_empty(&ctx, 2, 2)
            .expect_err("four slots exceed the three-item limit");
        assert!(matches!(
            error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Creo spline scalar grid"
        ));

        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0; 4], &arena, &service)
            .expect("small root is admitted");
        let grid = DimensionedScalars::admit_empty(&ctx, 2, 2)
            .expect("service profile admits four slots")
            .expect("valid shape");
        assert_eq!(grid.values(), &[None; 4]);
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
        assert_eq!(array.tokens(), original.tokens());
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
        assert_eq!(array.tokens(), Some([vec![0x0f], vec![0x0f]].as_slice()));
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
}
