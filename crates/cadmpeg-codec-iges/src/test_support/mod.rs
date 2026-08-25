// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic IGES byte-fixture builders for crate tests.
#![allow(clippy::unwrap_used)]

mod test_cards;
mod test_curves_and_surfaces;
mod test_drawing_and_trimming;
mod test_owned;
mod test_solids_and_structure;
mod test_tabulated_surfaces;

pub(crate) use test_cards::*;
pub(crate) use test_curves_and_surfaces::*;
pub(crate) use test_drawing_and_trimming::*;
pub(crate) use test_owned::*;
pub(crate) use test_solids_and_structure::*;
pub(crate) use test_tabulated_surfaces::*;
