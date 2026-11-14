// SPDX-License-Identifier: Apache-2.0
//! Allocation-scoped sheet groups and their packed face rosters.

mod endpoint_sites;
mod witnesses;

use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};
use std::rc::Rc;

use cadmpeg_core::decode::View;
use cadmpeg_ir::math::Point3;

use crate::wire::bytes::{allocation_reference, AllocationReference, AllocationReferenceEncoding};
use crate::wire::records::{
    parse_consolidated_allocation_record, ConsolidatedFamily as Family,
    ConsolidatedFrameFlag as Flag, ConsolidatedRecord, SourceExtent,
};

use super::roster::{
    allocation_width, legacy_body, logical_bytes, payload, sheet_references, wide_roster,
    BodyRoots, Member, SheetMembers,
};
use super::trimmed_face::Source;
use super::{boundary, fit, CadIr, Patch, Stream};

pub(super) fn root(record: &ConsolidatedRecord, bytes: &[u8]) -> bool {
    if record.family() != Family::B
        || record.flag() != Flag::Flag13
        || record.class() != 0x5b
        || allocation_width(bytes, record).is_none()
    {
        return false;
    }
    let Some(body) = payload(bytes, record).filter(|p| shell_slots(p).is_some()) else {
        return false;
    };
    if u8::from(record.width()) == 1
        && record.header_token() == 5
        && body.ends_with(&[8, 0xd6, 0x28])
    {
        return true;
    }
    let Some(next) = record
        .range()
        .and_then(|r| parse_consolidated_allocation_record(bytes, r.end, bytes.len()))
    else {
        return false;
    };
    next.class() == 0x61
        && next.flag() == Flag::Flag03
        && payload(bytes, &next)
            .and_then(|p| sheet_references(p, next.family()))
            .is_some()
}

pub(super) fn backward_root(record: &ConsolidatedRecord, bytes: &[u8]) -> bool {
    record.family() == Family::B
        && record.flag() == Flag::Flag13
        && record.class() == 0x5b
        && payload(bytes, record)
            .and_then(backward_shell_distance)
            .is_some()
        && allocation_width(bytes, record).is_some()
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
    bounds_trailer(bytes.get(at..)?, count)?;
    Some(slots)
}

fn backward_shell_distance(bytes: &[u8]) -> Option<usize> {
    if *bytes.first()? != 0x81 {
        return None;
    }
    let mut at = 1;
    let reference = allocation_reference(bytes, &mut at)?;
    if !matches!(
        reference.encoding,
        AllocationReferenceEncoding::WidthCoded | AllocationReferenceEncoding::BackwardDistance
    ) {
        return None;
    }
    bounds_trailer(bytes.get(at..)?, 1)?;
    usize::try_from(reference.value)
        .ok()
        .filter(|&value| value != 0)
}

