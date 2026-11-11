// SPDX-License-Identifier: Apache-2.0
//! Allocation-scoped sheet groups and their packed face rosters.

mod endpoint_sites;
mod witnesses;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point3;

use crate::wire::bytes::{allocation_reference, AllocationReference, AllocationReferenceEncoding};
use crate::wire::records::{
    parse_consolidated_allocation_record, ConsolidatedFamily as Family,
    ConsolidatedFrameFlag as Flag, ConsolidatedRecord, SourceExtent,
};

use super::roster::{
    allocation_width, backward_roster, legacy_body, logical_bytes, payload, wide_roster, Member,
};
use super::trimmed_face::Source;
use super::{boundary, fit, CadIr, Patch, Stream};

pub(super) fn root(record: &ConsolidatedRecord, bytes: &[u8]) -> bool {
    record.family() == Family::B
        && record.flag() == Flag::Flag13
        && record.class() == 0x5b
        && u8::from(record.width()) == 1
        && record.header_token() == 5
        && payload(bytes, record).and_then(shell_slots).is_some()
}

pub(super) fn backward_root(record: &ConsolidatedRecord, bytes: &[u8]) -> bool {
    record.family() == Family::B
        && record.flag() == Flag::Flag13
        && record.class() == 0x5b
        && payload(bytes, record).is_some_and(|payload| {
            let Some(distance) = backward_shell_distance(payload) else {
                return false;
            };
            allocation_width(bytes, record) == Some(distance)
        })
}

fn increasing_slots(bytes: &[u8], count: usize, at: &mut usize) -> Option<Vec<usize>> {
    if count == 0 || count > bytes.len().checked_sub(*at)? {
        return None;
    }
    let mut slots = Vec::new();
    for _ in 0..count {
        let reference = allocation_reference(bytes, at)?;
        if !matches!(
            reference.encoding,
            AllocationReferenceEncoding::OwnedChild
                | AllocationReferenceEncoding::TaggedU8
                | AllocationReferenceEncoding::TaggedU16
        ) {
            return None;
        }
        let slot = usize::try_from(reference.value).ok()?;
        if slots.last().is_some_and(|&previous| slot <= previous) {
            return None;
        }
        slots.push(slot);
    }
    (slots.first() == Some(&0)).then_some(slots)
}

fn shell_slots(bytes: &[u8]) -> Option<Vec<usize>> {
    let count = bytes.first()?.checked_sub(0x80)?;
    let mut at = 1;
    let slots = increasing_slots(bytes, usize::from(count), &mut at)?;
    let trailer = bounds_trailer(bytes.get(at..)?, count)?;
    (trailer == [0x08, 0xd6, 0x28]).then_some(slots)
}

fn backward_shell_distance(bytes: &[u8]) -> Option<usize> {
    if *bytes.first()? != 0x81 {
        return None;
    }
    let mut at = 1;
    let reference = allocation_reference(bytes, &mut at)?;
    if !matches!(reference.encoding, AllocationReferenceEncoding::WidthCoded)
        || bounds_trailer(bytes.get(at..)?, 1)? != [0x08, 0x64, 0x28]
    {
        return None;
    }
    usize::try_from(reference.value)
        .ok()
        .filter(|&value| value != 0)
}

fn bounds_trailer(bytes: &[u8], count: u8) -> Option<[u8; 3]> {
    let mut tail = View::over_retained(bytes);
    if tail.u8()? != count.checked_mul(4)?.checked_add(1)? || tail.u8()? != 1 {
        return None;
    }
    for _ in 0..3 {
        let lo = tail.f32_le()?;
        let hi = tail.f32_le()?;
        if !lo.is_finite() || !hi.is_finite() || lo > hi {
            return None;
        }
    }
    let trailer = tail.take(3)?.try_into().ok()?;
    (tail.remaining() == 0).then_some(trailer)
}

fn roster(bytes: &[u8], family: Family) -> Option<(Vec<usize>, &[u8])> {
    let (count, mut at) = match (*bytes.first()?, family) {
        (0x18, Family::A) => return wide_roster(bytes, 3),
        (0x08, Family::A) => (usize::from(*bytes.get(1)?), 2),
        (lead, _) => (usize::from(lead.checked_sub(0x80)?), 1),
    };
    let slots = increasing_slots(bytes, count, &mut at)?;
    let bits = bytes.get(at..)?.strip_prefix(&[0])?.strip_suffix(&[3])?;
    let used = (count + 4) % 8;
    if bits.len() != (count + 4).div_ceil(8)
        || bits.first()? & 0x0f != 3
        || (used != 0 && bits.last()? >> used != 0)
    {
        return None;
    }
    Some((slots, bits))
}

