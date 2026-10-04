// SPDX-License-Identifier: Apache-2.0
//! Borrow dimensional wire metadata and datum reference lists.

use super::{DatumReferences, DimensionKind, DimensionTolerance, PmiDimension, PmiValue};
use serde::{Serialize, Serializer};

impl Serialize for DatumReferences {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl Serialize for PmiDimension {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            dimension: &'a DimensionKind,
            #[serde(skip_serializing_if = "Option::is_none")]
            nominal: Option<PmiValue>,
            #[serde(skip_serializing_if = "Option::is_none")]
            tolerance: Option<&'a DimensionTolerance>,
        }
        Wire {
            dimension: &self.kind,
            nominal: self.nominal,
            tolerance: self.tolerance.as_ref(),
        }
        .serialize(serializer)
    }
}
