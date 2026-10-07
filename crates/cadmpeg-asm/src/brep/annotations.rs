// SPDX-License-Identifier: Apache-2.0
//! Source locations for decoded and synthetic ASM entities.

use super::attributes::unknown_record_id;
use super::geometry::is_edge_record;
use super::{AsmBrep, Carriers};
use crate::ids::IdFormat;
use crate::sab::Record;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use std::collections::HashMap;

/// Provenance tag for a source record or a synthetic procedural entity.
pub enum AnnotationTag {
    /// The full name stored by a source SAB record.
    Record(String),
    /// A procedural surface definition.
    ProceduralSurface,
    /// A procedural curve definition.
    ProceduralCurve,
    /// An embedded procedural surface support.
    ProceduralSupport,
    /// An embedded procedural curve child.
    ProceduralCurveChild,
}

impl AnnotationTag {
    /// Stable text used by the annotation wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Record(name) => name,
            Self::ProceduralSurface => "procedural_surface",
            Self::ProceduralCurve => "procedural_curve",
            Self::ProceduralSupport => "procedural_support",
            Self::ProceduralCurveChild => "procedural_curve_child",
        }
    }
}

/// One sparse v1 annotation produced while SAB record offsets are available.
pub struct AnnotationRecord {
    /// Globally unique IR entity id.
    pub id: String,
    /// BREP ZIP entry containing the source SAB record.
    pub stream: String,
    /// Byte offset in the decompressed ASM stream.
    pub offset: u64,
    /// Source SAB record name or synthetic annotation kind.
    pub tag: AnnotationTag,
    /// Serialized fields whose values were canonically derived.
    pub derived_fields: Vec<&'static str>,
}

