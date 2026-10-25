// SPDX-License-Identifier: Apache-2.0
//! Naming carriers share their genuine XML value callback with property admission.

use crate::native::element_map::ElementMapRecord;
use crate::native::{PropertyRecord, StringTableRecord, ValueRecord};
use crate::xml_framing::XmlSource;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ExpandSpec, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

mod legacy;
mod markers;

pub(crate) struct PendingCheckpoint {
    pending: usize,
    contents: usize,
    interpretations: usize,
}

pub(crate) struct Context<'a, 'source> {
    pub(crate) entries: &'a BTreeMap<String, View<'source>>,
    pub(crate) file_version: Option<usize>,
    declared_entries: Option<&'a BTreeSet<String>>,
    declared_objects: RefCell<BTreeSet<String>>,
    admitted_objects: RefCell<BTreeSet<String>>,
    pub(crate) pending_properties:
        RefCell<Vec<crate::persistence::document_properties::pending::PendingProperty<'source>>>,
    pub(crate) parents:
        Option<&'a BTreeMap<String, cadmpeg_ir::source_fidelity::parts::PartParent>>,
    tables: RefCell<BTreeMap<usize, (String, Option<BTreeSet<i64>>)>>,
    unavailable_strings: RefCell<BTreeMap<usize, BTreeSet<i64>>>,
    marker_owners: RefCell<BTreeMap<String, markers::Owner<'source>>>,
    marker_metadata: RefCell<BTreeMap<String, BTreeMap<String, String>>>,
    unavailable_parts: RefCell<BTreeSet<String>>,
    whitespace: RefCell<Vec<View<'source>>>,
    pub(crate) payload_sources: RefCell<crate::gui::payloads::sources::PayloadSources<'source>>,
    pub(crate) entry_owners: Option<&'a crate::container::parts::EntryOwners<'source>>,
    pub(crate) entry_contents: RefCell<Vec<crate::native::entry_contents::EntryContentUpdate>>,
    pub(crate) entry_interpretations:
        RefCell<crate::gui::payloads::sources::EntryInterpretations<'source>>,
}

