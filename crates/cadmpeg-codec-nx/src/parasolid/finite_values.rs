// SPDX-License-Identifier: Apache-2.0
//! Finite counted attribute values, retained from scanning through native JSON.

use std::marker::PhantomData;

use cadmpeg_core::decode::View;
use serde::{Deserialize, Deserializer, Serialize};

pub(crate) trait FiniteValue: Sized {
    const WIDTH: usize;
    fn read(bytes: &[u8]) -> Option<Self>;
    fn is_finite(&self) -> bool;
}

impl FiniteValue for f64 {
    const WIDTH: usize = 8;

    fn read(bytes: &[u8]) -> Option<Self> {
        View::f64_be_at(bytes, 0)
    }

    fn is_finite(&self) -> bool {
        f64::is_finite(*self)
    }
}

impl FiniteValue for [f64; 3] {
    const WIDTH: usize = 24;

    fn read(bytes: &[u8]) -> Option<Self> {
        crate::vec3_at::vec3_be_at(bytes, 0)
    }

    fn is_finite(&self) -> bool {
        self.iter().all(|value| value.is_finite())
    }
}

impl FiniteValue for [[f64; 3]; 2] {
    const WIDTH: usize = 48;

    fn read(bytes: &[u8]) -> Option<Self> {
        Some([
            <[f64; 3]>::read(bytes)?,
            <[f64; 3]>::read(bytes.get(24..)?)?,
        ])
    }

    fn is_finite(&self) -> bool {
        self.iter().all(FiniteValue::is_finite)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub(crate) struct FiniteValues<T>(Vec<T>);

impl<T: FiniteValue> FiniteValues<T> {
    pub(crate) fn new(values: Vec<T>) -> Result<Self, &'static str> {
        if values.is_empty() || !values.iter().all(FiniteValue::is_finite) {
            return Err("values must be nonempty and finite");
        }
        Ok(Self(values))
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        &self.0
    }
}

impl<'de, T: FiniteValue + Deserialize<'de>> Deserialize<'de> for FiniteValues<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Vec::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct FiniteLane<'a, T> {
    bytes: &'a [u8],
    value: PhantomData<T>,
}

impl<'a, T: FiniteValue> FiniteLane<'a, T> {
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

    pub(crate) fn materialize(self) -> Option<FiniteValues<T>> {
        self.bytes
            .chunks_exact(T::WIDTH)
            .map(T::read)
            .collect::<Option<Vec<_>>>()
            .map(FiniteValues)
    }
}

#[cfg(test)]
mod tests {
    use super::{FiniteLane, FiniteValues};

    #[test]
    fn finite_values_preserve_json_and_reject_empty_or_nonfinite_values() {
        let values = FiniteValues::new(vec![1.0, -2.0]).unwrap();
        assert_eq!(serde_json::to_string(&values).unwrap(), "[1.0,-2.0]");
        assert_eq!(
            serde_json::from_str::<FiniteValues<f64>>("[1.0,-2.0]").unwrap(),
            values
        );
        let axes = FiniteValues::new(vec![[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]]).unwrap();
        let wire = "[[[1.0,2.0,3.0],[4.0,5.0,6.0]]]";
        assert_eq!(serde_json::to_string(&axes).unwrap(), wire);
        assert_eq!(
            serde_json::from_str::<FiniteValues<[[f64; 3]; 2]>>(wire).unwrap(),
            axes
        );
        assert!(serde_json::from_str::<FiniteValues<f64>>("[]").is_err());
        assert!(FiniteValues::new(vec![f64::INFINITY]).is_err());
        assert!(FiniteValues::new(vec![[0.0, f64::NAN, 1.0]]).is_err());
        assert!(FiniteValues::new(vec![[[0.0; 3], [f64::NEG_INFINITY; 3]]]).is_err());
    }

    #[test]
    fn finite_lanes_require_whole_values_and_materialize_exactly() {
        let bytes = 1.0_f64.to_be_bytes();
        let lane = FiniteLane::<f64>::new(&bytes).unwrap();
        assert_eq!(lane.materialize().unwrap().as_slice(), &[1.0]);
        assert!(FiniteLane::<f64>::new(&[]).is_none());
        assert!(FiniteLane::<f64>::new(&bytes[..7]).is_none());
        assert!(FiniteLane::<f64>::new(&f64::NAN.to_be_bytes()).is_none());
        assert!(FiniteLane::<[[f64; 3]; 2]>::new(&[0; 24]).is_none());
        assert_eq!(
            FiniteLane::<[[f64; 3]; 2]>::new(&[0; 48])
                .unwrap()
                .materialize()
                .unwrap()
                .as_slice(),
            &[[[0.0; 3]; 2]]
        );
    }
}
