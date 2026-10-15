// SPDX-License-Identifier: Apache-2.0
//! Owned OM caches retain their validated source bytes and decoded text.

use std::{ops::Range, sync::Arc};

use super::{
    EntityRecord, FieldDefinition, FixedEntityRecord, IndexedSection, IndexedStore, OperationLabel,
    RecordArea, Section, TypeDefinition,
};

/// The range and its immutable owner are checked and retained together.
#[derive(Debug, Clone)]
struct CachedRange {
    source: Arc<[u8]>,
    range: Range<usize>,
}

impl CachedRange {
    fn new(source: &Arc<[u8]>, start: usize, length: usize) -> Option<Self> {
        let end = start.checked_add(length)?;
        source.get(start..end)?;
        Some(Self {
            source: source.clone(),
            range: start..end,
        })
    }

    fn bytes(&self) -> &[u8] {
        &self.source[self.range.clone()]
    }
    fn offset(&self) -> usize {
        self.range.start
    }
    fn len(&self) -> usize {
        self.range.len()
    }
}

#[derive(Debug, Clone)]
struct CachedDefinition {
    offset: usize,
    name: String,
    registry_tail: Box<[u8]>,
}

impl CachedDefinition {
    fn type_definition(&self) -> TypeDefinition<'_> {
        TypeDefinition {
            offset: self.offset,
            name: &self.name,
            registry_tail: &self.registry_tail,
        }
    }

    fn field_definition(&self) -> FieldDefinition<'_> {
        FieldDefinition {
            offset: self.offset,
            name: &self.name,
            registry_tail: &self.registry_tail,
        }
    }
}

impl CachedDefinition {
    fn new(offset: usize, name: &str, tail: &[u8]) -> Self {
        Self {
            offset,
            name: name.to_owned(),
            registry_tail: tail.into(),
        }
    }
}

#[derive(Debug, Clone)]
struct FixedCachedRecord {
    object_id: (u32, u64),
    bytes: CachedRange,
}