struct Allocations {
    starts: Vec<usize>,
    records: Vec<ConsolidatedRecord>,
    limit: usize,
}

impl Allocations {
    fn read(bytes: &[u8], start: usize, end: Option<usize>) -> Self {
        let mut starts = Vec::new();
        let mut records = Vec::new();
        let mut position = start;
        let mut limit = 0usize;
        while let Some(frame) =
            parse_consolidated_allocation_record(bytes, position, end.unwrap_or(bytes.len()))
        {
            if end.is_none() && (root(&frame, bytes) || frame.class() == 0x65) {
                break;
            }
            let Some(width) = allocation_width(bytes, &frame) else {
                break;
            };
            let Some(end) = limit.checked_add(width) else {
                break;
            };
            position = frame.source_range().end;
            starts.push(limit);
            records.push(frame);
            limit = end;
        }
        Self {
            starts,
            records,
            limit,
        }
    }

    fn resolve(
        &self,
        index: usize,
        reference: AllocationReference,
        end_anchored: bool,
    ) -> Option<usize> {
        let value = usize::try_from(reference.value).ok()?;
        let slot = match reference.encoding {
            AllocationReferenceEncoding::OwnedChild
            | AllocationReferenceEncoding::TaggedU8
            | AllocationReferenceEncoding::TaggedU16 => self.after(index).checked_add(value)?,
            AllocationReferenceEncoding::BackwardDistance
            | AllocationReferenceEncoding::WidthCoded
                if value != 0 =>
            {
                if end_anchored {
                    return self.backward_target(index, value);
                }
                self.starts.get(index)?.checked_sub(value)?
            }
            _ => return None,
        };
        if end_anchored {
            return self.ending_before(slot.checked_add(1)?);
        }
        self.starts.binary_search(&slot).ok()
    }

    fn backward_target(&self, index: usize, distance: usize) -> Option<usize> {
        let end = self.after(index).checked_sub(distance)?;
        self.ending_before(end)
    }

    fn ending_before(&self, end: usize) -> Option<usize> {
        if end == self.limit {
            return self.records.len().checked_sub(1);
        }
        self.starts.binary_search(&end).ok()?.checked_sub(1)
    }

    fn after(&self, index: usize) -> usize {
        self.starts.get(index + 1).copied().unwrap_or(self.limit)
    }

    fn faces(&self, bytes: &[u8], shells: &[usize]) -> Option<Vec<(usize, usize, bool)>> {
        let mut output = Vec::new();
        for (s, &slot) in shells.iter().enumerate() {
            let shell = self.starts.binary_search(&slot).ok()?;
            let frame = &self.records[shell];
            if frame.class() != 0x61 || frame.flag() != Flag::Flag03 {
                return None;
            }
            let (members, bits) = roster(payload(bytes, frame)?, frame.family())?;
            let first = self.after(shell);
            let end = shells.get(s + 1).copied().unwrap_or(self.limit);
            let end_index = self.starts.partition_point(|&start| start < end);
            if self
                .records
                .get(shell + 1..end_index)?
                .iter()
                .filter(|f| f.class() == 0x5f)
                .count()
                != members.len()
            {
                return None;
            }
            for (index, member) in members.into_iter().enumerate() {
                let slot = first.checked_add(member)?;
                if slot >= end {
                    return None;
                }
                let face = self.starts.binary_search(&slot).ok()?;
                let f = &self.records[face];
                if f.class() != 0x5f
                    || f.family() != Family::B
                    || f.flag() != Flag::Flag03
                    || self.after(face).checked_sub(slot)? != 1
                {
                    return None;
                }
                let bit = index + 4;
                output.push((shell, face, bits[bit / 8] & (1 << (bit % 8)) != 0));
            }
        }
        Some(output)
    }

