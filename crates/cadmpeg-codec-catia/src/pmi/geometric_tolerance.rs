// SPDX-License-Identifier: Apache-2.0
//! Source-scoped inline geometric tolerances and datum systems.

use std::num::NonZeroU32;

use cadmpeg_core::decode::{index_from_u32, index_from_u64, u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationStorage;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PmiId;
use cadmpeg_ir::index::{ModelIndex, StandardIndex};
use cadmpeg_ir::pmi::{
    DatumReference, GeometricToleranceKind, PmiAnnotation, PmiDefinition, PmiMagnitude,
    PmiQuantity, PmiTarget, PmiValue,
};

use super::source::{entry, framed, identity, same_entity_frame, schema};
use crate::decode::ModelingGraphScope;
use crate::native::entity_record::{CatiaEntityRecord, CatiaEntityRecordBody};
use crate::native::{CatiaCatalog, CatiaNative, CatiaObjectGraph, CatiaObjectRecord};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

const OPERATION: &str = "transfer inline CATIA geometric tolerances";
const MAX_INLINE_VALUES: usize = 79;
const PROVENANCE_STREAM: &str = "catia:outer-pmi";
const PROVENANCE_TAG: &str = "source-closed-inline-PMI";

#[derive(Default, Clone, Copy)]
pub(crate) struct Transfer {
    pub(crate) annotations: usize,
    pub(crate) main_tolerances: usize,
    pub(crate) secondary_tolerances: usize,
    pub(crate) unresolved_bindings: usize,
    pub(crate) unresolved_target_bindings: usize,
    pub(crate) unresolved_datum_systems: usize,
    pub(crate) unresolved_composite_links: usize,
    pub(crate) unresolved_semantics: usize,
    pub(crate) unresolved_field_keys: usize,
    pub(crate) unresolved_identity_collisions: usize,
    pub(crate) unresolved_candidates: usize,
    pub(crate) unresolved_secondaries: usize,
}

#[derive(Clone, Copy)]
enum ReferenceSlot {
    Null,
    Opaque,
    Entity(u32),
}

impl ReferenceSlot {
    fn entity(self) -> Option<u32> {
        match self {
            Self::Null | Self::Opaque => None,
            Self::Entity(id) => Some(id),
        }
    }
}

enum Admission {
    New,
    Existing,
    Collision,
}

fn compact(view: &mut View<'_>) -> Option<u32> {
    match view.u8()? {
        code @ 0x80..=0xd0 => Some(u32::from(code - 0x80)),
        code @ 0xd1..=0xe4 => Some(u32::from(code - 0xd1) * 256 + u32::from(view.u8()?) + 1),
        _ => None,
    }
}

fn inline(entity: &CatiaEntityRecord) -> Option<View<'_>> {
    match &entity.body {
        CatiaEntityRecordBody::Inline(bytes) => Some(View::over_retained(bytes)),
        CatiaEntityRecordBody::Nested { .. } => None,
    }
}

fn inline_type<'a>(entity: &CatiaEntityRecord, catalog: &'a CatiaCatalog) -> Option<&'a str> {
    let mut view = inline(entity)?;
    if view.u8()? != 3 {
        return None;
    }
    entry(catalog, schema(&mut view)?)?;
    let name = entry(catalog, schema(&mut view)?)?;
    (view.u8()? == 0xea && view.u32_le()? == entity.entity_id).then_some(name)
}

#[derive(Clone, Copy)]
struct Group<'a> {
    view: View<'a>,
    count: usize,
}

fn group<'a>(view: &mut View<'a>) -> Option<Group<'a>> {
    let count = u32::from(view.u8()?.checked_sub(0x81)?);
    if index_from_u32(count) > MAX_INLINE_VALUES {
        return None;
    }
    let count = view.counted(u64::from(count), 1)?.get();
    let start = view.position();
    for _ in 0..count {
        match view.unread().first()? {
            0xfe => view.skip(1)?,
            0xe5 => {
                view.skip(1)?;
                let length = index_from_u32(view.u32_le()?);
                view.skip(length)?;
                if view.u8()? != 0x81 {
                    return None;
                }
            }
            _ => {
                identity(view)?;
            }
        }
    }
    Some(Group {
        view: view.child(start, view.position())?,
        count,
    })
}

impl Group<'_> {
    fn reference(self, index: usize) -> Option<ReferenceSlot> {
        if index >= self.count {
            return None;
        }
        let mut view = self.view;
        for at in 0..=index {
            let value = if view.unread().first()? == &0xfe {
                view.skip(1)?;
                ReferenceSlot::Null
            } else if view.unread().first()? == &0xe5 {
                view.skip(1)?;
                let size = index_from_u32(view.u32_le()?);
                view.skip(size)?;
                if view.u8()? != 0x81 {
                    return None;
                }
                ReferenceSlot::Opaque
            } else {
                ReferenceSlot::Entity(identity(&mut view)?)
            };
            if at == index {
                return Some(value);
            }
        }
        None
    }

    fn single(self) -> Option<u32> {
        if self.count != 1 {
            return None;
        }
        self.reference(0)?.entity()
    }
}

#[derive(Clone, Copy)]
struct Field<'a> {
    schema: [u32; 2],
    schemas: [[u32; 2]; MAX_INLINE_VALUES],
    schema_count: usize,
    schema_position: usize,
    references: Group<'a>,
    parameters: Group<'a>,
}

