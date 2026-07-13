// SPDX-License-Identifier: Apache-2.0
//! Typed Siemens NX object-model records retained in the native namespace.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::container::Container;

/// Unit declared by an NX numeric expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionUnit {
    /// Canonical model length in millimeters.
    Millimeter,
    /// Angular value in degrees as stored by NX.
    Degree,
}

/// Explicit numeric expression serialized in one NX OM entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expression {
    /// Globally unique native-record identity.
    pub id: String,
    /// Persistent OM object identifier.
    pub object_id: Option<u32>,
    /// Owning entry in the native OM record directory, when externally bounded.
    pub record: Option<String>,
    /// NX parameter name.
    pub name: String,
    /// Declared native unit.
    pub unit: ExpressionUnit,
    /// Finite numeric value in the declared unit.
    pub value: f64,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the expression text.
    pub source_offset: u64,
}

/// Length-framed class definition from an NX OM type registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassDefinition {
    /// Globally unique native-record identity.
    pub id: String,
    /// Registered `UGS::` class name.
    pub name: String,
    /// Zero-based declaration ordinal used as class identity.
    pub ordinal: u32,
    /// Declaration code serialized after the class name.
    pub trailing_code: u8,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the definition's length byte.
    pub source_offset: u64,
}

/// Member declaration from an NX OM field registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDefinition {
    /// Globally unique declaration identity.
    pub id: String,
    /// Registered `m_` member name.
    pub name: String,
    /// Zero-based declaration ordinal within its section.
    pub ordinal: u32,
    /// Declaration code serialized immediately after the name.
    pub trailing_code: u8,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the declaration length byte.
    pub source_offset: u64,
}

/// Directory entry for one externally bounded NX OM entity record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Persistent OM object identifier when the section carries an ID table.
    pub object_id: Option<u32>,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based record ordinal within the indexed section.
    pub record_ordinal: u32,
    /// Exact serialized record length.
    pub byte_len: u64,
    /// SHA-256 of the exact serialized record bytes.
    pub sha256: String,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the record start.
    pub source_offset: u64,
}

/// Self-framed printable string carried by one NX OM record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringValue {
    /// Globally unique value identity.
    pub id: String,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Persistent OM object identifier when the section carries an ID table.
    pub object_id: Option<u32>,
    /// Zero-based occurrence ordinal within the owning record.
    pub ordinal: u32,
    /// Exact printable value.
    pub value: String,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the `66 32 03` marker.
    pub source_offset: u64,
}

/// Tagged reference family serialized in an NX OM record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectReferenceKind {
    /// `e0` marker followed by a 32-bit persistent handle.
    PersistentHandle,
    /// Four-byte `0xC?` tagged 28-bit reference.
    Tagged28,
}

/// Ordered tagged-reference occurrence owned by one NX OM record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectReference {
    /// Globally unique occurrence identity.
    pub id: String,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Persistent OM object identifier when the section carries an ID table.
    pub object_id: Option<u32>,
    /// Zero-based occurrence ordinal within the owning record.
    pub ordinal: u32,
    /// Tagged reference family.
    pub kind: ObjectReferenceKind,
    /// Reference value without marker/tag bits.
    pub value: u32,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the reference marker.
    pub source_offset: u64,
}

/// Cross-record identity established by equal persistent-handle values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentHandle {
    /// Globally unique handle identity.
    pub id: String,
    /// Unsigned persistent-handle value.
    pub value: u32,
    /// Ordered distinct OM directory records containing the handle.
    pub records: Vec<String>,
    /// Total number of serialized occurrences across those records.
    pub occurrence_count: u32,
}

/// Named NX arrangement from `/Root/part/arrangements`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Configuration {
    /// Globally unique native-record identity.
    pub id: String,
    /// Arrangement name.
    pub name: String,
    /// Whether NX marks this arrangement as the default.
    pub active: bool,
    /// Directory entry containing the arrangement XML.
    pub source_entry: String,
    /// Absolute file offset of the arrangement element.
    pub source_offset: u64,
}

/// Decode the explicit NX arrangement table.
pub fn configurations(container: &Container) -> Vec<Configuration> {
    container
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.name == "/Root/part/arrangements")
        .filter_map(|(entry_index, entry)| {
            let (offset, size) = entry.file_span?;
            let (offset_usize, size) = (usize::try_from(offset).ok()?, usize::try_from(size).ok()?);
            let payload = container
                .data
                .get(offset_usize..offset_usize.checked_add(size)?)?;
            let xml = std::str::from_utf8(payload).ok()?;
            let document = roxmltree::Document::parse(xml).ok()?;
            let root = document.root_element();
            if root.tag_name().name() != "Arrangements" {
                return None;
            }

            let mut active_count = 0usize;
            let mut configurations = Vec::new();
            for (ordinal, node) in root
                .children()
                .filter(roxmltree::Node::is_element)
                .enumerate()
            {
                if node.tag_name().name() != "Arrangement" {
                    return None;
                }
                let name = node.attribute("Name")?;
                if name.is_empty() {
                    return None;
                }
                let active = match node.attribute("Default")? {
                    "YES" => true,
                    "NO" => false,
                    _ => return None,
                };
                active_count += usize::from(active);
                configurations.push(Configuration {
                    id: format!("nx:arrangements-{entry_index}:configuration#{ordinal}"),
                    name: name.to_string(),
                    active,
                    source_entry: entry.name.clone(),
                    source_offset: offset + node.range().start as u64,
                });
            }
            (!configurations.is_empty() && active_count <= 1).then_some(configurations)
        })
        .flatten()
        .collect()
}