impl<'a, 'source> Context<'a, 'source> {
    pub(crate) fn new(
        entries: &'a BTreeMap<String, View<'source>>,
        file_version: Option<usize>,
    ) -> Self {
        Self {
            entries,
            file_version,
            declared_entries: None,
            declared_objects: RefCell::new(BTreeSet::new()),
            admitted_objects: RefCell::new(BTreeSet::new()),
            pending_properties: RefCell::new(Vec::new()),
            parents: None,
            tables: RefCell::new(BTreeMap::new()),
            unavailable_strings: RefCell::new(BTreeMap::new()),
            marker_owners: RefCell::new(BTreeMap::new()),
            marker_metadata: RefCell::new(BTreeMap::new()),
            unavailable_parts: RefCell::new(BTreeSet::new()),
            whitespace: RefCell::new(Vec::new()),
            payload_sources: RefCell::new(Default::default()),
            entry_owners: None,
            entry_contents: RefCell::new(Vec::new()),
            entry_interpretations: RefCell::new(Default::default()),
        }
    }
    pub(crate) fn with_entry_owners(
        mut self,
        owners: &'a crate::container::parts::EntryOwners<'source>,
    ) -> Self {
        self.entry_owners = Some(owners);
        self
    }
    pub(crate) fn take_entry_contents(
        &self,
    ) -> Vec<crate::native::entry_contents::EntryContentUpdate> {
        std::mem::take(&mut *self.entry_contents.borrow_mut())
    }
    pub(crate) fn take_entry_interpretations(
        &self,
    ) -> crate::gui::payloads::sources::EntryInterpretations<'source> {
        std::mem::take(&mut *self.entry_interpretations.borrow_mut())
    }
    pub(crate) fn take_payload_sources(
        &self,
    ) -> crate::gui::payloads::sources::PayloadSources<'source> {
        std::mem::take(&mut *self.payload_sources.borrow_mut())
    }
    pub(crate) fn set_declared_objects(&self, objects: BTreeSet<String>) {
        *self.declared_objects.borrow_mut() = objects;
    }
    pub(crate) fn has_declared_object(&self, id: &str) -> bool {
        self.declared_objects.borrow().contains(id)
    }
    pub(crate) fn set_admitted_objects(&self, objects: BTreeSet<String>) {
        *self.admitted_objects.borrow_mut() = objects;
    }
    pub(crate) fn has_admitted_object(&self, id: &str) -> bool {
        self.admitted_objects.borrow().contains(id)
    }
    pub(crate) fn pending_checkpoint(&self) -> PendingCheckpoint {
        PendingCheckpoint {
            pending: self.pending_properties.borrow().len(),
            contents: self.entry_contents.borrow().len(),
            interpretations: self.entry_interpretations.borrow().checkpoint(),
        }
    }
    pub(crate) fn rollback_pending(&self, checkpoint: PendingCheckpoint) {
        self.pending_properties
            .borrow_mut()
            .truncate(checkpoint.pending);
        self.entry_contents
            .borrow_mut()
            .truncate(checkpoint.contents);
        self.entry_interpretations
            .borrow_mut()
            .rollback(checkpoint.interpretations);
    }
    pub(crate) fn with_declared_entries(mut self, entries: &'a BTreeSet<String>) -> Self {
        self.declared_entries = Some(entries);
        self
    }
    pub(crate) fn has_declared_entry(&self, name: &str) -> bool {
        self.declared_entries
            .is_some_and(|entries| entries.contains(name))
    }
    pub(crate) fn with_parents(
        mut self,
        parents: &'a BTreeMap<String, cadmpeg_ir::source_fidelity::parts::PartParent>,
    ) -> Self {
        self.parents = Some(parents);
        self
    }
    pub(crate) fn take_source_whitespace(&self) -> Vec<View<'source>> {
        std::mem::take(&mut self.whitespace.borrow_mut())
    }
    fn spacing(&self, ctx: &DecodeContext<'_>, view: View<'source>) -> Result<(), CodecError> {
        let mut whitespace = self.whitespace.borrow_mut();
        if whitespace.len() == whitespace.capacity() {
            let additional = whitespace.capacity().max(8);
            let bytes = additional
                .checked_mul(size_of::<View<'source>>())
                .map(u64_from_index)
                .ok_or_else(|| {
                    CodecError::malformed("naming whitespace inventory storage overflows")
                })?;
            ctx.retain_bytes(bytes, "FreeCAD naming whitespace inventory")?;
            whitespace.try_reserve_exact(additional).map_err(|_| {
                cadmpeg_core::decode::ResourceLimit::allocation_failed(
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                    ctx.policy().limits.max_retained_bytes,
                    bytes,
                    "FreeCAD naming whitespace inventory",
                )
            })?;
        }
        whitespace.push(view);
        Ok(())
    }
    fn side_spec(&self, name: &str, level: &str) -> Result<PartSpec, CodecError> {
        let spec = empty_spec(level)?;
        Ok(match self.parents.and_then(|parents| parents.get(name)) {
            Some(parent) => spec.with_parent(parent),
            None => spec,
        })
    }
}

