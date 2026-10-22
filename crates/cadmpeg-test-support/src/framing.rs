// SPDX-License-Identifier: Apache-2.0
//! Appendix C metadata and source-addressed parts shared by codec framers.

mod table;
mod types;
mod validation;

pub mod freecad;
pub mod iges;
pub mod step;
pub mod zip;

pub use table::{PartParent, PartRule, PartTable};
pub use types::{FrameParent, FramedPart};

#[cfg(test)]
mod tests;
