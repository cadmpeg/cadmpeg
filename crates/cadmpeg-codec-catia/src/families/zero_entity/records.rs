//! Zero-entity `a9 03` stream surface decoders.
//!
//! Decodes analytic (plane, cylinder, cone, torus) and inline non-rational
//! NURBS surface carriers from a zero-entity record stream.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use cadmpeg_core::decode::View;
use cadmpeg_ir::eval::{nurbs_surface_point, pcurve_uv};
use cadmpeg_ir::geometry::{
    CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, PcurveNurbs,
    ProceduralCurveDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3};

use crate::layout::a9_03_frame as a9_03;
use crate::layout::zero_entity_edge_stride_5e1a as edge_5e1a;
use crate::layout::zero_entity_pcurve_2171 as pcurve_2171;
use crate::layout::zero_entity_vertex_owner_5d06 as vertex_5d06;
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
    /// Complete parameter-space curve carried by the support record.
    pub pcurve: Option<PcurveGeometry>,
    /// Exact model-space carrier derived from the pcurve and owning surface.
    pub model_curve: Option<CurveGeometry>,
    /// Exact procedural model-space carrier derived from the pcurve and owning surface.
    pub model_curve_construction: Option<ProceduralCurveDefinition>,
    /// Model-carrier parameters at the two stored UV endpoints.
    pub model_parameters: Option<[f64; 2]>,
    /// Surface point at the midpoint of the bounded pcurve parameter interval.
    pub model_midpoint: Option<Point3>,
    /// UV endpoints lifted through the owning surface carrier.
    pub model_endpoints: Option<[Point3; 2]>,
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

/// Terminal control byte following a zero-entity face allocation lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZeroEntityFaceControl {
    /// Control byte `0x03`.
    Control03,
    /// Control byte `0x05`.
    Control05,
}

impl ZeroEntityFaceControl {
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x03 => Some(Self::Control03),
            0x05 => Some(Self::Control05),
            _ => None,
        }
    }

    pub fn as_byte(self) -> u8 {
        match self {
            Self::Control03 => 0x03,
            Self::Control05 => 0x05,
        }
    }
}

/// One counted zero-entity `5fxx` face record.
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroEntityFace {
    /// Offset of the framed face record.
    pub pos: usize,
    /// One-based global record ordinal.
    pub record_ordinal: u32,
    /// Complete two-byte record tag.
    pub tag: [u8; 2],
    /// Counted allocation values in storage order.
    pub allocations: Vec<u32>,
    /// Positionally aligned loop records when the complete flattened roster agrees.
    pub loops: Option<Vec<ZeroEntityLoop>>,
    /// Terminal control byte following the allocation lane.
    pub terminal_control: ZeroEntityFaceControl,
}

impl ZeroEntityFace {
    pub fn loop_terminals(&self) -> Vec<u32> {
        let Some(first) = self.allocations.first().copied() else {
            return Vec::new();
        };
        self.allocations[1..]
            .iter()
            .filter_map(|allocation| first.checked_sub(*allocation))
            .collect()
    }
}

/// One counted zero-entity `62xx` loop record.
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroEntityLoop {
    /// Offset of the framed loop record.
    pub pos: usize,
    /// One-based global record ordinal.
    pub record_ordinal: u32,
    /// Complete two-byte record tag.
    pub tag: [u8; 2],
    /// Nonterminal even-lane logical member identifiers.
    pub member_ids: Vec<u32>,
    /// Odd-lane typed references in member order.
    pub typed_references: Vec<u32>,
    /// Face-local support record ordinals selected by the logical members.
    pub support_record_ordinals: Vec<u32>,
    /// Terminal even-lane logical identifier.
    pub terminal_id: u32,
    /// Difference between the terminal and first member identifiers.
    pub gap: u32,
    /// Stored loop-class byte.
    pub loop_class: u8,
    /// Absolute coedge senses in member order; `true` is forward.
    pub forward_senses: Vec<bool>,
    /// Complete sense-oriented model-space endpoint pairs in member order.
    pub oriented_model_endpoints: Vec<[Point3; 2]>,
}

/// One `5e1a` allocation tuple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityEdgeStride {
    /// Offset of the framed record.
    pub pos: usize,
    /// One-based global record ordinal in the zero-entity stream.
    pub record_ordinal: u32,
    /// Five allocation values following the fixed tagged-one prefix.
    pub allocations: [u32; 5],
    /// The three allocations in the `0638`/`2569` topology namespace, in
    /// source order `[T, T-1, T-2]`.
    pub topology_refs: [u32; 3],
    /// The two allocations selecting the adjacent surface-support slots, in
    /// source order `[X, Y]`.
    pub surface_support_refs: [u32; 2],
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
    /// Two stored allocation values.
    pub allocations: [u32; 2],
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
    /// Stored allocation values.
    pub allocations: Vec<u32>,
}

/// The terminal zero-entity body hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityOwnershipRoot {
    /// Offset of the counted `6142` face-roster record.
    pub face_roster_pos: usize,
    /// One-based global record ordinal of the face-roster record.
    pub face_roster_record_ordinal: u32,
    /// Descending one-based face-allocation slots.
    pub face_slots: Vec<u32>,
    /// Offset of the immediately following `6006` shell root.
    pub shell_pos: usize,
    /// Offset of the immediately following `6508` body root.
    pub body_pos: usize,
}

impl ZeroEntityOwnershipRoot {
    /// One-based ordinal of the immediately following `6006` shell root.
    pub fn shell_record_ordinal(&self) -> u32 {
        self.face_roster_record_ordinal.saturating_add(1)
    }

