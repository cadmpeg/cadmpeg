// SPDX-License-Identifier: Apache-2.0
//! Typed Siemens NX object-model records retained in the native namespace.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;

use crate::container::Container;
use crate::decode::Scan;
use crate::parasolid::Stream;

mod attach;
pub(crate) mod catalogue;
pub(crate) mod display_jt;
mod features;
pub(crate) mod history;
mod model;
mod om;
mod parasolid;
mod segments;
pub(crate) mod structure;
mod substrate;
mod toggle;
pub(crate) mod vector;

pub(crate) use model::{extract_segment_lineage, terminal_feature_body_ids, NativeModel};
pub(crate) use om::{
    canonical_expression_value, evaluate_parameterized_expression,
    expression_length_in_millimeters, expression_parameter_names,
};
pub(crate) use substrate::{topology_streams, ParsedStreams};
pub(crate) use toggle::has_complete_saved_toggle_stream;

/// Availability of typed native records during container retention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypedNative {
    /// Typed native extraction is available.
    Available,
    /// Only container records are retained.
    ContainerOnly,
}

/// Attach a pre-extracted [`NativeModel`] to `ir`: annotations, namespace arenas,
/// and semantic islands. Build the model with [`NativeModel::extract`].
pub(crate) fn attach_annotations(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    model: &NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
) -> Result<(), CodecError> {
    attach::attach(ctx, ir, model, scan, annotations, unknowns)
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
