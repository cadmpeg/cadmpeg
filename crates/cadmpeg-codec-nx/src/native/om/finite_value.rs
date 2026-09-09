use serde::{Deserialize, Serialize};

/// Finite numeric value of an NX expression.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub(crate) struct FiniteValue(f64);

impl FiniteValue {
    pub(crate) fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for FiniteValue {
    type Error = &'static str;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err("expression value must be finite")
        }
    }
}

impl From<FiniteValue> for f64 {
    fn from(value: FiniteValue) -> Self {
        value.get()
    }
}

#[cfg(test)]
mod tests {
    use super::FiniteValue;
    use serde::Deserialize;

    #[test]
    fn finite_values_admit_signed_zero_and_reject_nonfinite_rust_and_serde_values() {
        for value in [0.0, -0.0, -1.0, f64::MAX, f64::MIN] {
            let finite = FiniteValue::try_from(value).unwrap();
            assert_eq!(finite.get().to_bits(), value.to_bits());
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(FiniteValue::try_from(value).is_err());
            let deserializer =
                serde::de::value::F64Deserializer::<serde::de::value::Error>::new(value);
            assert!(FiniteValue::deserialize(deserializer)
                .unwrap_err()
                .to_string()
                .contains("value"));
        }
    }
}
