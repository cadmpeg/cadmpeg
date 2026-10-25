// SPDX-License-Identifier: Apache-2.0
//! Typed interpretations of an actual archive member's binary contents.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, SourceExtentProof, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::scalar::{FiniteBinary32, FiniteReal};
use serde::{Deserialize, Serialize};

/// Admit one canonical entry collection under the existing local expansion bound.
/// Temporary slots are not charged as retained output; actual allocation failures
/// within the admitted bound remain session-fatal through the core allocator.
pub(crate) fn alloc_slots<T: Clone>(
    ctx: &DecodeContext<'_>,
    count: usize,
    value: T,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let bytes = count
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(|| CodecError::malformed("entry slot storage overflows"))?;
    let allowance = ctx.per_expand_allowance();
    if u64_from_index(bytes) > allowance {
        return Err(ctx.refuse_part_limit(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            allowance,
            0,
            u64_from_index(bytes),
            operation,
        ));
    }
    ctx.alloc_filled(count, value, operation)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BinaryPrecision {
    Single,
    Double,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BinaryMaterial {
    pub(crate) ambient: u32,
    pub(crate) diffuse: u32,
    pub(crate) specular: u32,
    pub(crate) emissive: u32,
    pub(crate) shininess: FiniteBinary32,
    pub(crate) transparency: FiniteBinary32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MaterialSlot {
    pub(crate) fixed: Option<BinaryMaterial>,
    pub(crate) image: Option<String>,
    pub(crate) image_path: Option<String>,
    pub(crate) uuid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FilletValue {
    pub(crate) edge: i32,
    pub(crate) radius1: FiniteReal,
    pub(crate) radius2: FiniteReal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MeshFacet {
    pub(crate) points: [u32; 3],
    pub(crate) neighbours: [Option<u32>; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "components", content = "values", rename_all = "snake_case")]
pub(crate) enum F32Values {
    Scalar(Vec<Option<FiniteBinary32>>),
    Vector(Vec<Option<[FiniteBinary32; 3]>>),
    Curvature(Vec<Option<[FiniteBinary32; 8]>>),
}

/// Each vector's positions are the original semantic slots. An absent
/// interpretation denotes unknown arity; a present empty vector means count 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum EntryContents {
    Color {
        values: Vec<Option<u32>>,
    },
    Float {
        values: Vec<Option<FiniteReal>>,
    },
    Vector {
        precision: BinaryPrecision,
        values: Vec<Option<[FiniteReal; 3]>>,
    },
    Placement {
        precision: BinaryPrecision,
        values: Vec<Option<[FiniteReal; 7]>>,
    },
    Fillet {
        values: Vec<Option<FilletValue>>,
    },
    Material {
        version: u32,
        values: Vec<MaterialSlot>,
    },
    F32 {
        values: F32Values,
    },
    MeshMaterial {
        binding: Option<u32>,
        ambient: Option<Vec<Option<u32>>>,
        diffuse: Option<Vec<Option<u32>>>,
        specular: Option<Vec<Option<u32>>>,
        emissive: Option<Vec<Option<u32>>>,
        shininess: Option<Vec<Option<FiniteBinary32>>>,
        transparency: Option<Vec<Option<FiniteBinary32>>>,
    },
    Mesh {
        big_endian: bool,
        vertices: Vec<Option<FinitePoint3>>,
        facets: Vec<Option<MeshFacet>>,
        bounds: Option<[FiniteBinary32; 6]>,
    },
    Points {
        points: Vec<Option<FinitePoint3>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EntryGrammar {
    Color,
    Float,
    Vector(BinaryPrecision),
    Placement(BinaryPrecision),
    Fillet,
    Material(u32),
    F32(usize),
    MeshMaterial,
    Mesh(bool),
    Points,
}

impl EntryContents {
    pub(crate) fn grammar(&self) -> EntryGrammar {
        match self {
            Self::Color { .. } => EntryGrammar::Color,
            Self::Float { .. } => EntryGrammar::Float,
            Self::Vector { precision, .. } => EntryGrammar::Vector(*precision),
            Self::Placement { precision, .. } => EntryGrammar::Placement(*precision),
            Self::Fillet { .. } => EntryGrammar::Fillet,
            Self::Material { version, .. } => EntryGrammar::Material(*version),
            Self::F32 { values } => EntryGrammar::F32(match values {
                F32Values::Scalar(_) => 1,
                F32Values::Vector(_) => 3,
                F32Values::Curvature(_) => 8,
            }),
            Self::MeshMaterial { .. } => EntryGrammar::MeshMaterial,
            Self::Mesh { big_endian, .. } => EntryGrammar::Mesh(*big_endian),
            Self::Points { .. } => EntryGrammar::Points,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        match self {
            Self::Placement { values, .. } => {
                for value in values.iter().flatten() {
                    if crate::placement::placement_components(&value.map(FiniteReal::get)).is_none()
                    {
                        return Err("entry placement has an invalid quaternion".into());
                    }
                }
            }
            Self::Material { version, values } => {
                if *version > 3 {
                    return Err("entry material version is outside 0..=3".into());
                }
                for value in values {
                    if *version != 3
                        && (value.image.is_some()
                            || value.image_path.is_some()
                            || value.uuid.is_some())
                    {
                        return Err("material strings require version three".into());
                    }
                    if [&value.image, &value.image_path, &value.uuid]
                        .into_iter()
                        .flatten()
                        .any(|text| text.contains('\0'))
                    {
                        return Err("material string contains NUL".into());
                    }
                }
            }
            Self::MeshMaterial {
                binding: Some(binding),
                ..
            } if *binding > 2 => return Err("mesh material binding is outside 0..=2".into()),
            Self::Mesh {
                vertices, facets, ..
            } => {
                for facet in facets.iter().flatten() {
                    if facet.points.iter().any(|point| {
                        usize::try_from(*point).map_or(true, |point| point >= vertices.len())
                    }) || facet.neighbours.iter().flatten().any(|facet| {
                        usize::try_from(*facet).map_or(true, |facet| facet >= facets.len())
                    }) {
                        return Err("mesh facet index escapes its entry array".into());
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// A moved result from a genuine entry producer, before its pending header row
/// is serialized. Only source-backed constructors can make this handoff.
#[derive(Debug)]
pub(crate) struct EntryContentUpdate {
    entry: String,
    contents: EntryContents,
    proof: SourceExtentProof,
}
impl EntryContentUpdate {
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        entry: &str,
        source: View<'_>,
        contents: EntryContents,
    ) -> Result<Self, CodecError> {
        contents.validate().map_err(CodecError::malformed)?;
        ctx.retain_bytes(u64_from_index(entry.len()), "entry interpretation identity")?;
        Ok(Self {
            entry: entry.to_owned(),
            contents,
            proof: ctx.source_extent_proof(source)?,
        })
    }
    pub(crate) fn entry_id(&self) -> &str {
        &self.entry
    }
    pub(crate) fn apply(
        self,
        ctx: &DecodeContext<'_>,
        entry: &mut super::EntryRecord,
    ) -> Result<(), CodecError> {
        ctx.require_source_authority(&self.proof)?;
        if self.entry != entry.id() {
            return Err(CodecError::malformed(
                "entry interpretation targets a different actual header",
            ));
        }
        if entry
            .contents
            .iter()
            .any(|contents| contents == &self.contents)
        {
            return Ok(());
        }
        ctx.retain_bytes(
            u64_from_index(size_of::<EntryContents>()),
            "archive entry interpretation slot",
        )?;
        entry.contents.try_reserve_exact(1).map_err(|_| {
            cadmpeg_core::decode::ResourceLimit::allocation_failed(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                u64_from_index(size_of::<EntryContents>()),
                "archive entry interpretation slot",
            )
        })?;
        entry.contents.push(self.contents);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