/// Emit annotation records mapping every emitted entity, attribute, unknown,
/// and synthetic procedural id back to its source record offset.
pub(super) fn emit_annotation_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    carriers: &mut Carriers,
    stream: &str,
    format: IdFormat,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut index_storage = ctx.reserve_scoped(0, "ASM annotation indices")?;
    let curve_geometries = index_storage.with_storage(|| {
        ctx.collect_hash_map(
            ctx.admit_iter(&out.curves, "ASM annotation source arena")?
                .map(|curve| (curve.id.as_str(), &curve.geometry)),
            "ASM annotation curve geometry index",
        )
    })?;
    let emitted_ids = index_storage.with_storage(|| {
        ctx.collect_hash_set(
            ctx.admit_iter(&out.bodies, "ASM annotation source arena")?
                .map(|entity| entity.id.as_str())
                .chain(
                    ctx.admit_iter(&out.regions, "ASM annotation source arena")?
                        .map(|entity| entity.id.as_str()),
                )
                .chain(
                    ctx.admit_iter(&out.shells, "ASM annotation source arena")?
                        .map(|entity| entity.id.as_str()),
                )
                .chain(
                    ctx.admit_iter(&out.faces, "ASM annotation source arena")?
                        .map(|entity| entity.id.as_str()),
                )
                .chain(
                    ctx.admit_iter(&out.loops, "ASM annotation source arena")?
                        .map(|entity| entity.id.as_str()),
                )
                .chain(
                    ctx.admit_iter(&out.coedges, "ASM annotation source arena")?
                        .map(|entity| entity.id.as_str()),
                )
                .chain(
                    ctx.admit_iter(&out.edges, "ASM annotation source arena")?
                        .map(|entity| entity.id.as_str()),
                )
                .chain(
                    ctx.admit_iter(&out.vertices, "ASM annotation source arena")?
                        .map(|entity| entity.id.as_str()),
                )
                .chain(
                    ctx.admit_iter(&out.points, "ASM annotation source arena")?
                        .map(|entity| entity.id.as_str()),
                )
                .chain(
                    ctx.admit_iter(&out.surfaces, "ASM annotation source arena")?
                        .map(|entity| entity.id.as_str()),
                )
                .chain(
                    ctx.admit_iter(&out.curves, "ASM annotation source arena")?
                        .map(|entity| entity.id.as_str()),
                )
                .chain(
                    ctx.admit_iter(&out.pcurves, "ASM annotation source arena")?
                        .map(|entity| entity.id.as_str()),
                ),
            "ASM annotation emitted IDs",
        )
    })?;
    let attribute_ids = index_storage.with_storage(|| {
        ctx.collect_hash_set(
            ctx.admit_iter(&out.attributes, "ASM annotation source arena")?
                .map(|attribute| attribute.id.as_str()),
            "ASM annotation attribute IDs",
        )
    })?;
    let unknown_ids = index_storage.with_storage(|| {
        ctx.collect_hash_set(
            ctx.admit_iter(&out.unknowns, "ASM annotation source arena")?
                .map(|unknown| unknown.id().as_str()),
            "ASM annotation unknown IDs",
        )
    })?;
    let procedural_ids = index_storage.with_storage(|| {
        ctx.collect_hash_set(
            ctx.admit_iter(&out.procedural_surfaces, "ASM annotation source arena")?
                .map(|(_, entity)| entity.id.as_str())
                .chain(
                    ctx.admit_iter(&out.procedural_curves, "ASM annotation source arena")?
                        .map(|(_, entity)| entity.id.as_str()),
                ),
            "ASM annotation procedural IDs",
        )
    })?;
    for record in ctx.admit_iter(records, "ASM annotation source records")? {
        let mut candidates = ctx.reserve_scoped(0, "ASM annotation candidate IDs")?;
        let index = i64::try_from(record.index).map_err(|_| {
            ctx.refuse_codec_limit(
                "ASM record index",
                9_223_372_036_854_775_807,
                cadmpeg_core::decode::u64_from_index(record.index),
            )
        })?;
        let entity_id = ctx.format_scoped_text(
            &mut candidates,
            format_args!("{format}:brep:entity#{index}"),
            "ASM annotation entity candidate",
        )?;
        if ctx.contains_hash_set(
            &emitted_ids,
            &entity_id.as_str(),
            "ASM annotation entity lookup",
        )? {
            let mut derived_fields = Vec::new();
            match record.head() {
                "plane" => {
                    ctx.reserve_vec(&mut derived_fields, 2, "ASM annotation derived fields")?;
                    derived_fields.extend(["geometry.normal", "geometry.u_axis"]);
                }
                "cone" => {
                    ctx.reserve_vec(&mut derived_fields, 2, "ASM annotation derived fields")?;
                    derived_fields.extend(["geometry.axis", "geometry.ref_direction"]);
                }
                "sphere" => {
                    ctx.reserve_vec(&mut derived_fields, 2, "ASM annotation derived fields")?;
                    derived_fields.extend(["geometry.axis", "geometry.ref_direction"]);
                }
                "torus" => {
                    ctx.reserve_vec(&mut derived_fields, 2, "ASM annotation derived fields")?;
                    derived_fields.extend(["geometry.axis", "geometry.ref_direction"]);
                }
                "straight" => {
                    ctx.reserve_vec(&mut derived_fields, 1, "ASM annotation derived fields")?;
                    derived_fields.push("geometry.direction");
                }
                "ellipse" => match ctx.get_hash_map(
                    &curve_geometries,
                    &entity_id.as_str(),
                    "ASM annotation curve lookup",
                )? {
                    Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(_))) => {
                        ctx.reserve_vec(&mut derived_fields, 2, "ASM annotation derived fields")?;
                        derived_fields.extend(["geometry.axis", "geometry.ref_direction"]);
                    }
                    Some(CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(_))) => {
                        ctx.reserve_vec(&mut derived_fields, 2, "ASM annotation derived fields")?;
                        derived_fields.extend(["geometry.axis", "geometry.major_direction"]);
                    }
                    _ => {}
                },
                _ => {}
            }
            if is_edge_record(record) {
                if let Some(curve) = record
                    .ref_at(8)
                    .and_then(|reference| by_index.get(&reference))
                {
                    if curve.head() == "ellipse" {
                        ctx.reserve_vec(&mut derived_fields, 1, "ASM annotation derived fields")?;
                        derived_fields.push("param_range");
                    }
                }
            }
            ctx.reserve_vec(&mut out.annotation_records, 1, "ASM annotation records")?;
            out.annotation_records.push(AnnotationRecord {
                id: ctx.copy_retained_text(&entity_id, "ASM annotation entity id")?,
                stream: ctx.copy_retained_text(stream, "ASM annotation stream")?,
                offset: cadmpeg_core::decode::u64_from_index(record.offset),
                tag: AnnotationTag::Record(
                    ctx.copy_retained_text(&record.name, "ASM annotation record name")?,
                ),
                derived_fields,
            });
        }
        let attribute_id = ctx.format_scoped_text(
            &mut candidates,
            format_args!("{format}:brep:attribute#{}", record.index),
            "ASM annotation attribute candidate",
        )?;
        if ctx.contains_hash_set(
            &attribute_ids,
            &attribute_id.as_str(),
            "ASM annotation attribute lookup",
        )? {
            ctx.reserve_vec(&mut out.annotation_records, 1, "ASM annotation records")?;
            out.annotation_records.push(AnnotationRecord {
                id: ctx.copy_retained_text(&attribute_id, "ASM annotation attribute id")?,
                stream: ctx.copy_retained_text(stream, "ASM annotation stream")?,
                offset: cadmpeg_core::decode::u64_from_index(record.offset),
                tag: AnnotationTag::Record(
                    ctx.copy_retained_text(&record.name, "ASM annotation record name")?,
                ),
                derived_fields: Vec::new(),
            });
        }
        let unknown_id = candidates.with_storage(|| unknown_record_id(ctx, record, format))?;
        if ctx.contains_hash_set(
            &unknown_ids,
            &unknown_id.as_str(),
            "ASM annotation unknown lookup",
        )? {
            ctx.reserve_vec(&mut out.annotation_records, 1, "ASM annotation records")?;
            out.annotation_records.push(AnnotationRecord {
                id: ctx.copy_retained_text(unknown_id.as_str(), "ASM annotation unknown id")?,
                stream: ctx.copy_retained_text(stream, "ASM annotation stream")?,
                offset: cadmpeg_core::decode::u64_from_index(record.offset),
                tag: AnnotationTag::Record(
                    ctx.copy_retained_text(&record.name, "ASM annotation record name")?,
                ),
                derived_fields: Vec::new(),
            });
        }
        for (kind, tag) in [
            ("procedural_surface", AnnotationTag::ProceduralSurface),
            ("procedural_curve", AnnotationTag::ProceduralCurve),
        ] {
            let synthetic_id = ctx.format_scoped_text(
                &mut candidates,
                format_args!("{format}:brep:{kind}#{}", record.index),
                "ASM annotation procedural candidate",
            )?;
            if ctx.contains_hash_set(
                &procedural_ids,
                &synthetic_id.as_str(),
                "ASM annotation procedural lookup",
            )? {
                ctx.reserve_vec(&mut out.annotation_records, 1, "ASM annotation records")?;
                out.annotation_records.push(AnnotationRecord {
                    id: ctx.copy_retained_text(&synthetic_id, "ASM annotation procedural id")?,
                    stream: ctx.copy_retained_text(stream, "ASM annotation stream")?,
                    offset: cadmpeg_core::decode::u64_from_index(record.offset),
                    tag,
                    derived_fields: Vec::new(),
                });
            }
        }
    }
    for (index, entity_id, tag) in ctx
        .admit_iter(
            std::mem::take(&mut carriers.procedural_support_sources),
            "ASM procedural support annotations",
        )?
        .map(|(index, id)| {
            (
                index,
                cadmpeg_ir::ids::Identity::from(id).into_string(),
                AnnotationTag::ProceduralSupport,
            )
        })
        .chain(
            ctx.admit_iter(
                std::mem::take(&mut carriers.procedural_curve_child_sources),
                "ASM procedural child annotations",
            )?
            .map(|(index, id)| {
                (
                    index,
                    cadmpeg_ir::ids::Identity::from(id).into_string(),
                    AnnotationTag::ProceduralCurveChild,
                )
            }),
        )
    {
        let record = by_index.get(&index).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed(format_args!(
                "synthetic entity {entity_id} source record {index} is missing"
            ))
        })?;
        ctx.reserve_vec(&mut out.annotation_records, 1, "ASM annotation records")?;
        out.annotation_records.push(AnnotationRecord {
            id: entity_id,
            stream: ctx.copy_retained_text(stream, "ASM annotation stream")?,
            offset: cadmpeg_core::decode::u64_from_index(record.offset),
            tag,
            derived_fields: Vec::new(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
