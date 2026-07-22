// SPDX-License-Identifier: Apache-2.0
//! Typed Siemens NX object-model records retained in the native namespace.

use std::collections::{BTreeMap, BTreeSet};

use flate2::read::ZlibDecoder;
use serde::{Deserialize, Serialize};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;

use crate::container::Container;
use crate::decode::Scan;
use crate::parasolid::{Stream, StreamKind};

mod attach;
pub(crate) mod catalogue;
mod display_jt;
mod features;
mod model;
mod om;
mod parasolid;
mod segments;
mod substrate;
pub(crate) mod vector;

#[cfg(test)]
pub(crate) use attach::*;
pub use display_jt::*;
pub use features::*;
pub(crate) use model::*;
pub use om::*;
pub use parasolid::*;
pub use segments::*;
pub(crate) use substrate::ParsedStreams;

/// Attach the pre-extracted native object model to `ir`: emit annotations, the
/// namespace arenas, and the semantic islands. The single entry point the
/// decode tier calls once it holds a [`NativeModel`]. The model is passed in
/// rather than extracted here so the geometry path can also feed it to body
/// selection without extracting twice; build it with
/// [`NativeModel::extract`](model::NativeModel::extract).
pub(crate) fn attach_annotations(
    ir: &mut CadIr,
    model: &NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    attach::attach(ir, model, scan, annotations, unknowns)
}
