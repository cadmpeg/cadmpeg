// SPDX-License-Identifier: Apache-2.0
//! The operations of the feature vocabulary (§6), by family.

pub mod assembly;
pub mod body;
pub mod datum;
pub mod dressup;
pub mod shared;
pub mod sheet_metal;
pub mod sketch;
pub mod surface;
pub mod swept;

#[cfg(test)]
pub(in crate::design) mod tests;
