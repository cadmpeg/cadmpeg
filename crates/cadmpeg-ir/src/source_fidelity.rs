// SPDX-License-Identifier: Apache-2.0
//! Decode-time source fidelity and byte-accounting sidecar.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::annotations::Annotations;
use crate::byte_ledger::ByteLedger;

/// Source-fidelity schema version produced by this build.
pub const SOURCE_FIDELITY_VERSION: &str = "1";

/// Source-byte accounting and conversion facts accompanying one decoded IR.
///
/// This value is not part of the neutral product schema. Its version evolves
/// independently from [`crate::IR_VERSION`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceFidelity {
    /// Independently versioned sidecar schema.
    pub schema_version: String,
    /// Complete ownership of the decoded source byte stream.
    pub byte_ledger: ByteLedger,
    /// Sparse source locations and conversion exactness.
    #[serde(default)]
    pub annotations: Annotations,
}

impl Default for SourceFidelity {
    fn default() -> Self {
        Self {
            schema_version: SOURCE_FIDELITY_VERSION.into(),
            byte_ledger: ByteLedger::default(),
            annotations: Annotations::default(),
        }
    }
}

impl SourceFidelity {
    /// Canonicalize sidecar collections independently from the product model.
    pub fn finalize(&mut self) {
        self.byte_ledger.finalize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_version_is_independent_and_explicit() {
        let value = serde_json::to_value(SourceFidelity::default()).expect("serialize sidecar");

        assert_eq!(value["schema_version"], SOURCE_FIDELITY_VERSION);
        assert!(value.get("ir_version").is_none());
    }
}
