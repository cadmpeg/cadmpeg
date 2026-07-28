//! Zero-entity `a9 03` stream surface decoders.
//!
//! Decodes analytic (plane, cylinder, cone, torus) and inline non-rational
//! NURBS surface carriers from a zero-entity record stream.

use cadmpeg_ir::geometry::{NurbsSurface, SurfaceGeometry};
use cadmpeg_ir::le::u32_at as u32_le;

use crate::nurbs::expand_knots;
use crate::wire::bytes::{f64_le, f64_point, f64_vector};

/// A directly decoded analytic carrier in the zero-entity `a9 03` stream.
#[derive(Debug, Clone)]
pub struct ZeroEntitySurface {
    /// Offset of the framed record in the file.
    pub pos: usize,
    /// The decoded surface carrier.
    pub geometry: SurfaceGeometry,
}

/// One face-local support occurrence owned by a zero-entity surface carrier.
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroEntitySupportOccurrence {
    /// Offset of the framed `21xx` record.
    pub pos: usize,
    /// Complete two-byte record tag.
    pub tag: [u8; 2],
    /// Face-local support slot stored by the framed token at record offset 12.
    pub face_local_slot: u32,
    /// Stored UV endpoints when this support family carries them inline.
    pub uv_endpoints: Option<[[f64; 2]; 2]>,
}

/// One surface carrier and its maximal following `21xx` support run.
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroEntitySupportRun {
    /// Offset of the owning surface-carrier record.
    pub carrier_pos: usize,
    /// Face-local support occurrences in storage order.
    pub supports: Vec<ZeroEntitySupportOccurrence>,
}

#[derive(Debug, Clone, Copy)]
struct ZeroEntityRecord {
    pos: usize,
    end: usize,
    tag: [u8; 2],
}

fn zero_entity_records(data: &[u8]) -> Vec<ZeroEntityRecord> {
    let mut records = Vec::new();
    let mut position = 0usize;
    while position + 4 <= data.len() {
        if data[position..position + 2] != [0xa9, 0x03] {
            position += 1;
            continue;
        }
        let Some(end) = position.checked_add(usize::from(data[position + 3]) + 12) else {
            break;
        };
        if end > data.len() {
            break;
        }
        records.push(ZeroEntityRecord {
            pos: position,
            end,
            tag: [data[position + 2], data[position + 3]],
        });
        position = end;
    }
    records
}

/// Decode analytic surface carriers in a zero-entity `a9 03` stream.  The
/// record's second tag byte is also its length code (`length = tag + 12`), so
/// the decoder walks framed records.
pub fn zero_entity_surfaces(data: &[u8]) -> Vec<ZeroEntitySurface> {
    zero_entity_records(data)
        .into_iter()
        .filter_map(|record| {
            zero_entity_surface_at(data, record.pos).map(|geometry| ZeroEntitySurface {
                pos: record.pos,
                geometry,
            })
        })
        .collect()
}

/// Decode surface-carrier ownership and exact face-local support occurrences.
#[must_use]
pub fn zero_entity_support_runs(data: &[u8]) -> Vec<ZeroEntitySupportRun> {
    let records = zero_entity_records(data);
    let mut runs = Vec::new();
    let mut index = 0usize;
    while index + 1 < records.len() {
        let carrier_record = records[index];
        let Some(_) = zero_entity_surface_at(data, carrier_record.pos) else {
            index += 1;
            continue;
        };
        if records[index + 1].tag[0] != 0x21 {
            index += 1;
            continue;
        }
        let mut supports = Vec::new();
        let mut next = index + 1;
        while records
            .get(next)
            .is_some_and(|record| record.tag[0] == 0x21)
        {
            let record = records[next];
            if let Some(support) = zero_entity_support_occurrence(data, record) {
                supports.push(support);
            } else {
                supports.clear();
                break;
            }
            next += 1;
        }
        if !supports.is_empty() {
            runs.push(ZeroEntitySupportRun {
                carrier_pos: carrier_record.pos,
                supports,
            });
        }
        index = next.max(index + 1);
    }
    runs
}

