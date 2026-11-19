// SPDX-License-Identifier: Apache-2.0
//! CADIR v11 design layer.
//!
//! The design root's records: scopes, containers, features, parameters,
//! spreadsheets, programs, configurations, arrangements, publications, and
//! evaluations, and the foundation types they share with process records and
//! model-class targets: the v11 identities, references to entities and
//! states, structured names, selections, constructions, expressions,
//! dimensions, units, and the shared option types of the feature vocabulary.
//!
//! Each type has the wire form of §2.5 of the CADIR v11 specification. Read a
//! document that holds them through [`read::from_str`], which enforces the
//! nesting ceilings of [`cadmpeg_core::decode::ResourceLimits`].

pub mod configuration;
pub mod construction;
pub mod container;
pub mod dimension;
pub mod edit;
pub mod evaluation;
pub mod expr;
pub mod feature;
mod identity_rewrite;
pub mod ids;
pub mod names;
pub(crate) mod normalize;
pub mod ops;
pub mod options;
pub mod parameter;
pub mod program;
pub mod read;
pub mod refs;
pub mod scope;
pub mod selection;
pub mod spreadsheet;
pub mod units;

#[cfg(test)]
mod tests;
