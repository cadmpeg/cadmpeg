// SPDX-License-Identifier: Apache-2.0
//! Source-closed outer Fill inputs and directed planar boundary components.

use cadmpeg_core::decode::{index_from_u32, index_from_u64, u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use crate::boundary_roles::{
    segments_intersect_or_touch, strictly_inside_planar_polygon, MAX_PLANAR_BOUNDARY_POINTS,
};
use crate::decode::ModelingGraphScope;
use crate::native::entity_record::{CatiaEntityRecord, CatiaEntityRecordBody};
use crate::native::{CatiaCatalog, CatiaNative, CatiaObjectGraph};

use super::{geometry, SheetLine, SheetPlan};

const ENDPOINT_TOLERANCE: f64 = 1.0e-6;
const CONTOUR_TOLERANCE: f64 = 1.0e-10;
const RETAIN: &str = "bind outer Fill sheet boundaries";

#[derive(Clone, Copy)]
struct Record<'a, 's> {
    entity: &'a CatiaEntityRecord,
    payload: View<'s>,
    owner: u32,
    key: u32,
}

struct Graph<'a, 's> {
    root: &'a CatiaObjectGraph,
    catalog: &'a CatiaCatalog,
    entities: Vec<&'a CatiaEntityRecord>,
    source: View<'s>,
}

fn allocate<T: Clone>(
    ctx: &DecodeContext<'_>,
    count: usize,
    value: T,
) -> Result<Vec<T>, CodecError> {
    let bytes = u64_from_index(count)
        .checked_mul(u64_from_index(std::mem::size_of::<T>()))
        .ok_or_else(|| CodecError::malformed("outer sheet allocation exceeds its size lane"))?;
    ctx.retain_bytes(bytes, RETAIN)?;
    ctx.alloc_filled(count, value, RETAIN)
}

fn compact(view: &mut View<'_>) -> Option<u32> {
    match view.u8()? {
        code @ 0x80..=0xd0 => Some(u32::from(code - 0x80)),
        code @ 0xd1..=0xe4 => Some(u32::from(code - 0xd1) * 256 + u32::from(view.u8()?) + 1),
        _ => None,
    }
}

fn identity(view: &mut View<'_>) -> Option<u32> {
    match view.u8()? {
        0x32 | 0x80 => view.u32_le(),
        code @ 0x81..=0xd0 => Some(u32::from(code - 0x81)),
        code @ 0xd1..=0xe4 => Some(u32::from(code - 0xd1) * 256 + u32::from(view.u8()?)),
        _ => None,
    }
}

fn schema(view: &mut View<'_>) -> Option<u32> {
    match view.u8()? {
        0x32 => view.u32_le()?.checked_sub(1),
        code @ 0x34..=0xff => Some(u32::from(code - 0x34)),
        _ => None,
    }
}

fn nested(entity: &CatiaEntityRecord) -> Option<(&[u8], &[u8])> {
    let CatiaEntityRecordBody::Nested {
        definition_prefix,
        definition_suffix,
        ..
    } = &entity.body
    else {
        return None;
    };
    Some((definition_prefix, definition_suffix))
}

fn names<'a>(entity: &CatiaEntityRecord, catalog: &'a CatiaCatalog) -> Option<[&'a str; 2]> {
    let mut prefix = View::over_retained(nested(entity)?.0);
    prefix.skip(2)?;
    let mut result = [""; 2];
    for name in &mut result {
        let ordinal = schema(&mut prefix)?;
        let entry = catalog.entries.get(index_from_u32(ordinal))?;
        if entry.ordinal != ordinal {
            return None;
        }
        *name = &entry.value;
    }
    prefix.is_empty().then_some(result)
}

