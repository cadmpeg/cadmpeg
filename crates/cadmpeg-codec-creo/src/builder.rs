// SPDX-License-Identifier: Apache-2.0
//! Loss notes for Creo geometry substitutions and omissions.

use cadmpeg_ir::geometry::derive_reference_direction;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::report::LossNote;

use crate::loss::CreoLossCode;

/// Derive the missing in-plane axis of an `ActDatums` plane and record the
/// substitution.
pub(crate) fn datum_u_axis(
    sink: &mut Vec<LossNote>,
    normal: Vector3,
    plane_id: u32,
    offset: u64,
) -> Vector3 {
    sink.push(inferred_u_axis_note(plane_id, offset));
    derive_reference_direction(normal)
}

/// The note for a datum plane's conventionally derived in-plane u-axis.
fn inferred_u_axis_note(plane_id: u32, offset: u64) -> LossNote {
    CreoLossCode::DatumUAxisInferred.note(format!(
        "ActDatums datum plane surface#{plane_id} at offset {offset} carries no in-plane \
         reference direction; the u-axis was derived from the normal by convention."
    ))
}

/// The note for an incomplete `VisibGeom` plane local system that recovers no
/// origin, normal, or u-axis and so is not transferred as a placed carrier.
pub(crate) fn incomplete_frame_note(surface_id: u32, offset: u64) -> LossNote {
    CreoLossCode::IncompletePlaneFrame.note(format!(
        "VisibGeom plane local system surface#{surface_id} at offset {offset} is an \
         incomplete support frame (missing origin, normal, or u-axis); not transferred as a \
         model-space plane carrier."
    ))
}

/// The note for a `FeatDefs` sketch record preserved as native design data but
/// having no placed feature operation to carry it into the typed model.
pub(crate) fn unplaced_sketch_note(sketch_id: &str, feature_id: u32, offset: u64) -> LossNote {
    CreoLossCode::UnplacedSketchRecord.note(format!(
        "FeatDefs sketch record {sketch_id} (definition #{feature_id}) at offset {offset} was \
         preserved as a native design record but has no placed feature operation to carry it \
         into the typed model."
    ))
}
