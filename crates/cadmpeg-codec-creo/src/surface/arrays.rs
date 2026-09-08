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

    /// Fills the declared slots in source order.
    pub(crate) fn fill_values(&mut self, values: impl IntoIterator<Item = Option<f64>>) {
        for (target, value) in self.values.iter_mut().zip(values) {
            *target = value;
        }
    }

    /// Fills the declared slots with values and source tokens.
    pub(crate) fn fill_tokens(&mut self, slots: impl IntoIterator<Item = (Option<f64>, Vec<u8>)>) {
        let mut slots = slots.into_iter();
        self.tokens = Some(
            self.values
                .iter_mut()
                .map(|value| {
                    let (decoded, token) = slots.next().unwrap_or_default();
                    *value = decoded;
                    token
                })
                .collect(),
        );
    }

    /// Admits arrays whose declared extents and token lengths agree.
    #[cfg(test)]
    pub(crate) fn try_new(
        dimensions: u32,
        count: u32,
        values: Vec<Option<f64>>,
        tokens: Option<Vec<Vec<u8>>>,
    ) -> Option<Self> {
        let expected = usize::try_from(dimensions)
            .ok()?
            .checked_mul(usize::try_from(count).ok()?)?;
        (values.len() == expected
            && tokens
                .as_ref()
                .is_none_or(|tokens| tokens.len() == expected))
        .then_some(Self {
            dimensions,
            count,
            values,
            tokens,
        })
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

/// A counted scalar array with a token and value for every declared slot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CountedScalars {
    count: u32,
    values: Vec<Option<f64>>,
    tokens: Vec<Vec<u8>>,
}

impl CountedScalars {
    /// Allocates the declared shape with undecoded slots.
    pub(crate) fn empty(count: u32) -> Option<Self> {
        let len = usize::try_from(count).ok()?;
        Some(Self {
            count,
            values: std::iter::repeat_n(None, len).collect(),
            tokens: std::iter::repeat_with(Vec::new).take(len).collect(),
        })
    }

    /// Fills the declared slots with values and source tokens.
    pub(crate) fn fill_tokens(&mut self, slots: impl IntoIterator<Item = (Option<f64>, Vec<u8>)>) {
        for ((value, token), (decoded, bytes)) in
            self.values.iter_mut().zip(&mut self.tokens).zip(slots)
        {
            *value = decoded;
            *token = bytes;
        }
    }

    /// Admits arrays whose declared extents and token lengths agree.
    #[cfg(test)]
    pub(crate) fn try_new(
        count: u32,
        values: Vec<Option<f64>>,
        tokens: Vec<Vec<u8>>,
    ) -> Option<Self> {
        let expected = usize::try_from(count).ok()?;
        (values.len() == expected && tokens.len() == expected).then_some(Self {
            count,
            values,
            tokens,
        })
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
    pub(crate) fn tokens(&self) -> &[Vec<u8>] {
        &self.tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensioned_arrays_admit_only_complete_slot_shapes() {
        assert!(DimensionedScalars::try_new(2, 3, vec![None; 5], None).is_none());
        assert!(DimensionedScalars::try_new(2, 3, vec![None; 6], Some(vec![vec![]; 5])).is_none());
        assert!(DimensionedScalars::try_new(2, 3, vec![None; 6], Some(vec![vec![]; 6])).is_some());
        assert!(DimensionedScalars::try_new(0, 3, vec![], None).is_some());
    }

    #[test]
    fn counted_arrays_admit_only_complete_slot_shapes() {
        assert!(CountedScalars::try_new(2, vec![None], vec![vec![]; 2]).is_none());
        assert!(CountedScalars::try_new(2, vec![None; 2], vec![vec![]]).is_none());
        assert!(CountedScalars::try_new(2, vec![None; 2], vec![vec![]; 2]).is_some());
        assert!(CountedScalars::try_new(0, vec![], vec![]).is_some());
    }
}
