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
    /// One-based global record ordinal in the zero-entity stream.
    pub record_ordinal: u32,
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
    /// One-based global record ordinal of the owning surface carrier.
    pub carrier_record_ordinal: u32,
    /// Positionally aligned face record when the complete rosters agree.
    pub face: Option<ZeroEntityFace>,
    /// Face-local support occurrences in storage order.
    pub supports: Vec<ZeroEntitySupportOccurrence>,
}

/// One counted zero-entity `5fxx` face record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityFace {
    /// Offset of the framed face record.
    pub pos: usize,
    /// One-based global record ordinal.
    pub record_ordinal: u32,
    /// Complete two-byte record tag.
    pub tag: [u8; 2],
    /// Counted allocation values in storage order.
    pub allocations: Vec<u32>,
    /// Ordered loop terminals `allocations[0] - allocations[1..]`.
    pub loop_terminals: Vec<u32>,
    /// Terminal control byte following the allocation lane.
    pub terminal_control: u8,
}

/// One `5e1a` edge-stride record in the global zero-entity reference namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityEdgeStride {
    /// Offset of the framed record.
    pub pos: usize,
    /// One-based global record ordinal in the zero-entity stream.
    pub record_ordinal: u32,
    /// Six stored global-record references.
    pub references: [u32; 6],
}

/// One positional `0638` oriented use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityOrientedUse {
    /// Offset of the framed record.
    pub pos: usize,
    /// One-based global record ordinal in the zero-entity stream.
    pub record_ordinal: u32,
    /// Positional side number, either one or two.
    pub side: u32,
    /// Two stored global-record references.
    pub references: [u32; 2],
    /// Side-specific slots derived from the owning header's base columns.
    pub side_slots: [u32; 2],
}

/// One `2569` header and its two immediately following positional uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityOrientedUsePair {
    /// Offset of the `2569` header.
    pub header_pos: usize,
    /// One-based global record ordinal of the `2569` header.
    pub header_record_ordinal: u32,
    /// Stored base columns.
    pub base_columns: [u32; 2],
    /// Side-one then side-two oriented uses.
    pub uses: [ZeroEntityOrientedUse; 2],
}

/// One counted zero-entity vertex-incidence record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityVertexIncidence {
    /// Offset of the framed `05xx` record.
    pub pos: usize,
    /// One-based global record ordinal in the zero-entity stream.
    pub record_ordinal: u32,
    /// Complete two-byte record tag.
    pub tag: [u8; 2],
    /// Stored global-record references.
    pub references: Vec<u32>,
}

/// One framed record in the zero-entity global identity namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityRecordIdentity {
    /// Offset of the framed record.
    pub pos: usize,
    /// Exclusive logical end, including any inline continuation.
    pub end: usize,
    /// Complete two-byte record tag.
    pub tag: [u8; 2],
    /// One-based global record ordinal.
    pub record_ordinal: u32,
}

#[derive(Debug, Clone, Copy)]
struct ZeroEntityRecord {
    pos: usize,
    end: usize,
    tag: [u8; 2],
    ordinal: u32,
}

struct ZeroEntityNurbsLayout {
    u_distinct: Vec<f64>,
    u_mults: Vec<u32>,
    u_degree: u32,
    u_count: u32,
    v_distinct: Vec<f64>,
    v_mults: Vec<u32>,
    v_degree: u32,
    v_count: u32,
    grid: usize,
    end: usize,
}

fn zero_entity_fixed_logical_length(tag: [u8; 2]) -> Option<usize> {
    match tag {
        [0x21, 0x45] => Some(337),
        [0x21, 0x72] => Some(382),
        [0x21, 0x9f] => Some(427),
        _ => None,
    }
}

fn zero_entity_records(data: &[u8]) -> Vec<ZeroEntityRecord> {
    let mut records = Vec::new();
    let mut position = 0usize;
    while position + 4 <= data.len() {
        if data[position..position + 2] != [0xa9, 0x03] {
            position += 1;
            continue;
        }
        let tag = [data[position + 2], data[position + 3]];
        let nominal_end = position.checked_add(usize::from(data[position + 3]) + 12);
        let Some(nominal_end) = nominal_end else {
            break;
        };
        let end = if matches!(tag, [0x34, 0xc8 | 0x5e]) {
            let Some(end) = zero_entity_nurbs_logical_end(data, position) else {
                break;
            };
            end
        } else if let Some(length) = zero_entity_fixed_logical_length(tag) {
            let Some(end) = position.checked_add(length) else {
                break;
            };
            end
        } else {
            nominal_end
        };
        if end > data.len() {
            break;
        }
        let Some(one_based_ordinal) = records.len().checked_add(1) else {
            break;
        };
        let Ok(ordinal) = u32::try_from(one_based_ordinal) else {
            break;
        };
        records.push(ZeroEntityRecord {
            pos: position,
            end,
            tag,
            ordinal,
        });
        position = end;
    }
    records
}