fn field<'a>(body: &'a [u8], catalog: &CatiaCatalog, names: [&str; 2]) -> Option<Field<'a>> {
    let mut view = View::over_retained(body);
    if view.u8()? != 0x10 {
        return None;
    }
    identity(&mut view)?;
    let expected_fields = identity(&mut view)?;
    if index_from_u32(expected_fields) > MAX_INLINE_VALUES {
        return None;
    }
    let mut schemas = [[0; 2]; MAX_INLINE_VALUES];
    let mut selected = None;
    let mut fields_read = 0_u32;
    while !view.is_empty() {
        let schema = [schema(&mut view)?, schema(&mut view)?];
        *schemas.get_mut(index_from_u32(fields_read))? = schema;
        let fields = [entry(catalog, schema[0])?, entry(catalog, schema[1])?];
        let references = group(&mut view)?;
        let parameters = group(&mut view)?;
        let roles = view.u8()?;
        fields_read = fields_read.checked_add(1)?;
        let populated = u8::from(references.count > 0) * 2 + u8::from(parameters.count > 0) * 4;
        if !matches!(roles, 0 | 2 | 4 | 6) || populated & roles != populated {
            return None;
        }
        if fields == names
            && selected
                .replace((
                    schema,
                    references,
                    parameters,
                    index_from_u32(fields_read.checked_sub(1)?),
                ))
                .is_some()
        {
            return None;
        }
    }
    if fields_read != expected_fields {
        return None;
    }
    let (schema, references, parameters, schema_position) = selected?;
    Some(Field {
        schema,
        schemas,
        schema_count: index_from_u32(fields_read),
        schema_position,
        references,
        parameters,
    })
}

#[derive(Clone, Copy)]
struct Scalars {
    values: [u32; MAX_INLINE_VALUES],
    count: usize,
    key: Option<u32>,
}

fn scalar_declaration(entity: &CatiaEntityRecord, selected: &Field<'_>) -> Option<Scalars> {
    let body = inline(entity)?;
    let mut header = body;
    if header.u8()? != 3 {
        return None;
    }
    schema(&mut header)?;
    schema(&mut header)?;
    if header.u8()? != 0xea || header.u32_le()? != entity.entity_id {
        return None;
    }
    let mut at = header.position();
    let mut result = None;
    while at < body.end() {
        let mut opaque = body.child(at, body.end())?;
        if opaque.u8()? == 0x8e && matches!(opaque.u8(), Some(0xe8..=0xef)) {
            let code = *body
                .child(at.checked_add(1)?, body.end())?
                .unread()
                .first()?;
            if opaque.u8()? == 0x84 {
                at = at.checked_add(3 + usize::from(code - 0xe7))?;
                if at > body.end() {
                    return None;
                }
                continue;
            }
        }
        let mut candidate = body.child(at, body.end())?;
        let found = (|| {
            if [schema(&mut candidate)?, schema(&mut candidate)?] != selected.schema {
                return None;
            }
            let count = usize::from(candidate.u8()?.checked_sub(0x81)?);
            if count > MAX_INLINE_VALUES {
                return None;
            }
            candidate.counted(u64_from_index(count), 1)?;
            let mut values = [0; MAX_INLINE_VALUES];
            for value in values.iter_mut().take(count) {
                *value = identity(&mut candidate)?;
            }
            if candidate.array::<2>()? != [0x81, 0x81] {
                return None;
            }
            let attributes = usize::from(candidate.u8()?.checked_sub(0x81)?);
            if attributes > MAX_INLINE_VALUES {
                return None;
            }
            candidate.counted(u64_from_index(attributes), 1)?;
            for _ in 0..attributes {
                identity(&mut candidate)?;
            }
            if candidate.array::<2>()? != [0x81, 0x81]
                || !matches!(candidate.u8()?, 2 | 0x12)
                || candidate.array::<4>()? != [0xd5, 0, 0x82, 0x82]
            {
                return None;
            }
            let key = match candidate.u8()? {
                0x81 => None,
                0x80 => Some(candidate.u32_le()?),
                _ => return None,
            };
            if selected.schema_position + 1 < selected.schema_count {
                if [schema(&mut candidate)?, schema(&mut candidate)?]
                    != selected.schemas[selected.schema_position + 1]
                {
                    return None;
                }
            } else if !candidate.is_empty() {
                return None;
            }
            Some(Scalars { values, count, key })
        })();
        if let Some(found) = found {
            if result.replace(found).is_some() {
                return None;
            }
        }
        let mut token = body.child(at, body.end())?;
        let mut pair = token;
        let known_pair = (|| {
            let pair_indices = [schema(&mut pair)?, schema(&mut pair)?];
            selected.schemas[..selected.schema_count]
                .contains(&pair_indices)
                .then_some(pair.position().checked_sub(at)?)
        })();
        let width = if let Some(width) = known_pair {
            width
        } else {
            match token.u8()? {
                0x32 | 0x80 => 5,
                0xe6 => 9,
                0xe5 => index_from_u32(token.u32_le()?).checked_add(5)?,
                0xfe if token.unread().first() == Some(&0xf6) => 18,
                0xd1..=0xe4 => 2,
                _ => 1,
            }
        };
        at = at.checked_add(width)?;
        if at > body.end() {
            return None;
        }
    }
    result
}

