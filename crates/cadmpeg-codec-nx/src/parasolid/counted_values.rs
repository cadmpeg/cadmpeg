// SPDX-License-Identifier: Apache-2.0
//! Nonempty counted attribute values with finite floating-point lanes.

use cadmpeg_core::decode::View;
use serde::{Deserialize, Deserializer, Serialize};

pub(crate) trait CountedValue: Sized {
    type Checked: Serialize;
    const WIDTH: usize;
    fn read(bytes: &[u8]) -> Option<Self>;
    fn admit(self) -> Option<Self::Checked>;
    #[cfg(test)]
    fn raw(value: &Self::Checked) -> Self;
}

impl CountedValue for u32 {
    type Checked = u32;
    const WIDTH: usize = 4;

    fn read(bytes: &[u8]) -> Option<Self> {
        View::u32_be_at(bytes, 0)
    }
    fn admit(self) -> Option<Self::Checked> {
        Some(self)
    }
    #[cfg(test)]
    fn raw(value: &Self::Checked) -> Self {
        *value
    }
}

impl CountedValue for f64 {
    type Checked = cadmpeg_ir::scalar::FiniteReal;
    const WIDTH: usize = 8;

    fn read(bytes: &[u8]) -> Option<Self> {
        View::f64_be_at(bytes, 0)
    }

    fn admit(self) -> Option<Self::Checked> {
        Self::Checked::new(self)
    }
    #[cfg(test)]
    fn raw(value: &Self::Checked) -> Self {
        value.get()
    }
}

impl CountedValue for [f64; 3] {
    type Checked = cadmpeg_ir::units::FiniteVector<3>;
    const WIDTH: usize = 24;

    fn read(bytes: &[u8]) -> Option<Self> {
        crate::vec3_at::vec3_be_at(bytes, 0)
    }

    fn admit(self) -> Option<Self::Checked> {
        Self::Checked::new(self)
    }
    #[cfg(test)]
    fn raw(value: &Self::Checked) -> Self {
        value.get()
    }
}

impl CountedValue for [[f64; 3]; 2] {
    type Checked = [cadmpeg_ir::units::FiniteVector<3>; 2];
    const WIDTH: usize = 48;

    fn read(bytes: &[u8]) -> Option<Self> {
        Some([
            <[f64; 3]>::read(bytes)?,
            <[f64; 3]>::read(bytes.get(24..)?)?,
        ])
    }

    fn admit(self) -> Option<Self::Checked> {
        let [first, second] = self;
        Some([
            cadmpeg_ir::units::FiniteVector::new(first)?,
            cadmpeg_ir::units::FiniteVector::new(second)?,
        ])
    }
    #[cfg(test)]
    fn raw(value: &Self::Checked) -> Self {
        value.map(cadmpeg_ir::units::FiniteVector::get)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct CountedValues<T: CountedValue>(Vec<T::Checked>);

impl<T: CountedValue> CountedValues<T> {
    pub(crate) fn new(values: Vec<T>) -> Result<Self, &'static str> {
        if values.is_empty() {
            return Err("values must be nonempty and finite");
        }
        let values = values
            .into_iter()
            .map(T::admit)
            .collect::<Option<Vec<_>>>()
            .ok_or("values must be nonempty and finite")?;
        Ok(Self(values))
    }

    pub(crate) fn as_slice(&self) -> &[T::Checked] {
        &self.0
    }

    /// Admit one complete nonempty big-endian value lane.
    pub(super) fn from_be_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() || !bytes.len().is_multiple_of(T::WIDTH) {
            return None;
        }
        Some(Self(
            bytes
                .chunks_exact(T::WIDTH)
                .map(|bytes| T::read(bytes)?.admit())
                .collect::<Option<Vec<_>>>()?,
        ))
    }

    #[cfg(test)]
    pub(crate) fn raw_values(&self) -> Vec<T> {
        self.0.iter().map(T::raw).collect()
    }
}

impl<'de, T: CountedValue + Deserialize<'de>> Deserialize<'de> for CountedValues<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Vec::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::CountedValues;

    #[test]
    fn counted_integer_values_preserve_unsigned_values_and_reject_empty_lanes() {
        let values = CountedValues::<u32>::from_be_bytes(&[255; 4]).unwrap();
        assert_eq!(values.as_slice(), &[u32::MAX]);
        assert_eq!(serde_json::to_string(&values).unwrap(), "[4294967295]");
        assert!(CountedValues::<u32>::from_be_bytes(&[]).is_none());
        assert!(CountedValues::new(Vec::<u32>::new()).is_err());
        assert!(serde_json::from_str::<CountedValues<u32>>("[]").is_err());
    }

    #[test]
    fn finite_values_preserve_json_and_reject_empty_or_nonfinite_values() {
        let values = CountedValues::new(vec![1.0, -2.0]).unwrap();
        assert_eq!(serde_json::to_string(&values).unwrap(), "[1.0,-2.0]");
        assert_eq!(
            serde_json::from_str::<CountedValues<f64>>("[1.0,-2.0]").unwrap(),
            values
        );
        let axes = CountedValues::new(vec![[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]]).unwrap();
        let wire = "[[[1.0,2.0,3.0],[4.0,5.0,6.0]]]";
        assert_eq!(serde_json::to_string(&axes).unwrap(), wire);
        assert_eq!(
            serde_json::from_str::<CountedValues<[[f64; 3]; 2]>>(wire).unwrap(),
            axes
        );
        assert!(serde_json::from_str::<CountedValues<f64>>("[]").is_err());
        assert!(CountedValues::new(vec![f64::INFINITY]).is_err());
        assert!(CountedValues::new(vec![[0.0, f64::NAN, 1.0]]).is_err());
        assert!(CountedValues::new(vec![[[0.0; 3], [f64::NEG_INFINITY; 3]]]).is_err());
    }

    #[test]
    fn finite_lanes_require_whole_values_and_materialize_exactly() {
        let bytes = 1.0_f64.to_be_bytes();
        let lane = CountedValues::<f64>::from_be_bytes(&bytes).unwrap();
        assert_eq!(lane.raw_values().as_slice(), &[1.0]);
        assert!(CountedValues::<f64>::from_be_bytes(&[]).is_none());
        assert!(CountedValues::<f64>::from_be_bytes(&bytes[..7]).is_none());
        assert!(CountedValues::<f64>::from_be_bytes(&f64::NAN.to_be_bytes()).is_none());
        assert!(CountedValues::<[[f64; 3]; 2]>::from_be_bytes(&[0; 24]).is_none());
        assert_eq!(
            CountedValues::<[[f64; 3]; 2]>::from_be_bytes(&[0; 48])
                .unwrap()
                .raw_values()
                .as_slice(),
            &[[[0.0; 3]; 2]]
        );
    }
}
