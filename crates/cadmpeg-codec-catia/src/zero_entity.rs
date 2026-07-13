// SPDX-License-Identifier: Apache-2.0
//! Counted topology records in the zero-entity `a9 03` stream family.

use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::le::u32_at;

/// Resolved zero-entity `a9 03` stream: records, faces, loops, carrier runs,
/// and the edge/vertex tables recovered from them ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroEntityTopology {
    /// Every `a9 03` record found by the stream walk, in stream order.
    /// Indexed by `ordinal`, and by extension by every `*_ordinal` field
    /// below.
    pub records: Vec<ZeroEntityRecord>,
    /// `5f 0c` face records.
    pub faces: Vec<ZeroEntityFace>,
    /// `62 xx` loop records.
    pub loops: Vec<ZeroEntityLoop>,
    /// Carrier-then-supports runs: each surface carrier (`27 6a`/`28
    /// 8a`/`29 b8`/`2b c8`/`34 xx`) followed by its maximal run of `21 xx`
    /// support occurrences, one run per face ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
    pub carrier_runs: Vec<ZeroCarrierRun>,
    /// `21 xx` curve-support-on-surface records, across all carrier runs.
    pub supports: Vec<ZeroSupport>,
    /// `5e 1a` edge-stride records.
    pub physical_edges: Vec<ZeroPhysicalEdge>,
    /// `06 38` coedge records, two per physical edge (one per side).
    pub coedge_twins: Vec<ZeroCoedgeTwin>,
    /// `25 69` side-pair header records, each identifying its two `06 38`
    /// twin coedges.
    pub side_pairs: Vec<ZeroSidePair>,
    /// `05 0b`/`05 10`/`05 15` vertex-incidence records paired with their
    /// following `5d 06` marker.
    pub vertices: Vec<ZeroVertex>,
}

/// A resolved vertex-incidence pair: a `05 0b`/`05 10`/`05 15` incidence
/// record immediately followed by its `5d 06` vertex marker ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroVertex {
    /// `ordinal` of the following `5d 06` marker record.
    pub marker_ordinal: usize,
    /// `ordinal` of this `05 0x` incidence record.
    pub incidence_record_ordinal: usize,
    /// Referenced record ordinals from the incidence record's counted
    /// reference lane: 2 items for tag `0x0b`, 3 for `0x10`, 4 for `0x15`.
    pub incidence_items: Vec<u32>,
}

/// A resolved `5e 1a` edge-stride record (38 bytes; [spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroPhysicalEdge {
    /// `ordinal` of this record.
    pub record_ordinal: usize,
    /// Six `0x10`-tagged `u32` reference tokens at fixed offsets `7, 12,
    /// 17, 22, 27, 32`; meaning not decoded further.
    pub references: [u32; 6],
}

/// A resolved `06 38` coedge record: one of the two per-side halves of a
/// physical edge (68 bytes; [spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroCoedgeTwin {
    /// `ordinal` of this record.
    pub record_ordinal: usize,
    /// Side number, `1` or `2`, read from the byte following the `0x10`
    /// marker at the record's `0x83` position.
    pub side: u8,
    /// `0x10`-tagged `u32` reference tokens following the side byte, in
    /// serialized order.
    pub references: Vec<u32>,
}

/// A resolved `25 69` side-pair header record, linking two [`ZeroCoedgeTwin`]
/// records by side number ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroSidePair {
    /// `ordinal` of this record.
    pub record_ordinal: usize,
    /// The header's two base columns `[B0, B1]`.
    pub bases: [u32; 2],
    /// `record_ordinal`s of the two following `06 38` records: side `1`
    /// first, side `2` second.
    pub coedge_ordinals: [usize; 2],
    /// `[bases[i] + side]` for `side` in `1, 2`; each side's composite key
    /// must equal the first two references of its paired coedge.
    pub composite_keys: [[u32; 2]; 2],
}

/// One surface carrier and its maximal run of `21 xx` support occurrences,
/// aligned 1:1 with a face ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant), "Carrier run = per-face surface").
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroCarrierRun {
    /// `ordinal` of the carrier record (`27 6a`/`28 8a`/`29 b8`/`2b
    /// c8`/`34 xx`).
    pub carrier_ordinal: usize,
    /// `ordinal`s of the carrier's `21 xx` support records, in stream
    /// order.
    pub support_ordinals: Vec<usize>,
    /// Complete decoded carrier geometry.
    pub geometry: Option<SurfaceGeometry>,
}

