// SPDX-License-Identifier: Apache-2.0
//! Typed Siemens NX object-model records retained in the native namespace.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;

use crate::decode::Scan;

mod attach;
pub(crate) mod catalogue;
pub(crate) mod display_jt;
mod features;
pub(crate) mod hex;
pub(crate) mod history;
pub(crate) mod model;
pub(crate) mod om;
mod parasolid;
mod segments;
pub(crate) mod structure;
pub(crate) mod substrate;
pub(crate) mod toggle;
pub(crate) mod vector;

/// Availability of typed native records during container retention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypedNative {
    /// Typed native extraction is available.
    Available,
    /// Only container records are retained.
    ContainerOnly,
}

/// Attach a pre-extracted [`crate::native::model::NativeModel`] to `ir`: annotations, namespace arenas,
/// and semantic islands. Build the model with [`crate::native::model::NativeModel::extract`].
pub(crate) fn attach_annotations(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    attach::attach(ctx, ir, model, scan, annotations, unknowns, losses)
}

/// Preserve container-layer records without extracting typed native entities.
pub(crate) fn attach_container_layer(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
    typed_native: TypedNative,
) -> Result<(), CodecError> {
    attach::attach_container_layer(ctx, ir, scan, annotations, unknowns, typed_native)
}
