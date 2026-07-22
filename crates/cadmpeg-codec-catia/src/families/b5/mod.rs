// SPDX-License-Identifier: Apache-2.0
//! CATIA `b5 03` short-frame object topology family.
//!
//! [`graph`] scans the object stream into a reference-closed [`graph::B5Graph`];
//! [`transfer`] lowers that graph into the neutral IR through staged emit
//! passes. [`vecmath`] holds the vector helpers common to both.

pub mod graph;
pub(crate) mod transfer;
mod vecmath;
