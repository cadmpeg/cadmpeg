// SPDX-License-Identifier: Apache-2.0
//! One segment of a Base Feature design: its type table, its records, and
//! the `MetaStream` that indexes them (f3d.md §1.4; §3.1, "Records of a
//! Base Feature design").

use cadmpeg_core::CodecError;

use super::super::frames::DesignRecords;
use crate::records::references::DesignClassTag;
use crate::writer::generate::presentation::{dynamic_class_tag, GeneratedDesignRegistry, TypeRow};

/// The three segments of a Base Feature design.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::writer::generate) enum SegmentKind {
    Design,
    Act,
    Browser,
}

impl SegmentKind {
    /// The segment ID: the target of a cross-segment reference.
    pub(in crate::writer::generate) const fn id(self) -> u32 {
        match self {
            Self::Design => 0,
            Self::Act => 1,
            Self::Browser => 2,
        }
    }

    /// The segment type, which is also the short name.
    const fn name(self) -> &'static str {
        match self {
            Self::Design => "FusionDesignSegmentType",
            Self::Act => "FusionACTSegmentType",
            Self::Browser => "FusionBrowserSegmentType",
        }
    }

    /// The segment type code.
    const fn code(self) -> u32 {
        match self {
            Self::Design => 1,
            Self::Act => 5,
            Self::Browser => 6,
        }
    }

    /// The archive folder of the segment's streams.
    pub(super) fn folder(self) -> String {
        format!("{}1", self.name())
    }
}

/// A type table: rows in class-tag order with the record indices of each.
pub(in crate::writer::generate) struct TypeTable {
    registry: GeneratedDesignRegistry,
}

impl TypeTable {
    /// The rows of `records`' types in ascending record index, each
    /// followed by the rows of its ancestor types that no earlier row lists,
    /// from the nearest ancestor up; then every row of `known` no earlier row
    /// lists. A base type is looked up among `records` and `known`.
    pub(super) fn new(records: &[(u32, TypeRow)], known: &[TypeRow]) -> Result<Self, CodecError> {
        let mut ordered = records.to_vec();
        ordered.sort_by_key(|&(index, _)| index);
        let lookup = |guid: &str| {
            ordered
                .iter()
                .map(|(_, row)| row)
                .chain(known)
                .find(|row| row.guid.eq_ignore_ascii_case(guid))
                .copied()
        };
        let mut rows = Vec::<(TypeRow, Vec<u32>)>::new();
        let insert = |rows: &mut Vec<(TypeRow, Vec<u32>)>, row: TypeRow| {
            rows.iter()
                .position(|(known, _)| known.guid.eq_ignore_ascii_case(row.guid))
                .unwrap_or_else(|| {
                    rows.push((row, Vec::new()));
                    rows.len() - 1
                })
        };
        for (index, row) in &ordered {
            let position = insert(&mut rows, *row);
            rows[position].1.push(*index);
            let mut base = row.base;
            while let Some(guid) = base {
                let Some(ancestor) = lookup(guid) else {
                    break;
                };
                if rows
                    .iter()
                    .any(|(known, _)| known.guid.eq_ignore_ascii_case(ancestor.guid))
                {
                    break;
                }
                insert(&mut rows, ancestor);
                base = ancestor.base;
            }
        }
        for row in known {
            insert(&mut rows, *row);
        }
        if rows.len() > 999 - 256 {
            return Err(CodecError::NotImplemented(
                "a Base Feature type table exceeds three-digit class tags".into(),
            ));
        }
        let mut registry = GeneratedDesignRegistry::from_types(Vec::new());
        for (row, indices) in rows {
            registry.register(row, &indices)?;
        }
        Ok(Self { registry })
    }

    fn position(&self, guid: &str) -> Option<usize> {
        self.registry
            .types
            .iter()
            .position(|row| row.type_guid.as_str().eq_ignore_ascii_case(guid))
    }

