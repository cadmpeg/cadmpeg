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
mod display_jt;
mod features;
mod model;
mod om;
mod parasolid;
mod segments;

pub use display_jt::*;
pub use features::*;
pub(crate) use model::*;
pub use om::*;
pub use parasolid::*;
pub use segments::*;

/// Attach the native object model to `ir`: extract every record family from the
/// scanned container, then emit annotations, namespace arenas, and the semantic
/// islands. The single entry point the decode tier calls; extraction happens
/// inside so the decode tier never names the record families.
pub(crate) fn attach_annotations(
    ir: &mut CadIr,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let model = NativeModel::extract(&scan.container, &scan.streams);
    attach::attach(ir, &model, scan, annotations, unknowns)
}
