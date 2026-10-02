// SPDX-License-Identifier: Apache-2.0
//! Borrow local and external reference tokens during serialization.

use super::ReferenceTarget;
use serde::{Serialize, Serializer};

impl Serialize for ReferenceTarget {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum Wire<'a> { Null {}, Local { target: &'a str }, External { document: &'a str, object: &'a str } }
        match self { Self::Null => Wire::Null {}, Self::Local(target) => Wire::Local { target }, Self::External { document, object } => Wire::External { document, object } }.serialize(serializer)
    }
}
