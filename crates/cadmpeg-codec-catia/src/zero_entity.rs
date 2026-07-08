// SPDX-License-Identifier: Apache-2.0
//! Counted topology records in the zero-entity `a9 03` stream family.

#[derive(Debug, Clone, PartialEq)]
pub struct ZeroEntityTopology {
    pub records: Vec<ZeroEntityRecord>,
    pub faces: Vec<ZeroEntityFace>,
    pub loops: Vec<ZeroEntityLoop>,
    pub carrier_runs: Vec<ZeroCarrierRun>,
    pub supports: Vec<ZeroSupport>,
    pub physical_edges: Vec<ZeroPhysicalEdge>,
    pub coedge_twins: Vec<ZeroCoedgeTwin>,
    pub side_pairs: Vec<ZeroSidePair>,
    pub vertices: Vec<ZeroVertex>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroVertex {
    pub marker_ordinal: usize,
    pub incidence_record_ordinal: usize,
    pub incidence_items: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroPhysicalEdge {
    pub record_ordinal: usize,
    pub references: [u32; 6],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroCoedgeTwin {
    pub record_ordinal: usize,
    pub side: u8,
    pub references: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroSidePair {
    pub record_ordinal: usize,
    pub bases: [u32; 2],
    pub coedge_ordinals: [usize; 2],
    pub composite_keys: [[u32; 2]; 2],
}

#[derive(Debug, Clone, PartialEq)]
pub struct ZeroCarrierRun {
    pub carrier_ordinal: usize,
    pub support_ordinals: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ZeroSupport {
    pub record_ordinal: usize,
    pub owner_carrier_ordinal: usize,
    pub slot: u32,
    pub uv_endpoints: Option<[[f64; 2]; 2]>,
    pub lifted_endpoints: Option<[[f64; 3]; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityRecord {
    pub ordinal: usize,
    pub offset: usize,
    pub tag: [u8; 2],
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityFace {
    pub record_ordinal: usize,
    pub references: Vec<u32>,
    pub loop_terminals: Vec<u32>,
    pub loop_indices: Vec<usize>,
    pub carrier_run: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZeroEntityLoop {
    pub record_ordinal: usize,
    pub member_ids: Vec<u32>,
    pub secondary_refs: Vec<u32>,
    pub terminal_id: u32,
    pub gap: u32,
    pub loop_class: u8,
    pub inner: bool,
    pub reversed: Vec<bool>,
    pub support_indices: Vec<Option<usize>>,
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
    let (carrier_runs, supports) = parse_carrier_runs(&records)?;
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
        position += 1;
        let mut support_ordinals = Vec::new();
        while position < records.len() && records[position].tag[0] == 0x21 {
            let record = &records[position];
            let slot = token_u32(&record.bytes, 12)?;
            let uv_endpoints = support_uv_endpoints(record);
            let lifted_endpoints =
                uv_endpoints.and_then(|uv| lift_endpoints(&records[carrier], uv));
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
            });
        }
    }
    Some((runs, supports))
}

fn lift_endpoints(carrier: &ZeroEntityRecord, uv: [[f64; 2]; 2]) -> Option<[[f64; 3]; 2]> {
    let payload = carrier.bytes.get(4..)?;
    let lifted = match carrier.tag {
        [0x27, 0x6a] => {
            let origin = point(payload, 10)?;
            let x = point(payload, 34)?;
            let y = point(payload, 58)?;
            uv.map(|[u, v]| add(origin, add(scale(x, u), scale(y, v))))
        }
        [0x28, 0x8a] => {
            let origin = point(payload, 8)?;
            let x = unit(point(payload, 33)?)?;
            let axis = unit(cross(x, point(payload, 57)?))?;
            let y = cross(axis, x);
            let radius = scalar(payload, 81)?;
            if radius <= 0.0 {
                return None;
            }
            uv.map(|[u, v]| {
                let angle = u / radius;
                add(
                    origin,
                    add(
                        scale(add(scale(x, angle.cos()), scale(y, angle.sin())), radius),
                        scale(axis, v),
                    ),
                )
            })
        }
        [0x29, 0xb8] => {
            let origin = point(payload, 8)?;
            let x = unit(point(payload, 32)?)?;
            let y = unit(point(payload, 56)?)?;
            let axis = unit(point(payload, 80)?)?;
            let half_angle = std::f64::consts::FRAC_PI_2 - scalar(payload, 104)?;
            let radius = scalar(payload, 112)?;
            uv.map(|[u, v]| {
                let radial = radius + v * half_angle.sin();
                add(
                    origin,
                    add(
                        scale(add(scale(x, u.cos()), scale(y, u.sin())), radial),
                        scale(axis, v * half_angle.cos()),
                    ),
                )
            })
        }
        [0x2b, 0xc8] => {
            let center = point(payload, 8)?;
            let x = unit(point(payload, 32)?)?;
            let y = unit(point(payload, 56)?)?;
            let axis = unit(point(payload, 80)?)?;
            let major = scalar(payload, 104)?;
            let minor = scalar(payload, 112)?;
            if major <= 0.0 || minor <= 0.0 {
                return None;
            }
            uv.map(|[u, v]| {
                let theta = u / major;
                let phi = v / minor;
                let radial = major + minor * phi.cos();
                add(
                    center,
                    add(
                        scale(add(scale(x, theta.cos()), scale(y, theta.sin())), radial),
                        scale(axis, minor * phi.sin()),
                    ),
                )
            })
        }
        _ => return None,
    };
    lifted
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(lifted)
}

fn scalar(bytes: &[u8], offset: usize) -> Option<f64> {
    let value = f64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?);
    value.is_finite().then_some(value)
}

fn point(bytes: &[u8], offset: usize) -> Option<[f64; 3]> {
    Some([
        scalar(bytes, offset)?,
        scalar(bytes, offset + 8)?,
        scalar(bytes, offset + 16)?,
    ])
}

fn add(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn scale(value: [f64; 3], scalar: f64) -> [f64; 3] {
    [value[0] * scalar, value[1] * scalar, value[2] * scalar]
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn unit(value: [f64; 3]) -> Option<[f64; 3]> {
    let length = value
        .iter()
        .map(|component| component * component)
        .sum::<f64>()
        .sqrt();
    (length > f64::EPSILON).then(|| scale(value, 1.0 / length))
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
    Some(u32::from_le_bytes(
        bytes.get(offset + 1..offset + 5)?.try_into().ok()?,
    ))
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
