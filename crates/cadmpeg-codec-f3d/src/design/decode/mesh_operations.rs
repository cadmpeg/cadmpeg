// SPDX-License-Identifier: Apache-2.0

use super::sketch::IndexedRecordOffsets;
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
        let value = *self.bytes.get(self.at).ok_or(MeshOperationFraming(field))?;
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
            section_group: cursor.reference("section group reference")?,
            sketch_scope: cursor.reference("sketch scope reference")?,
        },
        MeshOperationKind::Reduce => {
            let tolerance_owner = cursor.reference("tolerance owner reference")?;
            let distribution = cursor.u32("distribution selector")?;
            cursor.expect_u8("reduce-byte-after-distribution", 0)?;
            let proportion_owner = cursor.reference("proportion owner reference")?;
            let target = cursor.u32("target selector")?;
            MeshOperationFields::Reduce {
                tolerance_owner,
                distribution,
                proportion_owner,
                target,
                face_count_owner: cursor.reference("face-count owner reference")?,
            }
        }
        MeshOperationKind::Repair => MeshOperationFields::Repair {
            density_owner: cursor.reference("density owner reference")?,
            offset_owner: cursor.reference("offset owner reference")?,
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
            let edge_length_owner = cursor.reference("edge-length owner reference")?;
            cursor.expect_u32("remesh-word-after-edge-length", 1)?;
            MeshOperationFields::Remesh {
                distribution,
                shape_preservation_owner,
                edge_length_owner,
            }
        }
        MeshOperationKind::Smooth => MeshOperationFields::Smooth {
            smoothness_owner: cursor.reference("smoothness owner reference")?,
        },
        MeshOperationKind::Shell => MeshOperationFields::Shell {
            thickness_owner: cursor.reference("thickness owner reference")?,
        },
        MeshOperationKind::ReverseNormals => MeshOperationFields::ReverseNormals,
    };
    let output_geometry = cursor.optional_reference("output geometry reference")?;
    let input_geometry = cursor.reference("input geometry reference")?;
    cursor.expect_u32("tail-word-after-input-geometry", 0)?;
    let source_group = cursor.reference("source group reference")?;
    cursor.expect_u8("tail-byte-after-source-group", 0)?;
    Ok(MeshOperationScope {
        prefix_word,
        fields,
        output_geometry,
        input_geometry,
        source_group,
        deviations: cursor.deviations,
    })
}

pub(crate) struct MeshOperationRecords<'a> {
    bytes: &'a [u8],
    offsets: IndexedRecordOffsets,
}

impl<'a> MeshOperationRecords<'a> {
    pub(crate) fn index(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            offsets: IndexedRecordOffsets::build(bytes),
        }
    }

    fn record(
        &self,
        record_index: u32,
        field: &'static str,
    ) -> Result<&'a [u8], MeshOperationFraming> {
        let start = *self
            .offsets
            .offsets(record_index)
            .first()
            .ok_or(MeshOperationFraming(field))?;
        let record = self.bytes.get(start..).ok_or(MeshOperationFraming(field))?;
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
        let record = match self.record(group, "source group record") {
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
                .and_then(|index| self.record(index, "source member record"))
                .and_then(|record| {
                    Cursor {
                        bytes: record,
                        at: member::BODY_REFERENCE,
                        deviations: Vec::new(),
                    }
                    .reference("source member body reference")
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