/// A resolved `21 xx` curve-support-on-surface record, with its UV
/// endpoints lifted through the owning carrier where possible ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroSupport {
    /// `ordinal` of this record.
    pub record_ordinal: usize,
    /// `ordinal` of the owning carrier record.
    pub owner_carrier_ordinal: usize,
    /// Local slot index at `+12`, used with a loop's `terminal_id` to
    /// address this support from a `62xx` loop member (`A = T - s`).
    pub slot: u32,
    /// `(u0,v0)`/`(u1,v1)` endpoint pairs read from the record's f64 tail
    /// at the family-specific offsets in [`support_uv_endpoints`], or
    /// `None` for an unrecognized support-record tag.
    pub uv_endpoints: Option<[[f64; 2]; 2]>,
    /// `uv_endpoints` lifted to world-frame 3D points through the owning
    /// carrier's analytic parameterization, or `None` when `uv_endpoints`
    /// is `None` or the carrier's tag is not one of the four supported
    /// analytic kinds ([`lift_geometry`]).
    pub lifted_endpoints: Option<[[f64; 3]; 2]>,
}

/// One length-framed `a9 03` record as found by the stream walk: framing
/// `a9 03 XX YY <payload[YY+8]>`, `record_length = YY + 12` ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityRecord {
    /// This record's position in the stream walk order. Records reference
    /// each other by this ordinal, not by byte offset ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
    pub ordinal: usize,
    /// Byte offset of the `a9 03` marker in the source stream.
    pub offset: usize,
    /// The two tag bytes (`XX`, `YY`) identifying the record family.
    pub tag: [u8; 2],
    /// The full record, including its `a9 03 XX YY` header.
    pub bytes: Vec<u8>,
}

/// A resolved `5f 0c` face record (24 bytes; [spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityFace {
    /// `ordinal` of this record.
    pub record_ordinal: usize,
    /// The record's counted reference lane `[R0, R1, ..., Rm]`: `R0` is
    /// the face's terminal base, `R1..` name loop terminals.
    pub references: Vec<u32>,
    /// Ordered loop terminals `T[j] = R0 - R[j+1]`, one per loop owned by
    /// this face.
    pub loop_terminals: Vec<u32>,
    /// Indices into the topology's `loops` vector, one per
    /// `loop_terminals` entry in the same order, resolved by
    /// [`bind_face_runs`]. Empty until binding runs.
    pub loop_indices: Vec<usize>,
    /// Index into the topology's `carrier_runs` vector for this face's
    /// surface carrier, resolved by [`bind_face_runs`]. `None` until
    /// binding runs or when no carrier run aligns with this face.
    pub carrier_run: Option<usize>,
}

/// A resolved `62 xx` loop record: an alternating even/odd reference lane
/// plus a packed 3-bit-per-member sense stream ([spec §8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#8-zero-entity-a9-03-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityLoop {
    /// `ordinal` of this record.
    pub record_ordinal: usize,
    /// Even-lane reference ids `A[j]`, one per loop member, satisfying
    /// `A[j] = T - g - j` for this loop's `terminal_id` (`T`) and `gap`
    /// (`g`).
    pub member_ids: Vec<u32>,
    /// Odd-lane reference ids interleaved with `member_ids`; meaning not
    /// decoded further.
    pub secondary_refs: Vec<u32>,
    /// The loop's terminal id `T`: the last entry of the record's counted
    /// reference lane.
    pub terminal_id: u32,
    /// `T - member_ids[0]`: the offset between the terminal id and the
    /// first even-lane member.
    pub gap: u32,
    /// Loop-class byte from the record header: `0x50` marks an inner
    /// (hole) loop, `0x41`/`0xc1` mark a non-inner loop.
    pub loop_class: u8,
    /// `true` when `loop_class == 0x50` (an inner/hole loop).
    pub inner: bool,
    /// Per-member coedge sense decoded from the packed 3-bit stream: code
    /// `7` (`.T.`, forward) decodes to `false`, code `2` (`.F.`, reversed)
    /// decodes to `true`. Index-aligned with `member_ids`.
    pub reversed: Vec<bool>,
    /// Per-member index into the topology's `supports` vector, resolved by
    /// [`bind_face_runs`] from each member's local slot `A = T - s`.
    /// `None` for a member whose slot resolves to no support in the
    /// owning carrier run, or before binding runs.
    pub support_indices: Vec<Option<usize>>,
}

