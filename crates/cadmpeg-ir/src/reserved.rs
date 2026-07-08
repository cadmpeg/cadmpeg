// SPDX-License-Identifier: Apache-2.0
//! Reserved IR areas that are deliberately **not implemented** in v0.
//!
//! The decision record for IR v0 is: model the exact B-rep + topology graph and
//! geometry carriers and feature history now; assembly structure remains reserved.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Reserved for assembly structure and mates/joints. **Not implemented in v0.**
///
/// v0 models single-body and multi-body documents but not assembly instancing,
/// component trees, or joint constraints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub enum Assembly {
    /// The only variant: an explicit marker that assembly structure was not
    /// decoded. Carries no data.
    Reserved,
}
