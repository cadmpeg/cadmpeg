// SPDX-License-Identifier: Apache-2.0
//! Finite support-UV tuples with their exact packing marker.

use super::SupportUv;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SupportUvPacking { Form2, Form3, Form4 }

impl TryFrom<u8> for SupportUvPacking {
    type Error = &'static str;
    fn try_from(marker: u8) -> Result<Self, Self::Error> {
        match marker {
            2 => Ok(Self::Form2), 3 => Ok(Self::Form3), 4 => Ok(Self::Form4),
            _ => Err("marker: must be 2, 3, or 4"),
        }
    }
}
impl SupportUvPacking {
    pub(crate) fn marker(self) -> u8 {
        match self { Self::Form2 => 2, Self::Form3 => 3, Self::Form4 => 4 }
    }
    fn width(self) -> usize {
        match self { Self::Form2 | Self::Form3 => 2, Self::Form4 => 4 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SupportUvValues {
    packing: SupportUvPacking,
    values: Vec<f64>,
}
impl SupportUvValues {
    pub(crate) fn new(packing: SupportUvPacking, values: Vec<f64>) -> Result<Self, &'static str> {
        u32::try_from(values.len()).map_err(|_| "values: scalar count exceeds u32")?;
        if values.len() < packing.width() * 2 || !values.len().is_multiple_of(packing.width()) {
            return Err("values: must contain at least two complete tuples for marker");
        }
        if !values.iter().all(|value| value.is_finite()) {
            return Err("values: scalars must be finite");
        }
        Ok(Self { packing, values })
    }

    pub(crate) fn count(&self) -> u32 { self.values.len() as u32 }
    pub(crate) fn marker(&self) -> u8 { self.packing.marker() }
    pub(crate) fn packing(&self) -> SupportUvPacking { self.packing }
    pub(crate) fn values(&self) -> &[f64] { &self.values }
    pub(crate) fn into_values(self) -> Vec<f64> { self.values }

    pub(crate) fn support_uv(&self) -> SupportUv {
        let first = self.values().chunks_exact(self.packing.width()).map(|row| [row[0], row[1]]).collect();
        let second = match self.packing {
            SupportUvPacking::Form2 | SupportUvPacking::Form3 => None,
            SupportUvPacking::Form4 => Some(self.values().chunks_exact(4).map(|row| [row[2], row[3]]).collect()),
        };
        [Some(first), second]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_requires_complete_finite_tuples() {
        for marker in [2, 3, 4] {
            let packing = SupportUvPacking::try_from(marker).unwrap();
            let width = packing.width();
            let values = SupportUvValues::new(packing, std::iter::repeat_n(0.0, width * 2).collect::<Vec<_>>()).unwrap();
            assert_eq!(values.marker(), marker);
            assert_eq!(values.support_uv()[1].is_some(), marker == 4);
            for len in [0, width, width * 2 + 1] {
                assert!(SupportUvValues::new(packing, std::iter::repeat_n(0.0, len).collect::<Vec<_>>()).is_err());
            }
            let mut values = std::iter::repeat_n(0.0, width * 2).collect::<Vec<_>>();
            values[0] = f64::NAN;
            assert!(SupportUvValues::new(packing, values).is_err());
        }
        assert!(SupportUvPacking::try_from(1).is_err());
    }
}