/// One loop-member occurrence participating in a geometrically closed radial
/// edge pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroResolvedOccurrence {
    /// Index into [`ZeroEntityTopology::loops`].
    pub loop_index: usize,
    /// Member index within the loop.
    pub member_index: usize,
    /// Index into [`ZeroEntityTopology::supports`].
    pub support_index: usize,
}

/// One physical edge resolved from two surface-side occurrences with equal
/// unordered world-space endpoint pairs.
#[derive(Debug, Clone, PartialEq)]
pub struct ZeroResolvedEdge {
    /// Canonical endpoint order inherited from the first occurrence.
    pub endpoints: [[f64; 3]; 2],
    /// The two radial surface-side occurrences.
    pub occurrences: [ZeroResolvedOccurrence; 2],
    /// Endpoint order after applying each occurrence's packed loop sense.
    pub occurrence_endpoints: [[[f64; 3]; 2]; 2],
}

/// Resolve the reference-closed subset of zero-entity edge occurrences.
///
/// Stored support endpoints are oriented by each loop's packed sense lane.
/// An occurrence without a lifted carrier is completed only when it is isolated
/// between two lifted occurrences in the same closed loop. Radial twins are the
/// unique pairs with equal unordered endpoints within single-precision storage
/// tolerance. Ambiguous and unpaired occurrences remain unresolved.
#[must_use]
pub fn resolve_occurrence_edges(topology: &ZeroEntityTopology) -> Vec<ZeroResolvedEdge> {
    const TOLERANCE: f64 = 2e-3;
    let mut occurrences = Vec::<(ZeroResolvedOccurrence, Option<[[f64; 3]; 2]>)>::new();
    for (loop_index, loop_) in topology.loops.iter().enumerate() {
        let mut endpoints: Vec<Option<[[f64; 3]; 2]>> = loop_
            .support_indices
            .iter()
            .zip(&loop_.reversed)
            .map(|(support, reversed)| {
                let mut endpoints = topology.supports.get((*support)?)?.lifted_endpoints?;
                if *reversed {
                    endpoints.swap(0, 1);
                }
                Some(endpoints)
            })
            .collect();
        if endpoints.is_empty() {
            continue;
        }
        for index in 0..endpoints.len() {
            let next = (index + 1) % endpoints.len();
            if let (Some(current), Some(next_endpoints)) = (endpoints[index], endpoints[next]) {
                if point_distance(current[1], next_endpoints[0]) > TOLERANCE {
                    endpoints.fill(None);
                    break;
                }
            }
        }
        let stored = endpoints.clone();
        for index in 0..endpoints.len() {
            if endpoints[index].is_some() {
                continue;
            }
            let previous = (index + endpoints.len() - 1) % endpoints.len();
            let next = (index + 1) % endpoints.len();
            if let (Some(previous), Some(next)) = (stored[previous], stored[next]) {
                if point_distance(previous[1], next[0]) > TOLERANCE {
                    endpoints[index] = Some([previous[1], next[0]]);
                }
            }
        }
        for (member_index, (support_index, endpoints)) in loop_
            .support_indices
            .iter()
            .copied()
            .zip(endpoints)
            .enumerate()
        {
            if let Some(support_index) = support_index {
                occurrences.push((
                    ZeroResolvedOccurrence {
                        loop_index,
                        member_index,
                        support_index,
                    },
                    endpoints,
                ));
            }
        }
    }

    let mut used = vec![false; occurrences.len()];
    let mut edges = Vec::new();
    for left in 0..occurrences.len() {
        let Some(endpoints) = occurrences[left].1 else {
            continue;
        };
        let matches: Vec<usize> = (left + 1..occurrences.len())
            .filter(|right| {
                !used[*right]
                    && occurrences[*right]
                        .1
                        .is_some_and(|other| same_endpoint_pair(endpoints, other, TOLERANCE))
            })
            .collect();
        if used[left] || matches.len() != 1 {
            continue;
        }
        let right = matches[0];
        let reverse_matches = (0..left).filter(|other| {
            !used[*other]
                && occurrences[*other]
                    .1
                    .is_some_and(|value| same_endpoint_pair(endpoints, value, TOLERANCE))
        });
        if reverse_matches.count() != 0 {
            continue;
        }
        used[left] = true;
        used[right] = true;
        let Some(right_endpoints) = occurrences[right].1 else {
            continue;
        };
        edges.push(ZeroResolvedEdge {
            endpoints,
            occurrences: [occurrences[left].0, occurrences[right].0],
            occurrence_endpoints: [endpoints, right_endpoints],
        });
    }
    edges
}