fn zero_entity_nurbs_logical_end(data: &[u8], record: usize) -> Option<usize> {
    Some(zero_entity_nurbs_layout(data, record)?.end)
}

fn zero_entity_nurbs_layout(data: &[u8], record: usize) -> Option<ZeroEntityNurbsLayout> {
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
    let pole_bytes = (u_count as usize)
        .checked_mul(v_count as usize)?
        .checked_mul(24)?;
    let grid = skip_u32_token_run(data, after_v_mults)?.checked_add(3)?;
    let end = grid.checked_add(pole_bytes)?;
    Some(ZeroEntityNurbsLayout {
        u_distinct,
        u_mults,
        u_degree,
        u_count,
        v_distinct,
        v_mults,
        v_degree,
        v_count,
        grid,
        end,
    })
}

/// Inventory every complete framed record in the one-based global namespace.
#[must_use]
pub fn zero_entity_record_inventory(data: &[u8]) -> Vec<ZeroEntityRecordIdentity> {
    zero_entity_records(data)
        .into_iter()
        .map(|record| ZeroEntityRecordIdentity {
            pos: record.pos,
            end: record.end,
            tag: record.tag,
            record_ordinal: record.ordinal,
        })
        .collect()
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
                carrier_record_ordinal: carrier_record.ordinal,
                face: None,
                supports,
            });
        }
        index = next.max(index + 1);
    }
    let faces = zero_entity_faces_from_records(data, &records);
    if faces.len() == runs.len() {
        for (run, face) in runs.iter_mut().zip(faces) {
            run.face = Some(face);
        }
    }
    runs
}

fn zero_entity_faces_from_records(
    data: &[u8],
    records: &[ZeroEntityRecord],
) -> Vec<ZeroEntityFace> {
    records
        .iter()
        .filter_map(|record| {
            if record.tag[0] != 0x5f || tagged_u32(data, record.pos + 7) != Some(1) {
                return None;
            }
            let count = usize::from(data.get(record.pos + 12)?.checked_sub(0x80)?);
            if count < 2 || record.pos.checked_add(14 + count.checked_mul(5)?)? != record.end {
                return None;
            }
            let allocations = (0..count)
                .map(|index| tagged_u32(data, record.pos + 13 + index * 5))
                .collect::<Option<Vec<_>>>()?;
            let first = *allocations.first()?;
            let loop_terminals = allocations[1..]
                .iter()
                .map(|allocation| first.checked_sub(*allocation))
                .collect::<Option<Vec<_>>>()?;
            Some(ZeroEntityFace {
                pos: record.pos,
                record_ordinal: record.ordinal,
                tag: record.tag,
                allocations,
                loop_terminals,
                terminal_control: *data.get(record.end - 1)?,
            })
        })
        .collect()
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
        record_ordinal: record.ordinal,
        tag: record.tag,
        face_local_slot,
        uv_endpoints,
    })
}

/// Decode complete `5e1a` edge-stride reference records.
#[must_use]
pub fn zero_entity_edge_strides(data: &[u8]) -> Vec<ZeroEntityEdgeStride> {
    let records = zero_entity_records(data);
    let Ok(record_count) = u32::try_from(records.len()) else {
        return Vec::new();
    };
    records
        .into_iter()
        .filter_map(|record| {
            if record.tag != [0x5e, 0x1a] || data.get(record.pos + 37) != Some(&0x21) {
                return None;
            }
            let mut references = [0; 6];
            for (index, reference) in references.iter_mut().enumerate() {
                *reference = tagged_u32(data, record.pos.checked_add(7 + index * 5)?)?;
            }
            if references
                .iter()
                .any(|reference| !(1..=record_count).contains(reference))
            {
                return None;
            }
            Some(ZeroEntityEdgeStride {
                pos: record.pos,
                record_ordinal: record.ordinal,
                references,
            })
        })
        .collect()
}