/// Decode class definitions from every framed OM section.
pub fn class_definitions(container: &Container) -> Vec<ClassDefinition> {
    let mut definitions = BTreeMap::new();
    for (entry, section) in container.om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("OM entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (ordinal, definition) in section.types.into_iter().enumerate() {
            definitions.insert(
                (entry_index, definition.offset),
                ClassDefinition {
                    id: format!("nx:om-entry-{entry_index}:class#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                },
            );
        }
    }
    for (entry, section) in container.indexed_om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("indexed entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (ordinal, definition) in section.types.into_iter().enumerate() {
            definitions
                .entry((entry_index, definition.offset))
                .or_insert_with(|| ClassDefinition {
                    id: format!("nx:om-entry-{entry_index}:class#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                });
        }
    }
    definitions.into_values().collect()
}

/// Decode member definitions from every framed OM section.
pub fn field_definitions(container: &Container) -> Vec<FieldDefinition> {
    let mut definitions = Vec::new();
    for (entry, section) in container.om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("OM entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        definitions.extend(
            section
                .fields
                .into_iter()
                .enumerate()
                .map(|(ordinal, definition)| FieldDefinition {
                    id: format!("nx:om-entry-{entry_index}:field#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                }),
        );
    }
    definitions
}

/// Catalog every externally bounded NX OM entity record.
pub fn object_records(container: &Container) -> Vec<ObjectRecord> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section
                .records
                .into_iter()
                .enumerate()
                .map(move |(record_ordinal, record)| ObjectRecord {
                    id: format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"),
                    object_id: record.object_id,
                    section_ordinal: section_ordinal as u32,
                    record_ordinal: record_ordinal as u32,
                    byte_len: record.bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(record.bytes),
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + record.offset as u64,
                })
        })
        .collect()
}

/// Decode self-framed printable values from bounded NX OM records.
pub fn string_values(container: &Container) -> Vec<StringValue> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section.string_values().into_iter().map(
                move |(record_ordinal, value_ordinal, object_id, value)| {
                    let record =
                        format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}");
                    StringValue {
                        id: format!(
                            "nx:om-string-values-{section_ordinal}-{record_ordinal}:value#{}",
                            value.offset
                        ),
                        record,
                        object_id,
                        ordinal: value_ordinal as u32,
                        value: value.value.to_string(),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + value.offset as u64,
                    }
                },
            )
        })
        .collect()
}

/// Decode ordered tagged references from bounded NX OM records.
pub fn object_references(container: &Container) -> Vec<ObjectReference> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section.references().into_iter().map(
                move |(record_ordinal, reference_ordinal, object_id, reference)| {
                    let record =
                        format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}");
                    ObjectReference {
                        id: format!(
                            "nx:om-references-{section_ordinal}-{record_ordinal}:reference#{}",
                            reference.offset
                        ),
                        record,
                        object_id,
                        ordinal: reference_ordinal as u32,
                        kind: match reference.kind {
                            crate::om::ReferenceKind::PersistentHandle => {
                                ObjectReferenceKind::PersistentHandle
                            }
                            crate::om::ReferenceKind::Tagged28 => ObjectReferenceKind::Tagged28,
                        },
                        value: reference.value,
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + reference.offset as u64,
                    }
                },
            )
        })
        .collect()
}

/// Group persistent-handle occurrences into cross-record identities.
pub fn persistent_handles(references: &[ObjectReference]) -> Vec<PersistentHandle> {
    let mut groups = BTreeMap::<u32, (Vec<String>, u32)>::new();
    for reference in references
        .iter()
        .filter(|reference| reference.kind == ObjectReferenceKind::PersistentHandle)
    {
        let (records, occurrence_count) = groups.entry(reference.value).or_default();
        *occurrence_count += 1;
        if records.last() != Some(&reference.record) && !records.contains(&reference.record) {
            records.push(reference.record.clone());
        }
    }
    groups
        .into_iter()
        .map(|(value, (records, occurrence_count))| PersistentHandle {
            id: format!("nx:om-persistent-handles:handle#{value:08x}"),
            value,
            records,
            occurrence_count,
        })
        .collect()
}

/// Decode explicit numeric expressions from all indexed OM sections.
pub fn expressions(container: &Container) -> Vec<Expression> {
    let mut indexed = BTreeMap::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        for (record_ordinal, expression) in section.numeric_expression_records() {
            indexed.insert(
                (entry.name.clone(), expression.offset),
                (
                    expression.object_id,
                    format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"),
                ),
            );
        }
    }
    let mut expressions = Vec::new();
    for (entry_index, entry) in container.entries.iter().enumerate() {
        let Some((entry_offset, size)) = entry.file_span else {
            continue;
        };
        let (Ok(offset), Ok(size)) = (usize::try_from(entry_offset), usize::try_from(size)) else {
            continue;
        };
        let Some(payload) = container.data.get(offset..offset.saturating_add(size)) else {
            continue;
        };
        for expression in crate::om::numeric_expressions(payload) {
            let indexed_record = indexed
                .get(&(entry.name.clone(), expression.offset))
                .cloned();
            expressions.push(Expression {
                id: format!("nx:om-entry-{entry_index}:expression#{}", expression.offset),
                object_id: indexed_record
                    .as_ref()
                    .and_then(|(object_id, _)| *object_id),
                record: indexed_record.map(|(_, record)| record),
                name: expression.name.to_string(),
                unit: match expression.unit {
                    crate::om::ExpressionUnit::Millimeter => ExpressionUnit::Millimeter,
                    crate::om::ExpressionUnit::Degree => ExpressionUnit::Degree,
                },
                value: expression.value,
                source_entry: entry.name.clone(),
                source_offset: entry_offset + expression.offset as u64,
            });
        }
    }
    expressions
}
