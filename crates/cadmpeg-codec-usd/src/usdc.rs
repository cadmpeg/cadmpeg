// SPDX-License-Identifier: Apache-2.0
//! The USDC binary layer reader (spec §9).
//!
//! The reader decodes the structural sections and the field values of a
//! layer into the parsed form of a text layer.

mod compression;
mod integers;
mod layer;
mod sections;
mod value;
pub(crate) mod write;

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::text::syntax::Layer;

/// Reads the USDC layer `file`, and names the kinds of value it did not
/// read.
pub(crate) fn read<'a>(
    ctx: &DecodeContext<'a>,
    file: View<'a>,
) -> Result<(Layer, BTreeSet<&'static str>), CodecError> {
    let structure = sections::read(ctx, file)?;
    layer::build(ctx, &structure)
}
