// SPDX-License-Identifier: Apache-2.0
//! Plane-reference records that name the entity supplying a Mirror plane.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Vector3;

use crate::feature::{
    property, PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload, PmDcWorkPlane,
};
use crate::pmdc::{content_header, Cursor, PmDcReference};

pub(crate) const TYPE: [u8; 16] = [
    0x61, 0xb8, 0x48, 0x0c, 0xd2, 0x11, 0x05, 0x41, 0x60, 0x00, 0x93, 0xb7, 0xb0, 0x35, 0xc3, 0xb0,
];

/// Distance and normal-direction bound for two work-plane records that give
/// one plane, in internal centimetres.
const EPS_PLANE_COINCIDENT: f64 = 1.0e-9;

pub(crate) fn parse(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<Option<PmDcFeaturePropertyPayload>, CodecError> {
    use crate::layout::pm_dc_mirror_plane_reference_tail_v19 as layout;

    if version != 19 {
        return Ok(None);
    }
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    if cursor.remaining() != layout::LEN {
        return Ok(None);
    }
    let mut tail = cursor.into_view();
    if tail.req_i32_le()? != layout::STATE_VALUE {
        return Ok(None);
    }
    let owner = PmDcReference::from_packed(tail.req_u32_le()?);
    if tail.req_u8()? != layout::MARKER0_VALUE || tail.req_u8()? != layout::MARKER1_VALUE {
        return Ok(None);
    }
    let target = PmDcReference::from_packed(tail.req_u32_le()?);
    let role = tail.req_u32_le()?;
    if !reference_form(owner, target) {
        return Ok(None);
    }
    Ok(Some(property(
        version,
        header,
        PmDcFeaturePropertyKind::Mirror {
            definition: super::Property::PlaneReference {
                owner,
                target,
                role,
            },
        },
    )))
}

/// Both references are qualified, nonzero, and distinct.
pub(super) fn reference_form(owner: PmDcReference, target: PmDcReference) -> bool {
    [owner, target]
        .into_iter()
        .all(|reference| reference.qualified() && reference.index() != 0)
        && owner != target
}

/// Whether two work-plane records give the same plane: parallel normals and
/// a common point.
pub(crate) fn same_plane(first: &PmDcWorkPlane, second: &PmDcWorkPlane) -> bool {
    let normal = |plane: &PmDcWorkPlane| {
        let u = Vector3::from(plane.u_axis.map(cadmpeg_ir::scalar::FiniteReal::get));
        let v = Vector3::from(plane.v_axis.map(cadmpeg_ir::scalar::FiniteReal::get));
        u.cross(v).unit()
    };
    let point = |plane: &PmDcWorkPlane| {
        Vector3::from(plane.origin.map(cadmpeg_ir::scalar::FiniteReal::get))
    };
    let (Some(first_normal), Some(second_normal)) = (normal(first), normal(second)) else {
        return false;
    };
    first_normal.cross(second_normal).norm() <= EPS_PLANE_COINCIDENT
        && second_normal.dot(point(first) - point(second)).abs() <= EPS_PLANE_COINCIDENT
}

#[cfg(test)]
mod tests;