#[derive(Debug, Clone)]
enum CachedStore {
    Fixed {
        records: Vec<FixedCachedRecord>,
    },
    OffsetOnly {
        control: CachedRange,
        column_storage: CachedRange,
        records: Vec<CachedRange>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedSectionLayout {
    base: usize,
    entity_index_offset: usize,
    object_id_table_offset: usize,
    types: Vec<CachedDefinition>,
    fields: Vec<CachedDefinition>,
    store: CachedStore,
}

impl IndexedSectionLayout {
    pub(crate) fn from_section(section: &IndexedSection<'_>, source: &Arc<[u8]>) -> Option<Self> {
        let store = match &section.store {
            IndexedStore::Fixed { records } => {
                let mut cached = Vec::new();
                for record in records.iter() {
                    let bytes = CachedRange::new(source, record.offset, record.bytes.len())?;
                    cached.push(FixedCachedRecord {
                        object_id: record.object_id,
                        bytes,
                    });
                }
                CachedStore::Fixed { records: cached }
            }
            IndexedStore::OffsetOnly {
                control,
                column_storage,
                records,
            } => {
                let start = control.offset.checked_add(control.bytes.len())?;
                let control = CachedRange::new(source, control.offset, control.bytes.len())?;
                let column_storage = CachedRange::new(source, start, column_storage.len())?;
                let mut cached = Vec::new();
                for record in records.iter() {
                    let range = CachedRange::new(source, record.offset, record.bytes.len())?;
                    cached.push(range);
                }
                CachedStore::OffsetOnly {
                    control,
                    column_storage,
                    records: cached,
                }
            }
        };
        let mut types = Vec::new();
        for definition in section.types.iter() {
            types.push(CachedDefinition::new(
                definition.offset,
                definition.name,
                definition.registry_tail,
            ));
        }
        let mut fields = Vec::new();
        for definition in section.fields.iter() {
            fields.push(CachedDefinition::new(
                definition.offset,
                definition.name,
                definition.registry_tail,
            ));
        }
        Some(Self {
            base: section.base,
            entity_index_offset: section.entity_index_offset,
            object_id_table_offset: section.object_id_table_offset,
            types,
            fields,
            store,
        })
    }

    pub(crate) fn materialize(&self) -> IndexedSection<'_> {
        let store = match &self.store {
            CachedStore::Fixed { records } => {
                let mut materialized = Vec::new();
                for record in records {
                    materialized.push(FixedEntityRecord {
                        object_id: record.object_id,
                        offset: record.bytes.offset(),
                        bytes: record.bytes.bytes(),
                    });
                }
                IndexedStore::Fixed {
                    records: materialized.into(),
                }
            }
            CachedStore::OffsetOnly {
                control,
                column_storage,
                records,
            } => {
                let mut materialized = Vec::new();
                for record in records {
                    materialized.push(EntityRecord {
                        offset: record.offset(),
                        bytes: record.bytes(),
                    });
                }
                IndexedStore::OffsetOnly {
                    control: EntityRecord {
                        offset: control.offset(),
                        bytes: control.bytes(),
                    },
                    column_storage: column_storage.bytes(),
                    records: materialized.into(),
                }
            }
        };
        let mut types = Vec::new();
        for definition in &self.types {
            types.push(definition.type_definition());
        }
        let mut fields = Vec::new();
        for definition in &self.fields {
            fields.push(definition.field_definition());
        }
        IndexedSection {
            base: self.base,
            entity_index_offset: self.entity_index_offset,
            object_id_table_offset: self.object_id_table_offset,
            types: types.into(),
            fields: fields.into(),
            store,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SectionLayout {
    frame: CachedRange,
    types: Vec<CachedDefinition>,
    fields: Vec<CachedDefinition>,
    record_area: Option<CachedRange>,
    operation_labels: Vec<CachedOperationLabel>,
}

impl SectionLayout {
    pub(crate) fn from_section(section: &Section<'_>, source: &Arc<[u8]>) -> Option<Self> {
        let record_area = match section.record_area {
            Some(area) => {
                let range = CachedRange::new(source, area.offset, area.bytes.len())?;
                Some(range)
            }
            None => None,
        };
        let frame = CachedRange::new(source, section.offset, section.byte_len)?;
        let mut types = Vec::new();
        for definition in section.types.iter() {
            types.push(CachedDefinition::new(
                definition.offset,
                definition.name,
                definition.registry_tail,
            ));
        }
        let mut fields = Vec::new();
        for definition in section.fields.iter() {
            fields.push(CachedDefinition::new(
                definition.offset,
                definition.name,
                definition.registry_tail,
            ));
        }
        let mut operation_labels = Vec::new();
        for label in section.cached_operation_labels.iter() {
            operation_labels.push(CachedOperationLabel::new(label));
        }
        Some(Self {
            frame,
            types,
            fields,
            record_area,
            operation_labels,
        })
    }

    pub(crate) fn materialize(&self) -> Section<'_> {
        let mut types = Vec::new();
        for definition in &self.types {
            types.push(definition.type_definition());
        }
        let mut fields = Vec::new();
        for definition in &self.fields {
            fields.push(definition.field_definition());
        }
        let mut cached_operation_labels = Vec::new();
        for label in &self.operation_labels {
            cached_operation_labels.push(label.materialize());
        }
        Section {
            offset: self.frame.offset(),
            byte_len: self.frame.len(),
            types: types.into(),
            fields: fields.into(),
            record_area: self.record_area.as_ref().map(|range| RecordArea {
                offset: range.offset(),
                bytes: range.bytes(),
            }),
            cached_operation_labels: cached_operation_labels.into(),
        }
    }
}

#[derive(Debug, Clone)]
struct CachedOperationLabel {
    header: super::header_references::OperationHeader,
    value: String,
}

impl CachedOperationLabel {
    fn new(value: &OperationLabel<'_>) -> Self {
        Self {
            header: value.header,
            value: value.value.to_owned(),
        }
    }
    fn materialize(&self) -> OperationLabel<'_> {
        OperationLabel {
            header: self.header,
            value: &self.value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CachedRange;
    use std::sync::Arc;

    #[test]
    fn cached_ranges_retain_their_owner_and_reject_invalid_bounds() {
        let mut bytes = vec![1, 2, 3];
        let source: Arc<[u8]> = Arc::from(bytes.as_slice());
        let range = CachedRange::new(&source, 1, 2).unwrap();
        bytes.clear();
        drop(source);
        assert_eq!(range.offset(), 1);
        assert_eq!(range.bytes(), &[2, 3]);
        let source: Arc<[u8]> = Arc::from([]);
        assert!(CachedRange::new(&source, 0, 0).is_some());
        assert!(CachedRange::new(&source, 0, 1).is_none());
        assert!(CachedRange::new(&source, usize::MAX, 1).is_none());
    }
}
