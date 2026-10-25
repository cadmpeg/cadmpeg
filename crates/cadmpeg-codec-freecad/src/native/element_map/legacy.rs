// SPDX-License-Identifier: Apache-2.0
//! Canonical legacy rows and their checked grouped-name projection.

use super::{ElementMapNodes, ElementMappedName};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One admitted legacy name row; its containing list keeps source null positions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LegacyMapRecord {
    /// Explicit topology label, such as Face3.
    pub(crate) indexed_name: String,
    /// Literal mapped-name encoding in the legacy grammar.
    pub(crate) mapped_name: String,
    /// Persistent string selectors retained in source order.
    pub(crate) string_ids: Vec<i64>,
}

pub(crate) fn project(rows: &[Option<LegacyMapRecord>]) -> Result<ElementMapNodes, String> {
    let missing = rows.iter().any(Option::is_none);
    let mut groups = BTreeMap::<String, Vec<Option<Vec<ElementMappedName>>>>::new();
    for row in rows.iter().flatten() {
        let (kind, index) = crate::element_map::split_indexed_name(&row.indexed_name)
            .map_err(|error| error.to_string())?;
        let names = groups.entry(kind).or_default();
        if names.len() <= index {
            names.resize_with(index + 1, || if missing { None } else { Some(Vec::new()) });
            // Position zero is reserved by the naming grammar, not an unknown row.
            if names[0].is_none() {
                names[0] = Some(Vec::new());
            }
        }
        names[index]
            .get_or_insert_with(Vec::new)
            .push(ElementMappedName {
                encoded: row.mapped_name.clone(),
                resolved: Some(row.mapped_name.clone()),
                string_ids: row.string_ids.clone(),
            });
    }
    ElementMapNodes::try_sparse(vec![Some(super::ElementMapNode {
        map_id: 0,
        groups: groups
            .into_iter()
            .map(|(indexed_name, names)| {
                Some(super::ElementMapGroup {
                    indexed_name,
                    children: Vec::new(),
                    names,
                })
            })
            .collect(),
    })])
}