pub(crate) fn decode<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    index: usize,
    property: Option<&PropertyRecord>,
    previous: Option<&[ValueRecord]>,
    context: &Context<'_, 'source>,
    stage: &mut PartOutput,
) -> Result<bool, CodecError> {
    match source.name(index)? {
        "StringHasher" | "StringHasher2" => {
            hasher(ctx, source, index, property, context, stage)?;
            Ok(true)
        }
        "Part"
            if property.is_some_and(|property| property.type_name == "Part::PropertyPartShape") =>
        {
            let checked = validate_part(ctx, source, index);
            if checked.is_err() {
                if let Some(property) = property {
                    context
                        .unavailable_parts
                        .borrow_mut()
                        .insert(property.id.clone());
                }
            }
            checked?;
            Ok(false)
        }
        "ElementMap" | "ElementMap2" => {
            element_map(ctx, source, index, property, previous, context, stage)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn hasher<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    index: usize,
    property: Option<&PropertyRecord>,
    context: &Context<'_, 'source>,
    stage: &mut PartOutput,
) -> Result<(), CodecError> {
    let parent = source.elements()[index]
        .parent()
        .ok_or_else(|| CodecError::malformed("StringHasher has no parent"))?;
    let marker = if source.name(index)? == "StringHasher2" {
        let prior = source
            .children(parent)
            .take_while(|child| *child < index)
            .last()
            .ok_or_else(|| CodecError::malformed("StringHasher2 has no marker"))?;
        if source.name(prior)? != "StringHasher" {
            return Err(CodecError::malformed(
                "StringHasher2 is not the direct successor of its marker",
            ));
        }
        prior
    } else {
        if let Some(next) = source.children(parent).find(|child| *child > index) {
            if source.name(next)? == "StringHasher2" {
                let table_index = table_index(source, index);
                let id = crate::native::native_id("string-table", table_index.to_string());
                let prepared = markers::prepare(
                    ctx,
                    source,
                    index,
                    &id,
                    "StringHasher",
                    property,
                    context,
                    stage,
                );
                if prepared.is_err() {
                    context.tables.borrow_mut().insert(table_index, (id, None));
                }
                return prepared;
            }
        }
        index
    };
    let table_index = table_index(source, marker);
    let id = crate::native::native_id("string-table", table_index.to_string());
    let unknown_policy = context
        .marker_owners
        .borrow()
        .get(&id)
        .is_some_and(|owner| matches!(owner, markers::Owner::UnknownPolicy));
    let decoded = markers::record(ctx, source, index, marker, &id, context, || {
        source.validate(index)?;
        validate_carrier(source, index, "StringHasher", property)?;
        let spacing = |view| context.spacing(ctx, view);
        let attributes = known_attributes(ctx, source, index, &["file"])?;
        let metadata = if unknown_policy {
            BTreeMap::new()
        } else if marker == index {
            known_attributes(ctx, source, marker, &["new", "saveall", "threshold"])?
        } else {
            context
                .marker_metadata
                .borrow()
                .get(&id)
                .cloned()
                .ok_or_else(|| {
                    CodecError::malformed("StringHasher2 has no admitted marker metadata")
                })?
        };
        let new_marker = metadata
            .get("new")
            .map(|value| super::marker_enabled(value))
            .transpose()?
            .unwrap_or(false);
        if marker != index && !new_marker && !unknown_policy {
            return Err(CodecError::malformed(
                "StringHasher2 marker does not select the new layout",
            ));
        }
        if marker == index && new_marker {
            return Err(CodecError::malformed(
                "StringHasher marker has no direct StringHasher2 successor",
            ));
        }
        let (save_all, threshold) = if unknown_policy {
            (None, None)
        } else {
            (
                metadata
                    .get("saveall")
                    .map(|value| super::require_bool(value))
                    .transpose()?
                    .or(Some(false)),
                metadata
                    .get("threshold")
                    .map(|value| super::parse_decimal(value, "threshold"))
                    .transpose()?
                    .or(Some(0)),
            )
        };
        let entry_name = attributes.get("file").filter(|name| !name.is_empty());
        let mut unavailable_ids = BTreeSet::new();
        let entries = if let Some(name) = entry_name {
            if let Some(view) = context.entries.get(name).copied() {
                ctx.decode_part(
                    stage,
                    view,
                    context.side_spec(name, "freecad.string_table_entry")?,
                    |entry_stage| {
                        let count = if marker == index {
                            Some(
                                source
                                    .attribute(ctx, index, "count")?
                                    .map_or(Ok(0), |count| {
                                        super::parse_usize(&count, "StringHasher count")
                                    })?,
                            )
                        } else {
                            None
                        };
                        let table = super::parts::table(
                            ctx,
                            view,
                            count,
                            true,
                            property.is_some(),
                            Some(&spacing),
                            entry_stage,
                        )?;
                        Ok(table.map(|table| {
                            unavailable_ids = table.unavailable_ids;
                            table.entries
                        }))
                    },
                )?
                .flatten()
            } else {
                None
            }
        } else {
            let count = source
                .attribute(ctx, index, "count")?
                .map_or(Ok(0), |value| {
                    super::parse_usize(&value, "StringHasher count")
                })?;
            if count > super::MAX_TABLE_ENTRIES {
                return Err(CodecError::malformed(
                    "FreeCAD naming record count exceeds limit",
                ));
            }
            let view = character_view(ctx, source, index)?;
            super::parts::table(
                ctx,
                view,
                Some(count),
                false,
                property.is_some(),
                Some(&spacing),
                stage,
            )?
            .map(|table| {
                unavailable_ids = table.unavailable_ids;
                table.entries
            })
        };
        let table = StringTableRecord::try_sparse(
            table_index,
            property.map(|property| property.id.clone()),
            save_all,
            threshold,
            entry_name.cloned(),
            entries,
        )
        .map_err(CodecError::malformed)?;
        let ids = table.entries().map(|entries| {
            entries
                .iter()
                .flatten()
                .map(|entry| entry.string_id)
                .collect()
        });
        let table_id = table.id();
        context
            .unavailable_strings
            .borrow_mut()
            .insert(table_index, unavailable_ids);
        stage.insert_native(ctx, "string_tables", table)?;
        context
            .tables
            .borrow_mut()
            .insert(table_index, (table_id, ids));
        Ok(())
    });
    if decoded.is_err() {
        context.tables.borrow_mut().insert(table_index, (id, None));
    }
    decoded
}

fn element_map<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    index: usize,
    property: Option<&PropertyRecord>,
    previous: Option<&[ValueRecord]>,
    context: &Context<'_, 'source>,
    stage: &mut PartOutput,
) -> Result<(), CodecError> {
    let property = property
        .ok_or_else(|| CodecError::malformed("element map has no owning shape property"))?;
    if property.type_name != "Part::PropertyPartShape" {
        return Err(CodecError::malformed(
            "element map is not owned by an exact shape property",
        ));
    }
    let part_attributes = previous
        .unwrap_or_else(|| property.values())
        .iter()
        .find(|value| value.parent.is_none() && value.tag == "Part")
        .map(|value| value.attributes.clone());
    let unavailable_part =
        context.unavailable_parts.borrow().contains(&property.id) || part_attributes.is_none();
    let data = if source.name(index)? == "ElementMap" {
        source.elements()[index]
            .parent()
            .and_then(|parent| {
                source
                    .children(parent)
                    .find(|child| *child > index)
                    .filter(|next| source.name(*next).is_ok_and(|name| name == "ElementMap2"))
            })
            .unwrap_or(index)
    } else {
        index
    };
    let missing_payload = source
        .peek_attribute(ctx, data, "file")?
        .filter(|file| !file.is_empty())
        .is_some_and(|file| !context.entries.contains_key(&file));
    if unavailable_part || missing_payload {
        source.validate(index)?;
        validate_carrier(source, index, "ElementMap", Some(property))?;
        source.peek_attributes(ctx, index)?;
        if source.name(index)? == "ElementMap" {
            let new = source
                .peek_attribute(ctx, index, "new")?
                .map(|value| super::marker_enabled(&value))
                .transpose()?
                .unwrap_or(false);
            if new != (data != index) {
                return Err(CodecError::malformed(
                    "element-map marker and successor disagree",
                ));
            }
        }
        compatibility_elements(ctx, source, index, context, stage)?;
        return Ok(());
    }
    let parent = source.elements()[index]
        .parent()
        .ok_or_else(|| CodecError::malformed("element map has no parent"))?;
    let marker = if source.name(index)? == "ElementMap2" {
        let prior = source
            .children(parent)
            .take_while(|child| *child < index)
            .last()
            .ok_or_else(|| CodecError::malformed("ElementMap2 has no marker"))?;
        if source.name(prior)? != "ElementMap" {
            return Err(CodecError::malformed("ElementMap2 has no direct marker"));
        }
        prior
    } else {
        if let Some(next) = source.children(parent).find(|child| *child > index) {
            if source.name(next)? == "ElementMap2" {
                let id = crate::native::native_child_id("element-map", &property.id, "map");
                return markers::prepare(
                    ctx,
                    source,
                    index,
                    &id,
                    "ElementMap",
                    Some(property),
                    context,
                    stage,
                );
            }
        }
        index
    };
    let part = source
        .children(parent)
        .find(|child| source.name(*child).ok() == Some("Part"))
        .ok_or_else(|| CodecError::malformed("element map has no direct Part carrier"))?;
    if part >= marker {
        return Err(CodecError::malformed(
            "element map precedes its direct Part carrier",
        ));
    }
    if marker == index
        && source
            .peek_attribute(ctx, index, "file")?
            .is_none_or(|file| file.is_empty())
        && source
            .peek_attribute(ctx, index, "new")?
            .map(|value| super::marker_enabled(&value))
            .transpose()?
            .is_none_or(|new| !new)
        && source
            .peek_attribute(ctx, index, "count")?
            .map(|value| super::parse_usize(&value, "ElementMap count"))
            .transpose()?
            .unwrap_or(0)
            == 0
        && source.children(index).next().is_none()
        && source.peek_text(ctx, index)?.trim().is_empty()
    {
        source.validate(index)?;
        validate_carrier(source, index, "ElementMap", Some(property))?;
        xml_spacing(source, index, &|view| context.spacing(ctx, view))?;
        return Ok(());
    }
    let id = crate::native::native_child_id("element-map", &property.id, "map");
    let mut not_formed = false;
    markers::record(ctx, source, index, marker, &id, context, || {
        source.validate(index)?;
        validate_carrier(source, index, "ElementMap", Some(property))?;
        let spacing = |view| context.spacing(ctx, view);
        let attributes = known_attributes(ctx, source, index, &["file"])?;
        let marker_attributes = if marker == index {
            known_attributes(ctx, source, marker, &["new"])?
        } else {
            context
                .marker_metadata
                .borrow()
                .get(&id)
                .cloned()
                .ok_or_else(|| {
                    CodecError::malformed("ElementMap2 has no admitted marker metadata")
                })?
        };
        let new_marker = marker_attributes
            .get("new")
            .map(|value| super::marker_enabled(value))
            .transpose()?
            .unwrap_or(false);
        if (marker != index) != new_marker {
            return Err(CodecError::malformed(
                "element-map marker and successor disagree",
            ));
        }
        let part_attributes = part_attributes
            .as_ref()
            .ok_or_else(|| CodecError::malformed("element map has no admitted Part metadata"))?;
        let hasher_index = part_attributes
            .get("HasherIndex")
            .map(|value| super::parse_usize(value, "HasherIndex"))
            .transpose()?;
        let hasher_count = source
            .elements()
            .iter()
            .enumerate()
            .filter(|(index, _)| source.name(*index).is_ok_and(|name| name == "StringHasher"))
            .count();
        if hasher_index.is_some_and(|index| index >= hasher_count) {
            return Err(CodecError::malformed(
                "HasherIndex references an unavailable string table",
            ));
        }

        let table = hasher_index
            .map(|index| {
                context.tables.borrow().get(&index).cloned().ok_or_else(|| {
                    CodecError::malformed("HasherIndex needs an unavailable string table")
                })
            })
            .transpose()?;
        let unavailable = context.unavailable_strings.borrow();
        let symbols = super::parts::StringIds {
            known: table.as_ref().and_then(|(_, ids)| ids.as_ref()),
            unavailable: hasher_index.and_then(|index| unavailable.get(&index)),
        };
        let source_entry = attributes
            .get("file")
            .filter(|name| !name.is_empty())
            .cloned();
        let new = source.name(index)? == "ElementMap2";
        let count = if new || source_entry.is_none() {
            source
                .attribute(ctx, index, "count")?
                .map(|value| super::parse_usize(&value, "ElementMap count"))
                .transpose()?
        } else {
            None
        };
        let payload = if let Some(name) = source_entry.as_ref() {
            let view = *context
                .entries
                .get(name)
                .ok_or_else(|| CodecError::malformed("element-map side entry is missing"))?;
            let framing = super::parts::entry_framing(view);
            let spec = match &framing {
                Ok(fields) => super::parts::spec("freecad.element_map_entry", view, fields)?,
                Err(_) => empty_spec("freecad.element_map_entry")?,
            };
            let spec = match context.parents.and_then(|parents| parents.get(name)) {
                Some(parent) => spec.with_parent(parent),
                None => spec,
            };
            ctx.decode_part(stage, view, spec, |entry_stage| {
                framing?;
                if view
                    .window()
                    .split(|byte| byte.is_ascii_whitespace())
                    .find(|token| !token.is_empty())
                    == Some(b"BeginElementMap".as_slice())
                {
                    super::parts::map(ctx, view, true, symbols, Some(&spacing), entry_stage)
                } else {
                    let (count, records) = super::parts::legacy(
                        ctx,
                        view,
                        None,
                        symbols,
                        Some(&spacing),
                        entry_stage,
                    )?;
                    Ok(Some(
                        super::legacy_map_payload(records, count, None)?.parsed,
                    ))
                }
            })?
            .flatten()
        } else if !new && context.file_version.is_none() {
            return Err(CodecError::malformed(
                "inline legacy ElementMap requires a verified FileVersion",
            ));
        } else if !new && context.file_version.is_some_and(|version| version <= 1) {
            Some(legacy::elements(
                ctx,
                source,
                index,
                count.unwrap_or(0),
                symbols,
                &spacing,
                stage,
            )?)
        } else {
            let view = character_view(ctx, source, index)?;
            if new {
                super::parts::map(ctx, view, false, symbols, Some(&spacing), stage)?
            } else {
                let (count, records) = super::parts::legacy(
                    ctx,
                    view,
                    Some(count.unwrap_or(0)),
                    symbols,
                    Some(&spacing),
                    stage,
                )?;
                Some(super::legacy_map_payload(records, count, None)?.parsed)
            }
        };
        let Some(parsed) = payload else {
            not_formed = true;
            return Ok(());
        };
        let record = ElementMapRecord {
            id: id.clone(),
            property: property.id.clone(),
            version: part_attributes
                .get("ElementMap")
                .cloned()
                .unwrap_or_default(),
            hasher_index,
            string_table: table.map(|(id, _)| id),
            source_entry,
            map_id: parsed.map_id,
            declared_count: count
                .or_else(|| parsed.legacy_records.as_ref().map(Vec::len))
                .or(Some(if new {
                    super::mapped_name_count(&parsed)
                } else {
                    super::element_map_size(&parsed)?
                })),
            legacy_records: parsed.legacy_records,
            postfixes: parsed.postfixes,
            maps: parsed.maps,
        };
        stage.insert_native(ctx, "element_maps", record)
    })?;
    if not_formed {
        let home = unformed_map_home(stage)?;
        let authority = ctx.record_capture_authority(&id)?.ok_or_else(|| {
            CodecError::malformed("unformed map has no genuine producer authority")
        })?;
        ctx.retire_record_capture(&authority)?;
        stage.drop_with_given_up(ctx, home.as_str(), [id])?;
    }
    Ok(())
}

fn unformed_map_home(stage: &PartOutput) -> Result<cadmpeg_ir::ids::UnknownId, CodecError> {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    // These are required fields of the canonical map. A failed optional child
    // or name chain cannot become the home of a separately missing root node.
    let parts = stage.fidelity().parts();
    let required = parts
        .iter()
        .find(|(_, part)| {
            matches!(
                part.level().as_str(),
                "freecad.element_map_header"
                    | "freecad.inline_map_postfix_dictionary"
                    | "freecad.element_map_postfix_dictionary"
            ) && part.state() == &PartState::GivenUp {}
        })
        .or_else(|| {
            parts
                .iter()
                .filter(|(_, part)| {
                    matches!(
                        part.level().as_str(),
                        "freecad.inline_map_node" | "freecad.element_map_node"
                    )
                })
                .max_by_key(|(_, part)| part.offset())
                .filter(|(_, part)| part.state() == &PartState::GivenUp {})
        })
        .or_else(|| {
            parts.iter().find(|(_, part)| {
                part.level().as_str() == "freecad.element_map_entry"
                    && part.state() == &PartState::GivenUp {}
            })
        });
    required
        .map(|(id, _)| id.clone())
        .ok_or_else(|| CodecError::malformed("unformed map has no actual given-up required child"))
}

fn validate_part(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    index: usize,
) -> Result<(), CodecError> {
    source.validate(index)?;
    let parent = source.elements()[index]
        .parent()
        .ok_or_else(|| CodecError::malformed("Part has no owning property"))?;
    if source
        .children(parent)
        .filter(|child| source.name(*child).is_ok_and(|name| name == "Part"))
        .count()
        != 1
    {
        return Err(CodecError::malformed(
            "shape property has multiple direct Part carriers",
        ));
    }
    if let Some(value) = source.attribute(ctx, index, "HasherIndex")? {
        let index = super::parse_usize(&value, "HasherIndex")?;
        let count = source
            .elements()
            .iter()
            .enumerate()
            .filter(|(index, _)| source.name(*index).is_ok_and(|name| name == "StringHasher"))
            .count();
        if index >= count {
            return Err(CodecError::malformed(
                "HasherIndex references an unavailable string table",
            ));
        }
    }
    Ok(())
}

fn table_index(source: &XmlSource<'_>, marker: usize) -> usize {
    source
        .elements()
        .iter()
        .enumerate()
        .take(marker)
        .filter(|(index, _)| source.name(*index).is_ok_and(|name| name == "StringHasher"))
        .count()
}

fn known_attributes(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    index: usize,
    names: &[&str],
) -> Result<BTreeMap<String, String>, CodecError> {
    source.peek_attributes(ctx, index)?;
    let mut attributes = BTreeMap::new();
    for name in names {
        if let Some(value) = source.attribute(ctx, index, name)? {
            attributes.insert((*name).to_owned(), value);
        }
    }
    Ok(attributes)
}

fn compatibility_elements<'source>(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'source>,
    index: usize,
    context: &Context<'_, 'source>,
    output: &mut PartOutput,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("FreeCAD naming compatibility elements")?;
    if matches!(source.name(index)?, "StringHasher" | "ElementMap") {
        xml_spacing(source, index, &|view| context.spacing(ctx, view))?;
    }
    for child in source.children(index) {
        ctx.decode_part(
            output,
            source.extent(child)?,
            source.spec(child, "freecad.value_element")?,
            |stage| {
                source.validate(child)?;
                source.peek_attributes(ctx, child)?;
                source.peek_text(ctx, child)?;
                compatibility_elements(ctx, source, child, context, stage)
            },
        )?;
    }
    Ok(())
}

