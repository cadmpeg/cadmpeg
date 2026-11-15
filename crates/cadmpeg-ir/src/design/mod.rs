// SPDX-License-Identifier: Apache-2.0
//! CADIR v11 design foundation types.
//!
//! These are the types that design features, process records, and model-class
//! targets share: the v11 identities, references to entities and states,
//! structured names, selections, constructions, expressions, dimensions,
//! units, and the shared option types of the feature vocabulary.
//!
//! Each type has the wire form of §2.5 of the CADIR v11 specification. Read a
//! document that holds them through [`read::from_str`], which enforces the
//! nesting ceilings of [`cadmpeg_core::decode::ResourceLimits`].

pub mod construction;
pub mod dimension;
pub mod expr;
pub mod ids;
pub mod names;
pub mod options;
pub mod read;
pub mod refs;
pub mod selection;
pub mod topology;
pub mod units;

#[cfg(test)]
mod tests;
