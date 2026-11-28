// SPDX-License-Identifier: Apache-2.0
//! Constructions that state only a resolved value.

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{Construction, ConstructionKind};

/// A construction with no stored inputs, steps, solution selector, or mode,
/// and no value in any other configuration. The caller states its resolved
/// value through struct update.
pub(crate) fn empty<K: ConstructionKind>() -> Construction<K> {
    Construction {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: None,
        resolved_in: BTreeMap::new(),
    }
}