    fn backward_faces(
        &self,
        bytes: &[u8],
        group: &ConsolidatedRecord,
    ) -> Option<Vec<(usize, usize, bool)>> {
        let index = self.records.len().checked_sub(1)?;
        if self.records[index].source_range() != group.source_range() {
            return None;
        }
        let distance = backward_shell_distance(payload(bytes, &self.records[index])?)?;
        let shell = self.backward_target(index, distance)?;
        let frame = &self.records[shell];
        if frame.class() != 0x61 || frame.family() != Family::A || frame.flag() != Flag::Flag03 {
            return None;
        }
        let (distances, bits) = backward_roster(payload(bytes, frame)?)?;
        let mut seen = BTreeSet::new();
        distances
            .into_iter()
            .enumerate()
            .map(|(index, distance)| {
                let face = self.backward_target(shell, distance)?;
                let frame = &self.records[face];
                if frame.class() != 0x5f
                    || frame.family() != Family::B
                    || frame.flag() != Flag::Flag03
                    || !seen.insert(face)
                {
                    return None;
                }
                let bit = index + 4;
                Some((shell, face, bits[bit / 8] & (1 << (bit % 8)) != 0))
            })
            .collect()
    }
}

fn file_offset(source: &[SourceExtent], logical: usize) -> Option<usize> {
    let mut offset = logical;
    for extent in source {
        let range = extent.range();
        if offset < range.len() {
            return range.start.checked_add(offset);
        }
        offset -= range.len();
    }
    None
}

const MAX_VERTEX_RETRIES: usize = 3;
const ANCHOR_AGREEMENT_MM: f64 = 0.0004;
pub(super) const VERTEX_RETRY_PRIORITY: u8 = 5;
pub(super) const ROLLING_BOUNDARY_RETRY_PRIORITY: u8 = u8::MAX;
const DISPUTED_VERTEX_PENALTY: u8 = 16;

type Sites = BTreeMap<(usize, usize), BTreeMap<usize, Option<Point3>>>;

fn add_site(sites: &mut Sites, body: (usize, usize), id: usize, point: Point3) {
    sites
        .entry(body)
        .or_default()
        .entry(id)
        .and_modify(|prior| {
            if prior.is_some_and(|p| p.distance(point) > ANCHOR_AGREEMENT_MM) {
                *prior = None;
            }
        })
        .or_insert(Some(point));
}