struct Graph<'a, 's> {
    root: &'a CatiaObjectGraph,
    catalog: &'a CatiaCatalog,
    entities: Vec<&'a CatiaEntityRecord>,
    source: View<'s>,
}

impl<'a> Graph<'a, '_> {
    fn record(&self, id: u32) -> Option<(&'a CatiaEntityRecord, &'a CatiaObjectRecord)> {
        let at = self
            .entities
            .binary_search_by_key(&id, |e| e.entity_id)
            .ok()?;
        if at
            .checked_sub(1)
            .and_then(|i| self.entities.get(i))
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
            || object.entity_id() != Some(id)
            || object.entity_record() != Some(entity.id.as_str())
        {
            return None;
        }
        same_entity_frame(self.source, entity)?;
        let frame = framed(self.source, object.byte_offset, object.byte_len, 9)?;
        if let Some(body) = &object.inline_body {
            if frame.unread() != body {
                return None;
            }
        }
        Some((entity, object))
    }

    fn selected(&self, id: u32, names: [&str; 2]) -> Option<Field<'a>> {
        let (_, object) = self.record(id)?;
        field(object.inline_body.as_deref()?, self.catalog, names)
    }

    fn targets(
        &self,
        ctx: &DecodeContext<'_>,
        entity: &CatiaEntityRecord,
    ) -> Result<(Vec<PmiTarget>, usize), CodecError> {
        let Some(field) = self
            .selected(entity.entity_id, ["TTRSBloc", "CATTPSTTRSData"])
            .filter(|field| field.parameters.count == 0 && field.references.count > 0)
        else {
            return Ok((Vec::new(), 1));
        };
        let mut selected = ctx.alloc_filled(field.references.count, None, OPERATION)?;
        let mut used = 0;
        let mut unresolved = 0;
        for at in 0..field.references.count {
            let candidate = (|| {
                let (target, _) = self.record(field.references.reference(at)?.entity()?)?;
                if inline_type(target, self.catalog)? != "CATMmrTTRS" {
                    return None;
                }
                Some(PmiTarget::ShapeAspect {
                    source_id: cadmpeg_core::text::NonBlankString::new(target.id.clone())?,
                })
            })();
            if let Some(candidate) = candidate {
                selected[used] = Some(candidate);
                used += 1;
            } else {
                unresolved += 1;
            }
        }
        let Some(first) = selected.first().and_then(Option::as_ref) else {
            return Ok((Vec::new(), unresolved));
        };
        let mut targets = ctx.alloc_filled(used, first.clone(), OPERATION)?;
        for (slot, target) in targets
            .iter_mut()
            .zip(selected.into_iter().take(used).flatten())
        {
            *slot = target;
        }
        Ok((targets, unresolved))
    }

    fn parameter(&self, id: u32, owner: u32, role: &str) -> Option<&'a CatiaEntityRecord> {
        let (entity, object) = self.record(id)?;
        let CatiaEntityRecordBody::Nested {
            definition_prefix,
            definition_suffix,
            ..
        } = &entity.body
        else {
            return None;
        };
        if entity.lead != 2 {
            return None;
        }
        let mut definition = View::over_retained(definition_prefix);
        if !matches!(
            definition.array::<2>()?,
            [0x00 | 0x10, 0x00 | 0x04 | 0x08 | 0x0c] | [1, 4] | [8, 8]
        ) {
            return None;
        }
        let kind = entry(self.catalog, schema(&mut definition)?)?;
        let expected = if role == "Label" || role.ends_with("TabValue") {
            "String"
        } else {
            "LENGTH"
        };
        if kind != expected
            || entry(self.catalog, schema(&mut definition)?)? != role
            || !definition.is_empty()
        {
            return None;
        }
        let mut frame = framed(self.source, object.byte_offset, object.byte_len, 9)?;
        if frame.array::<2>()? != [0x14, 1] {
            return None;
        }
        let class = compact(&mut frame)?;
        if identity(&mut frame)? != owner
            || frame.array::<2>()? != [0x7c, 0x0a]
            || frame.u32_le()? != 7
            || frame.u8()? != 0xfe
            || !frame.is_empty()
        {
            return None;
        }
        let suffix = View::over_retained(definition_suffix);
        let mut classes = 0;
        for offset in suffix.end().checked_sub(6)?..suffix.end() {
            let mut tail = suffix.child(offset, suffix.end())?;
            if tail.u8()? == 3 && compact(&mut tail) == Some(class) && tail.is_empty() {
                classes += 1;
            }
        }
        (classes == 1).then_some(entity)
    }

    /// A `LENGTH` magnitude and the exactness of its encoding: a stored
    /// binary64 is byte-exact, and an `E7` or `E8` control is derived.
    fn magnitude(&self, id: u32, owner: u32, role: &str) -> Option<(PmiMagnitude, Exactness)> {
        let entity = self.parameter(id, owner, role)?;
        let CatiaEntityRecordBody::Nested {
            definition_prefix,
            record_suffix,
            ..
        } = &entity.body
        else {
            return None;
        };
        let mut definition = View::over_retained(definition_prefix);
        definition.skip(2)?;
        let kind = schema(&mut definition)?;
        if entry(self.catalog, kind)? != "LENGTH" {
            return None;
        }
        let mut suffix = View::over_retained(record_suffix);
        for _ in 0..3 {
            compact(&mut suffix)?;
        }
        if schema(&mut suffix)? != kind {
            return None;
        }
        let (value, exactness) = match suffix.u8()? {
            0xe6 => (suffix.f64_le()?, Exactness::ByteExact),
            0xe7 => (1.0, Exactness::Derived),
            0xe8 => (0.0, Exactness::Derived),
            _ => return None,
        };
        if !matches!(suffix.unread(), [0x81, 0x92 | 0x93 | 0x9b | 0xdb]) {
            return None;
        }
        Some((
            PmiMagnitude::new(PmiValue::new(value, PmiQuantity::Length)?)?,
            exactness,
        ))
    }

    fn tabulation(&self, id: u32, owner: u32, role: &str) -> Option<()> {
        let entity = self.parameter(id, owner, role)?;
        let CatiaEntityRecordBody::Nested {
            definition_prefix,
            value_payload,
            record_suffix,
            ..
        } = &entity.body
        else {
            return None;
        };
        if value_payload != &[0xfe] {
            return None;
        }
        let mut definition = View::over_retained(definition_prefix);
        definition.skip(2)?;
        let kind = schema(&mut definition)?;
        let mut suffix = View::over_retained(record_suffix);
        for _ in 0..3 {
            compact(&mut suffix)?;
        }
        if schema(&mut suffix)? != kind {
            return None;
        }
        match suffix.unread().first()? {
            0x37 => suffix.skip(1)?,
            0x32 => {
                entry(self.catalog, schema(&mut suffix)?)?;
            }
            _ => return None,
        }
        matches!(suffix.unread(), [0x81, 0x92 | 0x93 | 0x9b | 0xdb]).then_some(())
    }

    fn parameter_key(&self, id: u32, expected: Option<u32>) -> Option<()> {
        let Some(expected) = expected else {
            return Some(());
        };
        let (entity, _) = self.record(id)?;
        let CatiaEntityRecordBody::Nested {
            definition_suffix, ..
        } = &entity.body
        else {
            return None;
        };
        let suffix = View::over_retained(definition_suffix);
        let mut matches = 0;
        for offset in suffix.end().checked_sub(6)?..suffix.end() {
            let mut end = suffix.child(offset, suffix.end())?;
            if end.u8()? != 3 || compact(&mut end).is_none() || !end.is_empty() {
                continue;
            }
            for length in [11, 15] {
                let Some(start) = offset.checked_sub(length) else {
                    continue;
                };
                let mut words = suffix.child(start, offset)?;
                let matches_key = (|| {
                    if words.u8()? != 0x80 || words.u32_le()? != expected || words.u8()? != 0x80 {
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
                    words.is_empty().then_some(())
                })();
                if matches_key.is_some() {
                    matches += 1;
                }
            }
        }
        (matches == 1).then_some(())
    }

    /// Whether exactly one distinct member of the field carries the stored
    /// key, with every entity member resolved. A field without a stored key
    /// needs no carrier.
    fn field_key(&self, field: &Field<'_>, expected: Option<u32>) -> bool {
        let Some(expected) = expected else {
            return true;
        };
        let mut carriers = [None; 2];
        for group in [field.references, field.parameters] {
            for at in 0..group.count {
                let Some(id) = group.reference(at).and_then(ReferenceSlot::entity) else {
                    continue;
                };
                let Some((entity, _)) = self.record(id) else {
                    return false;
                };
                let matches = match &entity.body {
                    CatiaEntityRecordBody::Inline(_) => (|| {
                        let mut body = inline(entity)?;
                        if body.u8()? != 3 {
                            return None;
                        }
                        entry(self.catalog, schema(&mut body)?)?;
                        entry(self.catalog, schema(&mut body)?)?;
                        (body.u8()? == 0xea
                            && body.u32_le()? == id
                            && body.array::<2>()? == [0x81, 0x80]
                            && body.u32_le()? == expected)
                            .then_some(())
                    })(),
                    CatiaEntityRecordBody::Nested { .. } => self.parameter_key(id, Some(expected)),
                };
                if matches.is_some() && carriers[0] != Some(id) {
                    if carriers[0].is_none() {
                        carriers[0] = Some(id);
                    } else {
                        carriers[1] = Some(id);
                    }
                }
            }
        }
        matches!(carriers, [Some(_), None])
    }

    fn label(&self, id: u32, owner: u32) -> Option<&'a str> {
        let entity = self.parameter(id, owner, "Label")?;
        let CatiaEntityRecordBody::Nested {
            definition_prefix,
            record_suffix,
            ..
        } = &entity.body
        else {
            return None;
        };
        let mut definition = View::over_retained(definition_prefix);
        definition.skip(2)?;
        let kind = schema(&mut definition)?;
        if entry(self.catalog, kind)? != "String" {
            return None;
        }
        let mut suffix = View::over_retained(record_suffix);
        if suffix.array::<3>()? != [0x84, 0x93, 0x82] || schema(&mut suffix)? != kind {
            return None;
        }
        let label = entry(self.catalog, schema(&mut suffix)?)?;
        (!label.trim().is_empty() && matches!(suffix.unread(), [0x81, 0x92 | 0x93 | 0x9b | 0xdb]))
            .then_some(label)
    }
}

