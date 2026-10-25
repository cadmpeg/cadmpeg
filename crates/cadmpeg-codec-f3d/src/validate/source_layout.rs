// SPDX-License-Identifier: Apache-2.0
//! Checks on the source layout of the `f3d` records, over the decode sidecar.
//!
//! A record's source offsets and frame length are source facts: they are the
//! `record_layout` pointers and the extent of its capture (#214), not document
//! content. [`validate_source_layout`] reads them from the document's
//! [`SourceFidelity`] and checks the frame arithmetic that relates them. A
//! record whose required pointer is absent from its layout fails its check.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::source_fidelity::{RecordLayout, SourceFidelity};

use super::{design_stream, push_constant_finding, Ctx};
use crate::layout::design_parameter_owner_legacy_68 as legacy_owner_68;
use crate::layout::design_parameter_owner_legacy_88 as legacy_owner_88;
use crate::layout::indexed_companion_record_prefix as companion_prefix;
use crate::layout::typed_reference_companion_prefix;
use crate::{native, records};
use cadmpeg_core::decode::u64_from_index;

/// Validate the source layout of the `f3d` native records of `ir` in
/// `fidelity`.
///
/// A namespace that does not load has no records to check here;
/// [`super::validate_native`] reports its shape.
pub(crate) fn validate_source_layout(
    decode: &DecodeContext<'_>,
    ir: &CadIr,
    fidelity: &SourceFidelity,
) -> Result<Vec<Finding>, CodecError> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(Vec::new());
    };
    let native = match native::F3dNative::load_inner(decode, namespace).map_err(CodecError::from) {
        Ok(native) => native,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(Vec::new()),
    };
    let ctx = Ctx::new(ir, &native, decode);
    Ok(validate_loaded(&ctx, &Layouts { fidelity }))
}

mod assembly;
mod extrude;
pub(super) mod scopes;

/// Validate the source layout of an already loaded native namespace.
fn validate_loaded(ctx: &Ctx<'_, '_>, layouts: &Layouts<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let located = scopes::LocatedScopes::new(ctx.native, layouts);
    scopes::validate_scope_layouts(ctx, &located, &mut findings);
    scopes::validate_mesh_feature_scope_layouts(ctx, &located, &mut findings);
    scopes::validate_image_scope_layouts(ctx, &located, layouts, &mut findings);
    scopes::validate_edge_treatment_vertex_layouts(ctx, layouts, &mut findings);
    validate_parameter_scope_owner_lanes(ctx, layouts, &located, &mut findings);
    validate_parameter_owner_layouts(ctx, layouts, &mut findings);
    validate_parameter_companion_layouts(ctx, layouts, &mut findings);
    findings
}

/// The record layouts of one decode sidecar, by native record id.
pub(super) struct Layouts<'f> {
    pub(super) fidelity: &'f SourceFidelity,
}

impl Layouts<'_> {
    /// The capture layout of the record `id`.
    fn layout(&self, id: &str) -> Option<&RecordLayout> {
        self.fidelity.record_layout().get(id)
    }

    /// The `record_layout` pointer `name` of the record `id`.
    fn pointer(&self, id: &str, name: &str) -> Option<u64> {
        self.layout(id)?.pointers().get(name).copied()
    }
}

/// Whether an owner frame of this length is a legacy owner, which stores no
/// scalar: its evaluated value is a field of the parameter record.
fn is_legacy_owner_frame(frame_length: u64) -> bool {
    frame_length == u64_from_index(legacy_owner_68::LEN)
        || frame_length == u64_from_index(legacy_owner_88::LEN)
}

/// Source position of an owner's evaluated value: the owner's own pointer,
/// or, for a legacy owner frame, the pointer of its parameter record.
fn owner_evaluated_value_offset(
    ctx: &Ctx<'_, '_>,
    layouts: &Layouts<'_>,
    owner: &records::parameters::DesignParameterOwner,
) -> Option<u64> {
    let layout = layouts.layout(owner.id())?;
    if let Some(offset) = layout.pointers().get("evaluated_value_offset") {
        return Some(*offset);
    }
    if !is_legacy_owner_frame(layout.byte_len()) {
        return None;
    }
    let parameter = ctx
        .parameters_by_index
        .get(&(design_stream(owner.id()), owner.parameter_record_index()))?;
    layouts.pointer(&parameter.id, "evaluated_value_offset")
}