fn same_endpoint_pair(left: [[f64; 3]; 2], right: [[f64; 3]; 2], tolerance: f64) -> bool {
    (point_distance(left[0], right[0]).max(point_distance(left[1], right[1])) <= tolerance)
        || (point_distance(left[0], right[1]).max(point_distance(left[1], right[0])) <= tolerance)
}

fn point_distance(left: [f64; 3], right: [f64; 3]) -> f64 {
    ((left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2) + (left[2] - right[2]).powi(2))
        .sqrt()
}

/// Walk native zero-entity records by `YY + 12`, then decode face counted
/// references and `62xx` alternating loop lanes with packed 3-bit senses.
#[must_use]
pub fn parse(bytes: &[u8]) -> Option<ZeroEntityTopology> {
    let records = walk_records(bytes);
    if records.is_empty() {
        return None;
    }
    let mut faces = records
        .iter()
        .filter(|record| record.tag[0] == 0x5f)
        .map(parse_face)
        .collect::<Option<Vec<_>>>()?;
    let mut loops = records
        .iter()
        .filter(|record| record.tag[0] == 0x62)
        .map(parse_loop)
        .collect::<Option<Vec<_>>>()?;
    if faces.is_empty() || loops.is_empty() {
        return None;
    }
    let (carrier_runs, supports) = parse_carrier_runs(&records, bytes)?;
    let physical_edges = records
        .iter()
        .filter(|record| record.tag == [0x5e, 0x1a])
        .map(parse_physical_edge)
        .collect::<Option<Vec<_>>>()?;
    let coedge_twins = records
        .iter()
        .filter(|record| record.tag == [0x06, 0x38])
        .map(parse_coedge_twin)
        .collect::<Option<Vec<_>>>()?;
    let side_pairs = parse_side_pairs(&records, &coedge_twins)?;
    let vertices = parse_vertices(&records)?;
    bind_face_runs(&mut faces, &mut loops, &carrier_runs, &supports);
    Some(ZeroEntityTopology {
        records,
        faces,
        loops,
        carrier_runs,
        supports,
        physical_edges,
        coedge_twins,
        side_pairs,
        vertices,
    })
}

fn parse_vertices(records: &[ZeroEntityRecord]) -> Option<Vec<ZeroVertex>> {
    let mut vertices = Vec::new();
    for (index, record) in records.iter().enumerate() {
        if !matches!(record.tag, [0x05, 0x0b | 0x10 | 0x15]) {
            continue;
        }
        let marker = records.get(index + 1)?;
        if marker.tag != [0x5d, 0x06] {
            return None;
        }
        let (incidence_items, end) = counted_references(&record.bytes, 12)?;
        if end != record.bytes.len()
            || incidence_items.len()
                != match record.tag[1] {
                    0x0b => 2,
                    0x10 => 3,
                    0x15 => 4,
                    _ => unreachable!(),
                }
        {
            return None;
        }
        vertices.push(ZeroVertex {
            marker_ordinal: marker.ordinal,
            incidence_record_ordinal: record.ordinal,
            incidence_items,
        });
    }
    Some(vertices)
}

fn parse_physical_edge(record: &ZeroEntityRecord) -> Option<ZeroPhysicalEdge> {
    let mut references = [0; 6];
    for (target, offset) in references.iter_mut().zip([7usize, 12, 17, 22, 27, 32]) {
        *target = token_u32(&record.bytes, offset)?;
    }
    Some(ZeroPhysicalEdge {
        record_ordinal: record.ordinal,
        references,
    })
}

