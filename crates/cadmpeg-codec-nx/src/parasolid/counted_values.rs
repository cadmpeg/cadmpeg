// SPDX-License-Identifier: Apache-2.0
//! Nonempty counted attribute values with finite floating-point lanes.

use std::marker::PhantomData;

use cadmpeg_core::decode::View;
use serde::{Deserialize, Deserializer, Serialize};

pub(crate) trait CountedValue: Sized {
    const WIDTH: usize;
    fn read(bytes: &[u8]) -> Option<Self>;
    fn is_finite(&self) -> bool;
}

impl CountedValue for u32 {
    const WIDTH: usize = 4;

    fn read(bytes: &[u8]) -> Option<Self> { View::u32_be_at(bytes, 0) }
    fn is_finite(&self) -> bool { true }
}

impl CountedValue for f64 {
    const WIDTH: usize = 8;

    fn read(bytes: &[u8]) -> Option<Self> {
        View::f64_be_at(bytes, 0)
    }

    fn is_finite(&self) -> bool {
        f64::is_finite(*self)
    }
}

impl CountedValue for [f64; 3] {
    const WIDTH: usize = 24;

    fn read(bytes: &[u8]) -> Option<Self> {
        crate::vec3_at::vec3_be_at(bytes, 0)
    }

    fn is_finite(&self) -> bool {
        self.iter().all(|value| value.is_finite())
    }
}

impl CountedValue for [[f64; 3]; 2] {
    const WIDTH: usize = 48;

    fn read(bytes: &[u8]) -> Option<Self> {
        Some([
            <[f64; 3]>::read(bytes)?,
            <[f64; 3]>::read(bytes.get(24..)?)?,
        ])
    }

    fn is_finite(&self) -> bool {
        self.iter().all(CountedValue::is_finite)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct CountedValues<T>(Vec<T>);

impl<T: CountedValue> CountedValues<T> {
    pub(crate) fn new(values: Vec<T>) -> Result<Self, &'static str> {
        if values.is_empty() || !values.iter().all(CountedValue::is_finite) {
            return Err("values must be nonempty and finite");
        }
        Ok(Self(values))
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        &self.0
    }
}

impl<'de, T: CountedValue + Deserialize<'de>> Deserialize<'de> for CountedValues<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Vec::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CountedLane<'a, T> {
    bytes: &'a [u8],
    value: PhantomData<T>,
}

impl<'a, T: CountedValue> CountedLane<'a, T> {
    pub(crate) fn new(bytes: &'a [u8]) -> Option<Self> {
        if bytes.is_empty() || !bytes.len().is_multiple_of(T::WIDTH) {
            return None;
        }
        for bytes in bytes.chunks_exact(T::WIDTH) {
            T::read(bytes)?.is_finite().then_some(())?;
        }
        Some(Self {
            bytes,
            value: PhantomData,
        })
    }

    pub(crate) fn materialize(self) -> Option<CountedValues<T>> {
        self.bytes
            .chunks_exact(T::WIDTH)
            .map(T::read)
            .collect::<Option<Vec<_>>>()
            .map(CountedValues)
    }
}

#[cfg(test)]
mod tests {
    use super::{CountedLane, CountedValues};

    #[test]
    fn counted_integer_values_preserve_unsigned_values_and_reject_empty_lanes() {
        let lane = CountedLane::<u32>::new(&[255; 4]).unwrap();
        let values = lane.materialize().unwrap();
        assert_eq!(values.as_slice(), &[u32::MAX]);
        assert_eq!(serde_json::to_string(&values).unwrap(), "[4294967295]");
        assert!(CountedLane::<u32>::new(&[]).is_none());
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
        let lane = CountedLane::<f64>::new(&bytes).unwrap();
        assert_eq!(lane.materialize().unwrap().as_slice(), &[1.0]);
        assert!(CountedLane::<f64>::new(&[]).is_none());
        assert!(CountedLane::<f64>::new(&bytes[..7]).is_none());
        assert!(CountedLane::<f64>::new(&f64::NAN.to_be_bytes()).is_none());
        assert!(CountedLane::<[[f64; 3]; 2]>::new(&[0; 24]).is_none());
        assert_eq!(
            CountedLane::<[[f64; 3]; 2]>::new(&[0; 48])
                .unwrap()
                .materialize()
                .unwrap()
                .as_slice(),
            &[[[0.0; 3]; 2]]
        );
    }
}