fn xml_spacing<'source>(
    source: &XmlSource<'source>,
    index: usize,
    spacing: &dyn Fn(View<'source>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let fields = source.elements()[index].framing().collect::<Vec<_>>();
    let Some(opening) = fields.get(1) else {
        return Ok(());
    };
    let mut at = opening.end();
    for child in source.children(index) {
        let extent = source.extent(child)?;
        let gap = source
            .view()
            .child(at, extent.start())
            .ok_or_else(|| CodecError::malformed("naming XML gap escapes source"))?;
        if !gap.window().is_empty()
            && gap
                .window()
                .iter()
                .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
        {
            spacing(gap)?;
        }
        at = extent.end();
    }
    if let Some(closing) = fields.get(2) {
        let gap = source
            .view()
            .child(at, closing.start())
            .ok_or_else(|| CodecError::malformed("naming XML tail escapes source"))?;
        if !gap.window().is_empty()
            && gap
                .window()
                .iter()
                .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
        {
            spacing(gap)?;
        }
    }
    Ok(())
}

fn character_view<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    index: usize,
) -> Result<View<'source>, CodecError> {
    let element = source.elements()[index];
    let fields = element.framing().collect::<Vec<_>>();
    if fields.len() < 3 {
        return element
            .extent()
            .child(element.extent().end(), element.extent().end())
            .ok_or_else(|| CodecError::malformed("empty naming carrier extent is invalid"));
    }
    let view = source
        .view()
        .child(fields[1].end(), fields[2].start())
        .ok_or_else(|| CodecError::malformed("naming character content escapes its carrier"))?;
    if !view
        .window()
        .iter()
        .any(|byte| matches!(byte, b'&' | b'<' | b'\r'))
    {
        return Ok(view);
    }
    let text = source.text(ctx, index)?;
    let mut expansion = ctx.begin_expand(ExpandSpec::Exact(u64_from_index(text.len())))?;
    expansion.write(text.as_bytes())?;
    let capture = ctx.capture_part_extent(element.extent())?;
    expansion.finalize_from(
        element.extent(),
        &format!(
            "fcstd:naming-text:{}:{}",
            capture.stream(),
            capture.offset()
        ),
    )
}