fn definition_identity(entity: &CatiaEntityRecord) -> Option<(u32, u32)> {
    let suffix = View::over_retained(nested(entity)?.1);
    let mut selected = None;
    for at in suffix.end().checked_sub(6)?..suffix.end() {
        let mut tail = suffix.child(at, suffix.end())?;
        if tail.u8()? != 3 {
            continue;
        }
        let Some(class) = compact(&mut tail) else {
            continue;
        };
        if !tail.is_empty() {
            continue;
        }
        let mut key = None;
        for length in [15, 11] {
            let candidate = (|| {
                let mut words = suffix.child(at.checked_sub(length)?, at)?;
                if words.u8()? != 0x80 {
                    return None;
                }
                let value = words.u32_le()?;
                if words.u8()? != 0x80 {
                    return None;
                }
                words.u32_le()?;
                if length == 15 {
                    if words.u8()? != 0x80 {
                        return None;
                    }
                    words.u32_le()?;
                } else if words.u8()? != 0x81 {
                    return None;
                }
                words.is_empty().then_some(value)
            })();
            if let Some(candidate) = candidate {
                if key.replace(candidate).is_some() {
                    return None;
                }
            }
        }
        if selected.replace((class, key?)).is_some() {
            return None;
        }
    }
    selected
}

fn framed(source: View<'_>, offset: u64, length: u64, marker: u8) -> Option<View<'_>> {
    let start = index_from_u64(offset)?;
    let end = index_from_u64(offset.checked_add(length)?)?;
    let mut frame = source.child(start, end)?;
    (frame.u8()? == 0x7c && frame.u8()? == marker && u64::from(frame.u32_le()?) == length)
        .then_some(frame)
}

impl<'a, 's> Graph<'a, 's> {
    fn schema_index(&self, name: &str) -> Option<u32> {
        let mut entries = self
            .catalog
            .entries
            .iter()
            .filter(|entry| entry.value == name);
        let (Some(entry), None) = (entries.next(), entries.next()) else {
            return None;
        };
        Some(entry.ordinal)
    }

    fn record(&self, id: u32) -> Option<Record<'a, 's>> {
        let at = self
            .entities
            .binary_search_by_key(&id, |entity| entity.entity_id)
            .ok()?;
        if at
            .checked_sub(1)
            .and_then(|at| self.entities.get(at))
            .is_some_and(|e| e.entity_id == id)
            || self.entities.get(at + 1).is_some_and(|e| e.entity_id == id)
        {
            return None;
        }
        let entity = *self.entities.get(at)?;
        let object = self.root.records.get(index_from_u64(entity.ordinal)?)?;
        if object.id != entity.object_record
            || object.parent != self.root.id
            || object.ordinal != entity.ordinal
            || object.entity_record() != Some(entity.id.as_str())
            || object.entity_id() != Some(id)
        {
            return None;
        }
        let frame = framed(self.source, object.byte_offset, object.byte_len, 9)?;
        let mut children =
            frame
                .unread()
                .windows(2)
                .enumerate()
                .filter_map(|(relative, marker)| {
                    if marker != [0x7c, 0x0a] {
                        return None;
                    }
                    let start = frame.position().checked_add(relative)?;
                    let mut child = frame.child(start, frame.end())?;
                    child.skip(2)?;
                    let length = usize::try_from(child.u32_le()?).ok()?;
                    (length >= 7 && start.checked_add(length) == Some(frame.end()))
                        .then_some((start, child))
                });
        let ((head_end, payload), None) = (children.next()?, children.next()) else {
            return None;
        };
        if payload.window().last() != Some(&0xfe) {
            return None;
        }
        let mut head = frame.child(frame.position(), head_end)?;
        let lead = head.u8()?;
        if !matches!(lead, 0x14 | 0x54) || lead != object.lead || head.u8()? != 1 {
            return None;
        }
        let class = compact(&mut head)?;
        let owner = identity(&mut head)?;
        if lead == 0x54 {
            identity(&mut head)?;
        }
        let (definition_class, key) = definition_identity(entity)?;
        (head.is_empty() && class == definition_class).then_some(Record {
            entity,
            payload,
            owner,
            key,
        })
    }

    fn persistent_members(
        &self,
        ctx: &DecodeContext<'_>,
        payload: &mut View<'_>,
    ) -> Result<Option<Vec<(u32, u32)>>, CodecError> {
        let Some(count) = compact(payload).and_then(|count| count.checked_sub(1)) else {
            return Ok(None);
        };
        let Some(count) = payload.counted(u64::from(count), 6) else {
            return Ok(None);
        };
        if payload.u8() != Some(0x81) {
            return Ok(None);
        }
        let mut members = allocate(ctx, count.get(), (0, 0))?;
        for slot in 0..members.len() {
            let Some(id) = identity(payload) else {
                return Ok(None);
            };
            if payload.u8() != Some(0x80) {
                return Ok(None);
            }
            let Some(key) = payload.u32_le() else {
                return Ok(None);
            };
            let Some(record) = self.record(id) else {
                return Ok(None);
            };
            if record.key != key || members[..slot].iter().any(|(stored, _)| *stored == id) {
                return Ok(None);
            }
            members[slot] = (id, key);
        }
        Ok(Some(members))
    }

    fn parent_contains(
        &self,
        ctx: &DecodeContext<'_>,
        fill: Record<'_, '_>,
        sketch: Record<'_, '_>,
    ) -> Result<bool, CodecError> {
        if fill.owner != sketch.owner {
            return Ok(false);
        }
        let Some(parent) = self.record(fill.owner) else {
            return Ok(false);
        };
        if names(parent.entity, self.catalog) != Some(["GSMTool", "Open_body"]) {
            return Ok(false);
        }
        let mut payload = parent.payload;
        if payload.u8() != Some(0x3b) {
            return Ok(false);
        }
        let Some(members) = self.persistent_members(ctx, &mut payload)? else {
            return Ok(false);
        };
        if payload.u8() != Some(0xfe) || !payload.is_empty() {
            return Ok(false);
        }
        let mut fill_count = 0;
        let mut sketch_count = 0;
        for (id, key) in members {
            let Some(member) = self.record(id) else {
                return Ok(false);
            };
            if member.owner != parent.entity.entity_id {
                return Ok(false);
            }
            fill_count += usize::from(id == fill.entity.entity_id && key == fill.key);
            sketch_count += usize::from(id == sketch.entity.entity_id && key == sketch.key);
        }
        Ok(fill_count == 1 && sketch_count == 1)
    }

    fn sketch_members(
        &self,
        ctx: &DecodeContext<'_>,
        sketch: Record<'_, '_>,
    ) -> Result<Option<(Vec<u32>, u32)>, CodecError> {
        let mut payload = sketch.payload;
        if payload.u8() != Some(0x3b) {
            return Ok(None);
        }
        let Some(count) = compact(&mut payload).and_then(|count| count.checked_sub(1)) else {
            return Ok(None);
        };
        let Some(count) = payload.counted(u64::from(count), 2) else {
            return Ok(None);
        };
        let mut members = allocate(ctx, count.get(), 0)?;
        for slot in 0..members.len() {
            if payload.u8() != Some(0x81) {
                return Ok(None);
            }
            let Some(id) = identity(&mut payload) else {
                return Ok(None);
            };
            let Some(member) = self.record(id) else {
                return Ok(None);
            };
            if member.owner != sketch.entity.entity_id || members[..slot].contains(&id) {
                return Ok(None);
            }
            members[slot] = id;
        }
        if payload.u8() != Some(0x81) {
            return Ok(None);
        }
        let Some(mut field) = schema(&mut payload) else {
            return Ok(None);
        };
        if Some(field) == self.schema_index("CurrentCenterLine") {
            let Some(center) = identity(&mut payload) else {
                return Ok(None);
            };
            let Some(center_record) = self.record(center) else {
                return Ok(None);
            };
            let Some(flags) = self.schema_index("MngFlgs") else {
                return Ok(None);
            };
            if !members.contains(&center)
                || names(center_record.entity, self.catalog) != Some(["2DLine", "Line"])
                || !matches!(geometry::flags(center_record.entity, flags), Ok(Some(flags)) if flags & 1 != 0)
            {
                return Ok(None);
            }
            let Some(next) = schema(&mut payload) else {
                return Ok(None);
            };
            field = next;
        }
        if Some(field) != self.schema_index("SolveManager") {
            return Ok(None);
        }
        let Some(solver) = identity(&mut payload).and_then(|id| self.record(id)) else {
            return Ok(None);
        };
        if !members.contains(&solver.entity.entity_id)
            || names(solver.entity, self.catalog) != Some(["SolveManager", "SolveManager"])
            || schema(&mut payload) != self.schema_index("AbsoluteAxis")
        {
            return Ok(None);
        }
        let Some(axis) = identity(&mut payload).and_then(|id| self.record(id)) else {
            return Ok(None);
        };
        if !members.contains(&axis.entity.entity_id)
            || names(axis.entity, self.catalog) != Some(["SketchSupport", "SketchSupport"])
        {
            return Ok(None);
        }
        Ok(Some((members, axis.entity.entity_id)))
    }

    fn line(
        &self,
        record: Record<'_, '_>,
        members: &[u32],
        data: u32,
        sense: u32,
        linear_tolerance: f64,
    ) -> Option<SheetLine> {
        let forward = match geometry::integer(record.entity, sense)? {
            1 => true,
            -1 => false,
            _ => return None,
        };
        let mut payload = record.payload;
        if schema(&mut payload)? != self.schema_index("StartPoint")? {
            return None;
        }
        let start_id = identity(&mut payload)?;
        if schema(&mut payload)? != self.schema_index("EndPoint")? {
            return None;
        }
        let end_id = identity(&mut payload)?;
        if payload.u8()? != 0xfe || !payload.is_empty() || start_id == end_id {
            return None;
        }
        let point = |id| {
            let point = self.record(id)?;
            (members.contains(&id)
                && point.owner == record.owner
                && names(point.entity, self.catalog) == Some(["2DPoint", "Point"]))
            .then(|| geometry::point(point.entity, data))?
        };
        let start = point(start_id)?;
        let end = point(end_id)?;
        let (origin, direction, interval) = geometry::line(record.entity, data)?;
        for (parameter, point) in interval.into_iter().zip([start, end]) {
            if (origin.u + parameter * direction.u - point.u)
                .hypot(origin.v + parameter * direction.v - point.v)
                > linear_tolerance
            {
                return None;
            }
        }
        Some(SheetLine {
            source_offset: record.entity.byte_offset,
            start_id,
            end_id,
            start,
            end,
            origin,
            direction,
            interval,
            forward,
        })
    }

    fn plan(
        &self,
        ctx: &DecodeContext<'_>,
        fill: Record<'_, '_>,
        linear_tolerance: f64,
    ) -> Result<Option<SheetPlan>, CodecError> {
        let (Some(boundary), Some(data), Some(sense), Some(flags)) = (
            self.schema_index("Boundary"),
            self.schema_index("DataBloc"),
            self.schema_index("Sense"),
            self.schema_index("MngFlgs"),
        ) else {
            return Ok(None);
        };
        let mut payload = fill.payload;
        if schema(&mut payload) != Some(boundary) {
            return Ok(None);
        }
        let Some(boundaries) = self.persistent_members(ctx, &mut payload)? else {
            return Ok(None);
        };
        let [(sketch_id, _)] = boundaries.as_slice() else {
            return Ok(None);
        };
        if !self.fill_metadata(payload, fill.entity.entity_id) {
            return Ok(None);
        }
        let Some(sketch) = self.record(*sketch_id) else {
            return Ok(None);
        };
        if names(sketch.entity, self.catalog) != Some(["PRTSketch", "Sketch"])
            || !self.parent_contains(ctx, fill, sketch)?
        {
            return Ok(None);
        }
        let Some((members, axis_id)) = self.sketch_members(ctx, sketch)? else {
            return Ok(None);
        };
        let mut chart = None;
        let mut lines = allocate(ctx, members.len(), None::<SheetLine>)?;
        let mut line_count = 0;
        for id in &members {
            let Some(member) = self.record(*id) else {
                return Ok(None);
            };
            let Some(kind) = names(member.entity, self.catalog) else {
                return Ok(None);
            };
            let Ok(member_flags) = geometry::flags(member.entity, flags) else {
                return Ok(None);
            };
            if member_flags.is_some_and(|flags| flags & 3 != 0)
                && matches!(
                    kind,
                    ["2DLine", "Line"] | ["2DPoint", "Point"] | [_, "ProjectedGeomSet"]
                )
            {
                continue;
            }
            match kind {
                ["SketchSupport", "SketchSupport"] => {
                    if *id != axis_id {
                        return Ok(None);
                    }
                    let Some(value) = geometry::chart(member.entity, data) else {
                        return Ok(None);
                    };
                    if chart.replace(value).is_some() {
                        return Ok(None);
                    }
                }
                ["2DLine", "Line"] => {
                    let Some(line) = self.line(member, &members, data, sense, linear_tolerance)
                    else {
                        return Ok(None);
                    };
                    lines[line_count] = Some(line);
                    line_count += 1;
                }
                ["SolveManager", "SolveManager"]
                | ["2DPoint", "Point"]
                | ["ConstraintDYS", "ConstraintDYS"]
                | ["FeatureFSUR", "FeatureFSUR"] => {}
                _ => return Ok(None),
            }
        }
        let Some((origin, u_axis, v_axis)) = chart else {
            return Ok(None);
        };
        lines.truncate(line_count);
        let components = components(ctx, &lines)?;
        if components.is_empty() {
            return Ok(None);
        }
        Ok(Some(SheetPlan {
            source_offset: fill.entity.byte_offset,
            origin,
            u_axis,
            v_axis,
            components,
        }))
    }

    fn fill_metadata(&self, mut payload: View<'_>, fill_id: u32) -> bool {
        let Some(deviation) = self.schema_index("Deviation") else {
            return false;
        };
        let Some(component) = self.schema_index("Component") else {
            return false;
        };
        if schema(&mut payload) != Some(deviation) {
            return false;
        }
        let Some(parameter) = identity(&mut payload) else {
            return false;
        };
        if schema(&mut payload) != Some(component)
            || compact(&mut payload) != Some(2)
            || payload.u8() != Some(0x81)
            || identity(&mut payload) != Some(parameter)
            || payload.u8() != Some(0x81)
            || payload.u8() != Some(0xfe)
            || !payload.is_empty()
        {
            return false;
        }
        self.record(parameter).is_some_and(|record| {
            record.owner == fill_id
                && names(record.entity, self.catalog) == Some(["LENGTH", "Deviation"])
        })
    }
}

fn ports(line: &SheetLine) -> (u32, u32) {
    if line.forward {
        (line.start_id, line.end_id)
    } else {
        (line.end_id, line.start_id)
    }
}

fn vertex(line: &SheetLine) -> Point2 {
    if line.forward {
        line.start
    } else {
        line.end
    }
}

fn components(
    ctx: &DecodeContext<'_>,
    lines: &[Option<SheetLine>],
) -> Result<Vec<Vec<SheetLine>>, CodecError> {
    if lines.len() > MAX_PLANAR_BOUNDARY_POINTS {
        return Ok(Vec::new());
    }
    let mut seen = allocate(ctx, lines.len(), false)?;
    let mut path = allocate(ctx, lines.len(), 0usize)?;
    let mut result = allocate(ctx, lines.len() / 3, Vec::<SheetLine>::new())?;
    let mut component_count = 0;
    for seed in 0..lines.len() {
        if seen[seed] {
            continue;
        }
        let Some(first) = lines[seed] else { continue };
        let initial = ports(&first).0;
        let mut current = seed;
        let mut count = 0;
        let closed = loop {
            if seen[current] {
                break false;
            }
            seen[current] = true;
            path[count] = current;
            count += 1;
            let Some(line) = lines[current] else {
                break false;
            };
            let (start, end) = ports(&line);
            if lines
                .iter()
                .flatten()
                .filter(|line| ports(line).0 == start)
                .count()
                != 1
                || lines
                    .iter()
                    .flatten()
                    .filter(|line| ports(line).1 == start)
                    .count()
                    != 1
            {
                break false;
            }
            if end == initial {
                break count >= 3;
            }
            let mut next = lines
                .iter()
                .enumerate()
                .filter(|(_, line)| line.as_ref().is_some_and(|line| ports(line).0 == end));
            let (Some((index, _)), None) = (next.next(), next.next()) else {
                break false;
            };
            current = index;
        };
        if closed {
            let mut component = allocate(ctx, count, first)?;
            for (slot, index) in component.iter_mut().zip(&path[..count]) {
                let Some(line) = lines[*index] else {
                    return Ok(Vec::new());
                };
                *slot = line;
            }
            result[component_count] = component;
            component_count += 1;
        }
    }
    result.truncate(component_count);
    if !valid_components(ctx, &result)? {
        result.clear();
    }
    for line in lines.iter().flatten() {
        if result
            .iter()
            .flatten()
            .any(|selected| selected.source_offset == line.source_offset)
        {
            continue;
        }
        for component in &result {
            for (a, b) in component
                .iter()
                .zip(component.iter().cycle().skip(1))
                .take(component.len())
            {
                if segments_intersect_or_touch(
                    line.start,
                    line.end,
                    vertex(a),
                    vertex(b),
                    CONTOUR_TOLERANCE,
                ) {
                    return Ok(Vec::new());
                }
            }
        }
    }
    Ok(result)
}

fn valid_components(
    ctx: &DecodeContext<'_>,
    components: &[Vec<SheetLine>],
) -> Result<bool, CodecError> {
    let mut polygons = allocate(ctx, components.len(), Vec::<Point2>::new())?;
    for (polygon, component) in polygons.iter_mut().zip(components) {
        let Some(first) = component.first() else {
            return Ok(false);
        };
        *polygon = allocate(ctx, component.len(), vertex(first))?;
        for (slot, line) in polygon.iter_mut().zip(component) {
            *slot = vertex(line);
        }
    }
    let scale = polygons
        .iter()
        .flatten()
        .flat_map(|point| [point.u.abs(), point.v.abs()])
        .fold(1.0, f64::max);
    let coordinate_tolerance = CONTOUR_TOLERANCE * scale;
    let area_tolerance = coordinate_tolerance * scale;
    for (outer, component) in components.iter().enumerate() {
        let area = component
            .iter()
            .zip(component.iter().cycle().skip(1))
            .take(component.len())
            .map(|(a, b)| {
                let a = vertex(a);
                let b = vertex(b);
                a.u * b.v - a.v * b.u
            })
            .sum::<f64>();
        if !area.is_finite() || area.abs() <= area_tolerance {
            return Ok(false);
        }
        for (left, a) in component.iter().enumerate() {
            let a0 = vertex(a);
            let a1 = vertex(&component[(left + 1) % component.len()]);
            if (a1.u - a0.u).hypot(a1.v - a0.v) <= coordinate_tolerance {
                return Ok(false);
            }
            for (other, candidate) in components.iter().enumerate().skip(outer) {
                for (right, b) in candidate.iter().enumerate() {
                    if outer == other
                        && (left == right
                            || (left + 1) % component.len() == right
                            || (right + 1) % component.len() == left)
                    {
                        continue;
                    }
                    let b0 = vertex(b);
                    let b1 = vertex(&candidate[(right + 1) % candidate.len()]);
                    if segments_intersect_or_touch(a0, a1, b0, b1, coordinate_tolerance) {
                        return Ok(false);
                    }
                }
            }
        }
        for (index, other) in components.iter().enumerate().skip(outer + 1) {
            let polygon = &polygons[index];
            let reverse = &polygons[outer];
            if strictly_inside_planar_polygon(vertex(&component[0]), polygon, coordinate_tolerance)
                || strictly_inside_planar_polygon(vertex(&other[0]), reverse, coordinate_tolerance)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(super) fn plans(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    native: &CatiaNative,
    scope: &ModelingGraphScope,
    linear_tolerance: f64,
) -> Result<Vec<SheetPlan>, CodecError> {
    if !linear_tolerance.is_finite() || linear_tolerance <= 0.0 {
        return Ok(Vec::new());
    }
    let linear_tolerance = ENDPOINT_TOLERANCE.min(linear_tolerance);
    let catalog = |graph: &CatiaObjectGraph| {
        let id = graph.catalog.as_deref()?;
        let mut catalogs = native.catalogs.iter().filter(|catalog| catalog.id == id);
        let (Some(catalog), None) = (catalogs.next(), catalogs.next()) else {
            return None;
        };
        Some(catalog)
    };
    let fill_count = native
        .object_graphs
        .iter()
        .filter(|graph| scope.contains(&graph.id))
        .filter_map(|graph| catalog(graph).map(|catalog| (graph, catalog)))
        .map(|(graph, catalog)| {
            native
                .entity_records
                .iter()
                .filter(|entity| {
                    entity.object_graph == graph.id
                        && names(entity, catalog) == Some(["GSMFill", "GSMFill"])
                })
                .count()
        })
        .sum();
    if fill_count == 0 {
        return Ok(Vec::new());
    }
    let mut result = allocate(
        ctx,
        fill_count,
        SheetPlan {
            source_offset: 0,
            origin: Point3::new(0.0, 0.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
            v_axis: Vector3::new(0.0, 1.0, 0.0),
            components: Vec::new(),
        },
    )?;
    let mut count = 0;
    for graph in &native.object_graphs {
        if !scope.contains(&graph.id)
            || native
                .object_graphs
                .iter()
                .filter(|other| other.id == graph.id)
                .count()
                != 1
        {
            continue;
        }
        let Some(catalog) = catalog(graph) else {
            continue;
        };
        if !native.entity_records.iter().any(|entity| {
            entity.object_graph == graph.id
                && names(entity, catalog) == Some(["GSMFill", "GSMFill"])
        }) {
            continue;
        }
        let Some(graph_view) = framed(source, graph.byte_offset, graph.byte_len, 8) else {
            continue;
        };
        let selected = || {
            native
                .entity_records
                .iter()
                .filter(|entity| entity.object_graph == graph.id)
        };
        let Some(first) = selected().next() else {
            continue;
        };
        let mut entities = allocate(ctx, selected().count(), first)?;
        for (slot, entity) in entities.iter_mut().zip(selected()) {
            *slot = entity;
        }
        entities.sort_unstable_by_key(|entity| entity.entity_id);
        let graph = Graph {
            root: graph,
            catalog,
            entities,
            source: graph_view,
        };
        for entity in &graph.entities {
            if names(entity, catalog) != Some(["GSMFill", "GSMFill"]) {
                continue;
            }
            let Some(fill) = graph.record(entity.entity_id) else {
                continue;
            };
            if let Some(plan) = graph.plan(ctx, fill, linear_tolerance)? {
                result[count] = plan;
                count += 1;
            }
        }
    }
    result.truncate(count);
    Ok(result)
}

#[cfg(test)]
mod tests;
