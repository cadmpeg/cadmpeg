// SPDX-License-Identifier: Apache-2.0
//! Packed plane charts in consolidated B27 records.

use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::checked::ExactHypotUnitVector3;
use crate::wire::records::{ConsolidatedFamily, ConsolidatedRecord};

pub(crate) fn packed_plane(data: View<'_>, record: &ConsolidatedRecord) -> Option<PlaneSurface> {
    if record.family() != ConsolidatedFamily::B || record.class() != 0x27 {
        return None;
    }
    let range = record.payload()?;
    let mut payload = data.child(range.start, range.end)?;
    if payload.u8()? != 0x08 {
        return None;
    }
    let control = payload.u16_le()?;
    let origin = Point3::new(payload.f64_le()?, payload.f64_le()?, payload.f64_le()?);
    let (u_axis, v_axis) = match control {
        0x1968 | 0x1960 => {
            let u = if control == 0x1968 {
                1.0
            } else {
                payload.f64_le()?
            };
            ([u, 0.0, 0.0], [0.0, payload.f64_le()?, payload.f64_le()?])
        }
        0x1c38 | 0x1c30 => {
            let u = if control == 0x1c38 {
                1.0
            } else {
                payload.f64_le()?
            };
            ([0.0, 0.0, u], [payload.f64_le()?, payload.f64_le()?, 0.0])
        }
        _ => return None,
    };
    if !payload.is_empty() {
        return None;
    }
    let u_axis = Vector3::from(ExactHypotUnitVector3::new(u_axis)?.get());
    let v_axis = Vector3::from(ExactHypotUnitVector3::new(v_axis)?.get());
    PlaneSurface::try_new(origin, u_axis.cross(v_axis), u_axis).ok()
}

#[cfg(test)]
mod tests;
