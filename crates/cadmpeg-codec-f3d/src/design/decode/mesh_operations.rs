// SPDX-License-Identifier: Apache-2.0

use super::meta::{dynamic_type, record_header_class_tag};
use crate::bytes::take_reference;
use crate::layout::indexed_design_record_header as header;
use crate::layout::mesh_operation_group_prefix as group;
use crate::layout::mesh_operation_scope_prefix as prefix;
use crate::layout::paramesh_operation_member_prefix as member;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MeshOperationKind {
    Section,
    Reduce,
    Repair,
    Remesh,
    Smooth,
    Shell,
    ReverseNormals,
}

impl MeshOperationKind {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "MeshSection" => Self::Section,
            "MeshReduce" => Self::Reduce,
            "MeshRepair" => Self::Repair,
            "MeshRemesh" => Self::Remesh,
            "MeshSmooth" => Self::Smooth,
            "MeshShell" => Self::Shell,
            "MeshReverseNormals" => Self::ReverseNormals,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MeshOperationFields {
    Section {
        section_group: u32,
        sketch_scope: u32,
    },
    Reduce {
        tolerance_owner: u32,
        distribution: u32,
        proportion_owner: u32,
        target: u32,
        face_count_owner: u32,
    },
    Repair {
        density_owner: u32,
        offset_owner: u32,
        rebuild_mode: u32,
        method: u32,
    },
    Remesh {
        distribution: u32,
        shape_preservation_owner: Option<u32>,
        edge_length_owner: u32,
    },
    Smooth {
        smoothness_owner: u32,
    },
    Shell {
        thickness_owner: u32,
    },
    ReverseNormals,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MeshOperationScope {
    pub(crate) prefix_word: Option<u32>,
    pub(crate) fields: MeshOperationFields,
    pub(crate) output_geometry: Option<u32>,
    pub(crate) input_geometry: u32,
    pub(crate) source_group: u32,
    pub(crate) tail_error: Option<MeshOperationFraming>,
    pub(crate) deviations: Vec<(&'static str, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MeshOperationFraming(pub(crate) &'static str);

impl std::fmt::Display for MeshOperationFraming {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    deviations: Vec<(&'static str, u32)>,
}

impl Cursor<'_> {
    fn u8(&mut self, field: &'static str) -> Result<u8, MeshOperationFraming> {
        let mut view = View::over_retained(self.bytes);
        view.seek(self.at).ok_or(MeshOperationFraming(field))?;
        let value = view.u8().ok_or(MeshOperationFraming(field))?;
        self.at += 1;
        Ok(value)
    }

    fn u32(&mut self, field: &'static str) -> Result<u32, MeshOperationFraming> {
        let value = View::u32_le_at(self.bytes, self.at).ok_or(MeshOperationFraming(field))?;
        self.at += 4;
        Ok(value)
    }

    fn optional_reference(
        &mut self,
        field: &'static str,
    ) -> Result<Option<u32>, MeshOperationFraming> {
        let start = self.at;
        let reference =
            take_reference(self.bytes, &mut self.at).ok_or(MeshOperationFraming(field))?;
        if self.at == start + 1 {
            return Ok(None);
        }
        reference
            .local()
            .filter(|_| self.at == start + header::LEN)
            .and_then(|(target, _)| u32::try_from(target).ok())
            .filter(|target| *target != 0)
            .map(Some)
            .ok_or(MeshOperationFraming(field))
    }

    fn reference(&mut self, field: &'static str) -> Result<u32, MeshOperationFraming> {
        self.optional_reference(field)?
            .ok_or(MeshOperationFraming(field))
    }

    fn expect_u8(&mut self, field: &'static str, expected: u8) -> Result<(), MeshOperationFraming> {
        let value = self.u8(field)?;
        if value != expected {
            self.deviations.push((field, u32::from(value)));
        }
        Ok(())
    }

    fn expect_u32(
        &mut self,
        field: &'static str,
        expected: u32,
    ) -> Result<(), MeshOperationFraming> {
        let value = self.u32(field)?;
        if value != expected {
            self.deviations.push((field, value));
        }
        Ok(())
    }
}

pub(crate) fn read_scope(
    kind: MeshOperationKind,
    scope: &[u8],
) -> Result<MeshOperationScope, MeshOperationFraming> {
    if scope
        .get(prefix::ZERO_RUN_9..prefix::FLAG)
        .is_none_or(|run| run.iter().any(|byte| *byte != 0))
    {
        return Err(MeshOperationFraming("scope zero run"));
    }
    let mut cursor = Cursor {
        bytes: scope,
        at: prefix::FLAG,
        deviations: Vec::new(),
    };
    let prefix_word = match cursor.u8("scope flag")? {
        0 => None,
        1 => Some(cursor.u32("scope prefix word")?),
        _ => return Err(MeshOperationFraming("scope flag")),
    };
    let fields = match kind {
        MeshOperationKind::Section => MeshOperationFields::Section {
            section_group: cursor
                .optional_reference("section group reference")?
                .unwrap_or(0),
            sketch_scope: cursor
                .optional_reference("sketch scope reference")?
                .unwrap_or(0),
        },
        MeshOperationKind::Reduce => {
            let tolerance_owner = cursor
                .optional_reference("tolerance owner reference")?
                .unwrap_or(0);
            let distribution = cursor.u32("distribution selector")?;
            cursor.expect_u8("reduce-byte-after-distribution", 0)?;
            let proportion_owner = cursor
                .optional_reference("proportion owner reference")?
                .unwrap_or(0);
            let target = cursor.u32("target selector")?;
            MeshOperationFields::Reduce {
                tolerance_owner,
                distribution,
                proportion_owner,
                target,
                face_count_owner: cursor
                    .optional_reference("face-count owner reference")?
                    .unwrap_or(0),
            }
        }
        MeshOperationKind::Repair => MeshOperationFields::Repair {
            density_owner: cursor
                .optional_reference("density owner reference")?
                .unwrap_or(0),
            offset_owner: cursor
                .optional_reference("offset owner reference")?
                .unwrap_or(0),
            rebuild_mode: cursor.u32("rebuild selector")?,
            method: cursor.u32("method selector")?,
        },
        MeshOperationKind::Remesh => {
            cursor.expect_u8("remesh-leading-byte:0", 0)?;
            cursor.expect_u8("remesh-leading-byte:1", 0)?;
            let distribution = cursor.u32("distribution selector")?;
            let shape_preservation_owner =
                cursor.optional_reference("shape-preservation owner reference")?;
            cursor.expect_u8("remesh-byte-after-shape-preservation", 0)?;
            let edge_length_owner = cursor
                .optional_reference("edge-length owner reference")?
                .unwrap_or(0);
            cursor.expect_u32("remesh-word-after-edge-length", 1)?;
            MeshOperationFields::Remesh {
                distribution,
                shape_preservation_owner,
                edge_length_owner,
            }
        }
        MeshOperationKind::Smooth => MeshOperationFields::Smooth {
            smoothness_owner: cursor
                .optional_reference("smoothness owner reference")?
                .unwrap_or(0),
        },
        MeshOperationKind::Shell => MeshOperationFields::Shell {
            thickness_owner: cursor
                .optional_reference("thickness owner reference")?
                .unwrap_or(0),
        },
        MeshOperationKind::ReverseNormals => MeshOperationFields::ReverseNormals,
    };
    let mut output_geometry = None;
    let mut input_geometry = 0;
    let mut source_group = 0;
    let tail_error = (|| {
        output_geometry = cursor.optional_reference("output geometry reference")?;
        input_geometry = cursor.reference("input geometry reference")?;
        cursor.expect_u32("tail-word-after-input-geometry", 0)?;
        source_group = cursor.reference("source group reference")?;
        cursor.expect_u8("tail-byte-after-source-group", 0)
    })()
    .err();
    Ok(MeshOperationScope {
        prefix_word,
        fields,
        output_geometry,
        input_geometry,
        source_group,
        tail_error,
        deviations: cursor.deviations,
    })
}

pub(crate) struct MeshOperationRecords<'a> {
    bytes: &'a [u8],
    meta: std::sync::Arc<crate::metastream::MetaStream>,
    primary: std::collections::HashMap<u64, Option<usize>>,
    secondary: std::collections::HashMap<u64, Option<usize>>,
    registrations: std::collections::HashMap<u64, Option<usize>>,
}

impl<'a> MeshOperationRecords<'a> {
    pub(crate) fn index(
        bytes: &'a [u8],
        meta: std::sync::Arc<crate::metastream::MetaStream>,
    ) -> Self {
        fn insert(
            index: &mut std::collections::HashMap<u64, Option<usize>>,
            entity: u64,
            ordinal: usize,
        ) {
            index
                .entry(entity)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(ordinal));
        }
        let mut primary = std::collections::HashMap::new();
        let mut secondary = std::collections::HashMap::new();
        let mut registrations = std::collections::HashMap::new();
        for (ordinal, record) in meta.records.iter().enumerate() {
            insert(&mut primary, record.entity_id, ordinal);
        }
        for (ordinal, record) in meta.secondary_records.iter().enumerate() {
            insert(&mut secondary, record.entity_id, ordinal);
        }
        for (ordinal, row) in meta.types.iter().enumerate() {
            for &entity in row.entities.values() {
                insert(&mut registrations, entity, ordinal);
            }
        }
        Self {
            bytes,
            meta,
            primary,
            secondary,
            registrations,
        }
    }

    pub(crate) fn validate_scope(
        &self,
        kind: MeshOperationKind,
        scope_index: u32,
    ) -> Result<&'a [u8], MeshOperationFraming> {
        let (guid, version) = match kind {
            MeshOperationKind::Section => ("8FFFCBD2-7102-4A62-BCA0-8229A3C3B9DB", 1),
            MeshOperationKind::Reduce => ("BE693D4C-741E-4794-A93E-5BFED71E9E13", 2),
            MeshOperationKind::Repair => ("68691143-5EE3-428E-9CD3-F728E4718559", 4),
            MeshOperationKind::Remesh => ("828DDA62-2750-447D-8728-06C9B7C2B117", 2),
            MeshOperationKind::Smooth => ("F84CD988-9696-458D-8AF3-795D12E6B9EF", 2),
            MeshOperationKind::Shell => ("1514CDB8-BB81-4895-A953-7B12F4504F99", 2),
            MeshOperationKind::ReverseNormals => ("4AB7A6BD-C40C-4C44-B7CA-0D415B6E4412", 0),
        };
        self.record(
            scope_index,
            "operation scope record",
            guid,
            "90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF",
            "Fusion",
            Some(version),
        )
    }

