// SPDX-License-Identifier: Apache-2.0
//! Owned OM caches retain their validated source bytes and decoded text.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
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
    fn new(
        ctx: &DecodeContext<'_>,
        offset: usize,
        name: &str,
        tail: &[u8],
    ) -> Result<Self, CodecError> {
        Ok(Self {
            offset,
            name: ctx.copy_retained_text(name, "NX cached definition name")?,
            registry_tail: ctx.into_boxed_slice(
                ctx.copy_retained(tail, "NX cached definition tail")?,
                "NX cached definition tail boxing",
            )?,
        })
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
    pub(crate) fn from_section(
        ctx: &DecodeContext<'_>,
        section: &IndexedSection<'_>,
        source: &Arc<[u8]>,
    ) -> Result<Option<Self>, CodecError> {
        let store = match &section.store {
            IndexedStore::Fixed { records } => {
                let mut cached = ctx.collection_vec(records.len(), "NX cached fixed records")?;
                for record in ctx.admit_iter(records.as_ref(), "NX cached record traversal")? {
                    let Some(bytes) = CachedRange::new(source, record.offset, record.bytes.len())
                    else {
                        return Ok(None);
                    };
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
                let Some(start) = control.offset.checked_add(control.bytes.len()) else {
                    return Ok(None);
                };
                let Some(control) = CachedRange::new(source, control.offset, control.bytes.len())
                else {
                    return Ok(None);
                };
                let Some(column_storage) = CachedRange::new(source, start, column_storage.len())
                else {
                    return Ok(None);
                };
                let mut cached = ctx.collection_vec(records.len(), "NX cached offset records")?;
                for record in ctx.admit_iter(records.as_ref(), "NX cached record traversal")? {
                    let Some(range) = CachedRange::new(source, record.offset, record.bytes.len())
                    else {
                        return Ok(None);
                    };
                    cached.push(range);
                }
                CachedStore::OffsetOnly {
                    control,
                    column_storage,
                    records: cached,
                }
            }
        };
        let mut types = ctx.collection_vec(section.types.len(), "NX cached types")?;
        for definition in ctx.admit_iter(section.types.as_ref(), "NX cached type traversal")? {
            types.push(CachedDefinition::new(
                ctx,
                definition.offset,
                definition.name,
                definition.registry_tail,
            )?);
        }
        let mut fields = ctx.collection_vec(section.fields.len(), "NX cached fields")?;
        for definition in ctx.admit_iter(section.fields.as_ref(), "NX cached field traversal")? {
            fields.push(CachedDefinition::new(
                ctx,
                definition.offset,
                definition.name,
                definition.registry_tail,
            )?);
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

    pub(crate) fn materialize(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<IndexedSection<'_>, CodecError> {
        let store = match &self.store {
            CachedStore::Fixed { records } => {
                let mut materialized =
                    ctx.collection_vec(records.len(), "NX materialized fixed records")?;
                for record in ctx.admit_iter(records, "NX materialized record traversal")? {
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
                let mut materialized =
                    ctx.collection_vec(records.len(), "NX materialized offset records")?;
                for record in ctx.admit_iter(records, "NX materialized record traversal")? {
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
        let mut types = ctx.collection_vec(self.types.len(), "NX materialized types")?;
        for definition in ctx.admit_iter(&self.types, "NX materialized type traversal")? {
            types.push(definition.type_definition());
        }
        let mut fields = ctx.collection_vec(self.fields.len(), "NX materialized fields")?;
        for definition in ctx.admit_iter(&self.fields, "NX materialized field traversal")? {
            fields.push(definition.field_definition());
        }
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
    pub(crate) fn from_section(
        ctx: &DecodeContext<'_>,
        section: &Section<'_>,
        source: &Arc<[u8]>,
    ) -> Result<Option<Self>, CodecError> {
        let record_area = match section.record_area {
            Some(area) => {
                let Some(range) = CachedRange::new(source, area.offset, area.bytes.len()) else {
                    return Ok(None);
                };
                Some(range)
            }
            None => None,
        };
        let Some(frame) = CachedRange::new(source, section.offset, section.byte_len) else {
            return Ok(None);
        };
        let mut types = ctx.collection_vec(section.types.len(), "NX cached framed types")?;
        for definition in ctx.admit_iter(section.types.as_ref(), "NX cached type traversal")? {
            types.push(CachedDefinition::new(
                ctx,
                definition.offset,
                definition.name,
                definition.registry_tail,
            )?);
        }
        let mut fields = ctx.collection_vec(section.fields.len(), "NX cached framed fields")?;
        for definition in ctx.admit_iter(section.fields.as_ref(), "NX cached field traversal")? {
            fields.push(CachedDefinition::new(
                ctx,
                definition.offset,
                definition.name,
                definition.registry_tail,
            )?);
        }
        let mut operation_labels = ctx.collection_vec(
            section.cached_operation_labels.len(),
            "NX cached operation labels",
        )?;
        for label in ctx.admit_iter(
            section.cached_operation_labels.as_ref(),
            "NX cached operation label traversal",
        )? {
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
        let mut types = ctx.collection_vec(self.types.len(), "NX materialized framed types")?;
        for definition in ctx.admit_iter(&self.types, "NX materialized type traversal")? {
            types.push(definition.type_definition());
        }
        let mut fields = ctx.collection_vec(self.fields.len(), "NX materialized framed fields")?;
        for definition in ctx.admit_iter(&self.fields, "NX materialized field traversal")? {
            fields.push(definition.field_definition());
        }
        let mut cached_operation_labels = ctx.collection_vec(
            self.operation_labels.len(),
            "NX materialized operation labels",
        )?;
        for label in ctx.admit_iter(
            &self.operation_labels,
            "NX materialized operation label traversal",
        )? {
            cached_operation_labels.push(label.materialize());
        }
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
            value: ctx.copy_retained_text(value.value, "NX cached operation label")?,
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
