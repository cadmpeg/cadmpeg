// SPDX-License-Identifier: Apache-2.0
//! Encode source-less F3D archives and apply supported edits to retained source
//! archives.

pub(crate) mod generate;
pub(crate) mod patch;
pub(crate) mod primitives;
pub(crate) mod saved;
pub(crate) mod target;

/// Every text field this writer fits to its width (cadir-v11.md §3.4).
#[cfg(test)]
pub(crate) const CAPACITY_FIELDS: &[cadmpeg_ir::codec::write::capacity::TextCapacity] = &[
    generate::configuration_root::TABLE_ID,
    generate::configuration_root::ROW_ID,
];

#[cfg(test)]
mod tests;