/// A legacy owner frame (68 or 88 bytes) stores no scalar. Its evaluated
/// value is the parameter record's, so the parameter layout holds that
/// pointer, and an owner pointer, when present, names the same byte.
fn validate_parameter_owner_layouts(
    ctx: &Ctx<'_, '_>,
    layouts: &Layouts<'_>,
    findings: &mut Vec<Finding>,
) {
    if !ctx.holds_design_intent() {
        return;
    }
    for owner in &ctx.native.design_parameter_owners {
        let valid = layouts.layout(owner.id()).is_some_and(|layout| {
            if !is_legacy_owner_frame(layout.byte_len()) {
                return true;
            }
            let parameter = ctx
                .parameters_by_index
                .get(&(design_stream(owner.id()), owner.parameter_record_index()));
            let parameter_offset = parameter
                .and_then(|parameter| layouts.pointer(&parameter.id, "evaluated_value_offset"));
            parameter_offset.is_some_and(|parameter_offset| {
                layout
                    .pointers()
                    .get("evaluated_value_offset")
                    .is_none_or(|owner_offset| *owner_offset == parameter_offset)
            })
        });
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter owner has an invalid frame or indexed link",
                Some(owner.id().to_owned()),
            );
        }
    }
}

/// A plain companion prefix stores its timestamp at offset 42 and is 58 bytes
/// long; a typed-reference prefix stores it at offset 82 and is 98 bytes long.
/// A bound payload starts after the prefix.
pub(super) fn validate_parameter_companion_layouts(
    ctx: &Ctx<'_, '_>,
    layouts: &Layouts<'_>,
    findings: &mut Vec<Finding>,
) {
    for companion in &ctx.native.design_parameter_companions {
        let id = companion.id();
        let start = layouts.pointer(id, "byte_offset");
        let timestamp = layouts.pointer(id, "timestamp_micros_offset");
        let prefix_len = [
            (companion_prefix::TIMESTAMP_MICROS, companion_prefix::LEN),
            (
                typed_reference_companion_prefix::TIMESTAMP_MICROS,
                typed_reference_companion_prefix::LEN,
            ),
        ]
        .into_iter()
        .find_map(|(timestamp_at, len)| {
            let expected = start?.checked_add(u64_from_index(timestamp_at))?;
            (timestamp == Some(expected)).then_some(len)
        });
        let timestamp_valid = prefix_len.is_some();
        let payload_valid = companion.payload().is_none_or(|_| {
            start
                .zip(prefix_len)
                .and_then(|(start, len)| start.checked_add(u64_from_index(len)))
                .is_some_and(|expected| {
                    layouts.pointer(id, "payload_byte_offset") == Some(expected)
                })
        });
        if !(timestamp_valid && payload_valid) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter companion has an invalid prefix or owner link",
                Some(id.to_owned()),
            );
        }
    }
}

/// A scope lane that names a parameter owner states where that owner's
/// evaluated value is. The owner's capture holds the same position.
fn validate_parameter_scope_owner_lanes(
    ctx: &Ctx<'_, '_>,
    layouts: &Layouts<'_>,
    located: &scopes::LocatedScopes<'_>,
    findings: &mut Vec<Finding>,
) {
    if !ctx.holds_design_intent() {
        return;
    }
    for (position, scope) in ctx.native.design_parameter_scopes.iter().enumerate() {
        let native_stream = design_stream(&scope.id);
        let valid = located.get(position).is_some_and(|located| {
            scopes::scope_owner_lanes(located)
                .into_iter()
                .all(|(record_index, value_offset)| {
                    ctx.owners_by_index
                        .get(&(native_stream, record_index))
                        .filter(|owner| owner.scope_record_index() == Some(scope.record_index))
                        .is_none_or(|owner| {
                            value_offset.is_some()
                                && owner_evaluated_value_offset(ctx, layouts, owner) == value_offset
                        })
                })
        });
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter scope has an invalid paired frame",
                Some(scope.id.clone()),
            );
        }
    }
}

#[cfg(test)]
mod tests;
