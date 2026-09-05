// SPDX-License-Identifier: Apache-2.0
//! Source locations for decoded and synthetic ASM entities.

use super::attributes::unknown_record_id;
use super::geometry::is_edge_record;
use super::{id, AsmBrep};
use crate::ids::IdFormat;
use crate::sab::Record;
use cadmpeg_ir::geometry::CurveGeometry;
use std::collections::{HashMap, HashSet};

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
pub(crate) fn emit_annotation_records(
    out: &mut AsmBrep,
    records: &[Record],
    by_index: &HashMap<i64, &Record>,
    stream: &str,
    format: IdFormat<'_>,
) {
    let curve_geometries = out
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<HashMap<_, _>>();
    let emitted_ids = out
        .bodies
        .iter()
        .map(|entity| entity.id.as_str())
        .chain(out.regions.iter().map(|entity| entity.id.as_str()))
        .chain(out.shells.iter().map(|entity| entity.id.as_str()))
        .chain(out.faces.iter().map(|entity| entity.id.as_str()))
        .chain(out.loops.iter().map(|entity| entity.id.as_str()))
        .chain(out.coedges.iter().map(|entity| entity.id.as_str()))
        .chain(out.edges.iter().map(|entity| entity.id.as_str()))
        .chain(out.vertices.iter().map(|entity| entity.id.as_str()))
        .chain(out.points.iter().map(|entity| entity.id.as_str()))
        .chain(out.surfaces.iter().map(|entity| entity.id.as_str()))
        .chain(out.curves.iter().map(|entity| entity.id.as_str()))
        .chain(out.pcurves.iter().map(|entity| entity.id.as_str()))
        .collect::<HashSet<_>>();
    let attribute_ids = out
        .attributes
        .iter()
        .map(|attribute| attribute.id.as_str())
        .collect::<HashSet<_>>();
    let unknown_ids = out
        .unknowns
        .iter()
        .map(|unknown| unknown.id().0.as_str())
        .collect::<HashSet<_>>();
    let procedural_ids = out
        .procedural_surfaces
        .iter()
        .map(|(_, entity)| entity.id.as_str())
        .chain(
            out.procedural_curves
                .iter()
                .map(|(_, entity)| entity.id.as_str()),
        )
        .collect::<HashSet<_>>();
    for record in records {
        let entity_id = id(format, record.index as i64);
        if emitted_ids.contains(entity_id.as_str()) {
            let mut derived_fields = Vec::new();
            match record.head() {
                "plane" => {
                    derived_fields.extend(["geometry.normal", "geometry.u_axis"]);
                }
                "cone" => {
                    derived_fields.extend(["geometry.axis", "geometry.ref_direction"]);
                }
                "sphere" => {
                    derived_fields.extend(["geometry.axis", "geometry.ref_direction"]);
                }
                "torus" => {
                    derived_fields.extend(["geometry.axis", "geometry.ref_direction"]);
                }
                "straight" => derived_fields.push("geometry.direction"),
                "ellipse" => match curve_geometries.get(entity_id.as_str()) {
                    Some(CurveGeometry::Circle { .. }) => {
                        derived_fields.extend(["geometry.axis", "geometry.ref_direction"]);
                    }
                    Some(CurveGeometry::Ellipse { .. }) => {
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
                        derived_fields.push("param_range");
                    }
                }
            }
            out.annotation_records.push(AnnotationRecord {
                id: entity_id,
                stream: stream.to_owned(),
                offset: record.offset as u64,
                tag: AnnotationTag::Record(record.name.clone()),
                derived_fields,
            });
        }
        let attribute_id = format!("{format}:brep:attribute#{}", record.index);
        if attribute_ids.contains(attribute_id.as_str()) {
            out.annotation_records.push(AnnotationRecord {
                id: attribute_id,
                stream: stream.to_owned(),
                offset: record.offset as u64,
                tag: AnnotationTag::Record(record.name.clone()),
                derived_fields: Vec::new(),
            });
        }
        let unknown_id = unknown_record_id(record, format);
        if unknown_ids.contains(unknown_id.as_str()) {
            out.annotation_records.push(AnnotationRecord {
                id: unknown_id,
                stream: stream.to_owned(),
                offset: record.offset as u64,
                tag: AnnotationTag::Record(record.name.clone()),
                derived_fields: Vec::new(),
            });
        }
        for (synthetic_id, tag) in [
            (
                format!("{format}:brep:procedural_surface#{}", record.index),
                AnnotationTag::ProceduralSurface,
            ),
            (
                format!("{format}:brep:procedural_curve#{}", record.index),
                AnnotationTag::ProceduralCurve,
            ),
        ] {
            if procedural_ids.contains(synthetic_id.as_str()) {
                out.annotation_records.push(AnnotationRecord {
                    id: synthetic_id,
                    stream: stream.to_owned(),
                    offset: record.offset as u64,
                    tag,
                    derived_fields: Vec::new(),
                });
            }
        }
    }
    let procedural_surface_prefix = format!("{format}:brep:procedural_surface#");
    for (entity_id, tag) in out
        .surfaces
        .iter()
        .map(|entity| (entity.id.as_str(), AnnotationTag::ProceduralSupport))
        .chain(
            out.curves
                .iter()
                .map(|entity| (entity.id.as_str(), AnnotationTag::ProceduralCurveChild)),
        )
    {
        if !entity_id.starts_with(&procedural_surface_prefix) {
            continue;
        }
        let Some(index) = entity_id
            .split_once('#')
            .and_then(|(_, suffix)| suffix.split(':').next())
            .and_then(|value| value.parse::<usize>().ok())
        else {
            continue;
        };
        let Some(record) = records.get(index) else {
            continue;
        };
        out.annotation_records.push(AnnotationRecord {
            id: entity_id.to_owned(),
            stream: stream.to_owned(),
            offset: record.offset as u64,
            tag,
            derived_fields: Vec::new(),
        });
    }
}