pub(super) fn patches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    stream: &Stream<'_>,
    carriers: &std::collections::BTreeMap<usize, usize>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<(Vec<Patch>, BTreeSet<usize>), cadmpeg_core::CodecError> {
    let &Stream {
        data,
        sources,
        records,
    } = stream;
    let mut output = Vec::new();
    let mut backward_sources = BTreeSet::new();
    for group in records
        .iter()
        .filter(|frame| root(frame, data) || backward_root(frame, data))
    {
        let source = group.source_index();
        let extents = &sources[source];
        let bytes = logical_bytes(ctx, data, extents)?;
        let backward = backward_root(group, data);
        let (alloc, members) = if backward {
            let Some(seed) = records.windows(3).find(|frames| {
                frames[0].source_index() == source
                    && frames[0].source_range().start < group.source_range().start
                    && legacy_body(frames, data)
            }) else {
                continue;
            };
            let alloc = Allocations::read(
                &bytes,
                seed[0].source_range().start,
                Some(group.source_range().end),
            );
            let members = alloc.backward_faces(&bytes, group);
            (alloc, members)
        } else {
            let alloc = Allocations::read(&bytes, group.source_range().end, None);
            let members = payload(data, group)
                .and_then(shell_slots)
                .and_then(|shells| alloc.faces(&bytes, &shells));
            (alloc, members)
        };
        let Some(members) = members else {
            continue;
        };
        if backward {
            backward_sources.insert(source);
        }
        let resolve = |index, reference, family, class| {
            let target = alloc.resolve(index, reference, backward)?;
            let frame = alloc.records.get(target)?;
            (frame.family() == family && frame.class() == class).then_some(target)
        };
        let physical = |position| file_offset(extents, position);
        let uncached = alloc
            .records
            .iter()
            .filter(|frame| {
                frame.family() == Family::A
                    && frame.class() == 0x34
                    && physical(frame.byte_offset()).is_some_and(|pos| !carriers.contains_key(&pos))
            })
            .cloned()
            .collect::<Vec<_>>();
        let tensors = crate::families::a5a8::records::a5_owned_surfaces_from_records(
            &bytes, &uncached, refusal,
        )?
        .into_iter()
        .map(|surface| (surface.pos, surface.geometry))
        .collect();
        let trim_source = Source {
            bytes: &bytes,
            records: &alloc.records,
            resolve: |index, reference| alloc.resolve(index, reference, backward),
            physical,
            tensors: &tensors,
        };
        let start = output.len();
        let mut pending = Vec::new();
        let mut native_faces = BTreeMap::new();
        let mut primary = witnesses::PrimarySites::default();
        let mut endpoints = endpoint_sites::EndpointSites::default();
        for (shell, face, forward) in members {
            let Some(face_pos) = physical(alloc.records[face].byte_offset()) else {
                continue;
            };
            let Some(shell_pos) = physical(alloc.records[shell].byte_offset()) else {
                continue;
            };
            let member = Member {
                body: (source, shell_pos),
                forward,
            };
            native_faces.insert(face_pos, (face, member));
            let rectangle = boundary(&bytes, &alloc.records, face, resolve, physical, true)
                .and_then(|boundary| {
                    fit(
                        ir,
                        *carriers.get(&boundary.carrier)?,
                        face_pos,
                        member,
                        &boundary,
                    )
                });
            if let Some(patch) = rectangle {
                output.push(patch);
                continue;
            }
            let Some(decoded) = trim_source.decode(ir, face, member, carriers, refusal) else {
                continue;
            };
            for site @ (vertex, _, point, explicit, _) in decoded.endpoint_sites() {
                if explicit {
                    primary.record(member.body, face_pos, std::iter::once((vertex, point)));
                }
                endpoints.record(member.body, face_pos, site);
            }
            if let Some(patch) = decoded.patch(ir, None) {
                output.push(patch);
            } else {
                pending.push(decoded);
            }
        }
        for _ in 0..MAX_VERTEX_RETRIES {
            if pending.is_empty() {
                break;
            }
            let mut witnesses = Sites::new();
            for patch in &output[start..] {
                for side in &patch.sides {
                    for (id, point) in side.vertices {
                        add_site(&mut witnesses, patch.body, id, point);
                    }
                }
            }
            let witnesses = witnesses
                .into_iter()
                .map(|(body, vertices)| {
                    (
                        body,
                        vertices
                            .into_iter()
                            .filter_map(|(id, point)| Some((id, point?)))
                            .collect(),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            let before = output.len();
            pending.retain(|face| {
                let Some(vertices) = witnesses.get(&face.member.body) else {
                    return true;
                };
                let Some(mut patch) = face.patch(ir, Some(vertices)) else {
                    return true;
                };
                patch.priority = VERTEX_RETRY_PRIORITY;
                output.push(patch);
                false
            });
            if output.len() == before {
                break;
            }
        }
        if !pending.is_empty() {
            let mut edges = BTreeMap::<_, BTreeMap<_, _>>::new();
            for patch in &output[start..] {
                let by_edge = edges.entry(patch.body).or_default();
                for side in &patch.sides {
                    by_edge.entry(side.edge).or_insert(side);
                }
            }
            let recovered = pending
                .iter()
                .filter_map(|face| face.recover_from_shell_edges(edges.get(&face.member.body)?))
                .collect::<Vec<_>>();
            output.extend(recovered);
        }
        let mut disputed = witnesses::disputed(&output[start..]);
        primary.corroborate(&mut disputed);
        if !disputed.is_empty() {
            let mut edges = BTreeMap::<_, BTreeMap<_, Vec<_>>>::new();
            for patch in &output[start..] {
                let by_edge = edges.entry(patch.body).or_default();
                for side in &patch.sides {
                    by_edge
                        .entry(side.edge)
                        .or_default()
                        .push((patch.face, side));
                }
            }
            let mut replacements = Vec::new();
            for (index, patch) in output[start..].iter().enumerate() {
                let Some(sites) = disputed.get(&patch.body) else {
                    continue;
                };
                if !witnesses::disagrees(patch, sites) {
                    continue;
                }
                let (face, member) = native_faces[&patch.face];
                let recovered = trim_source
                    .decode(ir, face, member, carriers, refusal)
                    .and_then(|face| {
                        face.patch(ir, Some(sites))
                            .filter(|refitted| !witnesses::disagrees(refitted, sites))
                            .or_else(|| face.recover_rolling_boundary(ir, Some(sites)))
                    })
                    .filter(|refitted| !witnesses::disagrees(refitted, sites))
                    .or_else(|| witnesses::repair_boundary(patch, sites, edges.get(&patch.body)?));
                replacements.push((index, recovered));
            }
            for (index, replacement) in replacements {
                let patch = &mut output[start + index];
                if let Some(mut refitted) = replacement {
                    refitted.priority = patch.priority;
                    *patch = refitted;
                } else {
                    patch.priority = patch
                        .priority
                        .checked_add(DISPUTED_VERTEX_PENALTY)
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed("CATIA patch priority overflow")
                        })?;
                }
            }
        }
        output.extend(
            pending
                .iter()
                .filter_map(|face| face.recover_rolling_boundary(ir, None)),
        );
        endpoints.resolve(&mut output[start..]);
    }
    Ok((output, backward_sources))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::records::parse_consolidated_record;

    #[test]
    fn backward_sheet_roster_selects_allocation_ends_in_roster_order() {
        let mut bytes = vec![
            0xb2, 3, 0x5f, 0, 0x0d, // Three slots.
            0xb5, 3, 0x20, 0, 0x0c, 4, 0, 0, // Four slots.
            0xb2, 3, 0x5f, 0, 0x05, // One slot.
            0xa5, 3, 0x61, 8, 0, 0, 0, 0x09, // Two slots.
            0x18, 2, 0, 0x09, 0x1d, 0, 0x23, 3,
        ];
        let group_at = bytes.len();
        bytes.extend([0xb3, 0x13, 0x5b, 32, 0x04, 5, 0x81, 0x04, 5, 5, 1]);
        bytes.extend([0; 24]);
        bytes.extend([0x08, 0x64, 0x28]);
        let group = parse_consolidated_record(&bytes, group_at, bytes.len())
            .expect("valid fixture sheet group");
        assert!(backward_root(&group, &bytes));
        let alloc = Allocations::read(&bytes, 0, Some(bytes.len()));
        assert_eq!(
            alloc.backward_faces(&bytes, &group),
            Some(vec![(3, 2, false), (3, 0, true)])
        );
        let forward = AllocationReference {
            value: 3,
            encoding: AllocationReferenceEncoding::OwnedChild,
        };
        assert_eq!(alloc.resolve(0, forward, true), Some(1));
        assert_eq!(alloc.resolve(0, forward, false), None);

        bytes[30] = 0x09;
        assert!(alloc.backward_faces(&bytes, &group).is_none());
        bytes[30] = 0x21;
        assert!(alloc.backward_faces(&bytes, &group).is_none());
    }

    #[test]
    fn wide_sheet_roster_retains_three_byte_slots_and_member_bits() {
        let sheet = [0x18, 2, 0, 3, 0x0e, 0x00, 0x01, 0x01, 0, 0x23, 3];
        let (slots, bits) = roster(&sheet, Family::A).expect("valid fixture sheet group");
        assert_eq!(slots, [0, 0x10100]);
        assert_eq!(bits, [0x23]);
        assert!(wide_roster(&sheet, 1).is_none());

        let mut invalid = sheet;
        invalid[9] |= 0x80;
        assert!(roster(&invalid, Family::A).is_none());
    }

    #[test]
    fn sheet_group_uses_per_roster_bits_and_exact_allocation_starts() {
        let mut bytes = vec![0xb2, 0x13, 0x5b, 33, 5, 0x82, 3, 0x06, 5, 9, 1];
        bytes.extend([0; 24]);
        bytes.extend([0x08, 0xd6, 0x28]);
        let root_frame =
            parse_consolidated_record(&bytes, 0, bytes.len()).expect("valid fixture sheet group");
        assert!(root(&root_frame, &bytes));
        let start = bytes.len();
        let first_roster = [0xa5, 3, 0x61, 7, 0, 0, 0, 5, 0x08, 2, 3, 0x0f, 0, 0x23, 3];
        let face = [0xb2, 3, 0x5f, 4, 5, 0x82, 0x07, 3, 5];
        let multi = [0xb2, 3, 0x25, 1, 0x09, 0];
        let second_roster = [0xb2, 3, 0x61, 5, 5, 0x81, 3, 0, 0x13, 3];
        for frame in [
            first_roster.as_slice(),
            &face,
            &multi,
            &face,
            &second_roster,
            &face,
        ] {
            bytes.extend(frame);
        }
        let slots = shell_slots(payload(&bytes, &root_frame).expect("valid fixture sheet group"))
            .expect("valid fixture sheet group");
        let alloc = Allocations::read(&bytes, start, None);
        assert_eq!(
            alloc.faces(&bytes, &slots),
            Some(vec![(0, 1, false), (0, 3, true), (4, 5, true)])
        );
        let backward = AllocationReference {
            value: 3,
            encoding: AllocationReferenceEncoding::WidthCoded,
        };
        assert_eq!(alloc.resolve(3, backward, false), Some(1));
        bytes[start + 11] = 0x0b;
        assert!(alloc.faces(&bytes, &slots).is_none());
    }
}