    /// One-based ordinal of the immediately following `6508` body root.
    pub fn body_record_ordinal(&self) -> u32 {
        self.face_roster_record_ordinal.saturating_add(2)
    }
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

fn zero_entity_2118_logical_end(data: &[u8], record: usize) -> Option<usize> {
    let end = record.checked_add(292)?;
    if end > data.len() || data.get(record.checked_add(12)?) != Some(&0x10) {
        return None;
    }
    let knots = [67usize, 75, 83, 91, 99].map(|offset| f64_le(data, record.checked_add(offset)?));
    let knots = knots.into_iter().collect::<Option<Vec<_>>>()?;
    if !knots
        .windows(2)
        .all(|pair| pair[0].is_finite() && pair[0] < pair[1])
        || !knots.last().is_some_and(|knot| knot.is_finite())
    {
        return None;
    }
    for (index, multiplicity) in [4u32, 2, 2, 2, 4].into_iter().enumerate() {
        if tagged_u32(
            data,
            record.checked_add(107usize.checked_add(index.checked_mul(5)?)?)?,
        )? != multiplicity
        {
            return None;
        }
    }
    for index in 0usize..10 {
        let pole = record.checked_add(132usize.checked_add(index.checked_mul(16)?)?)?;
        if !f64_le(data, pole)?.is_finite() || !f64_le(data, pole.checked_add(8)?)?.is_finite() {
            return None;
        }
    }
    Some(end)
}

fn zero_entity_face_roster_logical_end(data: &[u8], record: usize) -> Option<usize> {
    if tagged_u32(data, record.checked_add(7)?)? != 1 {
        return None;
    }
    let count = data.get(record.checked_add(12)?)?.checked_sub(0x80)?;
    if count == 0 {
        return None;
    }
    let count = usize::from(count);
    let values = record.checked_add(13)?;
    for index in 0..count {
        let value = tagged_u32(data, values.checked_add(index.checked_mul(5)?)?)?;
        if value != u32::try_from(count.checked_sub(index)?).ok()? {
            return None;
        }
    }
    let trailer = values.checked_add(count.checked_mul(5)?)?;
    let end = trailer.checked_add(11)?;
    (data.get(trailer..end)? == [0x00, 0x01, 0xc0, 0xff, 0xff, 0x3f, 0, 0, 0, 0, 0x03])
        .then_some(end)
}

fn zero_entity_records_in_range(data: &[u8], range: Range<usize>) -> Vec<ZeroEntityRecord> {
    if data.get(range.clone()).is_none() {
        return Vec::new();
    }
    let mut records = Vec::new();
    let mut position = range.start;
    while position + a9_03::LEN <= range.end {
        if data[position + a9_03::FAMILY..position + a9_03::TAG_HI] != [0xa9, 0x03] {
            position += 1;
            continue;
        }
        let tag = [
            data[position + a9_03::TAG_HI],
            data[position + a9_03::TAG_LO_LENGTH_DRIVER],
        ];
        let nominal_end =
            position.checked_add(usize::from(data[position + a9_03::TAG_LO_LENGTH_DRIVER]) + 12);
        let Some(nominal_end) = nominal_end else {
            break;
        };
        let end = if tag == [0x21, 0x18] {
            zero_entity_2118_logical_end(data, position).unwrap_or(nominal_end)
        } else if tag == [0x61, 0x42] {
            let Some(end) = zero_entity_face_roster_logical_end(data, position) else {
                break;
            };
            end
        } else if matches!(tag, [0x34, 0xc8 | 0x5e]) {
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
        if end > range.end {
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

#[cfg(test)]
fn zero_entity_records(data: &[u8]) -> Vec<ZeroEntityRecord> {
    zero_entity_records_in_range(data, 0..data.len())
}

fn zero_entity_nurbs_logical_end(data: &[u8], record: usize) -> Option<usize> {
    Some(zero_entity_nurbs_layout(data, record)?.end)
}

fn zero_entity_nurbs_layout(data: &[u8], record: usize) -> Option<ZeroEntityNurbsLayout> {
    let tag = [
        *data.get(record.checked_add(a9_03::TAG_HI)?)?,
        *data.get(record.checked_add(a9_03::TAG_LO_LENGTH_DRIVER)?)?,
    ];
    let (grid_offset, expected_u_count, expected_v_count) = zero_entity_nurbs_shape(tag)?;
    let knot_start = record.checked_add(23)?;
    let grid = record.checked_add(grid_offset)?;
    let pole_count = crate::nurbs_surface_control_count(expected_u_count, expected_v_count)?;
    let pole_bytes = pole_count.checked_mul(24)?;
    let end = grid.checked_add(pole_bytes)?;
    data.get(grid..end)?;

    // The carrier has no count word for either distinct-knot lane. Its fixed
    // pole boundary makes the two lanes a bounded parse: U knots, two tagged
    // V-dimension words, one V marker, V knots, V multiplicities, and the
    // three-byte pole marker. Enumerate the possible U lane widths and retain
    // exactly one structural interpretation. A value range or a first
    // terminator would turn ordinary model parameters into framing bytes.
    let pole_marker_start = grid.checked_sub(3)?;
    let available = pole_marker_start.checked_sub(knot_start)?;
    let minimum_after_u = 2usize.checked_mul(5)?.checked_add(1)?.checked_add(2 * 13)?;
    let max_u_distinct = available.checked_sub(minimum_after_u)?.checked_div(13)?;
    if max_u_distinct < 2 {
        return None;
    }
    let mut candidate = None;
    for u_distinct_count in 2..=max_u_distinct {
        let u_after = knot_start.checked_add(u_distinct_count.checked_mul(13)?)?;
        let Some((u_distinct, u_mults, u_degree, u_count)) =
            zero_entity_nurbs_knot_lane(data, knot_start, u_distinct_count, expected_u_count)
        else {
            continue;
        };
        let Some((_, after_dimensions)) = u32_tokens(data, u_after, 2) else {
            continue;
        };
        let v_start = after_dimensions.checked_add(1)?;
        if v_start > pole_marker_start {
            continue;
        }
        let v_bytes = pole_marker_start.checked_sub(v_start)?;
        if v_bytes % 13 != 0 {
            continue;
        }
        let v_distinct_count = v_bytes / 13;
        if v_distinct_count < 2 {
            continue;
        }
        let Some((v_distinct, v_mults, v_degree, v_count)) =
            zero_entity_nurbs_knot_lane(data, v_start, v_distinct_count, expected_v_count)
        else {
            continue;
        };
        if candidate.is_some() {
            return None;
        }
        candidate = Some((
            u_distinct, u_mults, u_degree, u_count, v_distinct, v_mults, v_degree, v_count,
        ));
    }
    let (u_distinct, u_mults, u_degree, u_count, v_distinct, v_mults, v_degree, v_count) =
        candidate?;
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

fn zero_entity_nurbs_knot_lane(
    data: &[u8],
    start: usize,
    distinct_count: usize,
    expected_control_count: usize,
) -> Option<(Vec<f64>, Vec<u32>, u32, u32)> {
    let distinct_end = start.checked_add(distinct_count.checked_mul(8)?)?;
    let mut distinct = Vec::with_capacity(distinct_count);
    for index in 0..distinct_count {
        let value = f64_le(data, start.checked_add(index.checked_mul(8)?)?)?;
        if !value.is_finite() || distinct.last().is_some_and(|last| value <= *last) {
            return None;
        }
        distinct.push(value);
    }
    let (mults, end) = u32_tokens(data, distinct_end, distinct_count)?;
    let degree = mults.first().copied()?.checked_sub(1)?;
    let control_count = mults
        .iter()
        .try_fold(0u32, |sum, value| sum.checked_add(*value))?
        .checked_sub(degree + 1)?;
    if !(1..=9).contains(&degree)
        || usize::try_from(control_count).ok()? != expected_control_count
        || end != distinct_end.checked_add(distinct_count.checked_mul(5)?)?
    {
        return None;
    }
    Some((distinct, mults, degree, control_count))
}

/// Inventory every complete framed record in the one-based global namespace.
#[must_use]
pub fn zero_entity_record_inventory(data: &[u8]) -> Vec<ZeroEntityRecordIdentity> {
    zero_entity_record_inventory_in_range(data, 0..data.len())
}

/// Inventory complete framed records whose extents stay inside `range`.
#[must_use]
pub(crate) fn zero_entity_record_inventory_in_range(
    data: &[u8],
    range: Range<usize>,
) -> Vec<ZeroEntityRecordIdentity> {
    zero_entity_records_in_range(data, range)
        .into_iter()
        .map(|record| ZeroEntityRecordIdentity {
            pos: record.pos,
            end: record.end,
            tag: record.tag,
            record_ordinal: record.ordinal,
        })
        .collect()
}

/// Decode the terminal face-roster, shell, and body ownership roots.
#[cfg(test)]
#[must_use]
pub fn zero_entity_ownership_root(data: &[u8]) -> Option<ZeroEntityOwnershipRoot> {
    zero_entity_ownership_root_in_range(data, 0..data.len())
}

/// Decode every complete ownership hierarchy in the zero-entity stream.
#[cfg(test)]
#[must_use]
pub fn zero_entity_ownership_roots(data: &[u8]) -> Vec<ZeroEntityOwnershipRoot> {
    zero_entity_ownership_roots_in_range(data, 0..data.len())
}

/// Decode ownership roots whose records stay inside `range`.
#[must_use]
pub(crate) fn zero_entity_ownership_root_in_range(
    data: &[u8],
    range: Range<usize>,
) -> Option<ZeroEntityOwnershipRoot> {
    let roots = zero_entity_ownership_roots_in_range(data, range);
    (roots.len() == 1)
        .then(|| roots.into_iter().next())
        .flatten()
}

/// Decode every ownership root whose records stay inside `range`.
pub(crate) fn zero_entity_ownership_roots_in_range(
    data: &[u8],
    range: Range<usize>,
) -> Vec<ZeroEntityOwnershipRoot> {
    let records = zero_entity_records_in_range(data, range);
    records
        .windows(3)
        .filter_map(|window| {
            let [face_roster, shell, body] = window else {
                return None;
            };
            let body_trailer = body.pos.checked_add(18)?;
            if face_roster.tag != [0x61, 0x42]
                || shell.tag != [0x60, 0x06]
                || body.tag != [0x65, 0x08]
                || face_roster.end != shell.pos
                || shell.end != body.pos
                || tagged_u32(data, shell.pos.checked_add(7)?)? != 1
                || data.get(shell.pos.checked_add(12)?) != Some(&0x81)
                || tagged_u32(data, shell.pos.checked_add(13)?)? != 1
                || tagged_u32(data, body.pos.checked_add(7)?)? != 1
                || data.get(body.pos.checked_add(12)?) != Some(&0x81)
                || tagged_u32(data, body.pos.checked_add(13)?)? != 1
                || data.get(body_trailer..body.end)? != [0x05, 0x0d]
            {
                return None;
            }
            let count = usize::from(data[face_roster.pos + 12] - 0x80);
            let face_slots = (0..count)
                .map(|index| tagged_u32(data, face_roster.pos + 13 + index * 5))
                .collect::<Option<Vec<_>>>()?;
            Some(ZeroEntityOwnershipRoot {
                face_roster_pos: face_roster.pos,
                face_roster_record_ordinal: face_roster.ordinal,
                face_slots,
                shell_pos: shell.pos,
                body_pos: body.pos,
            })
        })
        .collect()
}

/// Decode analytic surface carriers in a zero-entity `a9 03` stream.  The
/// record's second tag byte is also its length code (`length = tag + 12`), so
/// the decoder walks framed records.
#[cfg(test)]
pub fn zero_entity_surfaces(data: &[u8]) -> Vec<ZeroEntitySurface> {
    zero_entity_surfaces_in_range(data, 0..data.len())
}

/// Decode surface carriers whose records stay inside `range`.
#[must_use]
pub(crate) fn zero_entity_surfaces_in_range(
    data: &[u8],
    range: Range<usize>,
) -> Vec<ZeroEntitySurface> {
    zero_entity_records_in_range(data, range)
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
#[cfg(test)]
#[must_use]
pub fn zero_entity_support_runs(data: &[u8]) -> Vec<ZeroEntitySupportRun> {
    zero_entity_support_runs_in_range(data, 0..data.len())
}

/// Decode support runs whose complete record population stays inside `range`.
#[must_use]
pub(crate) fn zero_entity_support_runs_in_range(
    data: &[u8],
    range: Range<usize>,
) -> Vec<ZeroEntitySupportRun> {
    let records = zero_entity_records_in_range(data, range);
    let mut runs = Vec::new();
    let mut index = 0usize;
    while index + 1 < records.len() {
        let carrier_record = records[index];
        let Some(carrier_geometry) = zero_entity_surface_at(data, carrier_record.pos) else {
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
                let mut support = support;
                if let Some((curve, parameters)) = support.pcurve.as_ref().and_then(|pcurve| {
                    zero_entity_model_curve(&carrier_geometry, pcurve, support.uv_endpoints?)
                }) {
                    support.model_curve = Some(curve);
                    support.model_parameters = Some(parameters);
                }
                support.model_curve_construction = support.pcurve.as_ref().and_then(|pcurve| {
                    zero_entity_model_curve_construction(&carrier_geometry, pcurve)
                });
                if support.model_curve_construction.is_some() {
                    support.model_parameters = support
                        .uv_endpoints
                        .map(|endpoints| endpoints.map(|uv| uv[0]));
                }
                support.model_midpoint = support.pcurve.as_ref().and_then(|pcurve| {
                    let PcurveGeometry::Nurbs { nurbs } = pcurve else {
                        return None;
                    };
                    let start = *nurbs.knots().get(usize::try_from(nurbs.degree()).ok()?)?;
                    let end = *nurbs.knots().get(nurbs.control_points().len())?;
                    if !start.is_finite() || !end.is_finite() || start >= end {
                        return None;
                    }
                    let uv = pcurve_uv(pcurve, start + (end - start) * 0.5)?;
                    zero_entity_surface_point(&carrier_geometry, [uv.u, uv.v])
                });
                support.model_endpoints = support.uv_endpoints.and_then(|endpoints| {
                    let [first, second] =
                        endpoints.map(|uv| zero_entity_surface_point(&carrier_geometry, uv));
                    Some([first?, second?])
                });
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
    let mut faces = zero_entity_faces_from_records(data, &records);
    let loops = zero_entity_loops_from_records(data, &records);
    let flattened_terminals = faces
        .iter()
        .flat_map(|face| face.loop_terminals())
        .collect::<Vec<_>>();
    let loop_terminals = loops
        .iter()
        .map(|loop_record| loop_record.terminal_id)
        .collect::<Vec<_>>();
    let loop_roster_is_valid = flattened_terminals == loop_terminals && {
        let mut loop_index = 0;
        faces.iter().all(|face| {
            let terminals = face.loop_terminals();
            let loop_end = loop_index + terminals.len();
            let face_loops = &loops[loop_index..loop_end];
            loop_index = loop_end;
            face_loops.first().is_some_and(|outer| {
                matches!(outer.loop_class, 0x41 | 0xc1)
                    && face_loops[1..].iter().all(|inner| inner.loop_class == 0x50)
            })
        })
    };
    if loop_roster_is_valid {
        let mut loop_index = 0;
        for face in &mut faces {
            let terminals = face.loop_terminals();
            let loop_end = loop_index + terminals.len();
            face.loops = Some(loops[loop_index..loop_end].to_vec());
            loop_index = loop_end;
        }
    }
    let face_population = records
        .iter()
        .filter(|record| record.tag[0] == 0x5f)
        .count();
    let surface_population = records
        .iter()
        .filter(|record| zero_entity_surface_carrier_tag(record.tag))
        .count();
    // Equal filtered lengths are not enough: independent drops can shift the
    // positional rosters. Bind only when every framed candidate survived.
    let rosters_are_complete = faces.len() == face_population
        && runs.len() == surface_population
        && faces.len() == runs.len();
    if rosters_are_complete {
        for (run, mut face) in runs.iter_mut().zip(faces) {
            bind_face_support_occurrences(&mut face, &run.supports);
            run.face = Some(face);
        }
    }
    runs
}

fn bind_face_support_occurrences(
    face: &mut ZeroEntityFace,
    supports: &[ZeroEntitySupportOccurrence],
) {
    let loop_count = face.loops.as_ref().map(Vec::len);
    let Some(loop_count) = loop_count else {
        return;
    };
    if loop_count != face.loop_terminals().len() {
        return;
    }
    let mut supports_by_slot = HashMap::<u32, Option<u32>>::new();
    for support in supports {
        supports_by_slot
            .entry(support.face_local_slot)
            .and_modify(|record| *record = None)
            .or_insert(Some(support.record_ordinal));
    }
    let bindings = face
        .loops
        .as_ref()
        .into_iter()
        .flatten()
        .map(|loop_record| {
            loop_record
                .member_ids
                .iter()
                .map(|member| {
                    let slot = loop_record.terminal_id.checked_sub(*member)?;
                    supports_by_slot.get(&slot).copied().flatten()
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>();
    let Some(bindings) = bindings else {
        return;
    };
    if bindings.iter().map(Vec::len).sum::<usize>() != supports.len() {
        return;
    }
    let bound = bindings.iter().flatten().copied().collect::<HashSet<_>>();
    if bound.len() != supports.len() {
        return;
    }
    let Some(face_loops) = face.loops.as_mut() else {
        return;
    };
    for (loop_record, support_record_ordinals) in face_loops.iter_mut().zip(bindings) {
        loop_record.support_record_ordinals = support_record_ordinals;
    }
    let supports_by_ordinal = supports
        .iter()
        .map(|support| (support.record_ordinal, support))
        .collect::<HashMap<_, _>>();
    for loop_record in face.loops.as_mut().into_iter().flatten() {
        let endpoints = loop_record
            .support_record_ordinals
            .iter()
            .map(|ordinal| {
                supports_by_ordinal
                    .get(ordinal)
                    .and_then(|support| support.model_endpoints)
            })
            .collect::<Vec<_>>();
        if let Some(oriented) =
            oriented_closed_model_endpoints(&endpoints, &loop_record.forward_senses)
        {
            loop_record.oriented_model_endpoints = oriented;
        }
    }
}

pub(crate) fn oriented_closed_model_endpoints(
    endpoints: &[Option<[Point3; 2]>],
    forward_senses: &[bool],
) -> Option<Vec<[Point3; 2]>> {
    const CLOSURE_TOLERANCE: f64 = 2e-3;

    if endpoints.is_empty() || endpoints.len() != forward_senses.len() {
        return None;
    }
    let mut oriented = endpoints
        .iter()
        .zip(forward_senses)
        .map(|(endpoints, forward)| {
            endpoints.map(
                |[start, end]| {
                    if *forward {
                        [start, end]
                    } else {
                        [end, start]
                    }
                },
            )
        })
        .collect::<Vec<_>>();
    let missing = oriented
        .iter()
        .enumerate()
        .filter_map(|(index, endpoints)| endpoints.is_none().then_some(index))
        .collect::<Vec<_>>();
    match missing.as_slice() {
        [] => {}
        [index] if oriented.len() > 1 => {
            let previous = (*index + oriented.len() - 1) % oriented.len();
            let next = (*index + 1) % oriented.len();
            oriented[*index] = Some([oriented[previous]?[1], oriented[next]?[0]]);
        }
        _ => return None,
    }
    let oriented = oriented.into_iter().collect::<Option<Vec<_>>>()?;
    oriented
        .iter()
        .enumerate()
        .all(|(index, endpoints)| {
            endpoints[1].distance(oriented[(index + 1) % oriented.len()][0]) <= CLOSURE_TOLERANCE
        })
        .then_some(oriented)
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
            if allocations.contains(&0) {
                return None;
            }
            let first = *allocations.first()?;
            let loop_terminals = allocations[1..]
                .iter()
                .map(|allocation| first.checked_sub(*allocation))
                .collect::<Option<Vec<_>>>()?;
            if loop_terminals.contains(&0)
                || !loop_terminals[1..].windows(2).all(|pair| pair[0] < pair[1])
            {
                return None;
            }
            let terminal_control = ZeroEntityFaceControl::from_byte(*data.get(record.end - 1)?)?;
            Some(ZeroEntityFace {
                pos: record.pos,
                record_ordinal: record.ordinal,
                tag: record.tag,
                allocations,
                loops: None,
                terminal_control,
            })
        })
        .collect()
}

fn zero_entity_loops_from_records(
    data: &[u8],
    records: &[ZeroEntityRecord],
) -> Vec<ZeroEntityLoop> {
    records
        .iter()
        .filter_map(|record| {
            if record.tag[0] != 0x62 {
                return None;
            }
            let reference_count = usize::from(data.get(record.pos + 12)?.checked_sub(0x80)?);
            if reference_count < 3 || reference_count % 2 == 0 {
                return None;
            }
            let edge_count = (reference_count - 1) / 2;
            let references = (0..reference_count)
                .map(|index| tagged_u32(data, record.pos + 13 + index * 5))
                .collect::<Option<Vec<_>>>()?;
            let member_ids = references[..reference_count - 1]
                .iter()
                .step_by(2)
                .copied()
                .collect::<Vec<_>>();
            let typed_references = references[1..reference_count - 1]
                .iter()
                .step_by(2)
                .copied()
                .collect::<Vec<_>>();
            if typed_references.contains(&0) {
                return None;
            }
            let terminal_id = *references.last()?;
            let gap = terminal_id.checked_sub(*member_ids.first()?)?;
            if gap == 0
                || !member_ids.iter().enumerate().all(|(index, member)| {
                    u32::try_from(index)
                        .ok()
                        .and_then(|index| terminal_id.checked_sub(gap)?.checked_sub(index))
                        == Some(*member)
                })
            {
                return None;
            }
            let trailer = record
                .pos
                .checked_add(13 + reference_count.checked_mul(5)?)?;
            if data.get(trailer) != Some(&(0x80 + u8::try_from(edge_count).ok()?))
                || !matches!(data.get(trailer + 1), Some(0x41 | 0x50 | 0xc1))
            {
                return None;
            }
            let packed_length = edge_count.checked_mul(3)?.checked_add(7)? / 8;
            if trailer.checked_add(3 + packed_length)? != record.end
                || data.get(trailer + 2 + packed_length) != Some(&0x01)
            {
                return None;
            }
            let packed = data.get(trailer + 2..trailer + 2 + packed_length)?;
            let forward_senses = (0..edge_count)
                .map(|index| {
                    let bit = index * 3;
                    let code = (0..3).fold(0, |code, offset| {
                        code | (((packed[(bit + offset) / 8] >> ((bit + offset) % 8)) & 1)
                            << offset)
                    });
                    match code {
                        2 => Some(false),
                        7 => Some(true),
                        _ => None,
                    }
                })
                .collect::<Option<Vec<_>>>()?;
            Some(ZeroEntityLoop {
                pos: record.pos,
                record_ordinal: record.ordinal,
                tag: record.tag,
                member_ids,
                typed_references,
                support_record_ordinals: Vec::new(),
                terminal_id,
                gap,
                loop_class: data[trailer + 1],
                forward_senses,
                oriented_model_endpoints: Vec::new(),
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
    let face_local_slot = View::u32_le_at(data, record.pos + 13)?;
    if face_local_slot == 0 {
        return None;
    }
    let uv_offsets = match record.tag {
        [0x21, 0x18] => Some([132, 276]),
        [0x21, 0x45] => Some([145, 321]),
        [0x21, 0x71] => Some([pcurve_2171::POLES, pcurve_2171::POLES + 16]),
        [0x21, 0x72] => Some([158, 366]),
        [0x21, 0x91] => Some([93, 141]),
        [0x21, 0x99] => Some([93, 125]),
        [0x21, 0x9f] => Some([171, 411]),
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
    let pcurve = zero_entity_support_pcurve(data, record);
    if matches!(
        record.tag,
        [
            0x21,
            0x18 | 0x45 | 0x71 | 0x72 | 0x91 | 0x99 | 0x9f | 0xd6 | 0xe8
        ]
    ) && pcurve.is_none()
    {
        return None;
    }
    Some(ZeroEntitySupportOccurrence {
        pos: record.pos,
        record_ordinal: record.ordinal,
        tag: record.tag,
        face_local_slot,
        uv_endpoints,
        pcurve,
        model_curve: None,
        model_curve_construction: None,
        model_parameters: None,
        model_midpoint: None,
        model_endpoints: None,
    })
}

fn zero_entity_support_pcurve(data: &[u8], record: ZeroEntityRecord) -> Option<PcurveGeometry> {
    let (
        knot_offsets,
        multiplicity_start,
        expected_multiplicities,
        pole_start,
        control_count,
        weight_start,
        required_end,
    ) = match record.tag {
        [0x21, 0x18] => (
            &[67, 75, 83, 91, 99][..],
            107,
            &[4, 2, 2, 2, 4][..],
            132,
            10,
            None,
            292,
        ),
        [0x21, 0x45] => (
            &[67, 75, 83, 91, 99, 107][..],
            115,
            &[4, 2, 2, 2, 2, 4][..],
            145,
            12,
            None,
            337,
        ),
        [0x21, 0x71] => (
            &[pcurve_2171::KNOTS, pcurve_2171::KNOTS + 8][..],
            pcurve_2171::MULTIPLICITIES,
            &[2, 2][..],
            pcurve_2171::POLES,
            2,
            None,
            pcurve_2171::LEN,
        ),
        [0x21, 0x72] => (
            &[67, 75, 83, 91, 99, 107, 115][..],
            123,
            &[4, 2, 2, 2, 2, 2, 4][..],
            158,
            14,
            None,
            382,
        ),
        [0x21, 0x91] => (&[67, 75][..], 83, &[4, 4][..], 93, 4, None, 157),
        [0x21, 0x99] => (&[67, 75][..], 83, &[3, 3][..], 93, 3, Some(141), 165),
        [0x21, 0x9f] => (
            &[67, 75, 83, 91, 99, 107, 115, 123][..],
            131,
            &[4, 2, 2, 2, 2, 2, 2, 4][..],
            171,
            16,
            None,
            427,
        ),
        [0x21, 0xd6] => (&[67, 75, 83][..], 91, &[3, 2, 3][..], 106, 5, None, 186),
        [0x21, 0xe8] => (
            &[67, 75, 83, 91, 99][..],
            107,
            &[4, 1, 1, 1, 4][..],
            132,
            7,
            None,
            244,
        ),
        _ => return None,
    };
    if record.pos.checked_add(required_end)? > record.end {
        return None;
    }
    let distinct_knots = knot_offsets
        .iter()
        .map(|offset| f64_le(data, record.pos.checked_add(*offset)?))
        .collect::<Option<Vec<_>>>()?;
    if !distinct_knots
        .windows(2)
        .all(|pair| pair[0].is_finite() && pair[0] < pair[1])
        || !distinct_knots.last().is_some_and(|knot| knot.is_finite())
    {
        return None;
    }
    let multiplicities = (0..distinct_knots.len())
        .map(|index| {
            tagged_u32(
                data,
                record
                    .pos
                    .checked_add(multiplicity_start + index.checked_mul(5)?)?,
            )
        })
        .collect::<Option<Vec<_>>>()?;
    if multiplicities != expected_multiplicities {
        return None;
    }
    let degree = multiplicities.first().copied()?.checked_sub(1)?;
    let derived_control_count = multiplicities
        .iter()
        .try_fold(0u32, |sum, multiplicity| sum.checked_add(*multiplicity))?
        .checked_sub(degree.checked_add(1)?)?;
    if derived_control_count != u32::try_from(control_count).ok()? {
        return None;
    }
    let knots = expand_knots(&distinct_knots, &multiplicities)?;
    let control_points = (0usize..control_count)
        .map(|index| {
            let at = record
                .pos
                .checked_add(pole_start + index.checked_mul(16)?)?;
            let point = Point2::new(f64_le(data, at)?, f64_le(data, at.checked_add(8)?)?);
            (point.u.is_finite() && point.v.is_finite()).then_some(point)
        })
        .collect::<Option<Vec<_>>>()?;
    let weights = if let Some(weight_start) = weight_start {
        Some(
            (0usize..control_count)
                .map(|index| {
                    let weight = f64_le(
                        data,
                        record
                            .pos
                            .checked_add(weight_start + index.checked_mul(8)?)?,
                    )?;
                    (weight.is_finite() && weight > 0.0).then_some(weight)
                })
                .collect::<Option<Vec<_>>>()?,
        )
    } else {
        None
    };
    Some(PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::new(degree, knots, control_points, weights, false).ok()?,
    })
}

/// Convert a zero-entity support pcurve from the carrier's native chart into
/// the neutral IR chart.  The support records store cylindrical and toroidal
/// coordinates as arc lengths and conical latitude as slant length; IR
/// analytic surfaces use angles and axial distance respectively.
pub(crate) fn zero_entity_neutral_pcurve(
    surface: &SurfaceGeometry,
    pcurve: &PcurveGeometry,
) -> Option<PcurveGeometry> {
    let (u_scale, v_scale) = match surface {
        SurfaceGeometry::Cylinder { radius, .. } => (radius.recip(), 1.0),
        SurfaceGeometry::Cone { half_angle, .. } => (1.0, half_angle.cos()),
        SurfaceGeometry::Torus {
            major_radius,
            minor_radius,
            ..
        } => (major_radius.recip(), minor_radius.recip()),
        SurfaceGeometry::Plane { .. } | SurfaceGeometry::Nurbs(_) => (1.0, 1.0),
        _ => return None,
    };
    if !u_scale.is_finite() || !v_scale.is_finite() || u_scale == 0.0 || v_scale == 0.0 {
        return None;
    }
    let PcurveGeometry::Nurbs { nurbs } = pcurve else {
        return None;
    };
    let control_points = nurbs
        .control_points()
        .iter()
        .map(|point| {
            let point = Point2::new(point.u * u_scale, point.v * v_scale);
            ([point.u, point.v].into_iter().all(f64::is_finite)).then_some(point)
        })
        .collect::<Option<Vec<_>>>()?;
    Some(PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::new(
            nurbs.degree(),
            nurbs.knots().to_vec(),
            control_points,
            nurbs.weights().map(<[f64]>::to_vec),
            nurbs.periodic(),
        )
        .ok()?,
    })
}

fn zero_entity_model_curve(
    surface: &SurfaceGeometry,
    pcurve: &PcurveGeometry,
    uv_endpoints: [[f64; 2]; 2],
) -> Option<(CurveGeometry, [f64; 2])> {
    let PcurveGeometry::Nurbs { nurbs } = pcurve else {
        return None;
    };
    if nurbs.periodic() {
        return None;
    }
    let constant_coordinate = |dimension: usize| {
        let value = if dimension == 0 {
            nurbs.control_points().first()?.u
        } else {
            nurbs.control_points().first()?.v
        };
        nurbs
            .control_points()
            .iter()
            .all(|point| {
                if dimension == 0 {
                    point.u == value
                } else {
                    point.v == value
                }
            })
            .then_some(value)
    };
    match surface {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let v_axis = normal.cross(*u_axis);
            let degree_index = usize::try_from(nurbs.degree()).ok()?;
            let parameters = [
                *nurbs.knots().get(degree_index)?,
                *nurbs
                    .knots()
                    .get(nurbs.knots().len().checked_sub(degree_index + 1)?)?,
            ];
            Some((
                CurveGeometry::Nurbs(
                    NurbsCurve::new(
                        nurbs.degree(),
                        nurbs.knots().to_vec(),
                        nurbs
                            .control_points()
                            .iter()
                            .map(|point| {
                                Point3::new(
                                    origin.x + point.u * u_axis.x + point.v * v_axis.x,
                                    origin.y + point.u * u_axis.y + point.v * v_axis.y,
                                    origin.z + point.u * u_axis.z + point.v * v_axis.z,
                                )
                            })
                            .collect(),
                        nurbs.weights().map(<[f64]>::to_vec),
                        false,
                    )
                    .ok()?,
                ),
                parameters,
            ))
        }
        SurfaceGeometry::Cylinder { axis, .. } if constant_coordinate(0).is_some() => {
            let point = zero_entity_surface_point(surface, [constant_coordinate(0)?, 0.0])?;
            Some((
                CurveGeometry::Line {
                    origin: point,
                    direction: *axis,
                },
                uv_endpoints.map(|uv| uv[1]),
            ))
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } if constant_coordinate(1).is_some() => {
            let height = constant_coordinate(1)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(
                        origin.x + height * axis.x,
                        origin.y + height * axis.y,
                        origin.z + height * axis.z,
                    ),
                    axis: *axis,
                    ref_direction: *ref_direction,
                    radius: *radius,
                },
                uv_endpoints.map(|uv| uv[0] / radius),
            ))
        }
        SurfaceGeometry::Cone {
            axis,
            ref_direction,
            ratio: 1.0,
            half_angle,
            ..
        } if constant_coordinate(0).is_some() => {
            let angle = constant_coordinate(0)?;
            let transverse = axis.cross(*ref_direction);
            let radial = cadmpeg_ir::math::Vector3::new(
                angle.cos() * ref_direction.x + angle.sin() * transverse.x,
                angle.cos() * ref_direction.y + angle.sin() * transverse.y,
                angle.cos() * ref_direction.z + angle.sin() * transverse.z,
            );
            Some((
                CurveGeometry::Line {
                    origin: zero_entity_surface_point(surface, [angle, 0.0])?,
                    direction: cadmpeg_ir::math::Vector3::new(
                        half_angle.cos() * axis.x + half_angle.sin() * radial.x,
                        half_angle.cos() * axis.y + half_angle.sin() * radial.y,
                        half_angle.cos() * axis.z + half_angle.sin() * radial.z,
                    ),
                },
                uv_endpoints.map(|uv| uv[1]),
            ))
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio: 1.0,
            half_angle,
        } if constant_coordinate(1).is_some() => {
            let slant = constant_coordinate(1)?;
            let circle_radius = radius + slant * half_angle.sin();
            (circle_radius.is_finite() && circle_radius != 0.0).then_some(())?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(
                        origin.x + slant * half_angle.cos() * axis.x,
                        origin.y + slant * half_angle.cos() * axis.y,
                        origin.z + slant * half_angle.cos() * axis.z,
                    ),
                    axis: *axis,
                    ref_direction: if circle_radius > 0.0 {
                        *ref_direction
                    } else {
                        cadmpeg_ir::math::Vector3::new(
                            -ref_direction.x,
                            -ref_direction.y,
                            -ref_direction.z,
                        )
                    },
                    radius: circle_radius.abs(),
                },
                uv_endpoints.map(|uv| uv[0]),
            ))
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if constant_coordinate(0).is_some() => {
            let angle = constant_coordinate(0)? / major_radius;
            let transverse = axis.cross(*ref_direction);
            let radial = cadmpeg_ir::math::Vector3::new(
                angle.cos() * ref_direction.x + angle.sin() * transverse.x,
                angle.cos() * ref_direction.y + angle.sin() * transverse.y,
                angle.cos() * ref_direction.z + angle.sin() * transverse.z,
            );
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(
                        center.x + major_radius * radial.x,
                        center.y + major_radius * radial.y,
                        center.z + major_radius * radial.z,
                    ),
                    axis: radial.cross(*axis),
                    ref_direction: radial,
                    radius: *minor_radius,
                },
                uv_endpoints.map(|uv| uv[1] / minor_radius),
            ))
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if constant_coordinate(1).is_some() => {
            let angle = constant_coordinate(1)? / minor_radius;
            let circle_radius = major_radius + minor_radius * angle.cos();
            (circle_radius.is_finite() && circle_radius != 0.0).then_some(())?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(
                        center.x + minor_radius * angle.sin() * axis.x,
                        center.y + minor_radius * angle.sin() * axis.y,
                        center.z + minor_radius * angle.sin() * axis.z,
                    ),
                    axis: *axis,
                    ref_direction: if circle_radius > 0.0 {
                        *ref_direction
                    } else {
                        cadmpeg_ir::math::Vector3::new(
                            -ref_direction.x,
                            -ref_direction.y,
                            -ref_direction.z,
                        )
                    },
                    radius: circle_radius.abs(),
                },
                uv_endpoints.map(|uv| uv[0] / major_radius),
            ))
        }
        SurfaceGeometry::Nurbs(surface) if constant_coordinate(0).is_some() => Some((
            CurveGeometry::Nurbs(crate::nurbs::nurbs_surface_isocurve(
                surface,
                constant_coordinate(0)?,
                true,
            )?),
            uv_endpoints.map(|uv| uv[1]),
        )),
        SurfaceGeometry::Nurbs(surface) if constant_coordinate(1).is_some() => Some((
            CurveGeometry::Nurbs(crate::nurbs::nurbs_surface_isocurve(
                surface,
                constant_coordinate(1)?,
                false,
            )?),
            uv_endpoints.map(|uv| uv[0]),
        )),
        _ => None,
    }
}

fn zero_entity_model_curve_construction(
    surface: &SurfaceGeometry,
    pcurve: &PcurveGeometry,
) -> Option<ProceduralCurveDefinition> {
    let (
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio: 1.0,
            half_angle,
        },
        PcurveGeometry::Nurbs { nurbs },
    ) = (surface, pcurve)
    else {
        return None;
    };
    if nurbs.degree() != 1 || nurbs.weights().is_some() || nurbs.periodic() {
        return None;
    }
    let [first, second] = nurbs.control_points() else {
        return None;
    };
    if first.u == second.u || first.v == second.v {
        return None;
    }
    let [start, end] = if first.u < second.u {
        [first, second]
    } else {
        [second, first]
    };
    let slope = (end.v - start.v) / (end.u - start.u);
    let start_radius = radius + start.v * half_angle.sin();
    if !slope.is_finite() || !start_radius.is_finite() || start_radius == 0.0 {
        return None;
    }
    let transverse = axis.cross(*ref_direction);
    let major = cadmpeg_ir::math::Vector3::new(
        start_radius * ref_direction.x,
        start_radius * ref_direction.y,
        start_radius * ref_direction.z,
    );
    let minor = cadmpeg_ir::math::Vector3::new(
        start_radius * transverse.x,
        start_radius * transverse.y,
        start_radius * transverse.z,
    );
    Some(ProceduralCurveDefinition::Helix {
        angle_range: [start.u, end.u],
        center: Point3::new(
            origin.x + start.v * half_angle.cos() * axis.x,
            origin.y + start.v * half_angle.cos() * axis.y,
            origin.z + start.v * half_angle.cos() * axis.z,
        ),
        major,
        minor,
        pitch: cadmpeg_ir::math::Vector3::new(
            std::f64::consts::TAU * slope * half_angle.cos() * axis.x,
            std::f64::consts::TAU * slope * half_angle.cos() * axis.y,
            std::f64::consts::TAU * slope * half_angle.cos() * axis.z,
        ),
        apex_factor: std::f64::consts::TAU * slope * half_angle.sin() / start_radius,
        axis: *axis,
    })
}

fn zero_entity_surface_point(geometry: &SurfaceGeometry, [u, v]: [f64; 2]) -> Option<Point3> {
    let point = match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let v_axis = normal.cross(*u_axis);
            Point3::new(
                origin.x + u * u_axis.x + v * v_axis.x,
                origin.y + u * u_axis.y + v * v_axis.y,
                origin.z + u * u_axis.z + v * v_axis.z,
            )
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            let angle = u / radius;
            let transverse = axis.cross(*ref_direction);
            Point3::new(
                origin.x
                    + radius * (angle.cos() * ref_direction.x + angle.sin() * transverse.x)
                    + v * axis.x,
                origin.y
                    + radius * (angle.cos() * ref_direction.y + angle.sin() * transverse.y)
                    + v * axis.y,
                origin.z
                    + radius * (angle.cos() * ref_direction.z + angle.sin() * transverse.z)
                    + v * axis.z,
            )
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } if *ratio == 1.0 => {
            let transverse = axis.cross(*ref_direction);
            let axial = v * half_angle.cos();
            let radial = radius + v * half_angle.sin();
            Point3::new(
                origin.x
                    + radial * (u.cos() * ref_direction.x + u.sin() * transverse.x)
                    + axial * axis.x,
                origin.y
                    + radial * (u.cos() * ref_direction.y + u.sin() * transverse.y)
                    + axial * axis.y,
                origin.z
                    + radial * (u.cos() * ref_direction.z + u.sin() * transverse.z)
                    + axial * axis.z,
            )
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            let major_angle = u / major_radius;
            let minor_angle = v / minor_radius;
            let transverse = axis.cross(*ref_direction);
            let radial = major_radius + minor_radius * minor_angle.cos();
            Point3::new(
                center.x
                    + radial
                        * (major_angle.cos() * ref_direction.x + major_angle.sin() * transverse.x)
                    + minor_radius * minor_angle.sin() * axis.x,
                center.y
                    + radial
                        * (major_angle.cos() * ref_direction.y + major_angle.sin() * transverse.y)
                    + minor_radius * minor_angle.sin() * axis.y,
                center.z
                    + radial
                        * (major_angle.cos() * ref_direction.z + major_angle.sin() * transverse.z)
                    + minor_radius * minor_angle.sin() * axis.z,
            )
        }
        SurfaceGeometry::Nurbs(surface) => nurbs_surface_point(surface, u, v)?,
        _ => return None,
    };
    [point.x, point.y, point.z]
        .into_iter()
        .all(f64::is_finite)
        .then_some(point)
}