fn characteristic(kind: &str, value: u32) -> Option<GeometricToleranceKind> {
    use GeometricToleranceKind::{
        Angularity, Flatness, Parallelism, Perpendicularity, Position, SurfaceProfile,
    };
    match (kind, value) {
        ("CATTPSToleranceWithDRF", 0) => Some(Angularity),
        ("CATTPSToleranceWithDRF", 3) => Some(Perpendicularity),
        ("CATTPSToleranceWithDRF", 6) => Some(Parallelism),
        ("CATTPSToleranceWithDRF", 9 | 11 | 12) => Some(Position),
        ("CATTPSToleranceWithDRF", 21 | 22) => Some(SurfaceProfile),
        ("CATTPSToleranceWithoutDRF", 6) => Some(Flatness),
        ("CATTPSToleranceWithoutDRF", 16) => Some(SurfaceProfile),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct Datum<'a> {
    entity: &'a CatiaEntityRecord,
    label: &'a str,
}

#[derive(Clone, Copy)]
struct Frame<'a> {
    entity: &'a CatiaEntityRecord,
    datums: [Option<Datum<'a>>; MAX_INLINE_VALUES],
    transitions: [u32; MAX_INLINE_VALUES],
    count: usize,
}

impl Frame<'_> {
    fn admitted_prefix(&self, count: usize) -> Option<(&Self, usize)> {
        if count == 0
            || count > self.count
            || self.datums[..count].iter().any(Option::is_none)
            || self.transitions[..count - 1]
                .iter()
                .any(|v| !matches!(v, 1 | 2))
            || (count < self.count && self.transitions[count - 1] != 1)
        {
            return None;
        }
        Some((self, count))
    }
}

