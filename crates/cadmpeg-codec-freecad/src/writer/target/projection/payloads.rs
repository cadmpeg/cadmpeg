// SPDX-License-Identifier: Apache-2.0
//! Structural references proven by their current typed payload owner.

use crate::brep::ShapePayloadRecord;
use crate::native::{
    element_map::ElementMapRecord, EntryRecord, ProductNodeRecord, PropertyRecord,
    StringTableRecord,
};
use std::collections::BTreeSet;

/// A filename alone proves nothing. Each accepted entry also needs the exact
/// typed property owner and the carrier tag that names that payload family.
pub(super) fn entries(
    property: &PropertyRecord,
    shapes: &[ShapePayloadRecord],
    maps: &[ElementMapRecord],
    tables: &[StringTableRecord],
    products: &[ProductNodeRecord],
    archive_entries: &[EntryRecord],
) -> BTreeSet<String> {
    let mut entries = BTreeSet::new();
    let references = |tag: &str, entry: &str| {
        property.side_entries().iter().any(|name| name == entry)
            && archive_entries.iter().any(|candidate| {
                candidate.name() == entry && candidate.referenced_by().contains(&property.id)
            })
            && property.values().iter().any(|value| {
                value.parent.is_none()
                    && value.tag == tag
                    && value
                        .attributes
                        .get("file")
                        .is_some_and(|name| name == entry)
            })
    };
    if property.type_name == "Part::PropertyPartShape" {
        for shape in shapes.iter().filter(|shape| shape.property == property.id) {
            // Shape payloads name the archive-entry identity; XML carries
            // that entry's filename. Resolve the typed join before comparing.
            if let Some(entry) = archive_entries
                .iter()
                .find(|entry| entry.id() == shape.entry)
            {
                if references("Part", entry.name()) {
                    entries.insert(entry.name().to_owned());
                }
            }
        }
        for map in maps.iter().filter(|map| map.property == property.id) {
            if let Some(entry) = &map.source_entry {
                if references("ElementMap2", entry) {
                    entries.insert(entry.clone());
                }
            }
        }
        for table in tables.iter().filter(|table| {
            table.owner_property.as_deref() == Some(property.id.as_str())
                && table.entries().is_some()
        }) {
            if let Some(entry) = &table.source_entry {
                if references("StringHasher", entry) {
                    entries.insert(entry.clone());
                }
            }
        }
    }
    if property.side_entries().len() == 1 {
        let entry = &property.side_entries()[0];
        for product in products
            .iter()
            .filter(|product| product.object == property.owner && product.occurrence().is_some())
        {
            let (tag, length) = match (property.name.as_str(), property.type_name.as_str()) {
                ("PlacementList", "App::PropertyPlacementList") => {
                    ("PlacementList", product.element_transforms().len())
                }
                ("ScaleList", "App::PropertyVectorList") => {
                    ("VectorList", product.element_scales().len())
                }
                _ => continue,
            };
            // A present list must have a typed vector, or the explicit zero
            // cardinality that proves the empty array rather than its absence.
            if (length != 0 || product.element_count() == Some(0)) && references(tag, entry) {
                entries.insert(entry.clone());
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests;