/// Decode complete `5e1a` allocation tuples.
#[cfg(test)]
#[must_use]
pub fn zero_entity_edge_strides(data: &[u8]) -> Vec<ZeroEntityEdgeStride> {
    zero_entity_edge_strides_in_range(data, 0..data.len())
}

/// Decode edge strides whose records stay inside `range`.
#[must_use]
pub(crate) fn zero_entity_edge_strides_in_range(
    data: &[u8],
    range: Range<usize>,
) -> Vec<ZeroEntityEdgeStride> {
    let records = zero_entity_records_in_range(data, range);
    records
        .into_iter()
        .filter_map(|record| {
            if record.tag != [0x5e, 0x1a]
                || tagged_u32(data, record.pos + edge_5e1a::TAGGED_ONE_PREFIX) != Some(1)
                || data.get(record.pos + edge_5e1a::TERMINAL) != Some(&0x21)
            {
                return None;
            }
            let mut allocations = [0; 5];
            for (index, allocation) in allocations.iter_mut().enumerate() {
                *allocation = tagged_u32(
                    data,
                    record.pos.checked_add(edge_5e1a::ALLOCATIONS + index * 5)?,
                )?;
            }
            if allocations.contains(&0) {
                return None;
            }
            if allocations[0].checked_sub(1) != Some(allocations[3])
                || allocations[0].checked_sub(2) != Some(allocations[4])
            {
                return None;
            }
            Some(ZeroEntityEdgeStride {
                pos: record.pos,
                record_ordinal: record.ordinal,
                topology_refs: [allocations[0], allocations[3], allocations[4]],
                surface_support_refs: [allocations[1], allocations[2]],
                allocations,
            })
        })
        .collect()
}

