// SPDX-License-Identifier: Apache-2.0
//! Serialize composite stages without cloning their owned definitions.

use super::CompositePattern;
use serde::{Serialize, Serializer};

impl Serialize for CompositePattern {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}
