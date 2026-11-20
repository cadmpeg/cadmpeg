// SPDX-License-Identifier: Apache-2.0
//! Borrowed wire projection for edge carriers.

use super::EdgeCarrier;
use crate::ids::CurveId;
use serde::{Serialize, Serializer};

impl Serialize for EdgeCarrier {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            curve: Option<&'a CurveId>,
            #[serde(skip_serializing_if = "Option::is_none")]
            param_range: Option<[f64; 2]>,
        }
        Wire {
            curve: self.curve(),
            param_range: self.param_range().map(crate::units::FiniteVector::get),
        }
        .serialize(serializer)
    }
}