fn parse_coedge_twin(record: &ZeroEntityRecord) -> Option<ZeroCoedgeTwin> {
    let marker = record
        .bytes
        .get(7..)?
        .windows(1)
        .position(|value| value == [0x83])?
        + 7;
    if record.bytes.get(marker + 1) != Some(&0x10) {
        return None;
    }
    let side = *record.bytes.get(marker + 2)?;
    if !matches!(side, 1 | 2) {
        return None;
    }
    let mut references = Vec::new();
    let mut position = marker + 3;
    while position + 5 <= record.bytes.len() {
        if record.bytes[position] == 0x10 {
            references.push(token_u32(&record.bytes, position)?);
            position += 5;
        } else {
            position += 1;
        }
    }
    Some(ZeroCoedgeTwin {
        record_ordinal: record.ordinal,
        side,
        references,
    })
}

fn parse_side_pairs(
    records: &[ZeroEntityRecord],
    coedges: &[ZeroCoedgeTwin],
) -> Option<Vec<ZeroSidePair>> {
    let mut pairs = Vec::new();
    for (index, record) in records.iter().enumerate() {
        if record.tag != [0x25, 0x69] {
            continue;
        }
        let (references, _) = counted_references(&record.bytes, 12)?;
        let bases: [u32; 2] = references.try_into().ok()?;
        let first = records.get(index + 1)?;
        let second = records.get(index + 2)?;
        let coedge0 = coedges
            .iter()
            .find(|coedge| coedge.record_ordinal == first.ordinal)?;
        let coedge1 = coedges
            .iter()
            .find(|coedge| coedge.record_ordinal == second.ordinal)?;
        if coedge0.side != 1 || coedge1.side != 2 {
            return None;
        }
        let composite_keys = [
            [bases[0].checked_add(1)?, bases[1].checked_add(1)?],
            [bases[0].checked_add(2)?, bases[1].checked_add(2)?],
        ];
        if coedge0.references.get(..2) != Some(&composite_keys[0])
            || coedge1.references.get(..2) != Some(&composite_keys[1])
        {
            return None;
        }
        pairs.push(ZeroSidePair {
            record_ordinal: record.ordinal,
            bases,
            coedge_ordinals: [coedge0.record_ordinal, coedge1.record_ordinal],
            composite_keys,
        });
    }
    Some(pairs)
}

fn bind_face_runs(
    faces: &mut [ZeroEntityFace],
    loops: &mut [ZeroEntityLoop],
    carrier_runs: &[ZeroCarrierRun],
    supports: &[ZeroSupport],
) {
    let mut loop_cursor = 0;
    for (face_index, face) in faces.iter_mut().enumerate() {
        for terminal in &face.loop_terminals {
            let Some(relative) = loops[loop_cursor..]
                .iter()
                .position(|loop_| loop_.terminal_id == *terminal)
            else {
                return;
            };
            let loop_index = loop_cursor + relative;
            face.loop_indices.push(loop_index);
            loop_cursor = loop_index + 1;
        }
        let Some(run) = carrier_runs.get(face_index) else {
            continue;
        };
        face.carrier_run = Some(face_index);
        let slot_to_support: std::collections::HashMap<u32, usize> = run
            .support_ordinals
            .iter()
            .filter_map(|ordinal| {
                supports
                    .iter()
                    .position(|support| support.record_ordinal == *ordinal)
                    .map(|index| (supports[index].slot, index))
            })
            .collect();
        for &loop_index in &face.loop_indices {
            let loop_ = &mut loops[loop_index];
            loop_.support_indices = loop_
                .member_ids
                .iter()
                .map(|member| {
                    loop_
                        .terminal_id
                        .checked_sub(*member)
                        .and_then(|slot| slot_to_support.get(&slot).copied())
                })
                .collect();
        }
    }
}

