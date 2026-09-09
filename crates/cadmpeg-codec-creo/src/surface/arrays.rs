// SPDX-License-Identifier: Apache-2.0

/// A dimensioned scalar array with one value per declared slot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DimensionedScalars {
    dimensions: u32,
    count: u32,
    values: Vec<Option<f64>>,
    tokens: Option<Vec<Vec<u8>>>,
}

impl DimensionedScalars {
    /// Allocates the declared shape with undecoded slots.
    pub(crate) fn empty(dimensions: u32, count: u32) -> Option<Self> {
        let len = usize::try_from(dimensions)
            .ok()?
            .checked_mul(usize::try_from(count).ok()?)?;
        Some(Self {
            dimensions,
            count,
            values: std::iter::repeat_n(None, len).collect(),
            tokens: None,
        })
    }

    /// Replaces values only when the input matches the declared extent.
    pub(crate) fn fill_values(&mut self, values: Vec<Option<f64>>) -> Option<()> {
        if values.len() != self.values.len() {
            return None;
        }
        self.values = values;
        Some(())
    }

    /// Replaces values and tokens only when the input matches the declared extent.
    pub(crate) fn fill_tokens(&mut self, slots: Vec<(Option<f64>, Vec<u8>)>) -> Option<()> {
        if slots.len() != self.values.len() {
            return None;
        }
        let (values, tokens) = slots.into_iter().unzip();
        self.values = values;
        self.tokens = Some(tokens);
        Some(())
    }

    /// Stored outer dimension.
    pub(crate) fn dimensions(&self) -> u32 {
        self.dimensions
    }
    /// Stored scalar count per dimension.
    pub(crate) fn count(&self) -> u32 {
        self.count
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

/// A counted scalar array with undecoded values and optional source tokens.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CountedScalars {
    count: u32,
    values: Vec<Option<f64>>,
    tokens: Option<Vec<Vec<u8>>>,
}

impl CountedScalars {
    /// Allocates the declared shape with undecoded slots.
    pub(crate) fn empty(count: u32) -> Option<Self> {
        let len = usize::try_from(count).ok()?;
        Some(Self {
            count,
            values: std::iter::repeat_n(None, len).collect(),
            tokens: None,
        })
    }

    /// Replaces values and tokens only when the input matches the declared extent.
    pub(crate) fn fill_tokens(&mut self, slots: Vec<(Option<f64>, Vec<u8>)>) -> Option<()> {
        if slots.len() != self.values.len() {
            return None;
        }
        let (values, tokens) = slots.into_iter().unzip();
        self.values = values;
        self.tokens = Some(tokens);
        Some(())
    }

    /// Stored scalar count per dimension.
    pub(crate) fn count(&self) -> u32 {
        self.count
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