fn empty_spec(level: &str) -> Result<PartSpec, CodecError> {
    Ok(PartSpec::new(
        PartLevel::parse(level)
            .ok_or_else(|| CodecError::malformed("naming level is absent from Appendix C"))?,
        Vec::new(),
    ))
}

fn validate_carrier(
    source: &XmlSource<'_>,
    index: usize,
    marker_name: &str,
    property: Option<&PropertyRecord>,
) -> Result<(), CodecError> {
    let parent = source.elements()[index]
        .parent()
        .ok_or_else(|| CodecError::malformed("naming carrier has no enclosing element"))?;
    let mut marker = None;
    let mut successor = None;
    let mut part = None;
    for child in source.children(parent) {
        let name = source.name(child)?;
        if name == marker_name {
            if marker.replace(child).is_some() {
                return Err(CodecError::malformed(
                    "naming carrier has duplicate direct markers",
                ));
            }
        } else if name == format!("{marker_name}2") {
            if successor.replace(child).is_some() {
                return Err(CodecError::malformed(
                    "naming carrier has duplicate direct successors",
                ));
            }
        } else if name == "Part" && part.replace(child).is_some() {
            return Err(CodecError::malformed(
                "shape property has multiple direct Part carriers",
            ));
        }
    }
    if let Some(property) = property {
        if property.type_name != "Part::PropertyPartShape" {
            return Err(CodecError::malformed(
                "naming carrier has a non-shape property owner",
            ));
        }
        if part.is_none() || marker.is_none_or(|marker| part.is_some_and(|part| marker <= part)) {
            return Err(CodecError::malformed(
                "naming marker is not owned by a preceding direct Part",
            ));
        }
    } else if marker_name != "StringHasher" || source.name(parent)? != "Document" {
        return Err(CodecError::malformed(
            "naming carrier has no direct document or shape-property owner",
        ));
    }
    if let Some(successor) = successor {
        let marker =
            marker.ok_or_else(|| CodecError::malformed("naming successor has no direct marker"))?;
        if source.children(parent).find(|child| *child > marker) != Some(successor) {
            return Err(CodecError::malformed(
                "naming successor is not immediately after its marker",
            ));
        }
    }
    Ok(())
}
