// SPDX-License-Identifier: Apache-2.0
//! Owned OM caches retain their validated source bytes and decoded text.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
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
    pub(crate) fn from_section<'ctx>(
        ctx: &'ctx DecodeContext<'_>,
        section: &IndexedSection<'_>,
        source: &Arc<[u8]>,
    ) -> Result<(Option<Self>, ScopedReservation<'ctx>), CodecError> {
        let mut storage = ctx.reserve_scoped(0, "NX indexed cache layout candidate")?;
        let layout = storage.with_storage(|| Self::build(ctx, section, source))?;
        Ok((layout, storage))
    }

    fn build(
        ctx: &DecodeContext<'_>,
        section: &IndexedSection<'_>,
        source: &Arc<[u8]>,
    ) -> Result<Option<Self>, CodecError> {
        let store = match &section.store {
            IndexedStore::Fixed { records } => {
                let mut cached = ctx.collection_vec(records.len(), "NX cached fixed records")?;
                let mut visits = records.as_ref().iter();
                while visits.len() != 0 {
                    let Some(record) =
                        ctx.next_charged(&mut visits, "NX cached fixed record traversal")?
                    else {
                        break;
                    };
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
                let mut visits = records.as_ref().iter();
                while visits.len() != 0 {
                    let Some(record) =
                        ctx.next_charged(&mut visits, "NX cached offset record traversal")?
                    else {
                        break;
                    };
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
        let mut types = ctx.collection_vec(section.types.len(), "NX cached indexed types")?;
        let mut visits = section.types.as_ref().iter();
        while visits.len() != 0 {
            let Some(definition) =
                ctx.next_charged(&mut visits, "NX cached indexed type traversal")?
            else {
                break;
            };
            types.push(CachedDefinition::new(
                ctx,
                definition.offset,
                definition.name,
                definition.registry_tail,
            )?);
        }
        let mut fields = ctx.collection_vec(section.fields.len(), "NX cached indexed fields")?;
        let mut visits = section.fields.as_ref().iter();
        while visits.len() != 0 {
            let Some(definition) =
                ctx.next_charged(&mut visits, "NX cached indexed field traversal")?
            else {
                break;
            };
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
                for record in ctx.admit_iter(records, "NX materialized fixed record traversal")? {
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
                for record in ctx.admit_iter(records, "NX materialized offset record traversal")? {
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
        let mut types = ctx.collection_vec(self.types.len(), "NX materialized indexed types")?;
        for definition in ctx.admit_iter(&self.types, "NX materialized indexed type traversal")? {
            types.push(definition.type_definition());
        }
        let mut fields = ctx.collection_vec(self.fields.len(), "NX materialized indexed fields")?;
        for definition in ctx.admit_iter(&self.fields, "NX materialized indexed field traversal")? {
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
        let mut visits = section.types.as_ref().iter();
        while visits.len() != 0 {
            let Some(definition) =
                ctx.next_charged(&mut visits, "NX cached framed type traversal")?
            else {
                break;
            };
            types.push(CachedDefinition::new(
                ctx,
                definition.offset,
                definition.name,
                definition.registry_tail,
            )?);
        }
        let mut fields = ctx.collection_vec(section.fields.len(), "NX cached framed fields")?;
        let mut visits = section.fields.as_ref().iter();
        while visits.len() != 0 {
            let Some(definition) =
                ctx.next_charged(&mut visits, "NX cached framed field traversal")?
            else {
                break;
            };
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
        let mut visits = section.cached_operation_labels.as_ref().iter();
        while visits.len() != 0 {
            let Some(label) =
                ctx.next_charged(&mut visits, "NX cached operation label traversal")?
            else {
                break;
            };
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
        for definition in ctx.admit_iter(&self.types, "NX materialized framed type traversal")? {
            types.push(definition.type_definition());
        }
        let mut fields = ctx.collection_vec(self.fields.len(), "NX materialized framed fields")?;
        for definition in ctx.admit_iter(&self.fields, "NX materialized framed field traversal")? {
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
    use super::{CachedRange, FixedCachedRecord, IndexedSectionLayout, SectionLayout};
    use crate::om::{
        EntityRecord, FieldDefinition, FixedEntityRecord, IndexedSection, IndexedStore,
        OperationLabel, Section, TypeDefinition,
    };
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

    fn indexed_section_with_late_invalid_range(bytes: &[u8], fixed: bool) -> IndexedSection<'_> {
        let records = [
            EntityRecord {
                offset: 0,
                bytes: &bytes[..1],
            },
            EntityRecord {
                offset: 1,
                bytes: &bytes[..1],
            },
        ];
        let store = if fixed {
            IndexedStore::Fixed {
                records: Arc::from([
                    FixedEntityRecord {
                        object_id: (1, 0),
                        offset: records[0].offset,
                        bytes: records[0].bytes,
                    },
                    FixedEntityRecord {
                        object_id: (2, 4),
                        offset: records[1].offset,
                        bytes: records[1].bytes,
                    },
                ]),
            }
        } else {
            IndexedStore::OffsetOnly {
                control: EntityRecord {
                    offset: 0,
                    bytes: &bytes[..0],
                },
                column_storage: &[],
                records: Arc::from(records),
            }
        };
        IndexedSection {
            base: 0,
            entity_index_offset: 0,
            object_id_table_offset: 0,
            types: Arc::from([]),
            fields: Arc::from([]),
            store,
        }
    }

    fn indexed_section_with_invalid_second_fixed_record(bytes: &[u8]) -> IndexedSection<'_> {
        IndexedSection {
            base: 0,
            entity_index_offset: 0,
            object_id_table_offset: 0,
            types: Arc::from([]),
            fields: Arc::from([]),
            store: IndexedStore::Fixed {
                records: Arc::from([
                    FixedEntityRecord {
                        object_id: (1, 0),
                        offset: 0,
                        bytes: &bytes[..1],
                    },
                    FixedEntityRecord {
                        object_id: (2, 1),
                        offset: 1,
                        bytes: &bytes[..1],
                    },
                    FixedEntityRecord {
                        object_id: (3, 2),
                        offset: 0,
                        bytes: &bytes[..1],
                    },
                ]),
            },
        }
    }

    fn indexed_section_with_invalid_second_offset_record(bytes: &[u8]) -> IndexedSection<'_> {
        let records = [
            EntityRecord {
                offset: 0,
                bytes: &bytes[..1],
            },
            EntityRecord {
                offset: 1,
                bytes: &bytes[..1],
            },
            EntityRecord {
                offset: 0,
                bytes: &bytes[..1],
            },
        ];
        IndexedSection {
            base: 0,
            entity_index_offset: 0,
            object_id_table_offset: 0,
            types: Arc::from([]),
            fields: Arc::from([]),
            store: IndexedStore::OffsetOnly {
                control: EntityRecord {
                    offset: 0,
                    bytes: &bytes[..0],
                },
                column_storage: &[],
                records: Arc::from(records),
            },
        }
    }

    #[test]
    fn rejected_indexed_cache_candidates_release_partial_record_storage() {
        let bytes = [0];
        let source = Arc::<[u8]>::from(bytes.as_slice());
        for fixed in [true, false] {
            let section = indexed_section_with_late_invalid_range(&bytes, fixed);
            let cached_record_size = if fixed {
                std::mem::size_of::<FixedCachedRecord>()
            } else {
                std::mem::size_of::<CachedRange>()
            };
            let candidate_bytes = cadmpeg_core::decode::u64_from_index(2 * cached_record_size);
            crate::test_support::with_decode_context_over(
                &[],
                |policy| {
                    policy.limits.max_retained_bytes = 0;
                    policy.limits.max_materialized_bytes = candidate_bytes;
                },
                |ctx| {
                    let (layout, storage) =
                        IndexedSectionLayout::from_section(ctx, &section, &source)
                            .expect("partial candidate storage fits its scoped cap");
                    assert!(layout.is_none());
                    drop(storage);
                    let probe = ctx
                        .reserve_scoped(candidate_bytes, "verify failed cache candidate release")
                        .expect("rejected candidate storage was released");
                    drop(probe);
                    assert_eq!(ctx.resource_refusal(), None);
                },
            );
        }
    }

    #[test]
    fn indexed_cache_charges_only_visited_records_before_invalid_ranges() {
        let bytes = [0];
        let source = Arc::<[u8]>::from(bytes.as_slice());
        for (section, operation) in [
            (
                indexed_section_with_invalid_second_fixed_record(&bytes),
                "NX cached fixed record traversal",
            ),
            (
                indexed_section_with_invalid_second_offset_record(&bytes),
                "NX cached offset record traversal",
            ),
        ] {
            let error = crate::test_support::resource_refusal_at(
                &[],
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                operation,
                |ctx| {
                    let (layout, storage) =
                        IndexedSectionLayout::from_section(ctx, &section, &source)?;
                    drop(layout);
                    drop(storage);
                    Ok(())
                },
            );
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == operation && limit.additional == 1)
            );

            crate::test_support::with_decode_context_over(
                &[],
                |policy| policy.limits.max_work_units = 2,
                |ctx| {
                    let (layout, storage) =
                        IndexedSectionLayout::from_section(ctx, &section, &source)
                            .expect("two actual record visits fit");
                    assert!(layout.is_none());
                    drop(storage);
                    assert_eq!(ctx.resource_refusal(), None);
                },
            );
        }
    }

    #[test]
    fn indexed_cache_charges_each_definition_before_copying_its_name() {
        let source = Arc::<[u8]>::from([]);
        let store = || IndexedStore::Fixed {
            records: Arc::from([]),
        };
        let sections = [
            IndexedSection {
                base: 0,
                entity_index_offset: 0,
                object_id_table_offset: 0,
                types: Arc::from([
                    TypeDefinition {
                        offset: 0,
                        name: "A",
                        registry_tail: &[],
                    },
                    TypeDefinition {
                        offset: 1,
                        name: "B",
                        registry_tail: &[],
                    },
                ]),
                fields: Arc::from([]),
                store: store(),
            },
            IndexedSection {
                base: 0,
                entity_index_offset: 0,
                object_id_table_offset: 0,
                types: Arc::from([]),
                fields: Arc::from([
                    FieldDefinition {
                        offset: 0,
                        name: "A",
                        registry_tail: &[],
                    },
                    FieldDefinition {
                        offset: 1,
                        name: "B",
                        registry_tail: &[],
                    },
                ]),
                store: store(),
            },
        ];
        for section in sections {
            let error = crate::test_support::with_decode_context_over(
                &[],
                |policy| policy.limits.max_work_units = 1,
                |ctx| IndexedSectionLayout::from_section(ctx, &section, &source).map(|_| ()),
            )
            .expect_err("the first cached definition name copy exceeds admitted work");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "NX cached definition name"
                && limit.used == 1
                && limit.additional == 1)
            );
        }
    }

    #[test]
    fn framed_cache_charges_each_label_before_copying_its_value() {
        let source = Arc::<[u8]>::from([]);
        let header = crate::om::header_references::OperationHeader::<usize>::new(
            0,
            crate::om::header_references::HeaderReferences([None; 4]),
        )
        .expect("empty operation references fit the header");
        let section = Section {
            offset: 0,
            byte_len: 0,
            types: Arc::<[TypeDefinition<'_>]>::from([]),
            fields: Arc::<[FieldDefinition<'_>]>::from([]),
            record_area: None,
            cached_operation_labels: Arc::from([
                OperationLabel { header, value: "A" },
                OperationLabel { header, value: "B" },
            ]),
        };
        let error = crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 1,
            |ctx| SectionLayout::from_section(ctx, &section, &source).map(|_| ()),
        )
        .expect_err("the first cached label value copy exceeds admitted work");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "NX cached operation label"
                && limit.used == 1
                && limit.additional == 1)
        );
    }
}
