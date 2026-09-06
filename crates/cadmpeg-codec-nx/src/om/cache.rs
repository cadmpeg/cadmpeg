// SPDX-License-Identifier: Apache-2.0
//! Owned OM caches retain their validated source bytes and decoded text.

use std::{ops::Range, sync::Arc};

use super::{EntityRecord, FieldDefinition, FixedEntityRecord, IndexedSection, IndexedStore,
    OperationLabel, RecordArea, Section, TypeDefinition};

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
        Some(Self { source: source.clone(), range: start..end })
    }

    fn bytes(&self) -> &[u8] { &self.source[self.range.clone()] }
    fn offset(&self) -> usize { self.range.start }
    fn len(&self) -> usize { self.range.len() }
}

#[derive(Debug, Clone)]
struct CachedDefinition {
    offset: usize,
    name: String,
    registry_tail: Box<[u8]>,
}

impl CachedDefinition {
    fn type_definition(&self) -> TypeDefinition<'_> {
        TypeDefinition { offset: self.offset, name: &self.name, registry_tail: &self.registry_tail }
    }

    fn field_definition(&self) -> FieldDefinition<'_> {
        FieldDefinition { offset: self.offset, name: &self.name, registry_tail: &self.registry_tail }
    }
}

impl From<&TypeDefinition<'_>> for CachedDefinition {
    fn from(value: &TypeDefinition<'_>) -> Self {
        Self { offset: value.offset, name: value.name.to_owned(), registry_tail: value.registry_tail.into() }
    }
}

impl From<&FieldDefinition<'_>> for CachedDefinition {
    fn from(value: &FieldDefinition<'_>) -> Self {
        Self { offset: value.offset, name: value.name.to_owned(), registry_tail: value.registry_tail.into() }
    }
}

#[derive(Debug, Clone)]
struct FixedCachedRecord {
    object_id: (u32, u64),
    bytes: CachedRange,
}

#[derive(Debug, Clone)]
enum CachedStore {
    Fixed { records: Vec<FixedCachedRecord> },
    OffsetOnly { control: CachedRange, column_storage: CachedRange, records: Vec<CachedRange> },
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
            IndexedStore::Fixed { records } => CachedStore::Fixed {
                records: records.iter().map(|record| Some(FixedCachedRecord {
                    object_id: record.object_id,
                    bytes: CachedRange::new(source, record.offset, record.bytes.len())?,
                })).collect::<Option<_>>()?,
            },
            IndexedStore::OffsetOnly { control, column_storage, records } => {
                let start = control.offset.checked_add(control.bytes.len())?;
                CachedStore::OffsetOnly {
                    control: CachedRange::new(source, control.offset, control.bytes.len())?,
                    column_storage: CachedRange::new(source, start, column_storage.len())?,
                    records: records.iter().map(|record| CachedRange::new(source, record.offset, record.bytes.len()))
                        .collect::<Option<_>>()?,
                }
            }
        };
        Some(Self {
            base: section.base,
            entity_index_offset: section.entity_index_offset,
            object_id_table_offset: section.object_id_table_offset,
            types: section.types.iter().map(CachedDefinition::from).collect(),
            fields: section.fields.iter().map(CachedDefinition::from).collect(),
            store,
        })
    }

    pub(crate) fn materialize(&self) -> IndexedSection<'_> {
        let store = match &self.store {
            CachedStore::Fixed { records } => IndexedStore::Fixed {
                records: records.iter().map(|record| FixedEntityRecord {
                    object_id: record.object_id,
                    offset: record.bytes.offset(),
                    bytes: record.bytes.bytes(),
                }).collect::<Vec<_>>().into(),
            },
            CachedStore::OffsetOnly { control, column_storage, records } => IndexedStore::OffsetOnly {
                control: EntityRecord { offset: control.offset(), bytes: control.bytes() },
                column_storage: column_storage.bytes(),
                records: records.iter().map(|record| EntityRecord {
                    offset: record.offset(), bytes: record.bytes(),
                }).collect::<Vec<_>>().into(),
            },
        };
        IndexedSection {
            base: self.base,
            entity_index_offset: self.entity_index_offset,
            object_id_table_offset: self.object_id_table_offset,
            types: self.types.iter().map(CachedDefinition::type_definition).collect::<Vec<_>>().into(),
            fields: self.fields.iter().map(CachedDefinition::field_definition).collect::<Vec<_>>().into(),
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
            Some(area) => Some(CachedRange::new(source, area.offset, area.bytes.len())?),
            None => None,
        };
        Some(Self {
            frame: CachedRange::new(source, section.offset, section.byte_len)?,
            types: section.types.iter().map(CachedDefinition::from).collect(),
            fields: section.fields.iter().map(CachedDefinition::from).collect(),
            record_area,
            operation_labels: section.cached_operation_labels.iter().map(CachedOperationLabel::from).collect(),
        })
    }

    pub(crate) fn materialize(&self) -> Section<'_> {
        Section {
            offset: self.frame.offset(),
            byte_len: self.frame.len(),
            types: self.types.iter().map(CachedDefinition::type_definition).collect::<Vec<_>>().into(),
            fields: self.fields.iter().map(CachedDefinition::field_definition).collect::<Vec<_>>().into(),
            record_area: self.record_area.as_ref().map(|range| RecordArea {
                offset: range.offset(), bytes: range.bytes(),
            }),
            cached_operation_labels: self.operation_labels.iter().map(CachedOperationLabel::materialize).collect::<Vec<_>>().into(),
        }
    }
}

#[derive(Debug, Clone)]
struct CachedOperationLabel {
    header_offset: usize,
    offset: usize,
    value: String,
    object_indices: [Option<u32>; 4],
    object_index_offsets: [usize; 4],
}

impl From<&OperationLabel<'_>> for CachedOperationLabel {
    fn from(value: &OperationLabel<'_>) -> Self {
        Self {
            header_offset: value.header_offset, offset: value.offset, value: value.value.to_owned(),
            object_indices: value.object_indices, object_index_offsets: value.object_index_offsets,
        }
    }
}

impl CachedOperationLabel {
    fn materialize(&self) -> OperationLabel<'_> {
        OperationLabel {
            header_offset: self.header_offset, offset: self.offset, value: &self.value,
            object_indices: self.object_indices, object_index_offsets: self.object_index_offsets,
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