fn zero_entity_support_occurrence(
    data: &[u8],
    record: ZeroEntityRecord,
) -> Option<ZeroEntitySupportOccurrence> {
    if data.get(record.pos + 12) != Some(&0x10) {
        return None;
    }
    let face_local_slot = u32_le(data, record.pos + 13)?;
    let uv_offsets = match record.tag {
        [0x21, 0x71] => Some([93, 109]),
        [0x21, 0x91] => Some([93, 141]),
        [0x21, 0x99] => Some([93, 125]),
        [0x21, 0xd6] => Some([106, 170]),
        [0x21, 0xe8] => Some([132, 228]),
        _ => None,
    };
    let uv_endpoints = if let Some(offsets) = uv_offsets {
        let mut values = [[0.0; 2]; 2];
        for (index, offset) in offsets.into_iter().enumerate() {
            let absolute = record.pos.checked_add(offset)?;
            if absolute.checked_add(16)? > record.end {
                return None;
            }
            values[index] = [
                f64_le(data, absolute)?,
                f64_le(data, absolute.checked_add(8)?)?,
            ];
        }
        Some(values)
    } else {
        None
    };
    Some(ZeroEntitySupportOccurrence {
        pos: record.pos,
        tag: record.tag,
        face_local_slot,
        uv_endpoints,
    })
}

pub(crate) fn zero_entity_surface_at(data: &[u8], record: usize) -> Option<SurfaceGeometry> {
    let payload_end = record.checked_add(*data.get(record + 3)? as usize + 12)?;
    let payload = data.get(record + 4..payload_end)?;
    match (*data.get(record + 2)?, *data.get(record + 3)?) {
        (0x27, 0x6a) => zero_entity_plane(payload),
        (0x28, 0x8a) => zero_entity_cylinder(payload),
        (0x29, 0xb8) => zero_entity_cone(payload),
        (0x2b, 0xc8) => zero_entity_torus(payload),
        (0x34, 0xc8 | 0x5e) => zero_entity_nurbs_surface(data, record),
        _ => None,
    }
}

/// Decode the inline zero-entity non-rational NURBS carrier.  Its pole grid
/// follows the nominal framed record length, so this function receives the full
/// preamble.
fn zero_entity_nurbs_surface(data: &[u8], record: usize) -> Option<SurfaceGeometry> {
    let (u_distinct, after_u) = f64_run_to_one(data, record.checked_add(23)?)?;
    let (u_mults, after_u_mults) = u32_tokens(data, after_u, u_distinct.len())?;
    let u_degree = u_mults.first().copied()?.checked_sub(1)?;
    let u_count = u_mults
        .iter()
        .try_fold(0u32, |sum, value| sum.checked_add(*value))?
        .checked_sub(u_degree + 1)?;
    let after_u_tokens = skip_u32_token_run(data, after_u_mults)?;
    let extra_u_bytes = after_u_tokens.checked_sub(after_u_mults)?;
    if extra_u_bytes != 0 && extra_u_bytes < 10 {
        return None;
    }
    let (v_distinct, after_v) = f64_monotonic_run(data, after_u_tokens.checked_add(1)?)?;
    let (v_mults, after_v_mults) = u32_tokens(data, after_v, v_distinct.len())?;
    let v_degree = v_mults.first().copied()?.checked_sub(1)?;
    let v_count = v_mults
        .iter()
        .try_fold(0u32, |sum, value| sum.checked_add(*value))?
        .checked_sub(v_degree + 1)?;
    if !(1..=9).contains(&u_degree)
        || !(1..=9).contains(&v_degree)
        || !(2..=4096).contains(&u_count)
        || !(2..=4096).contains(&v_count)
    {
        return None;
    }
    let pole_count = (u_count as usize).checked_mul(v_count as usize)?;
    let grid = skip_u32_token_run(data, after_v_mults)?.checked_add(3)?;
    let mut control_points = Vec::with_capacity(pole_count);
    for pole in 0..pole_count {
        control_points.push(f64_point(data, grid.checked_add(pole.checked_mul(24)?)?)?);
    }
    Some(SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree,
        v_degree,
        u_knots: expand_knots(&u_distinct, &u_mults)?,
        v_knots: expand_knots(&v_distinct, &v_mults)?,
        u_count,
        v_count,
        control_points,
        weights: None,
        u_periodic: false,
        v_periodic: false,
    }))
}

fn skip_u32_token_run(data: &[u8], mut at: usize) -> Option<usize> {
    while data.get(at) == Some(&0x10) {
        u32_le(data, at + 1)?;
        at = at.checked_add(5)?;
    }
    Some(at)
}