/// Decode complete `2569` headers with their adjacent `(1, 2)` oriented uses.
#[must_use]
pub fn zero_entity_oriented_use_pairs(data: &[u8]) -> Vec<ZeroEntityOrientedUsePair> {
    let records = zero_entity_records(data);
    let Ok(record_count) = u32::try_from(records.len()) else {
        return Vec::new();
    };
    records
        .windows(3)
        .filter_map(|records| {
            let [header, side_one, side_two] = records else {
                return None;
            };
            if header.tag != [0x25, 0x69]
                || side_one.tag != [0x06, 0x38]
                || side_two.tag != [0x06, 0x38]
                || tagged_u32(data, header.pos + 7) != Some(1)
                || data.get(header.pos + 12) != Some(&0x82)
            {
                return None;
            }
            let base_columns = [
                tagged_u32(data, header.pos + 13)?,
                tagged_u32(data, header.pos + 18)?,
            ];
            let parse_use = |record: ZeroEntityRecord, expected_side: u32| {
                if tagged_u32(data, record.pos + 7) != Some(1)
                    || data.get(record.pos + 12) != Some(&0x83)
                    || tagged_u32(data, record.pos + 13) != Some(expected_side)
                {
                    return None;
                }
                let references = [
                    tagged_u32(data, record.pos + 18)?,
                    tagged_u32(data, record.pos + 23)?,
                ];
                if references
                    .iter()
                    .any(|reference| !(1..=record_count).contains(reference))
                {
                    return None;
                }
                Some(ZeroEntityOrientedUse {
                    pos: record.pos,
                    record_ordinal: record.ordinal,
                    side: expected_side,
                    references,
                    side_slots: [
                        base_columns[0].checked_add(expected_side)?,
                        base_columns[1].checked_add(expected_side)?,
                    ],
                })
            };
            Some(ZeroEntityOrientedUsePair {
                header_pos: header.pos,
                header_record_ordinal: header.ordinal,
                base_columns,
                uses: [parse_use(*side_one, 1)?, parse_use(*side_two, 2)?],
            })
        })
        .collect()
}

/// Decode complete counted `050b`, `0510`, and `0515` incidence records.
#[must_use]
pub fn zero_entity_vertex_incidences(data: &[u8]) -> Vec<ZeroEntityVertexIncidence> {
    let records = zero_entity_records(data);
    let Ok(record_count) = u32::try_from(records.len()) else {
        return Vec::new();
    };
    records
        .into_iter()
        .filter_map(|record| {
            let count = match record.tag {
                [0x05, 0x0b] => 2,
                [0x05, 0x10] => 3,
                [0x05, 0x15] => 4,
                _ => return None,
            };
            if tagged_u32(data, record.pos + 7) != Some(1)
                || data.get(record.pos + 12) != Some(&(0x80 + count as u8))
            {
                return None;
            }
            let references = (0..count)
                .map(|index| tagged_u32(data, record.pos + 13 + index * 5))
                .collect::<Option<Vec<_>>>()?;
            if references
                .iter()
                .any(|reference| !(1..=record_count).contains(reference))
            {
                return None;
            }
            Some(ZeroEntityVertexIncidence {
                pos: record.pos,
                record_ordinal: record.ordinal,
                tag: record.tag,
                references,
            })
        })
        .collect()
}