    pub(in crate::writer::generate::design) fn write_configuration_root(
        &mut self,
        plan: crate::writer::generate::configuration_root::Plan,
        out: &mut DesignRecords,
    ) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, CodecError> {
        let types = std::mem::take(&mut self.registry.types);
        self.registry = GeneratedDesignRegistry::from_types(types);
        let mut bulk = None;
        let losses = plan.without_source().write(&mut self.registry, &mut bulk)?;
        if let Some(bulk) = bulk {
            let start = out.bytes.len();
            for record in bulk.primary_records {
                let index = u32::try_from(record.entity_id).map_err(|_| {
                    CodecError::NotImplemented("Design record index exceeds u32".into())
                })?;
                let offset = usize::try_from(record.bulk_offset)
                    .ok()
                    .and_then(|offset| start.checked_add(offset))
                    .ok_or_else(|| {
                        CodecError::NotImplemented("Design record offset exceeds usize".into())
                    })?;
                out.primaries.push((index, offset));
            }
            out.bytes.extend(bulk.bytes);
        }
        Ok(losses)
    }

    pub(super) fn next_entity(&self) -> Result<u64, CodecError> {
        self.registry
            .types
            .iter()
            .flat_map(|row| &row.entity_ids)
            .max()
            .map_or(Ok(0), |index| {
                index.checked_add(1).ok_or_else(|| {
                    CodecError::NotImplemented("F3D Design entity ID space is full".into())
                })
            })
    }

    /// The class tag of the row of `guid`: `256` plus its position.
    pub(super) fn class(&self, guid: &str) -> Result<DesignClassTag, CodecError> {
        let position = self.position(guid).ok_or_else(|| {
            CodecError::malformed(format_args!("the type table lists no row {guid}"))
        })?;
        dynamic_class_tag(position)
    }
}

/// A segment's records with its type table and indexes.
pub(in crate::writer::generate) struct Segment<'a> {
    pub(super) kind: SegmentKind,
    pub(super) table: &'a TypeTable,
    /// The records in ascending record index.
    pub(super) records: &'a DesignRecords,
    /// The records that store an LP-ASCII name after four zero bytes.
    pub(super) named: Vec<u32>,
    pub(super) next_entity: u64,
}

impl Segment<'_> {
    /// The segment's `(MetaStream, BulkStream)` member paths and bytes,
    /// relative to the design folder.
    pub(super) fn members(&self) -> Result<[(String, Vec<u8>); 2], CodecError> {
        let folder = self.kind.folder();
        Ok([
            (format!("{folder}/MetaStream.dat"), self.metastream()?),
            (
                format!("{folder}/BulkStream.dat"),
                self.records.bytes.clone(),
            ),
        ])
    }

    /// The header with the generation words `1234, 8, 36, 708837440` and the
    /// zero asset GUID, the type table, the named-entity list, the primary
    /// and secondary indexes, the next-entity-ID counter, and an empty
    /// version-context list and property block.
    fn metastream(&self) -> Result<Vec<u8>, CodecError> {
        let mut out = DesignRecords::default();
        out.lp_ascii(self.kind.name())?
            .u32(self.kind.id())
            .lp_utf16("00000000-0000-0000-0000-000000000000")?;
        for word in [1234, 8, 36, 708_837_440] {
            out.u32(word);
        }
        out.lp_ascii(self.kind.name())?
            .lp_ascii("Fusion")?
            .u32(self.kind.code())
            .u32(0)
            .u32(count(self.table.registry.types.len())?);
        for row in &self.table.registry.types {
            out.lp_ascii(row.type_guid.as_str())?
                .lp_ascii(row.base_type_guid.as_ref().map_or("", |guid| guid.as_str()))?
                .u32(row.version)
                .lp_ascii(&row.module)?
                .u32(count(row.entity_ids.len())?);
            for id in &row.entity_ids {
                out.u64(*id);
            }
        }
        out.u32(count(self.named.len())?);
        for id in &self.named {
            out.u64(u64::from(*id));
        }
        for index in [&self.records.primaries, &self.records.secondaries] {
            let mut entries = index.clone();
            entries.sort_by_key(|&(_, offset)| offset);
            out.u32(count(entries.len())?);
            for (id, offset) in entries {
                out.u64(u64::from(id))
                    .u64(u64::try_from(offset).map_err(|_| {
                        CodecError::NotImplemented("Design record offset exceeds u64".into())
                    })?);
            }
        }
        out.u64(self.next_entity).u32(0).u32(0);
        Ok(out.bytes)
    }
}

fn count(length: usize) -> Result<u32, CodecError> {
    u32::try_from(length).map_err(|_| CodecError::NotImplemented("Design run exceeds u32".into()))
}