fn parse_carrier_runs(
    records: &[ZeroEntityRecord],
    bytes: &[u8],
) -> Option<(Vec<ZeroCarrierRun>, Vec<ZeroSupport>)> {
    let mut runs = Vec::new();
    let mut supports = Vec::new();
    let mut position = 0;
    while position < records.len() {
        if !matches!(records[position].tag[0], 0x27 | 0x28 | 0x29 | 0x2b | 0x34) {
            position += 1;
            continue;
        }
        let carrier = position;
        let geometry = crate::geometry::zero_entity_surface_at(bytes, records[carrier].offset);
        position += 1;
        let mut support_ordinals = Vec::new();
        while position < records.len() && records[position].tag[0] == 0x21 {
            let record = &records[position];
            let slot = token_u32(&record.bytes, 12)?;
            let uv_endpoints = support_uv_endpoints(record);
            let lifted_endpoints = uv_endpoints
                .and_then(|uv| geometry.as_ref().and_then(|value| lift_geometry(value, uv)));
            supports.push(ZeroSupport {
                record_ordinal: record.ordinal,
                owner_carrier_ordinal: records[carrier].ordinal,
                slot,
                uv_endpoints,
                lifted_endpoints,
            });
            support_ordinals.push(record.ordinal);
            position += 1;
        }
        if !support_ordinals.is_empty() {
            runs.push(ZeroCarrierRun {
                carrier_ordinal: records[carrier].ordinal,
                support_ordinals,
                geometry,
            });
        }
    }
    Some((runs, supports))
}

fn lift_geometry(geometry: &SurfaceGeometry, uv: [[f64; 2]; 2]) -> Option<[[f64; 3]; 2]> {
    uv.map(|[u, v]| {
        let neutral = match geometry {
            SurfaceGeometry::Cylinder { radius, .. } => [u / radius, v],
            SurfaceGeometry::Cone { half_angle, .. } => [u, v * half_angle.cos()],
            SurfaceGeometry::Torus {
                major_radius,
                minor_radius,
                ..
            } => [u / major_radius, v / minor_radius],
            SurfaceGeometry::Plane { .. } | SurfaceGeometry::Nurbs(_) => [u, v],
            SurfaceGeometry::Sphere { .. } | SurfaceGeometry::Unknown { .. } => return None,
        };
        let point = cadmpeg_ir::eval::surface_point(geometry, neutral[0], neutral[1])?;
        Some([point.x, point.y, point.z])
    })
    .into_iter()
    .collect::<Option<Vec<_>>>()?
    .try_into()
    .ok()
}

fn support_uv_endpoints(record: &ZeroEntityRecord) -> Option<[[f64; 2]; 2]> {
    let offsets = match record.tag {
        [0x21, 0x71] => [93, 101, 109, 117],
        [0x21, 0x91] => [93, 101, 141, 149],
        [0x21, 0x99] => [93, 101, 125, 133],
        [0x21, 0xd6] => [106, 114, 170, 178],
        [0x21, 0xe8] => [132, 140, 228, 236],
        _ => return None,
    };
    let values = offsets.map(|offset| {
        f64::from_le_bytes(
            record.bytes[offset..offset + 8]
                .try_into()
                .expect("validated record-family offset"),
        )
    });
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some([[values[0], values[1]], [values[2], values[3]]])
}

fn token_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    if bytes.get(offset) != Some(&0x10) {
        return None;
    }
    u32_at(bytes, offset + 1)
}

fn walk_records(bytes: &[u8]) -> Vec<ZeroEntityRecord> {
    let mut records = Vec::new();
    let mut position = 0;
    while position + 4 <= bytes.len() {
        if bytes.get(position..position + 2) != Some(&[0xa9, 0x03]) {
            position += 1;
            continue;
        }
        let length = usize::from(bytes[position + 3]) + 12;
        let Some(end) = position.checked_add(length) else {
            break;
        };
        if end > bytes.len() {
            break;
        }
        records.push(ZeroEntityRecord {
            ordinal: records.len(),
            offset: position,
            tag: [bytes[position + 2], bytes[position + 3]],
            bytes: bytes[position..end].to_vec(),
        });
        position = end;
    }
    records
}

fn parse_face(record: &ZeroEntityRecord) -> Option<ZeroEntityFace> {
    let (references, _) = counted_references(&record.bytes, 12)?;
    if references.len() < 2 {
        return None;
    }
    let base = references[0];
    let loop_terminals = references[1..]
        .iter()
        .map(|reference| base.checked_sub(*reference))
        .collect::<Option<Vec<_>>>()?;
    Some(ZeroEntityFace {
        record_ordinal: record.ordinal,
        references,
        loop_terminals,
        loop_indices: Vec::new(),
        carrier_run: None,
    })
}