impl<'a> Graph<'a, '_> {
    fn datum_frame(&self, id: u32) -> Option<Frame<'a>> {
        let (entity, _) = self.record(id)?;
        if inline_type(entity, self.catalog)? != "CATTPSReferenceFrame" {
            return None;
        }
        let field = self.selected(id, ["FrameBloc", "CATTPSFrameData"])?;
        let declaration = scalar_declaration(entity, &field)?;
        if field.parameters.count != 0
            || field.references.count == 0
            || declaration.count.checked_add(1)? != field.references.count
        {
            return None;
        }
        let mut datums = [None; MAX_INLINE_VALUES];
        for at in 0..field.references.count {
            datums[at] = (|| {
                let (datum, _) = self.record(field.references.reference(at)?.entity()?)?;
                if inline_type(datum, self.catalog)? != "CATTPSDatum"
                    || datums[..at]
                        .iter()
                        .flatten()
                        .any(|stored: &Datum<'_>| stored.entity.entity_id == datum.entity_id)
                {
                    return None;
                }
                let label = self.selected(datum.entity_id, ["LabelBloc", "CATTPSCkeData"])?;
                if label.references.count != 0 {
                    return None;
                }
                Some(Datum {
                    entity: datum,
                    label: self.label(label.parameters.single()?, datum.entity_id)?,
                })
            })();
        }
        Some(Frame {
            entity,
            datums,
            transitions: declaration.values,
            count: field.references.count,
        })
    }
}

pub(super) fn identifier(offset: u64, role: &str, count: usize) -> Result<PmiId, CodecError> {
    PmiId::mint(format!("catia:model:pmi#{role}-{offset:010}-{count}"))
        .map_err(CodecError::malformed)
}

#[derive(Clone)]
struct Staged {
    annotation: PmiAnnotation,
    offset: u64,
    magnitude_exactness: Exactness,
}

struct Prepared<'a> {
    index: ModelIndex<'a>,
    records: Vec<Option<Staged>>,
    used: usize,
    limit: usize,
}

impl Prepared<'_> {
    fn rollback(&mut self, length: usize) {
        for slot in &mut self.records[length..self.used] {
            *slot = None;
        }
        self.used = length;
    }

    fn append(
        &mut self,
        ctx: &DecodeContext<'_>,
        annotation: PmiAnnotation,
        offset: u64,
        magnitude_exactness: Exactness,
        stats: &mut Transfer,
    ) -> Result<Option<bool>, CodecError> {
        let staged = self.records[..self.used]
            .iter()
            .flatten()
            .find(|record| record.annotation.id == annotation.id)
            .map(|record| &record.annotation);
        let existing = staged.or_else(|| {
            self.index
                .ir()
                .model
                .pmi
                .iter()
                .find(|p| p.id == annotation.id)
        });
        if let Some(existing) = existing {
            return Ok((existing == &annotation).then_some(false));
        }
        if self.index.contains(annotation.id.as_str(), StandardIndex) {
            return Ok(None);
        }
        if self.used == self.records.len() {
            let next = self
                .records
                .len()
                .checked_mul(2)
                .and_then(|n| n.checked_add(usize::from(n == 0)))
                .map(|n| n.min(self.limit))
                .filter(|n| *n > self.used)
                .ok_or_else(|| CodecError::malformed("PMI staging exceeds its source bound"))?;
            let mut records = ctx.alloc_filled(next, None, OPERATION)?;
            for (new, old) in records.iter_mut().zip(&mut self.records) {
                *new = old.take();
            }
            self.records = records;
        }
        self.records[self.used] = Some(Staged {
            annotation,
            offset,
            magnitude_exactness,
        });
        self.used += 1;
        stats.annotations += 1;
        Ok(Some(true))
    }
}

