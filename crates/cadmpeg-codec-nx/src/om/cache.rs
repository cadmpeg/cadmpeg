// SPDX-License-Identifier: Apache-2.0
//! Owned OM caches retain their validated source bytes and decoded text.

use std::{ops::Range, sync::Arc};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

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
    fn new(ctx: &DecodeContext<'_>, offset: usize, name: &str, tail: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            offset,
            name: cached_text(ctx, name, "NX cached definition name")?,
            registry_tail: ctx.copy_retained(tail, "NX cached definition tail")?.into_boxed_slice(),
        })
    }
}

pub(crate) fn charged_items<T>(ctx: &DecodeContext<'_>, count: usize, operation: &'static str) -> Result<Vec<T>, CodecError> {
    let count_u64 = u64_from_index(count);
    let bytes = count_u64.checked_mul(u64_from_index(std::mem::size_of::<T>()))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, count_u64))?;
    ctx.charge_collection_items(count_u64, operation)?;
    ctx.charge_retained(bytes, operation)?;
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| ctx.refuse_codec_limit(operation, count_u64, count_u64))?;
    Ok(values)
}

fn cached_text(ctx: &DecodeContext<'_>, value: &str, operation: &'static str) -> Result<String, CodecError> {
    let length = u64_from_index(value.len());
    ctx.charge_retained(length, operation)?;
    let mut text = String::new();
    text.try_reserve_exact(value.len()).map_err(|_| ctx.refuse_codec_limit(operation, length, length))?;
    text.push_str(value);
    Ok(text)
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
    pub(crate) fn from_section(ctx: &DecodeContext<'_>, section: &IndexedSection<'_>, source: &Arc<[u8]>) -> Result<Option<Self>, CodecError> {
        let store = match &section.store {
            IndexedStore::Fixed { records } => {
                let mut cached = charged_items(ctx, records.len(), "NX cached fixed records")?;
                for record in records.iter() {
                    let Some(bytes) = CachedRange::new(source, record.offset, record.bytes.len()) else { return Ok(None); };
                    cached.push(FixedCachedRecord { object_id: record.object_id, bytes });
                }
                CachedStore::Fixed { records: cached }
            }
            IndexedStore::OffsetOnly {
                control,
                column_storage,
                records,
            } => {
                let Some(start) = control.offset.checked_add(control.bytes.len()) else { return Ok(None); };
                let Some(control) = CachedRange::new(source, control.offset, control.bytes.len()) else { return Ok(None); };
                let Some(column_storage) = CachedRange::new(source, start, column_storage.len()) else { return Ok(None); };
                let mut cached = charged_items(ctx, records.len(), "NX cached offset records")?;
                for record in records.iter() {
                    let Some(range) = CachedRange::new(source, record.offset, record.bytes.len()) else { return Ok(None); };
                    cached.push(range);
                }
                CachedStore::OffsetOnly {
                    control,
                    column_storage,
                    records: cached,
                }
            }
        };
        let mut types = charged_items(ctx, section.types.len(), "NX cached types")?;
        for definition in section.types.iter() {
            types.push(CachedDefinition::new(ctx, definition.offset, definition.name, definition.registry_tail)?);
        }
        let mut fields = charged_items(ctx, section.fields.len(), "NX cached fields")?;
        for definition in section.fields.iter() {
            fields.push(CachedDefinition::new(ctx, definition.offset, definition.name, definition.registry_tail)?);
        }
        Ok(Some(Self {
            base: section.base,
            entity_index_offset: section.entity_index_offset,
            object_id_table_offset: section.object_id_table_offset,
            types,
            fields,
            store,
        }))
    }

    pub(crate) fn materialize(&self, ctx: &DecodeContext<'_>) -> Result<IndexedSection<'_>, CodecError> {
        let store = match &self.store {
            CachedStore::Fixed { records } => {
                let mut materialized = charged_items(ctx, records.len(), "NX materialized fixed records")?;
                for record in records.iter() {
                    materialized.push(FixedEntityRecord {
                        object_id: record.object_id,
                        offset: record.bytes.offset(),
                        bytes: record.bytes.bytes(),
                    });
                }
                IndexedStore::Fixed { records: materialized.into() }
            }
            CachedStore::OffsetOnly {
                control,
                column_storage,
                records,
            } => {
                let mut materialized = charged_items(ctx, records.len(), "NX materialized offset records")?;
                for record in records.iter() {
                    materialized.push(EntityRecord { offset: record.offset(), bytes: record.bytes() });
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
        let mut types = charged_items(ctx, self.types.len(), "NX materialized types")?;
        for definition in &self.types { types.push(definition.type_definition()); }
        let mut fields = charged_items(ctx, self.fields.len(), "NX materialized fields")?;
        for definition in &self.fields { fields.push(definition.field_definition()); }
        Ok(IndexedSection {
            base: self.base,
            entity_index_offset: self.entity_index_offset,
            object_id_table_offset: self.object_id_table_offset,
            types: types.into(),
            fields: fields.into(),
            store,
        })
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
    pub(crate) fn from_section(ctx: &DecodeContext<'_>, section: &Section<'_>, source: &Arc<[u8]>) -> Result<Option<Self>, CodecError> {
        let record_area = match section.record_area {
            Some(area) => {
                let Some(range) = CachedRange::new(source, area.offset, area.bytes.len()) else { return Ok(None); };
                Some(range)
            }
            None => None,
        };
        let Some(frame) = CachedRange::new(source, section.offset, section.byte_len) else { return Ok(None); };
        let mut types = charged_items(ctx, section.types.len(), "NX cached framed types")?;
        for definition in section.types.iter() {
            types.push(CachedDefinition::new(ctx, definition.offset, definition.name, definition.registry_tail)?);
        }
        let mut fields = charged_items(ctx, section.fields.len(), "NX cached framed fields")?;
        for definition in section.fields.iter() {
            fields.push(CachedDefinition::new(ctx, definition.offset, definition.name, definition.registry_tail)?);
        }
        let mut operation_labels = charged_items(ctx, section.cached_operation_labels.len(), "NX cached operation labels")?;
        for label in section.cached_operation_labels.iter() {
            operation_labels.push(CachedOperationLabel::new(ctx, label)?);
        }
        Ok(Some(Self {
            frame,
            types,
            fields,
            record_area,
            operation_labels,
        }))
    }

    pub(crate) fn materialize(&self, ctx: &DecodeContext<'_>) -> Result<Section<'_>, CodecError> {
        let mut types = charged_items(ctx, self.types.len(), "NX materialized framed types")?;
        for definition in &self.types { types.push(definition.type_definition()); }
        let mut fields = charged_items(ctx, self.fields.len(), "NX materialized framed fields")?;
        for definition in &self.fields { fields.push(definition.field_definition()); }
        let mut cached_operation_labels = charged_items(ctx, self.operation_labels.len(), "NX materialized operation labels")?;
        for label in &self.operation_labels { cached_operation_labels.push(label.materialize()); }
        Ok(Section {
            offset: self.frame.offset(),
            byte_len: self.frame.len(),
            types: types.into(),
            fields: fields.into(),
            record_area: self.record_area.as_ref().map(|range| RecordArea {
                offset: range.offset(),
                bytes: range.bytes(),
            }),
            cached_operation_labels: cached_operation_labels.into(),
        })
    }
}

#[derive(Debug, Clone)]
struct CachedOperationLabel {
    header: super::header_references::OperationHeader,
    value: String,
}

impl CachedOperationLabel {
    fn new(ctx: &DecodeContext<'_>, value: &OperationLabel<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            header: value.header,
            value: cached_text(ctx, value.value, "NX cached operation label")?,
        })
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