fn bounds_trailer(bytes: &[u8], count: u8) -> Option<&[u8]> {
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
    let trailer = tail.take(tail.remaining())?;
    (trailer == [3] || (trailer.len() == 3 && trailer[0] == 8)).then_some(trailer)
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

fn allocation_run_start(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    source: usize,
    seed: usize,
) -> Option<usize> {
    let mut starts = records
        .iter()
        .filter(|record| record.source_index() == source)
        .map(|record| record.source_range().start);
    let mut start = starts.next()?;
    let mut position = start;
    while position < seed {
        let next = parse_consolidated_allocation_record(bytes, position, bytes.len())
            .filter(|frame| allocation_width(bytes, frame).is_some());
        if let Some(frame) = next {
            position = frame.source_range().end;
        } else {
            start = starts.find(|&candidate| candidate > position)?;
            position = start;
        }
    }
    (position == seed).then_some(start)
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
        let (references, bits) = sheet_references(payload(bytes, frame)?, frame.family())?;
        let mut seen = BTreeSet::new();
        references
            .into_iter()
            .enumerate()
            .map(|(index, reference)| {
                let face = self.resolve(shell, reference, true)?;
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

    fn mixed_faces(
        &self,
        bytes: &[u8],
        group: &ConsolidatedRecord,
    ) -> Option<Vec<(usize, usize, bool)>> {
        let group_index = self
            .records
            .binary_search_by_key(&group.source_range().start, |r| r.source_range().start)
            .ok()?;
        let slots = payload(bytes, &self.records[group_index]).and_then(shell_slots)?;
        let mut output = Vec::new();
        let mut seen = BTreeSet::new();
        for slot in slots {
            let shell = self
                .starts
                .binary_search(&self.after(group_index).checked_add(slot)?)
                .ok()?;
            let frame = &self.records[shell];
            if frame.class() != 0x61 || frame.flag() != Flag::Flag03 {
                return None;
            }
            let (references, bits) = sheet_references(payload(bytes, frame)?, frame.family())?;
            if !references.iter().any(|r| {
                matches!(
                    r.encoding,
                    AllocationReferenceEncoding::BackwardDistance
                        | AllocationReferenceEncoding::WidthCoded
                )
            }) {
                return None;
            }
            for (index, reference) in references.into_iter().enumerate() {
                let face = self.resolve(shell, reference, true)?;
                let frame = &self.records[face];
                if frame.family() != Family::B
                    || frame.class() != 0x5f
                    || frame.flag() != Flag::Flag03
                    || !seen.insert(face)
                {
                    return None;
                }
                let bit = index + 4;
                output.push((shell, face, bits[bit / 8] & (1 << (bit % 8)) != 0));
            }
        }
        Some(output)
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

pub(super) struct SelectedGroups {
    pub(super) source: usize,
    groups: BTreeSet<usize>,
    pub(super) legacy_bodies: BTreeSet<super::roster::RecordKey>,
    legacy_roots: BodyRoots,
}

pub(super) struct Sheets {
    pub(super) patches: Vec<Patch>,
    pub(super) members: SheetMembers,
    pub(super) selection: Option<SelectedGroups>,
    pub(super) roots: BodyRoots,
}

fn selected_indices(alloc: &Allocations, ends: &[usize]) -> Option<Vec<usize>> {
    ends.iter()
        .map(|&end| {
            let index = alloc.ending_before(end)?;
            let frame = &alloc.records[index];
            (frame.class() == 0x5b && frame.flag() == Flag::Flag13).then_some(index)
        })
        .collect()
}

fn selected_groups(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    records: &[ConsolidatedRecord],
    selection: &crate::container::body_selection::BodySelection,
) -> Result<Option<SelectedGroups>, cadmpeg_core::CodecError> {
    let source = selection.source;
    let extents = &sources[source];
    let bytes = logical_bytes(ctx, data, extents)?;
    let mut selected = None;
    let mut end = 0;
    for candidate in records.iter().filter(|r| r.source_index() == source) {
        let start = candidate.source_range().start;
        if start < end {
            continue;
        }
        let alloc = Allocations::read(&bytes, start, Some(bytes.len()));
        end = alloc
            .records
            .last()
            .map_or(start + 1, |r| r.source_range().end);
        let Some(indices) = selected_indices(&alloc, &selection.allocation_ends) else {
            continue;
        };
        let groups = indices
            .iter()
            .map(|&index| alloc.records[index].source_range().start)
            .collect::<BTreeSet<_>>();
        let mut legacy_bodies = BTreeSet::new();
        let mut legacy_roots = BodyRoots::new();
        for index in indices {
            let next = index + 1;
            if payload(&bytes, &alloc.records[index])
                .and_then(shell_slots)
                .as_deref()
                == Some(&[0])
                && alloc
                    .records
                    .get(next..next + 3)
                    .is_some_and(|r| legacy_body(r, &bytes))
            {
                if let Some(body) = file_offset(extents, alloc.records[next].byte_offset()) {
                    legacy_bodies.insert((source, body));
                    if let Some(root) = file_offset(extents, alloc.records[index].byte_offset()) {
                        legacy_roots.entry((source, body)).or_default().insert(root);
                    }
                }
            }
        }
        if let Some(prior) = &selected {
            let prior: &SelectedGroups = prior;
            if prior.groups != groups {
                return Ok(None);
            }
        }
        selected = Some(SelectedGroups {
            source,
            groups,
            legacy_bodies,
            legacy_roots,
        });
    }
    Ok(selected)
}

pub(super) fn patches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    stream: &Stream<'_>,
    carriers: &std::collections::BTreeMap<usize, usize>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Sheets, cadmpeg_core::CodecError> {
    let &Stream {
        data,
        sources,
        records,
        selection,
    } = stream;
    let selected = selection
        .map(|selection| selected_groups(ctx, data, sources, records, selection))
        .transpose()?
        .flatten();
    if selection.is_some() && selected.is_none() {
        refusal.push_body_selection(false, 0);
    }
    let mut output = Vec::new();
    let mut roots = selected
        .as_ref()
        .map(|s| s.legacy_roots.clone())
        .unwrap_or_default();
    let mut sheet_members = SheetMembers::new();
    let mut allocation_starts = BTreeMap::new();
    let mut source_bytes = BTreeMap::new();
    let mut mixed_allocations = BTreeMap::new();
    let mut excluded_groups = 0;
    for group in records
        .iter()
        .filter(|frame| root(frame, data) || backward_root(frame, data))
    {
        let source = group.source_index();
        if selected.as_ref().is_some_and(|selected| {
            selected.source == source && !selected.groups.contains(&group.source_range().start)
        }) {
            excluded_groups += 1;
            continue;
        }
        let extents = &sources[source];
        let bytes = match source_bytes.entry(source) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(logical_bytes(ctx, data, extents)?),
        };
        let bytes = bytes.as_ref();
        let backward = backward_root(group, data);
        let mut end_anchored = backward;
        let (alloc, members) = if backward {
            let Some(seed) = records.windows(3).find(|frames| {
                frames[0].source_index() == source
                    && frames[0].source_range().start < group.source_range().start
                    && legacy_body(frames, data)
            }) else {
                continue;
            };
            let seed = seed[0].source_range().start;
            let start = *allocation_starts.entry((source, seed)).or_insert_with(|| {
                allocation_run_start(bytes, records, source, seed).unwrap_or(seed)
            });
            let alloc = Rc::new(Allocations::read(
                bytes,
                start,
                Some(group.source_range().end),
            ));
            let members = alloc.backward_faces(bytes, group);
            (alloc, members)
        } else {
            let alloc = Rc::new(Allocations::read(bytes, group.source_range().end, None));
            let members = payload(data, group)
                .and_then(shell_slots)
                .and_then(|shells| alloc.faces(bytes, &shells));
            if members.is_some() {
                (alloc, members)
            } else {
                let mixed = alloc.records.first().is_some_and(|frame| {
                    frame.class() == 0x61
                        && payload(bytes, frame)
                            .and_then(|p| sheet_references(p, frame.family()))
                            .is_some_and(|(refs, _)| {
                                refs.iter().any(|r| {
                                    matches!(
                                        r.encoding,
                                        AllocationReferenceEncoding::BackwardDistance
                                            | AllocationReferenceEncoding::WidthCoded
                                    )
                                })
                            })
                });
                if !mixed {
                    continue;
                }
                let Some(seed) = records.windows(3).find(|frames| {
                    frames[0].source_index() == source
                        && frames[0].source_range().start < group.source_range().start
                        && legacy_body(frames, data)
                }) else {
                    continue;
                };
                let seed = seed[0].source_range().start;
                let start = *allocation_starts.entry((source, seed)).or_insert_with(|| {
                    allocation_run_start(bytes, records, source, seed).unwrap_or(seed)
                });
                let alloc =
                    Rc::clone(mixed_allocations.entry((source, start)).or_insert_with(|| {
                        Rc::new(Allocations::read(bytes, start, Some(bytes.len())))
                    }));
                let members = alloc.mixed_faces(bytes, group);
                end_anchored = true;
                (alloc, members)
            }
        };
        let Some(members) = members else {
            continue;
        };
        let resolve = |index, reference, family, class| {
            let target = alloc.resolve(index, reference, end_anchored)?;
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
            bytes, &uncached, refusal,
        )?
        .into_iter()
        .map(|surface| (surface.pos, surface.geometry))
        .collect();
        let trim_source = Source {
            bytes,
            records: &alloc.records,
            resolve: |index, reference| alloc.resolve(index, reference, end_anchored),
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
            roots
                .entry(member.body)
                .or_default()
                .insert(group.byte_offset());
            let owners = sheet_members
                .entry((source, alloc.records[face].source_range().start))
                .or_default();
            if !owners.contains(&member) {
                owners.push(member);
            }
            native_faces.insert(face_pos, (face, member));
            let rectangle = boundary(bytes, &alloc.records, face, resolve, physical, true)
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
    if excluded_groups != 0 {
        refusal.push_body_selection(true, excluded_groups);
    }
    Ok(Sheets {
        patches: output,
        members: sheet_members,
        selection: selected,
        roots,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::records::parse_consolidated_record;

    #[test]
    fn visible_body_addresses_select_allocation_ends_and_require_body_types() {
        let bytes = [
            0xb2, 0x13, 0x5b, 0, 5, 0xb2, 3, 0x25, 0, 0x0d, 0xb2, 0x13, 0x5b, 0, 5,
        ];
        let alloc = Allocations::read(&bytes, 0, Some(bytes.len()));
        assert_eq!(selected_indices(&alloc, &[5]), Some(vec![2]));
        assert_eq!(selected_indices(&alloc, &[1, 5]), Some(vec![0, 2]));
        assert!(selected_indices(&alloc, &[1, 4]).is_none());
        assert!(selected_indices(&alloc, &[3]).is_none());
    }

    #[test]
    fn visible_body_addresses_accept_wide_family_body_frames() {
        let bytes = [
            0xb2, 0x13, 0x5b, 0, 5, 0xa5, 0x13, 0x5b, 0, 0, 0, 0, 5, 0xa5, 3, 0x5b, 0, 0, 0, 0, 5,
        ];
        let alloc = Allocations::read(&bytes, 0, Some(bytes.len()));
        assert_eq!(selected_indices(&alloc, &[1, 2]), Some(vec![0, 1]));
        assert!(selected_indices(&alloc, &[1, 3]).is_none());
    }

    #[test]
    fn visible_body_selection_rejects_competing_allocation_sequences() {
        let bytes = [0xb2, 0x13, 0x5b, 0, 5, 0xff, 0xb2, 0x13, 0x5b, 0, 5];
        let sources = vec![vec![
            SourceExtent::within(&bytes, 0, bytes.len()).expect("source")
        ]];
        let records = crate::wire::records::consolidated_records_in_sources(&bytes, &sources)
            .expect("records");
        let selection = crate::container::body_selection::BodySelection {
            source: 0,
            allocation_ends: vec![1],
        };
        crate::test_support::with_service_context(|ctx| {
            assert!(selected_groups(ctx, &bytes, &sources, &records, &selection)
                .expect("selection")
                .is_none());
        });
    }

    #[test]
    fn sheet_references_reach_allocations_before_the_legacy_body() {
        let mut bytes = vec![0xb2, 3, 0x5f, 0, 5];
        bytes.extend([0xa8, 3, 0x32, 0, 0, 0, 0, 0x0c, 4, 0, 0]);
        let seed = bytes.len();
        bytes.extend([0xb2, 3, 0x65, 4, 5, 0x81, 3, 5, 0x0d]);
        bytes.extend([0xb2, 3, 0x60, 2, 5, 0x81, 3]);
        bytes.extend([0xa5, 3, 0x61, 7, 0, 0, 0, 5, 0x18, 1, 0, 3, 0, 0x11, 3]);
        bytes.extend([0xb2, 3, 0x5f, 0, 5]);
        bytes.extend([
            0xa5, 3, 0x61, 8, 0, 0, 0, 9, 0x18, 2, 0, 0x29, 0x09, 0, 0x13, 3,
        ]);
        let group_at = bytes.len();
        bytes.extend([0xb2, 0x13, 0x5b, 29, 5, 0x81, 5, 5, 1]);
        bytes.extend([0; 24]);
        bytes.push(3);
        let group =
            parse_consolidated_record(&bytes, group_at, bytes.len()).expect("backward sheet owner");
        let partial = Allocations::read(&bytes, seed, Some(bytes.len()));
        assert!(partial.backward_faces(&bytes, &group).is_none());
        let complete = Allocations::read(&bytes, 0, Some(bytes.len()));
        assert_eq!(
            allocation_run_start(&bytes, &complete.records, 0, seed),
            Some(0)
        );
        assert_eq!(
            complete.backward_faces(&bytes, &group),
            Some(vec![(6, 0, true), (6, 5, false)])
        );
    }

    #[test]
    fn allocation_run_prefix_stops_at_unframed_bytes() {
        let bytes = [0xb2, 3, 0x5f, 0, 5, 0xff, 0xb2, 3, 0x5f, 0, 5];
        let records = [0, 6].map(|offset| {
            parse_consolidated_record(&bytes, offset, bytes.len()).expect("native frame")
        });
        assert_eq!(allocation_run_start(&bytes, &records, 0, 6), Some(6));
    }

    #[test]
    fn backward_sheet_owner_keeps_new_and_reused_face_occurrences() {
        let mut bytes = vec![0xb2, 3, 0x5f, 0, 5];
        bytes.extend([0xa8, 3, 0x32, 0, 0, 0, 0, 0x0c, 4, 0, 0]);
        bytes.extend([
            0xa5, 3, 0x61, 8, 0, 0, 0, 9, 0x18, 2, 0, 3, 0x19, 0, 0x13, 3,
        ]);
        bytes.extend([0xb2, 3, 0x5f, 0, 5]);
        let group_at = bytes.len();
        bytes.extend([0xb2, 0x13, 0x5b, 29, 5, 0x81, 0x09, 5, 1]);
        bytes.extend([0; 24]);
        bytes.push(3);
        let group =
            parse_consolidated_record(&bytes, group_at, bytes.len()).expect("backward sheet owner");
        assert!(backward_root(&group, &bytes));
        let alloc = Allocations::read(&bytes, 0, Some(bytes.len()));
        assert_eq!(
            alloc.backward_faces(&bytes, &group),
            Some(vec![(2, 3, true), (2, 0, false)])
        );
    }

    #[test]
    fn backward_sheet_roster_crosses_wide_a_frames_and_intervening_allocations() {
        for (reference, trailer) in [(vec![4, 3], vec![8, 0x64, 0x28]), (vec![0x0d], vec![3])] {
            let mut bytes = vec![0xb2, 3, 0x5f, 0, 5];
            let wide_at = bytes.len();
            bytes.extend([0xa8, 3, 0x32, 0, 0, 0, 0, 0x0c, 4, 0, 0]);
            bytes.extend([0xa5, 3, 0x61, 7, 0, 0, 0, 5, 0x18, 1, 0, 0x15, 0, 0x13, 3]);
            bytes.extend([0xb2, 3, 0x25, 1, 9, 0]);
            let group_at = bytes.len();
            let length = u8::try_from(27 + reference.len() + trailer.len()).expect("group length");
            bytes.extend([0xb2, 0x13, 0x5b, length, 5, 0x81]);
            bytes.extend(&reference);
            bytes.extend([5, 1]);
            bytes.extend([0; 24]);
            bytes.extend(trailer);
            assert!(parse_consolidated_record(&bytes, wide_at, bytes.len()).is_none());
            let group = parse_consolidated_record(&bytes, group_at, bytes.len())
                .expect("sheet group frame");
            assert!(backward_root(&group, &bytes));
            let alloc = Allocations::read(&bytes, 0, Some(bytes.len()));
            assert_eq!(alloc.records.len(), 5);
            assert_eq!(
                alloc.backward_faces(&bytes, &group),
                Some(vec![(2, 0, true)])
            );

            if reference.len() == 2 {
                bytes[group_at + 7] = 2;
            } else {
                bytes[group_at + 6] = 0x09;
            }
            let group = parse_consolidated_record(&bytes, group_at, bytes.len())
                .expect("sheet group frame");
            let alloc = Allocations::read(&bytes, 0, Some(bytes.len()));
            assert!(alloc.backward_faces(&bytes, &group).is_none());
        }
    }

    #[test]
    fn owned_sheet_roster_resolves_new_and_reused_faces_in_occurrence_order() {
        let mut bytes = vec![0xb2, 3, 0x5f, 0, 5];
        bytes.extend([0xa8, 3, 0x32, 0, 0, 0, 0, 0x0c, 4, 0, 0]);
        let group_at = bytes.len();
        bytes.extend([0xb2, 0x13, 0x5b, 29, 5, 0x81, 3, 5, 1]);
        bytes.extend([0; 24]);
        bytes.push(3);
        let roster_at = bytes.len();
        bytes.extend([
            0xa5, 3, 0x61, 8, 0, 0, 0, 9, 0x18, 2, 0, 3, 0x1d, 0, 0x13, 3,
        ]);
        bytes.extend([0xb2, 3, 0x5f, 0, 5]);
        let group =
            parse_consolidated_record(&bytes, group_at, bytes.len()).expect("sheet group frame");
        assert!(root(&group, &bytes));
        let alloc = Allocations::read(&bytes, 0, Some(bytes.len()));
        assert_eq!(
            alloc.mixed_faces(&bytes, &group),
            Some(vec![(3, 4, true), (3, 0, false)])
        );
        bytes[roster_at + 12] = 0x19;
        let alloc = Allocations::read(&bytes, 0, Some(bytes.len()));
        assert!(alloc.mixed_faces(&bytes, &group).is_none());
        bytes[roster_at + 2] = 0x5c;
        assert!(!root(&group, &bytes));
    }

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