fn tagged_u32(data: &[u8], at: usize) -> Option<u32> {
    (data.get(at) == Some(&0x10)).then(|| u32_le(data, at + 1))?
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
    let layout = zero_entity_nurbs_layout(data, record)?;
    let pole_count = (layout.u_count as usize).checked_mul(layout.v_count as usize)?;
    let mut control_points = Vec::with_capacity(pole_count);
    for pole in 0..pole_count {
        control_points.push(f64_point(
            data,
            layout.grid.checked_add(pole.checked_mul(24)?)?,
        )?);
    }
    Some(SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: layout.u_degree,
        v_degree: layout.v_degree,
        u_knots: expand_knots(&layout.u_distinct, &layout.u_mults)?,
        v_knots: expand_knots(&layout.v_distinct, &layout.v_mults)?,
        u_count: layout.u_count,
        v_count: layout.v_count,
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
    use crate::tests::{
        zero_entity_face_support_stream, zero_entity_support_stream, zero_entity_topology_stream,
    };

    fn write_tagged_u32(record: &mut [u8], at: usize, value: u32) {
        record[at] = 0x10;
        record[at + 1..at + 5].copy_from_slice(&value.to_le_bytes());
    }

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

    #[test]
    fn record_walk_skips_complete_logical_support_continuations() {
        let fixture = zero_entity_support_stream();
        let plane = &fixture[..0x6a + 12];
        let mut support = vec![0u8; zero_entity_fixed_logical_length([0x21, 0x45]).unwrap()];
        support[..4].copy_from_slice(&[0xa9, 0x03, 0x21, 0x45]);
        support[12] = 0x10;
        support[13..17].copy_from_slice(&42u32.to_le_bytes());
        support[100..100 + plane.len()].copy_from_slice(plane);

        let mut stream = plane.to_vec();
        stream.extend(support);
        stream.extend(plane);

        assert_eq!(zero_entity_surfaces(&stream).len(), 2);
        let runs = zero_entity_support_runs(&stream);
        let [run] = runs.as_slice() else {
            panic!("one support run")
        };
        assert_eq!(run.supports.len(), 1);
        assert_eq!(run.supports[0].tag, [0x21, 0x45]);
    }

    #[test]
    fn malformed_extended_carrier_stops_before_its_unbounded_continuation() {
        let fixture = zero_entity_support_stream();
        let plane = &fixture[..0x6a + 12];
        let mut stream = vec![0u8; 0xc8 + 12];
        stream[..4].copy_from_slice(&[0xa9, 0x03, 0x34, 0xc8]);
        stream[80..80 + plane.len()].copy_from_slice(plane);
        stream.extend(plane);

        assert!(zero_entity_record_inventory(&stream).is_empty());
        assert!(zero_entity_surfaces(&stream).is_empty());
    }

    #[test]
    fn topology_records_retain_global_ordinals_and_separate_namespaces() {
        let stream = zero_entity_topology_stream();
        let edge_strides = zero_entity_edge_strides(&stream);
        let [edge_stride] = edge_strides.as_slice() else {
            panic!("one edge stride")
        };
        assert_eq!(edge_stride.record_ordinal, 1);
        assert_eq!(edge_stride.references, [1, 2, 3, 4, 5, 1]);

        let pairs = zero_entity_oriented_use_pairs(&stream);
        let [pair] = pairs.as_slice() else {
            panic!("one oriented-use pair")
        };
        assert_eq!(pair.header_record_ordinal, 2);
        assert_eq!(pair.base_columns, [100, 200]);
        assert_eq!(pair.uses[0].record_ordinal, 3);
        assert_eq!(pair.uses[0].references, [1, 2]);
        assert_eq!(pair.uses[0].side_slots, [101, 201]);
        assert_eq!(pair.uses[1].record_ordinal, 4);
        assert_eq!(pair.uses[1].references, [3, 4]);
        assert_eq!(pair.uses[1].side_slots, [102, 202]);

        let incidences = zero_entity_vertex_incidences(&stream);
        let [incidence] = incidences.as_slice() else {
            panic!("one vertex incidence")
        };
        assert_eq!(incidence.record_ordinal, 5);
        assert_eq!(incidence.tag, [0x05, 0x10]);
        assert_eq!(incidence.references, [1, 2, 5]);
    }

    #[test]
    fn oriented_use_pair_requires_adjacent_ordered_sides() {
        let mut stream = zero_entity_topology_stream();
        let second_use = 38 + (0x69 + 12) + (0x38 + 12);
        write_tagged_u32(&mut stream, second_use + 13, 1);
        assert!(zero_entity_oriented_use_pairs(&stream).is_empty());
    }

    #[test]
    fn topology_global_references_are_one_based() {
        let mut stream = zero_entity_topology_stream();
        write_tagged_u32(&mut stream, 7, 0);
        assert!(zero_entity_edge_strides(&stream).is_empty());
    }

    #[test]
    fn complete_face_roster_aligns_to_surface_support_runs() {
        let stream = zero_entity_face_support_stream();
        let runs = zero_entity_support_runs(&stream);
        let [run] = runs.as_slice() else {
            panic!("one support run")
        };
        let face = run.face.as_ref().expect("positionally aligned face");
        assert_eq!(face.record_ordinal, 3);
        assert_eq!(face.tag, [0x5f, 0x0c]);
        assert_eq!(face.allocations, [10, 3]);
        assert_eq!(face.loop_terminals, [7]);
        assert_eq!(face.terminal_control, 0x05);
    }

    #[test]
    fn mismatched_face_roster_does_not_partially_bind_support_runs() {
        let mut stream = zero_entity_face_support_stream();
        let face = stream[stream.len() - (0x0c + 12)..].to_vec();
        stream.extend(face);
        let runs = zero_entity_support_runs(&stream);
        let [run] = runs.as_slice() else {
            panic!("one support run")
        };
        assert!(run.face.is_none());
    }
}
