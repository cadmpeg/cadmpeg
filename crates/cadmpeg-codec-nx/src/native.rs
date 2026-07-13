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

/// Decode explicit numeric expressions from all indexed OM sections.
pub fn expressions(container: &Container) -> Vec<Expression> {
    let mut indexed = BTreeMap::new();
    for (entry, section) in container.indexed_om_sections() {
        for expression in section.numeric_expressions() {
            indexed.insert(
                (entry.name.clone(), expression.offset),
                expression.object_id,
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
            let object_id = indexed
                .get(&(entry.name.clone(), expression.offset))
                .copied()
                .flatten();
            expressions.push(Expression {
                id: format!("nx:om-entry-{entry_index}:expression#{}", expression.offset),
                object_id,
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
