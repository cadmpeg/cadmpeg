// SPDX-License-Identifier: Apache-2.0
//! Decode, export, loss, and validation reports.

use std::fmt;

use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod check;
pub mod decode;
pub mod export;
pub mod loss;

/// Severity of a loss note or validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum Severity {
    /// Informational; no action needed.
    Info,
    /// Non-fatal approximation or normalization.
    Warning,
    /// A correctness problem in the produced IR or export.
    Error,
    /// A hard stop: the requested operation cannot be completed faithfully.
    Blocking,
}

impl DecodeCost for Severity {
    const FIXED_BYTES: Option<u64> = Some(u64_from_index(std::mem::size_of::<Self>()));

    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(u64_from_index(std::mem::size_of::<Self>()))
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Blocking => "blocking",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Severity;
    use cadmpeg_core::decode::cost::DecodeCost;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    #[test]
    fn severity_has_fixed_comparison_cost() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        assert_eq!(
            Severity::Warning
                .decode_cost(&ctx, "compare severity")
                .expect("cost"),
            u64::try_from(std::mem::size_of::<Severity>()).expect("test size fits")
        );
        assert_eq!(
            <Severity as DecodeCost>::FIXED_BYTES,
            Some(u64::try_from(std::mem::size_of::<Severity>()).expect("test size fits"))
        );
    }
}
