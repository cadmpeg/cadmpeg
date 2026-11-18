// SPDX-License-Identifier: Apache-2.0
//! Short entity names.
//!
//! ISO 10303-21 lets an exchange structure name an entity by the short
//! name its schema assigns. `short-names.txt` is the ISO 10303 short-name
//! list as ISO publishes it at
//! <https://standards.iso.org/iso/10303/tech/short_names/short-names.txt>:
//! one entity name and its short name per line.

use std::sync::OnceLock;

const LIST: &str = include_str!("short_names/short-names.txt");

/// `(short name, entity name)` pairs sorted by short name. A short name
/// equal to its own entity name is left out.
fn table() -> &'static [(&'static str, &'static str)] {
    static TABLE: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut pairs: Vec<(&str, &str)> = LIST
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let entity = fields.next()?;
                let short = fields.next()?;
                (fields.next().is_none() && short != entity).then_some((short, entity))
            })
            .collect();
        pairs.sort_unstable();
        pairs
    })
}

/// The standard entity name of the short name `name`.
pub(super) fn entity_name(name: &str) -> Option<&'static str> {
    let table = table();
    table
        .binary_search_by(|(short, _)| (*short).cmp(name))
        .ok()
        .map(|index| table[index].1)
}

#[cfg(test)]
mod tests;