fn emit_frame(
    ctx: &DecodeContext<'_>,
    prepared: &mut Prepared<'_>,
    graph: &Graph<'_, '_>,
    frame: &Frame<'_>,
    count: usize,
    stats: &mut Transfer,
) -> Result<Option<PmiId>, CodecError> {
    let first =
        frame.datums[0].ok_or_else(|| CodecError::malformed("admitted datum disappeared"))?;
    let first_id = identifier(first.entity.byte_offset, "datum", 0)?;
    let mut references = ctx.alloc_filled(
        count,
        DatumReference {
            datum: first_id,
            precedence: NonZeroU32::MIN,
            common_group: None,
            modifiers: Vec::new(),
        },
        OPERATION,
    )?;
    let mut precedence = 1_u32;
    for (at, reference) in references.iter_mut().enumerate() {
        if at > 0 && frame.transitions[at - 1] == 1 {
            precedence = precedence
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("datum precedence exceeds its lane"))?;
        }
        let datum =
            frame.datums[at].ok_or_else(|| CodecError::malformed("admitted datum disappeared"))?;
        let id = identifier(datum.entity.byte_offset, "datum", 0)?;
        let (targets, unresolved_targets) = graph.targets(ctx, datum.entity)?;
        let target_count = targets.len();
        let admitted = prepared.append(
            ctx,
            PmiAnnotation {
                id: id.clone(),
                name: None,
                visible: None,
                targets,
                definition: PmiDefinition::Datum {
                    identification: datum.label.to_owned(),
                },
            },
            datum.entity.byte_offset,
            Exactness::ByteExact,
            stats,
        )?;
        let Some(admitted) = admitted else {
            return Ok(None);
        };
        if admitted {
            stats.unresolved_bindings += target_count;
            stats.unresolved_target_bindings += unresolved_targets;
        }
        let common = (at > 0 && frame.transitions[at - 1] == 2)
            || (at + 1 < count && frame.transitions[at] == 2);
        *reference = DatumReference {
            datum: id,
            precedence: NonZeroU32::new(precedence)
                .ok_or_else(|| CodecError::malformed("zero datum precedence"))?,
            common_group: common.then_some(precedence),
            modifiers: Vec::new(),
        };
    }
    let id = identifier(frame.entity.byte_offset, "datum-system", count)?;
    if prepared
        .append(
            ctx,
            PmiAnnotation {
                id: id.clone(),
                name: None,
                visible: None,
                targets: Vec::new(),
                definition: PmiDefinition::DatumSystem {
                    references: references.try_into().map_err(CodecError::malformed)?,
                },
            },
            frame.entity.byte_offset,
            Exactness::ByteExact,
            stats,
        )?
        .is_none()
    {
        return Ok(None);
    }
    Ok(Some(id))
}

struct Tolerance<'a> {
    entity: &'a CatiaEntityRecord,
    target_owner: &'a CatiaEntityRecord,
    kind: GeometricToleranceKind,
    magnitude: (PmiMagnitude, Exactness),
    frame: Option<(&'a Frame<'a>, usize)>,
    datum_system_unresolved: bool,
}

fn emit_tolerance(
    ctx: &DecodeContext<'_>,
    prepared: &mut Prepared<'_>,
    graph: &Graph<'_, '_>,
    tolerance: Tolerance<'_>,
    stats: &mut Transfer,
) -> Result<Admission, CodecError> {
    let id = identifier(tolerance.entity.byte_offset, "geometric-tolerance", 0)?;
    let saved_length = prepared.used;
    let saved_stats = *stats;
    let mut unresolved_datum_system = tolerance.datum_system_unresolved;
    let datum_system = if let Some((frame, count)) = tolerance.frame {
        match emit_frame(ctx, prepared, graph, frame, count, stats)? {
            Some(system) => Some(system),
            None => {
                prepared.rollback(saved_length);
                *stats = saved_stats;
                stats.unresolved_identity_collisions += 1;
                unresolved_datum_system = true;
                None
            }
        }
    } else {
        None
    };
    let (targets, unresolved_targets) = graph.targets(ctx, tolerance.target_owner)?;
    let target_count = targets.len();
    let emitted = prepared.append(
        ctx,
        PmiAnnotation {
            id,
            name: None,
            visible: None,
            targets,
            definition: PmiDefinition::GeometricTolerance {
                tolerance: tolerance.kind,
                magnitude: tolerance.magnitude.0,
                defined_unit: None,
                defined_area_unit: None,
                defined_area_second_unit: None,
                datum_system,
                modifiers: Vec::new(),
                constraints: cadmpeg_ir::pmi::zones::ToleranceConstraints::default(),
            },
        },
        tolerance.entity.byte_offset,
        tolerance.magnitude.1,
        stats,
    )?;
    let Some(emitted) = emitted else {
        prepared.rollback(saved_length);
        *stats = saved_stats;
        stats.unresolved_identity_collisions += 1;
        return Ok(Admission::Collision);
    };
    if emitted {
        stats.unresolved_bindings += target_count;
        stats.unresolved_target_bindings += unresolved_targets;
        stats.unresolved_datum_systems += usize::from(unresolved_datum_system);
        stats.unresolved_semantics += 1;
        Ok(Admission::New)
    } else {
        Ok(Admission::Existing)
    }
}