/// Decode complete `2569` headers with their adjacent `(1, 2)` oriented uses.
#[cfg(test)]
#[must_use]
pub fn zero_entity_oriented_use_pairs(data: &[u8]) -> Vec<ZeroEntityOrientedUsePair> {
    zero_entity_oriented_use_pairs_in_range(data, 0..data.len())
}

/// Decode oriented-use pairs whose records stay inside `range`.
#[must_use]
pub(crate) fn zero_entity_oriented_use_pairs_in_range(
    data: &[u8],
    range: Range<usize>,
) -> Vec<ZeroEntityOrientedUsePair> {
    let records = zero_entity_records_in_range(data, range);
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
                let allocations = [
                    tagged_u32(data, record.pos + 18)?,
                    tagged_u32(data, record.pos + 23)?,
                ];
                let expected_allocations = [
                    base_columns[0].checked_add(expected_side)?,
                    base_columns[1].checked_add(expected_side)?,
                ];
                if allocations != expected_allocations {
                    return None;
                }
                Some(ZeroEntityOrientedUse {
                    pos: record.pos,
                    record_ordinal: record.ordinal,
                    side: expected_side,
                    allocations,
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
#[cfg(test)]
#[must_use]
pub fn zero_entity_vertex_incidences(data: &[u8]) -> Vec<ZeroEntityVertexIncidence> {
    zero_entity_vertex_incidences_in_range(data, 0..data.len())
}

/// Decode vertex incidences whose records stay inside `range`.
#[must_use]
pub(crate) fn zero_entity_vertex_incidences_in_range(
    data: &[u8],
    range: Range<usize>,
) -> Vec<ZeroEntityVertexIncidence> {
    let records = zero_entity_records_in_range(data, range);
    records
        .windows(2)
        .filter_map(|records| {
            let [record, owner] = records else {
                return None;
            };
            let count = match record.tag {
                [0x05, 0x0b] => 2,
                [0x05, 0x10] => 3,
                [0x05, 0x15] => 4,
                _ => return None,
            };
            if tagged_u32(data, record.pos + 7) != Some(1)
                || data.get(record.pos + 12) != Some(&(0x80 + count as u8))
                || record.end != owner.pos
                || !zero_entity_vertex_owner(data, *owner)
            {
                return None;
            }
            let allocations = (0..count)
                .map(|index| tagged_u32(data, record.pos + 13 + index * 5))
                .collect::<Option<Vec<_>>>()?;
            if allocations.contains(&0) {
                return None;
            }
            Some(ZeroEntityVertexIncidence {
                pos: record.pos,
                record_ordinal: record.ordinal,
                tag: record.tag,
                allocations,
            })
        })
        .collect()
}

fn zero_entity_vertex_owner(data: &[u8], record: ZeroEntityRecord) -> bool {
    let Some(expected_end) = record.pos.checked_add(vertex_5d06::LEN) else {
        return false;
    };
    let Some(first_token) = record.pos.checked_add(vertex_5d06::TAGGED_ONE_A) else {
        return false;
    };
    let Some(second_token) = record.pos.checked_add(vertex_5d06::TAGGED_ONE_B) else {
        return false;
    };
    let Some(terminal) = record.pos.checked_add(vertex_5d06::TERMINAL) else {
        return false;
    };
    record.tag == [0x5d, 0x06]
        && record.end == expected_end
        && tagged_u32(data, first_token) == Some(1)
        && tagged_u32(data, second_token) == Some(1)
        && data.get(terminal) == Some(&0)
}

fn tagged_u32(data: &[u8], at: usize) -> Option<u32> {
    (data.get(at) == Some(&0x10)).then(|| View::u32_le_at(data, at + 1))?
}

pub(crate) fn zero_entity_surface_at(data: &[u8], record: usize) -> Option<SurfaceGeometry> {
    let tag = [
        *data.get(record + a9_03::TAG_HI)?,
        *data.get(record + a9_03::TAG_LO_LENGTH_DRIVER)?,
    ];
    if !zero_entity_surface_carrier_tag(tag) {
        return None;
    }
    let payload_end =
        record.checked_add(*data.get(record + a9_03::TAG_LO_LENGTH_DRIVER)? as usize + 12)?;
    let payload = data.get(record + a9_03::LEN..payload_end)?;
    match tag {
        [0x27, 0x6a] => zero_entity_plane(payload),
        [0x28, 0x8a] => zero_entity_cylinder(payload),
        [0x29, 0xb8] => zero_entity_cone(payload),
        [0x2b, 0xc8] => zero_entity_torus(payload),
        [0x34, 0xc8 | 0x5e] => zero_entity_nurbs_surface(data, record),
        _ => None,
    }
}

fn zero_entity_surface_carrier_tag(tag: [u8; 2]) -> bool {
    matches!(
        tag,
        [0x27, 0x6a] | [0x28, 0x8a] | [0x29, 0xb8] | [0x2b | 0x34, 0xc8] | [0x34, 0x5e]
    )
}

fn zero_entity_nurbs_shape(tag: [u8; 2]) -> Option<(usize, usize, usize)> {
    match tag {
        [0x34, 0xc8] => Some((167, 7, 7)),
        [0x34, 0x5e] => Some((141, 5, 7)),
        _ => None,
    }
}

/// Decode the inline zero-entity non-rational NURBS carrier. Its pole grid
/// extends past the nominal framed record at a tag-specific fixed offset.
fn zero_entity_nurbs_surface(data: &[u8], record: usize) -> Option<SurfaceGeometry> {
    let layout = zero_entity_nurbs_layout(data, record)?;
    let pole_count =
        crate::nurbs_surface_control_count(layout.u_count as usize, layout.v_count as usize)?;
    let mut control_points = Vec::with_capacity(pole_count);
    for pole in 0..pole_count {
        control_points.push(f64_point(
            data,
            layout.grid.checked_add(pole.checked_mul(24)?)?,
        )?);
    }
    Some(SurfaceGeometry::Nurbs(
        NurbsSurface::new(
            layout.u_degree,
            layout.v_degree,
            expand_knots(&layout.u_distinct, &layout.u_mults)?,
            expand_knots(&layout.v_distinct, &layout.v_mults)?,
            layout.u_count,
            layout.v_count,
            control_points,
            None,
            false,
            false,
            false,
        )
        .ok()?,
    ))
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
    if !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    Some(geometry)
}

fn zero_entity_cone(payload: &[u8]) -> Option<SurfaceGeometry> {
    let mut c = crate::wire::cursor::Cursor::new_at(payload, 8);
    let (geometry, radius, half_angle) = crate::analytic::cone_ozra(&mut c)?;
    if !(radius.is_finite()
        && radius > 0.0
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
        && minor_radius.is_finite()
        && minor_radius > 0.0)
    {
        return None;
    }
    Some(geometry)
}

fn u32_tokens(bytes: &[u8], at: usize, count: usize) -> Option<(Vec<u32>, usize)> {
    let mut view = View::over_retained(bytes);
    view.seek(at)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        if view.u8()? != 0x10 {
            return None;
        }
        let value = view.u32_le()?;
        if value == 0 {
            return None;
        }
        values.push(value);
    }
    Some((values, view.position()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        zero_entity_face_loop_support_stream, zero_entity_face_support_stream,
        zero_entity_ownership_stream, zero_entity_support_stream, zero_entity_topology_stream,
    };

    fn test_pcurve(points: Vec<Point2>) -> PcurveGeometry {
        PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::new(1, vec![0.0, 0.0, 1.0, 1.0], points, None, false).unwrap(),
        }
    }

    fn write_tagged_u32(record: &mut [u8], at: usize, value: u32) {
        record[at] = 0x10;
        record[at + 1..at + 5].copy_from_slice(&value.to_le_bytes());
    }

    fn nurbs_carrier(
        tag: [u8; 2],
        u_knots: &[f64],
        u_mults: &[u32],
        v_knots: &[f64],
        v_mults: &[u32],
    ) -> Vec<u8> {
        let (grid_offset, u_count, v_count) =
            zero_entity_nurbs_shape(tag).expect("unsupported test carrier");
        let pole_count = crate::nurbs_surface_control_count(u_count, v_count)
            .expect("test NURBS dimensions fit the codec limit");
        let mut bytes = vec![0u8; grid_offset + pole_count * 24];
        bytes[..4].copy_from_slice(&[0xa9, 0x03, tag[0], tag[1]]);
        let mut at = 23;
        for knot in u_knots {
            bytes[at..at + 8].copy_from_slice(&knot.to_le_bytes());
            at += 8;
        }
        for multiplicity in u_mults {
            write_tagged_u32(&mut bytes, at, *multiplicity);
            at += 5;
        }
        write_tagged_u32(&mut bytes, at, 1);
        at += 5;
        write_tagged_u32(&mut bytes, at, 1);
        at += 5;
        bytes[at] = 0x04;
        at += 1;
        for knot in v_knots {
            bytes[at..at + 8].copy_from_slice(&knot.to_le_bytes());
            at += 8;
        }
        for multiplicity in v_mults {
            write_tagged_u32(&mut bytes, at, *multiplicity);
            at += 5;
        }
        bytes[at..at + 3].copy_from_slice(&[0x08, 0x00, 0x00]);
        at += 3;
        assert_eq!(at, grid_offset);
        for pole in 0..pole_count {
            let value = pole as f64;
            for coordinate in 0..3 {
                let offset = at + pole * 24 + coordinate * 8;
                bytes[offset..offset + 8]
                    .copy_from_slice(&(value + coordinate as f64).to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn nurbs_layout_uses_fixed_boundaries_without_parameter_windows() {
        let carriers = [
            (
                [0x34, 0xc8],
                vec![10.0, 20.0, 30.0, 40.0, 50.0],
                vec![4, 1, 1, 1, 4],
                vec![-100.0, 0.0, 100.0, 200.0, 300.0],
                vec![4, 1, 1, 1, 4],
                167usize + 49 * 24,
            ),
            (
                [0x34, 0x5e],
                vec![10.0, 20.0, 30.0],
                vec![4, 1, 4],
                vec![-100.0, 0.0, 100.0, 200.0, 300.0],
                vec![4, 1, 1, 1, 4],
                141usize + 35 * 24,
            ),
        ];
        for (tag, u_knots, u_mults, v_knots, v_mults, expected_end) in carriers {
            let bytes = nurbs_carrier(tag, &u_knots, &u_mults, &v_knots, &v_mults);
            let layout = zero_entity_nurbs_layout(&bytes, 0).expect("bounded NURBS layout");
            assert_eq!(layout.end, expected_end);
            assert_eq!(layout.u_distinct, u_knots);
            assert_eq!(layout.v_distinct, v_knots);
            assert!(matches!(
                zero_entity_surface_at(&bytes, 0),
                Some(SurfaceGeometry::Nurbs(_))
            ));
        }
    }

    #[test]
    fn nurbs_layout_rejects_an_invalid_dimension_token() {
        let mut bytes = nurbs_carrier(
            [0x34, 0x5e],
            &[10.0, 20.0, 30.0],
            &[4, 1, 4],
            &[-100.0, 0.0, 100.0, 200.0, 300.0],
            &[4, 1, 1, 1, 4],
        );
        let first_dimension_value = 23 + 3 * 13 + 1;
        bytes[first_dimension_value..first_dimension_value + 4].fill(0);
        assert!(zero_entity_nurbs_layout(&bytes, 0).is_none());
    }

    #[test]
    fn nurbs_layout_rejects_nonfinite_knots() {
        let mut bytes = nurbs_carrier(
            [0x34, 0xc8],
            &[10.0, f64::NAN, 30.0, 40.0, 50.0],
            &[4, 1, 1, 1, 4],
            &[-100.0, 0.0, 100.0, 200.0, 300.0],
            &[4, 1, 1, 1, 4],
        );
        bytes[23..31].copy_from_slice(&10.0f64.to_le_bytes());
        assert!(zero_entity_nurbs_layout(&bytes, 0).is_none());
    }

    #[test]
    fn nurbs_layout_requires_the_declared_pole_grid_before_allocation() {
        let mut bytes = vec![0u8; 79];
        bytes[23..31].copy_from_slice(&0.0f64.to_le_bytes());
        bytes[31..39].copy_from_slice(&1.0f64.to_le_bytes());
        write_tagged_u32(&mut bytes, 39, 2);
        write_tagged_u32(&mut bytes, 44, 1000);
        bytes[50..58].copy_from_slice(&0.0f64.to_le_bytes());
        bytes[58..66].copy_from_slice(&1.0f64.to_le_bytes());
        write_tagged_u32(&mut bytes, 66, 2);
        write_tagged_u32(&mut bytes, 71, 1000);

        assert!(zero_entity_nurbs_layout(&bytes, 0).is_none());
    }

    #[test]
    fn nurbs_surface_control_count_has_one_codec_wide_ceiling() {
        assert_eq!(
            crate::nurbs_surface_control_count(1000, 1000),
            Some(1_000_000)
        );
        assert_eq!(crate::nurbs_surface_control_count(1001, 1000), None);
        assert_eq!(crate::nurbs_surface_control_count(usize::MAX, 2), None);
    }

    fn support_pcurve_record(tag: u8) -> Vec<u8> {
        let logical_len = if tag == 0x18 {
            292
        } else {
            zero_entity_fixed_logical_length([0x21, tag]).unwrap_or(usize::from(tag) + 12)
        };
        let mut record = vec![0u8; logical_len];
        record[..4].copy_from_slice(&[0xa9, 0x03, 0x21, tag]);
        write_tagged_u32(&mut record, 12, 1);
        let (knots, multiplicities, control_count, rational): (&[_], &[_], usize, bool) = match tag
        {
            0x18 => (&[0.0, 0.25, 0.5, 0.75, 1.0], &[4, 2, 2, 2, 4], 10, false),
            0x45 => (
                &[0.0, 0.2, 0.4, 0.6, 0.8, 1.0],
                &[4, 2, 2, 2, 2, 4],
                12,
                false,
            ),
            0x71 => (&[0.0, 1.0], &[2, 2], 2, false),
            0x72 => (
                &[0.0, 1.0 / 6.0, 2.0 / 6.0, 0.5, 4.0 / 6.0, 5.0 / 6.0, 1.0],
                &[4, 2, 2, 2, 2, 2, 4],
                14,
                false,
            ),
            0x91 => (&[0.0, 1.0], &[4, 4], 4, false),
            0x99 => (&[0.0, 1.0], &[3, 3], 3, true),
            0x9f => (
                &[
                    0.0,
                    1.0 / 7.0,
                    2.0 / 7.0,
                    3.0 / 7.0,
                    4.0 / 7.0,
                    5.0 / 7.0,
                    6.0 / 7.0,
                    1.0,
                ],
                &[4, 2, 2, 2, 2, 2, 2, 4],
                16,
                false,
            ),
            0xd6 => (&[0.0, 0.5, 1.0], &[3, 2, 3], 5, false),
            0xe8 => (&[0.0, 0.25, 0.5, 0.75, 1.0], &[4, 1, 1, 1, 4], 7, false),
            _ => unreachable!(),
        };
        let knot_start = 67;
        for (index, knot) in knots.iter().enumerate() {
            record[knot_start + index * 8..knot_start + (index + 1) * 8]
                .copy_from_slice(&f64::to_le_bytes(*knot));
        }
        let multiplicity_start = knot_start + knots.len() * 8;
        for (index, multiplicity) in multiplicities.iter().copied().enumerate() {
            write_tagged_u32(&mut record, multiplicity_start + index * 5, multiplicity);
        }
        let pole_start = multiplicity_start + multiplicities.len() * 5;
        let points = (0..control_count)
            .map(|index| {
                let parameter = index as f64 / (control_count - 1) as f64;
                [parameter, parameter * (1.0 - parameter)]
            })
            .collect::<Vec<_>>();
        for (index, point) in points.iter().enumerate() {
            let at = pole_start + index * 16;
            record[at..at + 8].copy_from_slice(&f64::to_le_bytes(point[0]));
            record[at + 8..at + 16].copy_from_slice(&f64::to_le_bytes(point[1]));
        }
        if rational {
            let weight_start = pole_start + points.len() * 16;
            for (index, weight) in (0..control_count)
                .map(|index| if index == control_count / 2 { 0.5 } else { 1.0 })
                .enumerate()
            {
                let at = weight_start + index * 8;
                record[at..at + 8].copy_from_slice(&f64::to_le_bytes(weight));
            }
        }
        record
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
        assert_eq!(
            support.pcurve,
            Some(test_pcurve(vec![
                Point2::new(-2.0, 4.0),
                Point2::new(6.0, 8.0),
            ]))
        );
        assert!(matches!(
            support.model_curve,
            Some(CurveGeometry::Nurbs(ref curve))
                if curve.degree() == 1
                    && curve.weights().is_none()
                    && !curve.periodic()
                    && curve.control_points()
                == &[Point3::new(-1.0, 6.0, 3.0), Point3::new(7.0, 10.0, 3.0)]
        ));
        assert_eq!(support.model_parameters, Some([0.0, 1.0]));
        assert_eq!(support.model_midpoint, Some(Point3::new(3.0, 8.0, 3.0)));
        assert_eq!(
            support.model_endpoints,
            Some([Point3::new(-1.0, 6.0, 3.0), Point3::new(7.0, 10.0, 3.0)])
        );
    }

    #[test]
    fn support_pcurves_decode_each_complete_clamped_family() {
        for (tag, degree, control_count, rational) in [
            (0x18, 3, 10, false),
            (0x45, 3, 12, false),
            (0x71, 1, 2, false),
            (0x72, 3, 14, false),
            (0x91, 3, 4, false),
            (0x99, 2, 3, true),
            (0x9f, 3, 16, false),
            (0xd6, 2, 5, false),
            (0xe8, 3, 7, false),
        ] {
            let bytes = support_pcurve_record(tag);
            let records = zero_entity_records(&bytes);
            let [record] = records.as_slice() else {
                panic!("one support record")
            };
            let support =
                zero_entity_support_occurrence(&bytes, *record).expect("complete support pcurve");
            assert_eq!(support.uv_endpoints, Some([[0.0, 0.0], [1.0, 0.0]]));
            let Some(PcurveGeometry::Nurbs { nurbs }) = support.pcurve else {
                panic!("NURBS support pcurve")
            };
            assert_eq!(nurbs.degree(), degree);
            assert_eq!(nurbs.control_points().len(), control_count);
            assert_eq!(nurbs.weights().is_some(), rational);

            let multiplicity_start = match tag {
                0x18 => 107,
                0x45 => 115,
                0x71 | 0x91 | 0x99 => 83,
                0x72 => 123,
                0x9f => 131,
                0xd6 => 91,
                0xe8 => 107,
                _ => unreachable!(),
            };
            let mut malformed = bytes;
            malformed[multiplicity_start + 1] ^= 1;
            let malformed_records = zero_entity_records(&malformed);
            let [record] = malformed_records.as_slice() else {
                panic!("one malformed support record")
            };
            assert!(zero_entity_support_occurrence(&malformed, *record).is_none());
        }
    }

    #[test]
    fn rational_support_pcurve_rejects_a_nonpositive_weight_atomically() {
        let mut bytes = support_pcurve_record(0x99);
        bytes[149..157].copy_from_slice(&0.0f64.to_le_bytes());
        let records = zero_entity_records(&bytes);
        let [record] = records.as_slice() else {
            panic!("one support record")
        };
        assert!(zero_entity_support_occurrence(&bytes, *record).is_none());
    }

    #[test]
    fn affine_cone_pcurve_lifts_to_an_exact_conical_helix() {
        use cadmpeg_ir::math::Vector3;

        let half_angle = 0.25;
        let surface = SurfaceGeometry::Cone {
            origin: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
            ratio: 1.0,
            half_angle,
        };
        let pcurve = test_pcurve(vec![Point2::new(0.0, 1.0), Point2::new(0.5, 2.0)]);
        let Some(ProceduralCurveDefinition::Helix {
            angle_range,
            center,
            major,
            minor,
            pitch,
            apex_factor,
            axis,
        }) = zero_entity_model_curve_construction(&surface, &pcurve)
        else {
            panic!("conical helix")
        };
        let slope = 2.0;
        let start_radius = 2.0 + half_angle.sin();
        assert_eq!(angle_range, [0.0, 0.5]);
        assert_eq!(center, Point3::new(1.0, 2.0, 3.0 + half_angle.cos()));
        assert_eq!(major, Vector3::new(start_radius, 0.0, 0.0));
        assert_eq!(minor, Vector3::new(0.0, start_radius, 0.0));
        assert_eq!(
            pitch,
            Vector3::new(0.0, 0.0, std::f64::consts::TAU * slope * half_angle.cos())
        );
        assert_eq!(
            apex_factor,
            std::f64::consts::TAU * slope * half_angle.sin() / start_radius
        );
        assert_eq!(axis, Vector3::new(0.0, 0.0, 1.0));

        let revolution_fraction = angle_range[1] / std::f64::consts::TAU;
        let end_radius = start_radius * (1.0 + apex_factor * revolution_fraction);
        assert!((end_radius - (2.0 + 2.0 * half_angle.sin())).abs() < 1.0e-12);
        assert!(
            (center.z + pitch.z * revolution_fraction - (3.0 + 2.0 * half_angle.cos())).abs()
                < 1.0e-12
        );
        for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let angle = angle_range[0] + fraction * (angle_range[1] - angle_range[0]);
            let revolution_fraction = (angle - angle_range[0]) / std::f64::consts::TAU;
            let radial_scale = 1.0 + apex_factor * revolution_fraction;
            let construction_point = Point3::new(
                center.x
                    + radial_scale * (major.x * angle.cos() + minor.x * angle.sin())
                    + pitch.x * revolution_fraction,
                center.y
                    + radial_scale * (major.y * angle.cos() + minor.y * angle.sin())
                    + pitch.y * revolution_fraction,
                center.z
                    + radial_scale * (major.z * angle.cos() + minor.z * angle.sin())
                    + pitch.z * revolution_fraction,
            );
            let surface_point = zero_entity_surface_point(&surface, [angle, 1.0 + fraction])
                .expect("finite cone point");
            assert!((construction_point.x - surface_point.x).abs() < 1.0e-12);
            assert!((construction_point.y - surface_point.y).abs() < 1.0e-12);
            assert!((construction_point.z - surface_point.z).abs() < 1.0e-12);
        }
    }

    #[test]
    fn negative_cone_latitude_radius_flips_the_circle_reference_direction() {
        use cadmpeg_ir::eval::curve_point;
        use cadmpeg_ir::math::Vector3;

        let surface = SurfaceGeometry::Cone {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 1.0,
            ratio: 1.0,
            half_angle: std::f64::consts::FRAC_PI_4,
        };
        let endpoints = [[0.0, -2.0], [1.0, -2.0]];
        let pcurve = test_pcurve(
            endpoints
                .map(|[u, v]| Point2::new(u, v))
                .into_iter()
                .collect(),
        );
        let (curve, parameters) =
            zero_entity_model_curve(&surface, &pcurve, endpoints).expect("cone latitude");
        for index in 0..2 {
            let curve_point = curve_point(&curve, parameters[index]).expect("circle point");
            let surface_point =
                zero_entity_surface_point(&surface, endpoints[index]).expect("cone point");
            assert!((curve_point.x - surface_point.x).abs() < 1.0e-12);
            assert!((curve_point.y - surface_point.y).abs() < 1.0e-12);
            assert!((curve_point.z - surface_point.z).abs() < 1.0e-12);
        }
    }

    #[test]
    fn analytic_support_endpoints_use_native_surface_charts() {
        use cadmpeg_ir::math::Vector3;

        let origin = Point3::new(0.0, 0.0, 0.0);
        let x = Vector3::new(1.0, 0.0, 0.0);
        let z = Vector3::new(0.0, 0.0, 1.0);
        let cylinder = SurfaceGeometry::Cylinder {
            origin,
            axis: z,
            ref_direction: x,
            radius: 2.0,
        };
        let cone = SurfaceGeometry::Cone {
            origin,
            axis: z,
            ref_direction: x,
            radius: 2.0,
            ratio: 1.0,
            half_angle: std::f64::consts::FRAC_PI_4,
        };
        let torus = SurfaceGeometry::Torus {
            center: origin,
            axis: z,
            ref_direction: x,
            major_radius: 4.0,
            minor_radius: 2.0,
        };

        let cylinder_point =
            zero_entity_surface_point(&cylinder, [std::f64::consts::PI, 3.0]).expect("cylinder");
        let cone_point =
            zero_entity_surface_point(&cone, [std::f64::consts::FRAC_PI_2, 3.0]).expect("cone");
        let torus_point =
            zero_entity_surface_point(&torus, [2.0 * std::f64::consts::PI, std::f64::consts::PI])
                .expect("torus");

        assert!(cylinder_point.x.abs() < 1.0e-12);
        assert!((cylinder_point.y - 2.0).abs() < 1.0e-12);
        assert_eq!(cylinder_point.z, 3.0);
        assert!(cone_point.x.abs() < 1.0e-12);
        assert!((cone_point.y - (2.0 + 3.0 / 2.0_f64.sqrt())).abs() < 1.0e-12);
        assert!((cone_point.z - 3.0 / 2.0_f64.sqrt()).abs() < 1.0e-12);
        assert!(torus_point.x.abs() < 1.0e-12);
        assert!((torus_point.y - 4.0).abs() < 1.0e-12);
        assert_eq!(torus_point.z, 2.0);
    }

    #[test]
    fn support_pcurve_conversion_uses_the_neutral_surface_chart() {
        use cadmpeg_ir::math::Vector3;

        let pcurve = test_pcurve(vec![Point2::new(2.0, 3.0), Point2::new(4.0, 5.0)]);
        let cylinder = SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        assert_eq!(
            zero_entity_neutral_pcurve(&cylinder, &pcurve),
            Some(test_pcurve(vec![
                Point2::new(1.0, 3.0),
                Point2::new(2.0, 5.0),
            ]))
        );

        let cone = SurfaceGeometry::Cone {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
            ratio: 1.0,
            half_angle: 0.25,
        };
        let Some(PcurveGeometry::Nurbs { nurbs }) = zero_entity_neutral_pcurve(&cone, &pcurve)
        else {
            panic!("neutral cone pcurve")
        };
        assert_eq!(nurbs.control_points()[0].u, 2.0);
        assert_eq!(nurbs.control_points()[0].v, 3.0 * 0.25_f64.cos());

        let torus = SurfaceGeometry::Torus {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: 4.0,
            minor_radius: 2.0,
        };
        let Some(PcurveGeometry::Nurbs { nurbs }) = zero_entity_neutral_pcurve(&torus, &pcurve)
        else {
            panic!("neutral torus pcurve")
        };
        assert_eq!(nurbs.control_points()[1], Point2::new(1.0, 2.5));
    }

    #[test]
    fn analytic_carriers_have_no_model_size_cutoff() {
        let mut cylinder = vec![0_u8; 89];
        cylinder[33..41].copy_from_slice(&1.0_f64.to_le_bytes());
        cylinder[65..73].copy_from_slice(&1.0_f64.to_le_bytes());
        cylinder[81..89].copy_from_slice(&2_000_000.0_f64.to_le_bytes());
        assert!(matches!(
            zero_entity_cylinder(&cylinder),
            Some(SurfaceGeometry::Cylinder {
                radius: 2_000_000.0,
                ..
            })
        ));

        let mut cone = vec![0_u8; 120];
        cone[32..40].copy_from_slice(&1.0_f64.to_le_bytes());
        cone[96..104].copy_from_slice(&1.0_f64.to_le_bytes());
        cone[104..112].copy_from_slice(&std::f64::consts::FRAC_PI_4.to_le_bytes());
        cone[112..120].copy_from_slice(&2_000_000.0_f64.to_le_bytes());
        assert!(matches!(
            zero_entity_cone(&cone),
            Some(SurfaceGeometry::Cone {
                radius: 2_000_000.0,
                ..
            })
        ));

        let mut torus = vec![0_u8; 120];
        torus[32..40].copy_from_slice(&1.0_f64.to_le_bytes());
        torus[96..104].copy_from_slice(&1.0_f64.to_le_bytes());
        torus[104..112].copy_from_slice(&2_000_000.0_f64.to_le_bytes());
        torus[112..120].copy_from_slice(&1_500_000.0_f64.to_le_bytes());
        assert!(matches!(
            zero_entity_torus(&torus),
            Some(SurfaceGeometry::Torus {
                major_radius: 2_000_000.0,
                minor_radius: 1_500_000.0,
                ..
            })
        ));
    }

    #[test]
    fn oriented_endpoint_tape_closes_one_missing_occurrence() {
        let first = Point3::new(1.0, 0.0, 0.0);
        let second = Point3::new(0.0, 1.0, 0.0);
        let third = Point3::new(0.0, 0.0, 1.0);
        let endpoints = [Some([first, second]), Some([third, second]), None];

        assert_eq!(
            oriented_closed_model_endpoints(&endpoints, &[true, false, true]),
            Some(vec![[first, second], [second, third], [third, first],])
        );
        assert!(oriented_closed_model_endpoints(&[endpoints[0], None, None], &[true; 3]).is_none());
        assert!(oriented_closed_model_endpoints(
            &[
                Some([first, second]),
                Some([Point3::new(1.0, 1.0, 0.0), first]),
            ],
            &[true; 2],
        )
        .is_none());
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
        let mut support = support_pcurve_record(0x45);
        support[13..17].copy_from_slice(&42u32.to_le_bytes());
        support[200..204].copy_from_slice(&[0xa9, 0x03, 0x27, 0x6a]);

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
    fn ownership_root_extends_the_counted_face_roster_and_binds_shell_and_body() {
        let stream = zero_entity_ownership_stream(62);
        let records = zero_entity_record_inventory(&stream);
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].tag, [0x61, 0x42]);
        assert_eq!(records[0].end, 13 + 62 * 5 + 11);
        assert_eq!(records[0].end, records[1].pos);

        let root = zero_entity_ownership_root(&stream).expect("complete ownership root");
        assert_eq!(root.face_roster_record_ordinal, 1);
        assert_eq!(root.face_slots, (1..=62).rev().collect::<Vec<_>>());
        assert_eq!(root.shell_record_ordinal(), 2);
        assert_eq!(root.body_record_ordinal(), 3);
    }

    #[test]
    fn ownership_root_requires_one_complete_candidate_for_unique_selection() {
        let first = zero_entity_ownership_stream(3);
        let mut stream = first.clone();
        stream.extend(first);

        let roots = zero_entity_ownership_roots(&stream);
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].face_roster_record_ordinal, 1);
        assert_eq!(roots[1].face_roster_record_ordinal, 4);
        assert!(zero_entity_ownership_root(&stream).is_none());
    }

    #[test]
    fn ownership_root_rejects_noncanonical_face_allocations_atomically() {
        let mut stream = zero_entity_ownership_stream(3);
        write_tagged_u32(&mut stream, 13, 2);
        assert!(zero_entity_record_inventory(&stream).is_empty());
        assert!(zero_entity_ownership_root(&stream).is_none());
    }

    #[test]
    fn topology_records_retain_global_ordinals_and_separate_namespaces() {
        let stream = zero_entity_topology_stream();
        let edge_strides = zero_entity_edge_strides(&stream);
        let [edge_stride] = edge_strides.as_slice() else {
            panic!("one edge stride")
        };
        assert_eq!(edge_stride.record_ordinal, 1);
        assert_eq!(edge_stride.allocations, [5, 7, 8, 4, 3]);
        assert_eq!(edge_stride.topology_refs, [5, 4, 3]);
        assert_eq!(edge_stride.surface_support_refs, [7, 8]);

        let pairs = zero_entity_oriented_use_pairs(&stream);
        let [pair] = pairs.as_slice() else {
            panic!("one oriented-use pair")
        };
        assert_eq!(pair.header_record_ordinal, 2);
        assert_eq!(pair.base_columns, [100, 200]);
        assert_eq!(pair.uses[0].record_ordinal, 3);
        assert_eq!(pair.uses[0].allocations, [101, 201]);
        assert_eq!(pair.uses[1].record_ordinal, 4);
        assert_eq!(pair.uses[1].allocations, [102, 202]);

        let incidences = zero_entity_vertex_incidences(&stream);
        let [incidence] = incidences.as_slice() else {
            panic!("one vertex incidence")
        };
        assert_eq!(incidence.record_ordinal, 5);
        assert_eq!(incidence.tag, [0x05, 0x10]);
        assert_eq!(incidence.allocations, [1, 2, 5]);
    }

    #[test]
    fn vertex_incidence_requires_the_complete_adjacent_owner_production() {
        for owner_offset in [7usize, 12, 17] {
            let mut stream = zero_entity_topology_stream();
            let owner = zero_entity_records(&stream)[5];
            stream[owner.pos + owner_offset] ^= 1;
            assert!(zero_entity_vertex_incidences(&stream).is_empty());
        }
    }

    #[test]
    fn oriented_use_pair_requires_adjacent_ordered_sides() {
        let mut stream = zero_entity_topology_stream();
        let second_use = 38 + (0x69 + 12) + (0x38 + 12);
        write_tagged_u32(&mut stream, second_use + 13, 1);
        assert!(zero_entity_oriented_use_pairs(&stream).is_empty());
    }

    #[test]
    fn oriented_use_allocations_must_extend_the_header_columns() {
        let mut stream = zero_entity_topology_stream();
        let first_use = 38 + 0x69 + 12;
        write_tagged_u32(&mut stream, first_use + 18, 102);
        assert!(zero_entity_oriented_use_pairs(&stream).is_empty());
    }

    #[test]
    fn topology_allocation_lanes_are_not_global_record_ordinals() {
        let mut stream = zero_entity_topology_stream();
        let header = 38;
        let second_use = header + (0x69 + 12) + (0x38 + 12);
        let incidence = second_use + 0x38 + 12;
        for (offset, value) in [
            (incidence + 13, 105),
            (incidence + 18, 106),
            (incidence + 23, 107),
        ] {
            write_tagged_u32(&mut stream, offset, value);
        }

        let pairs = zero_entity_oriented_use_pairs(&stream);
        let [pair] = pairs.as_slice() else {
            panic!("one oriented-use pair")
        };
        assert_eq!(pair.uses[0].allocations, [101, 201]);
        assert_eq!(pair.uses[1].allocations, [102, 202]);
        let incidences = zero_entity_vertex_incidences(&stream);
        let [incidence] = incidences.as_slice() else {
            panic!("one vertex incidence")
        };
        assert_eq!(incidence.allocations, [105, 106, 107]);
    }

    #[test]
    fn edge_stride_requires_its_fixed_tagged_one_prefix() {
        let mut stream = zero_entity_topology_stream();
        write_tagged_u32(&mut stream, 7, 2);
        assert!(zero_entity_edge_strides(&stream).is_empty());
    }

    #[test]
    fn edge_stride_rejects_a_zero_allocation() {
        let mut stream = zero_entity_topology_stream();
        write_tagged_u32(&mut stream, 12, 0);
        assert!(zero_entity_edge_strides(&stream).is_empty());
    }

    #[test]
    fn edge_stride_requires_its_descending_terminal_tail() {
        let mut stream = zero_entity_topology_stream();
        write_tagged_u32(&mut stream, 27, 3);
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
        assert_eq!(face.loop_terminals(), [7]);
        assert_eq!(face.terminal_control, ZeroEntityFaceControl::Control05);
    }

    #[test]
    fn face_roster_admits_only_the_two_terminal_controls() {
        for control in [0x03, 0x05] {
            let mut stream = zero_entity_face_support_stream();
            *stream.last_mut().expect("face terminal") = control;
            assert_eq!(
                zero_entity_support_runs(&stream)[0]
                    .face
                    .as_ref()
                    .expect("admitted face")
                    .terminal_control
                    .as_byte(),
                control
            );
        }

        let mut stream = zero_entity_face_support_stream();
        *stream.last_mut().expect("face terminal") = 0x04;
        assert!(zero_entity_support_runs(&stream)[0].face.is_none());
    }

    #[test]
    fn face_roster_requires_nonzero_allocation_lanes() {
        for allocation_offset in [13usize, 18] {
            let mut stream = zero_entity_face_support_stream();
            let face = zero_entity_records(&stream)[2];
            write_tagged_u32(&mut stream, face.pos + allocation_offset, 0);
            assert!(zero_entity_support_runs(&stream)[0].face.is_none());
        }
    }

    #[test]
    fn face_roster_requires_positive_loop_terminals() {
        let mut stream = zero_entity_face_support_stream();
        let face = zero_entity_records(&stream)[2];
        write_tagged_u32(&mut stream, face.pos + 18, 10);
        assert!(zero_entity_support_runs(&stream)[0].face.is_none());
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

    #[test]
    fn independently_incomplete_rosters_do_not_shift_face_bindings() {
        let mut stream = zero_entity_support_stream();
        stream[38..62].fill(0);
        stream.extend(zero_entity_support_stream());

        let face_stream = zero_entity_face_support_stream();
        let face_start = zero_entity_support_stream().len();
        let valid_face = face_stream[face_start..].to_vec();
        stream.extend_from_slice(&valid_face);
        let mut invalid_face = valid_face;
        *invalid_face.last_mut().expect("face terminal") = 0x04;
        stream.extend(invalid_face);

        let runs = zero_entity_support_runs(&stream);
        let [run] = runs.as_slice() else {
            panic!("one complete support run")
        };
        assert_eq!(run.carrier_record_ordinal, 3);
        assert!(run.face.is_none());
    }

    #[test]
    fn complete_loop_roster_aligns_to_face_terminals() {
        let runs = zero_entity_support_runs(&zero_entity_face_loop_support_stream());
        let [run] = runs.as_slice() else {
            panic!("one support run")
        };
        let face = run.face.as_ref().expect("face");
        let [loop_record] = face.loops.as_deref().unwrap_or(&[]) else {
            panic!("one loop")
        };
        assert_eq!(loop_record.record_ordinal, 4);
        assert_eq!(loop_record.tag, [0x62, 0x14]);
        assert_eq!(loop_record.member_ids, [6]);
        assert_eq!(loop_record.typed_references, [1]);
        assert_eq!(loop_record.terminal_id, 7);
        assert_eq!(loop_record.gap, 1);
        assert_eq!(loop_record.loop_class, 0x41);
        assert_eq!(loop_record.forward_senses, [true]);
        assert!(loop_record.support_record_ordinals.is_empty());
    }

    #[test]
    fn loop_roster_requires_one_leading_outer_loop() {
        let mut stream = zero_entity_face_loop_support_stream();
        let loop_record = zero_entity_records(&stream)[3];
        stream[loop_record.end - 3] = 0x50;
        let runs = zero_entity_support_runs(&stream);
        let face = runs[0].face.as_ref().expect("face");
        assert!(face.loops.is_none());
    }

    #[test]
    fn face_roster_requires_strictly_ascending_inner_terminals() {
        let mut stream = zero_entity_support_stream();
        let mut face = vec![0u8; 0x16 + 12];
        face[..4].copy_from_slice(&[0xa9, 0x03, 0x5f, 0x16]);
        write_tagged_u32(&mut face, 7, 1);
        face[12] = 0x84;
        for (index, allocation) in [20, 13, 10, 11].into_iter().enumerate() {
            write_tagged_u32(&mut face, 13 + index * 5, allocation);
        }
        face[33] = 0x05;
        stream.extend(face);
        for (terminal, class) in [(7, 0x41), (10, 0x50), (9, 0x50)] {
            let mut loop_record = vec![0u8; 0x14 + 12];
            loop_record[..4].copy_from_slice(&[0xa9, 0x03, 0x62, 0x14]);
            loop_record[12] = 0x83;
            for (index, value) in [terminal - 1, 1, terminal].into_iter().enumerate() {
                write_tagged_u32(&mut loop_record, 13 + index * 5, value);
            }
            loop_record[28..].copy_from_slice(&[0x81, class, 0x07, 0x01]);
            stream.extend(loop_record);
        }

        let runs = zero_entity_support_runs(&stream);
        assert!(runs[0].face.is_none());
    }

    #[test]
    fn loop_member_lane_must_remain_strictly_below_its_terminal() {
        let mut stream = zero_entity_face_loop_support_stream();
        let loop_record = zero_entity_records(&stream)[3];
        write_tagged_u32(&mut stream, loop_record.pos + 23, 6);
        assert!(zero_entity_loops_from_records(&stream, &zero_entity_records(&stream)).is_empty());
    }

    #[test]
    fn loop_typed_lane_requires_one_based_record_references() {
        let mut stream = zero_entity_face_loop_support_stream();
        let loop_record = zero_entity_records(&stream)[3];
        write_tagged_u32(&mut stream, loop_record.pos + 18, 0);
        assert!(zero_entity_loops_from_records(&stream, &zero_entity_records(&stream)).is_empty());
    }

    #[test]
    fn support_occurrence_requires_a_nonzero_face_local_slot() {
        let mut stream = zero_entity_face_support_stream();
        let support = zero_entity_records(&stream)[1];
        stream[support.pos + 13..support.pos + 17].copy_from_slice(&0u32.to_le_bytes());
        assert!(zero_entity_support_occurrence(&stream, support).is_none());
    }

    #[test]
    fn loop_members_bind_unique_face_local_support_slots() {
        let mut stream = zero_entity_face_loop_support_stream();
        let support_slot = 0x6a + 12 + 13;
        stream[support_slot..support_slot + 4].copy_from_slice(&1u32.to_le_bytes());

        let runs = zero_entity_support_runs(&stream);
        let face = runs[0].face.as_ref().expect("face");
        assert_eq!(
            face.loops.as_ref().expect("aligned loops")[0].support_record_ordinals,
            [2]
        );
    }

    #[test]
    fn loop_support_binding_rejects_duplicate_face_local_slots_atomically() {
        let mut stream = zero_entity_face_loop_support_stream();
        let carrier_length = 0x6a + 12;
        let support_length = 0x71 + 12;
        let support = stream[carrier_length..carrier_length + support_length].to_vec();
        stream.splice(
            carrier_length + support_length..carrier_length + support_length,
            support,
        );
        let support_slot = carrier_length + 13;
        stream[support_slot..support_slot + 4].copy_from_slice(&1u32.to_le_bytes());
        let duplicate_slot = carrier_length + support_length + 13;
        stream[duplicate_slot..duplicate_slot + 4].copy_from_slice(&1u32.to_le_bytes());

        let runs = zero_entity_support_runs(&stream);
        let face = runs[0].face.as_ref().expect("face");
        assert!(face.loops.as_ref().expect("aligned loops")[0]
            .support_record_ordinals
            .is_empty());
    }

    #[test]
    fn invalid_loop_tape_does_not_partially_bind_faces() {
        let mut stream = zero_entity_face_loop_support_stream();
        *stream.last_mut().expect("loop terminator") = 0;
        let runs = zero_entity_support_runs(&stream);
        let face = runs[0].face.as_ref().expect("face");
        assert!(face.loops.is_none());
    }
}
