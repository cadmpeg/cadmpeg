// SPDX-License-Identifier: Apache-2.0
//! Complete null-reference trailers and count-selected numeric tails.

use crate::intersection::TermUseForm;
use cadmpeg_core::decode::View;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NullTailForm {
    Two,
    Four,
}
impl NullTailForm {
    pub(crate) fn raw(self) -> &'static [u8] {
        match self {
            Self::Two => &[0, 1, 0, 1],
            Self::Four => &[0, 1, 0, 1, 0, 1, 0, 1],
        }
    }
    pub(crate) fn references(self) -> &'static [u32] {
        match self {
            Self::Two => &[1; 2],
            Self::Four => &[1; 4],
        }
    }
    pub(crate) fn from_references(references: &[u32]) -> Result<Self, &'static str> {
        match references {
            [1, 1] => Ok(Self::Two),
            [1, 1, 1, 1] => Ok(Self::Four),
            _ => Err("references: expected two or four null XMT references"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerminalNullReferences {
    offset: usize,
    form: NullTailForm,
}
impl TerminalNullReferences {
    pub(crate) fn at_end(stream: &[u8]) -> Option<Self> {
        [NullTailForm::Four, NullTailForm::Two]
            .into_iter()
            .find_map(|form| {
                stream.ends_with(form.raw()).then(|| Self {
                    offset: stream.len() - form.raw().len(),
                    form,
                })
            })
    }
    pub(crate) fn offset(self) -> usize {
        self.offset
    }
    pub(crate) fn end(self) -> usize {
        self.offset + self.form.raw().len()
    }
    pub(crate) fn form(self) -> NullTailForm {
        self.form
    }
}

#[derive(Debug, Clone, PartialEq)]
enum NumericValues {
    One([f64; 8]),
    Two([f64; 19]),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NumericTailValues(NumericValues);
impl NumericTailValues {
    pub(crate) fn new(term_use_count: u32, values: Vec<f64>) -> Result<Self, &'static str> {
        if !values.iter().all(|value| value.is_finite()) {
            return Err("values: numeric tail scalars must be finite");
        }
        Ok(Self(match term_use_count {
            1 => NumericValues::One(
                values
                    .try_into()
                    .map_err(|_| "term_use_count/values: count 1 requires eight values")?,
            ),
            2 => NumericValues::Two(
                values
                    .try_into()
                    .map_err(|_| "term_use_count/values: count 2 requires nineteen values")?,
            ),
            _ => return Err("term_use_count: must be 1 or 2"),
        }))
    }
    pub(crate) fn term_use_count(&self) -> u32 {
        match self.0 {
            NumericValues::One(_) => 1,
            NumericValues::Two(_) => 2,
        }
    }
    pub(crate) fn values(&self) -> &[f64] {
        match &self.0 {
            NumericValues::One(values) => values,
            NumericValues::Two(values) => values,
        }
    }
    pub(crate) fn byte_len(&self) -> usize {
        self.values().len() * 8
    }
    pub(crate) fn bytes(&self) -> Vec<u8> {
        self.values()
            .iter()
            .flat_map(|value| value.to_be_bytes())
            .collect()
    }
    pub(crate) fn into_values(self) -> Vec<f64> {
        match self.0 {
            NumericValues::One(values) => values.to_vec(),
            NumericValues::Two(values) => values.to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TermUseNumericTail {
    pub(crate) term_use_xmt: u32,
    offset: usize,
    values: NumericTailValues,
}
impl TermUseNumericTail {
    pub(crate) fn read(
        stream: &[u8],
        offset: usize,
        term_use_xmt: u32,
        form: TermUseForm,
    ) -> Option<Self> {
        let count = match form {
            TermUseForm::LQuestion => 8,
            TermUseForm::Tf | TermUseForm::Ts => 19,
        };
        let mut view = View::over_retained(stream).child(offset, stream.len())?;
        let values = view.read_counted(count, 8, View::f64_be)?;
        let values = NumericTailValues::new(form.count(), values).ok()?;
        offset.checked_add(values.byte_len())?;
        Some(Self {
            term_use_xmt,
            offset,
            values,
        })
    }
    pub(crate) fn offset(&self) -> usize {
        self.offset
    }
    pub(crate) fn end(&self) -> usize {
        self.offset + self.values().byte_len()
    }
    pub(crate) fn values(&self) -> &NumericTailValues {
        &self.values
    }
    pub(crate) fn into_values(self) -> NumericTailValues {
        self.values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_values_require_the_selected_finite_cardinality() {
        for (count, values) in [(1, vec![0.0; 8]), (2, vec![0.0; 19])] {
            let tail = NumericTailValues::new(count, values.clone()).unwrap();
            assert_eq!(tail.term_use_count(), count);
            assert_eq!(tail.bytes().len(), values.len() * 8);
            let mut short = values.clone();
            short.pop();
            assert!(NumericTailValues::new(count, short).is_err());
            let mut nonfinite = values;
            nonfinite[0] = f64::INFINITY;
            assert!(NumericTailValues::new(count, nonfinite).is_err());
        }
    }
}