fn retained_size(annotation: &PmiAnnotation) -> Result<usize, CodecError> {
    let mut bytes = std::mem::size_of::<PmiAnnotation>();
    let mut add = |value: usize| -> Result<(), CodecError> {
        bytes = bytes
            .checked_add(value)
            .ok_or_else(|| CodecError::malformed("PMI retention exceeds its size lane"))?;
        Ok(())
    };
    add(annotation.id.as_str().len())?;
    for target in &annotation.targets {
        add(std::mem::size_of::<PmiTarget>())?;
        if let PmiTarget::ShapeAspect { source_id } = target {
            add(source_id.as_str().len())?;
        }
    }
    match &annotation.definition {
        PmiDefinition::Datum { identification } => add(identification.len())?,
        PmiDefinition::DatumSystem { references } => {
            for reference in references.as_slice() {
                add(std::mem::size_of::<DatumReference>())?;
                add(reference.datum.as_str().len())?;
            }
        }
        PmiDefinition::GeometricTolerance {
            datum_system: Some(system),
            ..
        } => {
            add(system.as_str().len())?;
        }
        _ => {}
    }
    add(std::mem::size_of::<(
        String,
        cadmpeg_ir::AnnotationProvenance,
    )>())?;
    add(annotation.id.as_str().len())?;
    add(std::mem::size_of::<cadmpeg_ir::StreamName>())?;
    add(std::mem::size_of::<usize>() * 2)?;
    add(PROVENANCE_STREAM.len())?;
    add(PROVENANCE_TAG.len())?;
    add(std::mem::size_of::<(
        String,
        cadmpeg_ir::annotations::ExactnessNote,
    )>())?;
    add(annotation.id.as_str().len())?;
    for (field, _) in exact_fields(annotation) {
        add(std::mem::size_of::<(
            cadmpeg_ir::annotations::FieldName,
            Exactness,
        )>())?;
        add(field.len())?;
    }
    Ok(bytes)
}

fn exact_fields(annotation: &PmiAnnotation) -> &'static [(&'static str, Exactness)] {
    match &annotation.definition {
        PmiDefinition::Datum { .. } => &[
            ("definition.identification", Exactness::ByteExact),
            ("targets", Exactness::Derived),
        ],
        PmiDefinition::DatumSystem { .. } => &[("definition.references", Exactness::Derived)],
        PmiDefinition::GeometricTolerance {
            datum_system: None, ..
        } => &[
            ("definition.tolerance", Exactness::Derived),
            ("definition.magnitude", Exactness::ByteExact),
            ("targets", Exactness::Derived),
        ],
        _ => &[
            ("definition.tolerance", Exactness::Derived),
            ("definition.magnitude", Exactness::ByteExact),
            ("definition.datum_system", Exactness::Derived),
            ("targets", Exactness::Derived),
        ],
    }
}