    fn record(
        &self,
        record_index: u32,
        field: &'static str,
        guid: &str,
        base: &str,
        module: &str,
        version: Option<u32>,
    ) -> Result<&'a [u8], MeshOperationFraming> {
        let error = MeshOperationFraming(field);
        if record_index == 0 {
            return Err(error);
        }
        let entity = u64::from(record_index);
        let ordinal = self.primary.get(&entity).copied().flatten().ok_or(error)?;
        let indexed = &self.meta.records[ordinal];
        let start = usize::try_from(indexed.bulk_offset).map_err(|_| error)?;
        let end = match self.meta.records.get(ordinal + 1) {
            Some(next) => usize::try_from(next.bulk_offset).map_err(|_| error)?,
            None => self.bytes.len(),
        };
        if start >= end
            || end > self.bytes.len()
            || ordinal.checked_sub(1).is_some_and(|previous| {
                self.meta.records[previous].bulk_offset >= indexed.bulk_offset
            })
        {
            return Err(error);
        }
        let member_end = if let Some(nested_index) = self.secondary.get(&entity) {
            let nested = &self.meta.secondary_records[nested_index.ok_or(error)?];
            let at = usize::try_from(nested.bulk_offset).map_err(|_| error)?;
            if at <= start || at >= end {
                return Err(error);
            }
            let tag = record_header_class_tag(self.bytes, at, end, entity).ok_or(error)?;
            dynamic_type(&self.meta, &tag).ok_or(error)?;
            at
        } else {
            end
        };
        let tag = record_header_class_tag(self.bytes, start, member_end, entity).ok_or(error)?;
        let (ordinal, kind) = dynamic_type(&self.meta, &tag).ok_or(error)?;
        if !kind.type_guid.as_str().eq_ignore_ascii_case(guid)
            || !kind
                .base_type_guid
                .value()
                .is_some_and(|value| value.as_str().eq_ignore_ascii_case(base))
            || version.is_some_and(|version| kind.version != version)
            || kind.module != module
            || self.registrations.get(&entity).copied().flatten() != Some(ordinal)
        {
            return Err(error);
        }
        let record = self.bytes.get(start..member_end).ok_or(error)?;
        Ok(record)
    }

    fn source_record(
        &self,
        record_index: u32,
        field: &'static str,
        guid: &str,
        base: &str,
    ) -> Result<&'a [u8], MeshOperationFraming> {
        let record = self.record(record_index, field, guid, base, "Fusion", Some(0))?;
        if record
            .get(member::ZERO_RUN_10..member::BODY_REFERENCE)
            .is_none_or(|run| run.iter().any(|byte| *byte != 0))
        {
            return Err(MeshOperationFraming(field));
        }
        Ok(record)
    }

    pub(crate) fn group_bodies(
        &self,
        ctx: &DecodeContext<'_>,
        group: u32,
    ) -> Result<Result<Vec<u32>, MeshOperationFraming>, CodecError> {
        let record = match self.source_record(
            group,
            "source group record",
            "22975A91-16AD-4CC9-A5E4-B4F8B4C27881",
            "2CA5A1CD-C99B-4C9B-91AF-57989148E841",
        ) {
            Ok(record) => record,
            Err(error) => return Ok(Err(error)),
        };
        let mut cursor = Cursor {
            bytes: record,
            at: group::MEMBER_COUNT,
            deviations: Vec::new(),
        };
        let declared = match cursor.u32("source group member count") {
            Ok(declared) => declared,
            Err(error) => return Ok(Err(error)),
        };
        let Some(count) = cadmpeg_core::decode::bounded_len(
            u64::from(declared),
            header::LEN,
            record.len() - cursor.at,
        )
        .filter(|count| *count != 0) else {
            return Ok(Err(MeshOperationFraming("source group member count")));
        };
        let mut bodies = ctx.alloc_filled(count, 0, "F3D mesh operation source bodies")?;
        for body in &mut bodies {
            let read = cursor
                .reference("source group member reference")
                .and_then(|index| {
                    self.source_record(
                        index,
                        "source member record",
                        "1AA9CC7B-5C34-45DA-AE14-0444E7F966F2",
                        "6144EA59-3ACA-4172-9F14-A6531E94D69B",
                    )
                })
                .and_then(|record| {
                    Cursor {
                        bytes: record,
                        at: member::BODY_REFERENCE,
                        deviations: Vec::new(),
                    }
                    .reference("source member body reference")
                })
                .and_then(|body| {
                    self.record(
                        body,
                        "source body record",
                        "EA90DA22-556C-4C61-89BB-20C2681B7A9D",
                        "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
                        "ParaMesh",
                        None,
                    )?;
                    Ok(body)
                });
            match read {
                Ok(index) => *body = index,
                Err(error) => return Ok(Err(error)),
            }
        }
        Ok(Ok(bodies))
    }
}

#[cfg(test)]
mod tests;
