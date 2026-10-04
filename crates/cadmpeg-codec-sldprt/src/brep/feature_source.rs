// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// A history-feature source identifier outside the absent wire values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) struct FeatureSourceId(NonZeroU32);

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSourceId {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

impl FeatureSourceId {
    pub(crate) fn value(self) -> u32 {
        self.0.get()
    }
}

impl TryFrom<u32> for FeatureSourceId {
    type Error = &'static str;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == u32::MAX {
            return Err("feature_source_id is absent");
        }
        NonZeroU32::new(value)
            .map(Self)
            .ok_or("feature_source_id is absent")
    }
}

impl From<FeatureSourceId> for u32 {
    fn from(value: FeatureSourceId) -> Self {
        value.value()
    }
}

#[cfg(test)]
mod tests {
    use super::FeatureSourceId;

    #[test]
    fn source_id_admission_excludes_only_absent_values() {
        for value in [0, u32::MAX] {
            assert!(FeatureSourceId::try_from(value).is_err());
            assert!(serde_json::from_value::<FeatureSourceId>(serde_json::json!(value)).is_err());
        }
        for value in [1, 75, u32::MAX - 1] {
            let source = FeatureSourceId::try_from(value).expect("present source ID");
            assert_eq!(source.value(), value);
            assert_eq!(
                serde_json::to_value(source).expect("source ID JSON"),
                serde_json::json!(value)
            );
        }
    }
}
