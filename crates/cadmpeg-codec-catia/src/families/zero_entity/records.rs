//! Zero-entity `a9 03` stream surface decoders.
//!
//! Decodes analytic (plane, cylinder, cone, torus) and inline non-rational
//! NURBS surface carriers from a zero-entity record stream.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::eval::nurbs_surface_point;
use cadmpeg_ir::geometry::{NurbsSurface, PcurveGeometry, SurfaceGeometry};
use cadmpeg_ir::le::u32_at as u32_le;
use cadmpeg_ir::math::{Point2, Point3};

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
    /// Ordered loop terminals `allocations[0] - allocations[1..]`.
    pub loop_terminals: Vec<u32>,
    /// Positionally aligned loop records when the complete flattened roster agrees.
    pub loops: Vec<ZeroEntityLoop>,
    /// Terminal control byte following the allocation lane.
    pub terminal_control: u8,
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
    /// One-based global record ordinal of the shell root.
    pub shell_record_ordinal: u32,
    /// Offset of the immediately following `6508` body root.
    pub body_pos: usize,
    /// One-based global record ordinal of the body root.
    pub body_record_ordinal: u32,
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
        let end = if tag == [0x61, 0x42] {
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

/// Decode the terminal face-roster, shell, and body ownership roots.
#[must_use]
pub fn zero_entity_ownership_root(data: &[u8]) -> Option<ZeroEntityOwnershipRoot> {
    let records = zero_entity_records(data);
    records.windows(3).find_map(|window| {
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
            shell_record_ordinal: shell.ordinal,
            body_pos: body.pos,
            body_record_ordinal: body.ordinal,
        })
    })
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
        .flat_map(|face| face.loop_terminals.iter().copied())
        .collect::<Vec<_>>();
    if flattened_terminals
        == loops
            .iter()
            .map(|loop_record| loop_record.terminal_id)
            .collect::<Vec<_>>()
    {
        let mut loop_index = 0;
        for face in &mut faces {
            let loop_end = loop_index + face.loop_terminals.len();
            face.loops.extend_from_slice(&loops[loop_index..loop_end]);
            loop_index = loop_end;
        }
    }
    if faces.len() == runs.len() {
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
    if face.loops.len() != face.loop_terminals.len() {
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
        .iter()
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
    for (loop_record, support_record_ordinals) in face.loops.iter_mut().zip(bindings) {
        loop_record.support_record_ordinals = support_record_ordinals;
    }
    let supports_by_ordinal = supports
        .iter()
        .map(|support| (support.record_ordinal, support))
        .collect::<HashMap<_, _>>();
    for loop_record in &mut face.loops {
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
                loops: Vec::new(),
                terminal_control: *data.get(record.end - 1)?,
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
            let terminal_id = *references.last()?;
            let gap = terminal_id.checked_sub(*member_ids.first()?)?;
            if !member_ids.iter().enumerate().all(|(index, member)| {
                u32::try_from(index)
                    .ok()
                    .and_then(|index| terminal_id.checked_sub(gap)?.checked_sub(index))
                    == Some(*member)
            }) {
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
    let pcurve = zero_entity_support_pcurve(data, record);
    if matches!(record.tag, [0x21, 0x71 | 0x91 | 0x99 | 0xd6]) && pcurve.is_none() {
        return None;
    }
    Some(ZeroEntitySupportOccurrence {
        pos: record.pos,
        record_ordinal: record.ordinal,
        tag: record.tag,
        face_local_slot,
        uv_endpoints,
        pcurve,
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
        [0x21, 0x71] => (&[67, 75][..], 83, &[2, 2][..], 93, 2, None, 125),
        [0x21, 0x91] => (&[67, 75][..], 83, &[4, 4][..], 93, 4, None, 157),
        [0x21, 0x99] => (&[67, 75][..], 83, &[3, 3][..], 93, 3, Some(141), 165),
        [0x21, 0xd6] => (&[67, 75, 83][..], 91, &[3, 2, 3][..], 106, 5, None, 186),
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
        degree,
        knots,
        control_points,
        weights,
        periodic: false,
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
        zero_entity_face_loop_support_stream, zero_entity_face_support_stream,
        zero_entity_ownership_stream, zero_entity_support_stream, zero_entity_topology_stream,
    };

    fn write_tagged_u32(record: &mut [u8], at: usize, value: u32) {
        record[at] = 0x10;
        record[at + 1..at + 5].copy_from_slice(&value.to_le_bytes());
    }

    fn support_pcurve_record(tag: u8) -> Vec<u8> {
        let mut record = vec![0u8; usize::from(tag) + 12];
        record[..4].copy_from_slice(&[0xa9, 0x03, 0x21, tag]);
        write_tagged_u32(&mut record, 12, 1);
        let (knots, multiplicities, pole_start, points, weights): (
            &[_],
            &[_],
            usize,
            &[_],
            Option<&[_]>,
        ) = match tag {
            0x91 => (
                &[0.0, 1.0],
                &[4, 4],
                93,
                &[[0.0, 0.0], [0.25, 0.5], [0.75, 0.5], [1.0, 1.0]],
                None,
            ),
            0x99 => (
                &[0.0, 1.0],
                &[3, 3],
                93,
                &[[0.0, 0.0], [0.5, 1.0], [1.0, 0.0]],
                Some(&[1.0, 0.5, 1.0]),
            ),
            0xd6 => (
                &[0.0, 0.5, 1.0],
                &[3, 2, 3],
                106,
                &[[0.0, 0.0], [0.25, 0.5], [0.5, 1.0], [0.75, 0.5], [1.0, 0.0]],
                None,
            ),
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
        for (index, point) in points.iter().enumerate() {
            let at = pole_start + index * 16;
            record[at..at + 8].copy_from_slice(&f64::to_le_bytes(point[0]));
            record[at + 8..at + 16].copy_from_slice(&f64::to_le_bytes(point[1]));
        }
        if let Some(weights) = weights {
            let weight_start = pole_start + points.len() * 16;
            for (index, weight) in weights.iter().enumerate() {
                let at = weight_start + index * 8;
                record[at..at + 8].copy_from_slice(&f64::to_le_bytes(*weight));
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
            Some(PcurveGeometry::Nurbs {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![Point2::new(-2.0, 4.0), Point2::new(6.0, 8.0)],
                weights: None,
                periodic: false,
            })
        );
        assert_eq!(
            support.model_endpoints,
            Some([Point3::new(-1.0, 6.0, 3.0), Point3::new(7.0, 10.0, 3.0)])
        );
    }

    #[test]
    fn support_pcurves_decode_each_complete_clamped_family() {
        for (tag, degree, control_count, rational) in
            [(0x91, 3, 4, false), (0x99, 2, 3, true), (0xd6, 2, 5, false)]
        {
            let bytes = support_pcurve_record(tag);
            let records = zero_entity_records(&bytes);
            let [record] = records.as_slice() else {
                panic!("one support record")
            };
            let support =
                zero_entity_support_occurrence(&bytes, *record).expect("complete support pcurve");
            let Some(PcurveGeometry::Nurbs {
                degree: actual_degree,
                control_points,
                weights,
                ..
            }) = support.pcurve
            else {
                panic!("NURBS support pcurve")
            };
            assert_eq!(actual_degree, degree);
            assert_eq!(control_points.len(), control_count);
            assert_eq!(weights.is_some(), rational);
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

        assert!(cylinder_point.x.abs() < 1e-12);
        assert!((cylinder_point.y - 2.0).abs() < 1e-12);
        assert_eq!(cylinder_point.z, 3.0);
        assert!(cone_point.x.abs() < 1e-12);
        assert!((cone_point.y - (2.0 + 3.0 / 2.0_f64.sqrt())).abs() < 1e-12);
        assert!((cone_point.z - 3.0 / 2.0_f64.sqrt()).abs() < 1e-12);
        assert!(torus_point.x.abs() < 1e-12);
        assert!((torus_point.y - 4.0).abs() < 1e-12);
        assert_eq!(torus_point.z, 2.0);
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
        let mut support = vec![
            0u8;
            zero_entity_fixed_logical_length([0x21, 0x45])
                .expect("class 2145 has a fixed logical length")
        ];
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
        assert_eq!(root.shell_record_ordinal, 2);
        assert_eq!(root.body_record_ordinal, 3);
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
        assert_eq!(edge_stride.references, [1, 2, 7, 8, 5, 1]);

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

    #[test]
    fn complete_loop_roster_aligns_to_face_terminals() {
        let runs = zero_entity_support_runs(&zero_entity_face_loop_support_stream());
        let [run] = runs.as_slice() else {
            panic!("one support run")
        };
        let face = run.face.as_ref().expect("face");
        let [loop_record] = face.loops.as_slice() else {
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
    fn loop_members_bind_unique_face_local_support_slots() {
        let mut stream = zero_entity_face_loop_support_stream();
        let support_slot = 0x6a + 12 + 13;
        stream[support_slot..support_slot + 4].copy_from_slice(&1u32.to_le_bytes());

        let runs = zero_entity_support_runs(&stream);
        let face = runs[0].face.as_ref().expect("face");
        assert_eq!(face.loops[0].support_record_ordinals, [2]);
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
        assert!(face.loops[0].support_record_ordinals.is_empty());
    }

    #[test]
    fn invalid_loop_tape_does_not_partially_bind_faces() {
        let mut stream = zero_entity_face_loop_support_stream();
        *stream.last_mut().expect("loop terminator") = 0;
        let runs = zero_entity_support_runs(&stream);
        let face = runs[0].face.as_ref().expect("face");
        assert!(face.loops.is_empty());
    }
}
