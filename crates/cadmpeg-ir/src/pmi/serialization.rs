// SPDX-License-Identifier: Apache-2.0
//! Borrow datum reference lists.

use super::DatumReferences;
use serde::{Serialize, Serializer};

impl Serialize for DatumReferences {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}