fn zero_entity_plane(payload: &[u8]) -> Option<SurfaceGeometry> {
    let origin = f64_point(payload, 10)?;
    let row0 = f64_vector(payload, 34)?;
    let row1 = f64_vector(payload, 58)?;
    Some(SurfaceGeometry::Plane {
        origin,
        normal: row0.cross(row1).unit()?,
        u_axis: row0.unit()?,
    })
}

fn zero_entity_cylinder(payload: &[u8]) -> Option<SurfaceGeometry> {
    // The origin sits at offset 8; a one-byte gap separates it from the
    // contiguous frame-row block at offset 33.
    let mut c = crate::wire::cursor::Cursor::new_at(payload, 8);
    let origin = c.point3()?;
    c.skip(1)?;
    let (geometry, radius) = crate::analytic::cylinder_uvr(&mut c, origin)?;
    if !(radius.is_finite() && radius > 0.0 && radius < 1e6) {
        return None;
    }
    Some(geometry)
}

fn zero_entity_cone(payload: &[u8]) -> Option<SurfaceGeometry> {
    let mut c = crate::wire::cursor::Cursor::new_at(payload, 8);
    let (geometry, radius, half_angle) = crate::analytic::cone_ozra(&mut c)?;
    if !(radius.is_finite()
        && radius > 0.0
        && radius < 1e6
        && half_angle.is_finite()
        && half_angle > 0.0
        && half_angle < std::f64::consts::FRAC_PI_2)
    {
        return None;
    }
    Some(geometry)
}

fn zero_entity_torus(payload: &[u8]) -> Option<SurfaceGeometry> {
    let mut c = crate::wire::cursor::Cursor::new_at(payload, 8);
    let (geometry, major_radius, minor_radius) = crate::analytic::torus_ozrr(&mut c)?;
    if !(major_radius.is_finite()
        && major_radius > 0.0
        && major_radius < 1e6
        && minor_radius.is_finite()
        && minor_radius > 0.0
        && minor_radius < 1e6)
    {
        return None;
    }
    Some(geometry)
}

fn f64_run_to_one(bytes: &[u8], mut at: usize) -> Option<(Vec<f64>, usize)> {
    let mut values = Vec::new();
    loop {
        let value = f64_le(bytes, at)?;
        if !(0.0..=1.0).contains(&value) || values.last().is_some_and(|last| value < *last) {
            return None;
        }
        values.push(value);
        at = at.checked_add(8)?;
        if value == 1.0 {
            return (values.len() >= 2).then_some((values, at));
        }
        if values.len() > 4096 {
            return None;
        }
    }
}

fn f64_monotonic_run(bytes: &[u8], mut at: usize) -> Option<(Vec<f64>, usize)> {
    let mut values = Vec::new();
    while let Some(value) = f64_le(bytes, at) {
        if !(0.0..=50.0).contains(&value) || values.last().is_some_and(|last| value < *last) {
            break;
        }
        values.push(value);
        at = at.checked_add(8)?;
        if values.len() > 4096 {
            return None;
        }
    }
    (values.len() >= 2).then_some((values, at))
}

fn u32_tokens(bytes: &[u8], mut at: usize, count: usize) -> Option<(Vec<u32>, usize)> {
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        if *bytes.get(at)? != 0x10 {
            return None;
        }
        let raw: [u8; 4] = bytes.get(at + 1..at + 5)?.try_into().ok()?;
        let value = u32::from_le_bytes(raw);
        if value == 0 {
            return None;
        }
        values.push(value);
        at = at.checked_add(5)?;
    }
    Some((values, at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::zero_entity_support_stream;

    #[test]
    fn support_run_binds_maximal_21xx_lane_to_preceding_surface() {
        let runs = zero_entity_support_runs(&zero_entity_support_stream());
        let [run] = runs.as_slice() else {
            panic!("one support run")
        };
        assert_eq!(run.carrier_pos, 0);
        let [support] = run.supports.as_slice() else {
            panic!("one support occurrence")
        };
        assert_eq!(support.tag, [0x21, 0x71]);
        assert_eq!(support.face_local_slot, 42);
        assert_eq!(support.uv_endpoints, Some([[-2.0, 4.0], [6.0, 8.0]]));
    }

    #[test]
    fn malformed_support_invalidates_its_structural_run() {
        let mut stream = zero_entity_support_stream();
        stream[0x6a + 12 + 12] = 0;
        assert!(zero_entity_support_runs(&stream).is_empty());
    }
}