fn commit(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl AnnotationStorage>,
    records: Vec<Option<Staged>>,
    used: usize,
) -> Result<(), CodecError> {
    ctx.charge_entities(u64_from_index(used), OPERATION)?;
    for slot in &records[..used] {
        let record = slot
            .as_ref()
            .ok_or_else(|| CodecError::malformed("PMI staging slot is empty"))?;
        ctx.retain_bytes(
            u64_from_index(retained_size(&record.annotation)?),
            OPERATION,
        )?;
    }
    for record in records.into_iter().take(used).flatten() {
        let annotation = record.annotation;
        crate::assemble::annotate(
            annotations,
            &annotation.id,
            "outer-pmi",
            record.offset,
            PROVENANCE_TAG,
            Exactness::Derived,
        )?;
        for (field, exactness) in exact_fields(&annotation) {
            let exactness = if *field == "definition.magnitude" {
                record.magnitude_exactness
            } else {
                *exactness
            };
            annotations.field_exactness(&annotation.id, *field, exactness)?;
        }
        ir.model.pmi.push(annotation);
    }
    Ok(())
}

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl AnnotationStorage>,
    native: &CatiaNative,
    scope: &ModelingGraphScope,
) -> Result<Transfer, CodecError> {
    let mut stats = Transfer::default();
    let mut candidate_count = 0_usize;
    for root in native
        .object_graphs
        .iter()
        .filter(|g| scope.contains(g.id.as_str()))
    {
        let Some(catalog) = root
            .catalog
            .as_ref()
            .and_then(|id| native.catalogs.iter().find(|c| &c.id == id))
        else {
            continue;
        };
        let count = native
            .entity_records
            .iter()
            .filter(|e| e.object_graph == root.id)
            .filter(|e| {
                matches!(
                    inline_type(e, catalog),
                    Some("CATTPSToleranceWithDRF" | "CATTPSToleranceWithoutDRF")
                )
            })
            .count();
        candidate_count = candidate_count
            .checked_add(count)
            .ok_or_else(|| CodecError::malformed("PMI candidate count exceeds its lane"))?;
    }
    if candidate_count == 0 {
        return Ok(stats);
    }
    let limit = candidate_count
        .checked_mul(MAX_INLINE_VALUES + 4)
        .ok_or_else(|| CodecError::malformed("PMI output bound exceeds its lane"))?;
    let (records, used) = {
        let mut prepared = Prepared {
            index: ModelIndex::new_model_only(ir),
            records: Vec::new(),
            used: 0,
            limit,
        };
        for root in native
            .object_graphs
            .iter()
            .filter(|g| scope.contains(g.id.as_str()))
        {
            let Some(catalog) = root
                .catalog
                .as_ref()
                .and_then(|id| native.catalogs.iter().find(|c| &c.id == id))
            else {
                continue;
            };
            let in_scope = |e: &&CatiaEntityRecord| e.object_graph == root.id;
            let candidates = native
                .entity_records
                .iter()
                .filter(in_scope)
                .filter(|entity| {
                    matches!(
                        inline_type(entity, catalog),
                        Some("CATTPSToleranceWithDRF" | "CATTPSToleranceWithoutDRF")
                    )
                })
                .count();
            if candidates == 0 {
                continue;
            }
            let count = native.entity_records.iter().filter(in_scope).count();
            let mut entities = ctx.alloc_filled(count, &native.entity_records[0], OPERATION)?;
            for (slot, entity) in entities
                .iter_mut()
                .zip(native.entity_records.iter().filter(in_scope))
            {
                *slot = entity;
            }
            entities.sort_unstable_by_key(|entity| entity.entity_id);
            let graph = Graph {
                root,
                catalog,
                entities,
                source,
            };
            for entity in &graph.entities {
                let Some(kind_name) = inline_type(entity, catalog) else {
                    continue;
                };
                if !matches!(
                    kind_name,
                    "CATTPSToleranceWithDRF" | "CATTPSToleranceWithoutDRF"
                ) {
                    continue;
                }
                stats.unresolved_candidates += 1;
                let Some(field) =
                    graph.selected(entity.entity_id, ["TolBloc", "CATTPSToleranceData"])
                else {
                    continue;
                };
                let Some(declaration) = scalar_declaration(entity, &field).filter(|d| d.count == 5)
                else {
                    continue;
                };
                let Some(kind) = characteristic(kind_name, declaration.values[0]) else {
                    continue;
                };
                let main = (|| {
                    if field.parameters.count != 2 {
                        return None;
                    }
                    let number = field.parameters.reference(0)?.entity()?;
                    let tabulation = field.parameters.reference(1)?.entity()?;
                    graph.tabulation(tabulation, entity.entity_id, "TolTabValue")?;
                    graph.parameter_key(number, declaration.key)?;
                    graph.magnitude(number, entity.entity_id, "TolNumValue")
                })();
                let with_drf =
                    graph.selected(entity.entity_id, ["TolWiDRF", "CATTPSToleranceWiDRFData"]);
                let frame = if kind_name == "CATTPSToleranceWithDRF" {
                    with_drf
                        .and_then(|f| f.references.single())
                        .and_then(|id| graph.datum_frame(id))
                } else {
                    None
                };
                let main_frame = frame.as_ref().and_then(|f| f.admitted_prefix(f.count));
                if let Some(magnitude) = main {
                    match emit_tolerance(
                        ctx,
                        &mut prepared,
                        &graph,
                        Tolerance {
                            entity,
                            target_owner: entity,
                            kind: kind.clone(),
                            magnitude,
                            frame: main_frame,
                            datum_system_unresolved: kind_name == "CATTPSToleranceWithDRF"
                                && main_frame.is_none(),
                        },
                        &mut stats,
                    )? {
                        Admission::New => {
                            stats.main_tolerances += 1;
                            stats.unresolved_candidates -= 1;
                        }
                        Admission::Existing => stats.unresolved_candidates -= 1,
                        Admission::Collision => {}
                    }
                }
                if kind_name != "CATTPSToleranceWithDRF" {
                    continue;
                }
                let Some(with_drf) = with_drf else {
                    stats.unresolved_secondaries += 1;
                    continue;
                };
                let Some(secondary) =
                    scalar_declaration(entity, &with_drf).filter(|d| d.count == 4)
                else {
                    stats.unresolved_secondaries += 1;
                    continue;
                };
                let datum_count = index_from_u32(secondary.values[1]);
                if datum_count == 0 {
                    continue;
                }
                let secondary_frame = frame.as_ref().and_then(|f| f.admitted_prefix(datum_count));
                let staged = (|| {
                    let number = with_drf.parameters.reference(0)?.entity()?;
                    let tabulation = with_drf.parameters.reference(1)?.entity()?;
                    graph.tabulation(tabulation, entity.entity_id, "RefTolTabValue")?;
                    let magnitude = graph.magnitude(number, entity.entity_id, "RefTolNumValue")?;
                    let (number, _) = graph.record(number)?;
                    Some((number, magnitude))
                })();
                let Some((number, magnitude)) = staged else {
                    stats.unresolved_secondaries += 1;
                    continue;
                };
                let field_key_resolved = graph.field_key(&with_drf, secondary.key);
                if matches!(
                    emit_tolerance(
                        ctx,
                        &mut prepared,
                        &graph,
                        Tolerance {
                            entity: number,
                            target_owner: entity,
                            kind,
                            magnitude,
                            frame: secondary_frame,
                            datum_system_unresolved: secondary_frame.is_none(),
                        },
                        &mut stats
                    )?,
                    Admission::New
                ) {
                    stats.secondary_tolerances += 1;
                    stats.unresolved_composite_links += 1;
                    stats.unresolved_field_keys += usize::from(!field_key_resolved);
                }
            }
        }
        (prepared.records, prepared.used)
    };
    commit(ctx, ir, annotations, records, used)?;
    Ok(stats)
}

#[cfg(test)]
mod tests;
