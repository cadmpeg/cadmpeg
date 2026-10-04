// SPDX-License-Identifier: Apache-2.0
//! Shared fixtures for the solver test trees.

use std::{collections::HashSet, sync::Arc};

mod coordinate_closure;

pub(super) fn repeated_domain(domain: HashSet<usize>, count: usize) -> Vec<Arc<HashSet<usize>>> {
    let domain = Arc::new(domain);
    vec![domain; count]
}