fn parse_loop(record: &ZeroEntityRecord) -> Option<ZeroEntityLoop> {
    let (references, mut position) = counted_references(&record.bytes, 12)?;
    if references.len() < 3 || references.len() % 2 == 0 {
        return None;
    }
    let segment_count = (references.len() - 1) / 2;
    let member_ids: Vec<u32> = references[..references.len() - 1]
        .iter()
        .step_by(2)
        .copied()
        .collect();
    let secondary_refs: Vec<u32> = references[1..references.len() - 1]
        .iter()
        .step_by(2)
        .copied()
        .collect();
    let terminal_id = *references.last()?;
    let gap = terminal_id.checked_sub(*member_ids.first()?)?;
    for (index, member) in member_ids.iter().enumerate() {
        if *member != terminal_id - gap - u32::try_from(index).ok()? {
            return None;
        }
    }
    if record.bytes.get(position) != Some(&(0x80u8.checked_add(u8::try_from(segment_count).ok()?)?))
    {
        return None;
    }
    let loop_class = *record.bytes.get(position + 1)?;
    position += 2;
    let packed_length = (3 * segment_count).div_ceil(8);
    let packed = record.bytes.get(position..position + packed_length)?;
    position += packed_length;
    if record.bytes.get(position) != Some(&0x01) {
        return None;
    }
    let mut reversed = Vec::with_capacity(segment_count);
    for member in 0..segment_count {
        let mut code = 0u8;
        for bit in 0..3 {
            let bit_position = member * 3 + bit;
            code |= ((packed[bit_position / 8] >> (bit_position % 8)) & 1) << bit;
        }
        reversed.push(match code {
            7 => false,
            2 => true,
            _ => return None,
        });
    }
    if matches!(loop_class, 0x41 | 0xc1) && !matches!(gap, 1 | 2) {
        return None;
    }
    Some(ZeroEntityLoop {
        record_ordinal: record.ordinal,
        member_ids,
        secondary_refs,
        terminal_id,
        gap,
        loop_class,
        inner: loop_class == 0x50,
        reversed,
        support_indices: Vec::new(),
    })
}

fn counted_references(bytes: &[u8], position: usize) -> Option<(Vec<u32>, usize)> {
    let count = usize::from(bytes.get(position)?.checked_sub(0x80)?);
    let mut cursor = position + 1;
    let mut references = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(cursor) != Some(&0x10) {
            return None;
        }
        references.push(u32::from_le_bytes(
            bytes.get(cursor + 1..cursor + 5)?.try_into().ok()?,
        ));
        cursor += 5;
    }
    Some((references, cursor))
}

#[cfg(test)]
mod occurrence_tests {
    use super::*;

    fn support(index: usize, endpoints: Option<[[f64; 3]; 2]>) -> ZeroSupport {
        ZeroSupport {
            record_ordinal: index,
            owner_carrier_ordinal: 0,
            slot: index as u32,
            uv_endpoints: None,
            lifted_endpoints: endpoints,
        }
    }

    fn loop_(support_indices: [usize; 3]) -> ZeroEntityLoop {
        ZeroEntityLoop {
            record_ordinal: 0,
            member_ids: vec![0; 3],
            secondary_refs: vec![0; 3],
            terminal_id: 0,
            gap: 0,
            loop_class: 0x41,
            inner: false,
            reversed: vec![false; 3],
            support_indices: support_indices.into_iter().map(Some).collect(),
        }
    }

    #[test]
    fn isolated_unlifted_occurrence_closes_and_pairs_from_neighbors() {
        let a = [0.0, 0.0, 0.0];
        let b = [1.0, 0.0, 0.0];
        let c = [0.0, 1.0, 0.0];
        let topology = ZeroEntityTopology {
            records: Vec::new(),
            faces: Vec::new(),
            loops: vec![loop_([0, 1, 2]), loop_([3, 4, 5])],
            carrier_runs: Vec::new(),
            supports: vec![
                support(0, Some([a, b])),
                support(1, None),
                support(2, Some([c, a])),
                support(3, Some([b, a])),
                support(4, Some([a, c])),
                support(5, Some([c, b])),
            ],
            physical_edges: Vec::new(),
            coedge_twins: Vec::new(),
            side_pairs: Vec::new(),
            vertices: Vec::new(),
        };
        let edges = resolve_occurrence_edges(&topology);
        assert_eq!(edges.len(), 3);
        assert!(edges
            .iter()
            .any(|edge| same_endpoint_pair(edge.endpoints, [b, c], 1e-12)));
    }
}
