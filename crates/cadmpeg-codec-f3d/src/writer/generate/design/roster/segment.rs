// SPDX-License-Identifier: Apache-2.0
//! One segment of a Base Feature design: its type table, its records, and
//! the `MetaStream` that indexes them (f3d.md §1.4; §3.1, "Records of a
//! Base Feature design").

use cadmpeg_core::CodecError;

use super::super::frames::DesignRecords;
use crate::records::references::DesignClassTag;
use crate::writer::generate::presentation::{dynamic_class_tag, TypeRow};

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
    rows: Vec<(TypeRow, Vec<u64>)>,
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
        let mut table = Self { rows: Vec::new() };
        for (index, row) in &ordered {
            let position = table.insert(*row);
            table.rows[position].1.push(u64::from(*index));
            let mut base = row.base;
            while let Some(guid) = base {
                let Some(ancestor) = lookup(guid) else {
                    break;
                };
                if table.position(ancestor.guid).is_some() {
                    break;
                }
                table.insert(ancestor);
                base = ancestor.base;
            }
        }
        for row in known {
            table.insert(*row);
        }
        if table.rows.len() > 999 - 256 {
            return Err(CodecError::NotImplemented(
                "a Base Feature type table exceeds three-digit class tags".into(),
            ));
        }
        Ok(table)
    }

    fn position(&self, guid: &str) -> Option<usize> {
        self.rows
            .iter()
            .position(|(row, _)| row.guid.eq_ignore_ascii_case(guid))
    }

    /// The position of `row`, appended when no row has its GUID.
    fn insert(&mut self, row: TypeRow) -> usize {
        self.position(row.guid).unwrap_or_else(|| {
            self.rows.push((row, Vec::new()));
            self.rows.len() - 1
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
            .u32(count(self.table.rows.len())?);
        for (row, ids) in &self.table.rows {
            out.lp_ascii(row.guid)?
                .lp_ascii(row.base.unwrap_or(""))?
                .u32(row.version)
                .lp_ascii(row.module)?
                .u32(count(ids.len())?);
            for id in ids {
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
