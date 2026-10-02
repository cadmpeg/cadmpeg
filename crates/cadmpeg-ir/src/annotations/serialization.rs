// SPDX-License-Identifier: Apache-2.0
//! Borrow exactness keys and tables during serialization.

use super::{FieldName, NonEmptyMap};
use serde::{Serialize, Serializer};

impl Serialize for FieldName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { serializer.serialize_str(self.as_str()) }
}

impl Serialize for NonEmptyMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { self.0.serialize(serializer) }
}
