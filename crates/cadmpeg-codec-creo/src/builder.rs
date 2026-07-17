// SPDX-License-Identifier: Apache-2.0
//! Typed lossy construction at creo's §10 Phase 4B boundaries.
//!
//! The decoder reaches every value substitution and omission through
//! [`LossBuilder`], the codec's single lossy-construction path. It wraps the
//! platform [`Builder`](cadmpeg_ir::transfer::Builder) over the decode's loss
//! channel so a defaulted axis or a dropped record cannot enter the model
//! without surrendering its [`LossNote`]: [`LossBuilder::datum_u_axis`] resolves
//! a [`Transfer::fallback`] and [`LossBuilder::omit`] resolves a
//! [`Transfer::omitted`], and neither yields its value without pushing the note.
//!
//! Creo's three named boundaries are the datum-plane u-axis (resolver to
//! fallback axis), the incomplete `VisibGeom` support frame (unsupported concept
//! to omission), and the unplaced `FeatDefs` sketch (decoder record to
//! omission). Every typed record the codec emits is otherwise representable by
//! construction — datum normals are basis vectors, frame bases are normalized,
//! and the scalar decoder masks its leading byte so no value is ever non-finite
//! — so no value-level mandatory semantic can be unrepresentable, and strict
//! mode has nothing to reject at this layer beyond the platform's
//! unresolved-ticket and transfer-accounting path.

use cadmpeg_ir::geometry::derive_reference_direction;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::report::{LossCategory, LossCode, LossNote, Severity};
use cadmpeg_ir::transfer::{Builder, Transfer};

/// The codec's single typed-lossy construction path: one platform
/// [`Builder`](cadmpeg_ir::transfer::Builder) threaded over the decode's loss
/// channel for the duration of one document's construction.
pub(crate) struct LossBuilder<'a> {
    inner: Builder<'a, Vec<LossNote>>,
}

impl<'a> LossBuilder<'a> {
    /// Wrap the decode's dropped-loss channel for typed construction.
    pub(crate) fn new(sink: &'a mut Vec<LossNote>) -> Self {
        LossBuilder {
            inner: Builder::new(sink),
        }
    }

    /// Resolver-to-fallback-axis boundary: an `ActDatums` datum plane carries no
    /// in-plane reference direction, so the u-axis is synthesized from the normal
    /// by convention. Resolving the [`Transfer::fallback`] records the
    /// substitution note before returning the derived axis.
    pub(crate) fn datum_u_axis(&mut self, normal: Vector3, plane_id: u32, offset: u64) -> Vector3 {
        self.inner
            .take(Transfer::fallback(
                derive_reference_direction(normal),
                inferred_u_axis_note(plane_id, offset),
            ))
            .expect("Transfer::fallback always yields its value")
    }

    /// Unsupported-concept / record-to-omission boundary: drain an omission's
    /// note through the builder so the record cannot be skipped silently. The
    /// note is also recorded against the record's `Dropped` disposition by the
    /// caller, so pass a clone here.
    pub(crate) fn omit(&mut self, note: LossNote) {
        let dropped: Option<()> = self.inner.take(Transfer::omitted(note));
        debug_assert!(dropped.is_none());
    }
}

/// The note for a datum plane's conventionally derived in-plane u-axis.
fn inferred_u_axis_note(plane_id: u32, offset: u64) -> LossNote {
    LossNote {
        code: LossCode::CarrierAxisInferred,
        category: LossCategory::Geometry,
        severity: Severity::Info,
        message: format!(
            "ActDatums datum plane surface#{plane_id} at offset {offset} carries no in-plane \
             reference direction; the u-axis was derived from the normal by convention."
        ),
        provenance: None,
    }
}

/// The note for an incomplete `VisibGeom` plane local system that recovers no
/// origin, normal, or u-axis and so is not transferred as a placed carrier.
pub(crate) fn incomplete_frame_note(surface_id: u32, offset: u64) -> LossNote {
    LossNote {
        code: LossCode::GeometryNotTransferred,
        category: LossCategory::Geometry,
        severity: Severity::Info,
        message: format!(
            "VisibGeom plane local system surface#{surface_id} at offset {offset} is an \
             incomplete support frame (missing origin, normal, or u-axis); not transferred as a \
             model-space plane carrier."
        ),
        provenance: None,
    }
}

/// The note for a `FeatDefs` sketch record preserved as native design data but
/// having no placed feature operation to carry it into the typed model.
pub(crate) fn unplaced_sketch_note(sketch_id: &str, feature_id: u32, offset: u64) -> LossNote {
    LossNote {
        code: LossCode::PassthroughRecordOmitted,
        category: LossCategory::Attribute,
        severity: Severity::Info,
        message: format!(
            "FeatDefs sketch record {sketch_id} (definition #{feature_id}) at offset {offset} was \
             preserved as a native design record but has no placed feature operation to carry it \
             into the typed model."
        ),
        provenance: None,
    }
}
