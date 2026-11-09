// SPDX-License-Identifier: Apache-2.0
//! Neutral values the F3D wire derives or holds in a reduced form. Each pass
//! stores the value the decoder reads back and names the record in a
//! `writer.value-normalized` loss (#209 §5.2, `REDUCED_FIELDS`).

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::F3dLossCode;

/// Stored feature-family names, localized, whose scopes carry a suppression
/// state in their history-state identities (spec §3.1).
const SUPPRESSIBLE_FAMILIES: [&str; 9] = [
    "Extrude",
    "Extrusion",
    "Extrusão",
    "Fillet",
    "Congé",
    "Abrundung",
    "Arredondamento",
    "Chamfer",
    "Chanfrein",
];

/// Whether the scope a feature writes to stores its suppression state.
fn stores_suppression(feature: &cadmpeg_ir::features::Feature) -> bool {
    match feature.source_tag.as_deref() {
        Some(tag) => SUPPRESSIBLE_FAMILIES.contains(&tag),
        None => matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(
                FeatureOperation::Extrude { .. }
                    | FeatureOperation::Fillet { .. }
                    | FeatureOperation::FullRoundFillet { .. }
                    | FeatureOperation::Chamfer { .. }
            )
        ),
    }
}

/// A scope outside the Extrude, Fillet and Chamfer families stores no
/// suppression; such a feature is written unsuppressed.
pub(super) fn feature_suppression(target: &mut CadIr, losses: &mut Vec<LossNote>) {
    for feature in &mut target.model.features {
        if feature.suppressed == Some(true) && !stores_suppression(feature) {
            feature.suppressed = Some(false);
            losses.push(F3dLossCode::WriterValueNormalized.note(format!(
                "feature {}: its scope family stores no suppression; it is written unsuppressed",
                feature.id
            )));
        }
    }
}

/// Occurrence placements are rigid row-major matrices (spec §3.1
/// `CopyPaste`). A placement whose linear part is not a proper rotation keeps
/// its translation and stores the rotation of its unit columns, with the
/// third column reversed when that makes the frame right-handed, or the
/// identity when neither is a proper rotation.
pub(super) fn occurrence_placements(target: &mut CadIr, losses: &mut Vec<LossNote>) {
    for occurrence in &mut target.model.occurrences {
        let Some(transform) = occurrence.transform else {
            continue;
        };
        if transform.is_proper_rigid() {
            continue;
        }
        occurrence.transform = Some(rigid_part(transform));
        losses.push(F3dLossCode::WriterValueNormalized.note(format!(
            "occurrence {}: its placement is not rigid; the rigid part is written",
            occurrence.id
        )));
    }
}

fn rigid_part(transform: cadmpeg_ir::transform::Transform) -> cadmpeg_ir::transform::Transform {
    let rows = transform.affine_rows();
    let placed = |columns: [[f64; 3]; 3]| {
        cadmpeg_ir::transform::Transform::affine(std::array::from_fn(|row| {
            [
                columns[0][row],
                columns[1][row],
                columns[2][row],
                rows[row][3],
            ]
        }))
        .filter(cadmpeg_ir::transform::Transform::is_proper_rigid)
    };
    let mut columns: [[f64; 3]; 3] =
        std::array::from_fn(|column| std::array::from_fn(|row| rows[row][column]));
    let unit = columns.iter_mut().all(|column| {
        let length = column.iter().map(|value| value * value).sum::<f64>().sqrt();
        if !length.is_finite() || length == 0.0 {
            return false;
        }
        for value in column.iter_mut() {
            *value /= length;
        }
        true
    });
    let rotation = if unit {
        placed(columns).or_else(|| {
            for value in &mut columns[2] {
                *value = -*value;
            }
            placed(columns)
        })
    } else {
        None
    };
    rotation
        .or_else(|| placed([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]))
        .unwrap_or(transform)
}

/// Stored feature-family names, localized, of sketch scopes (spec §3.1).
const SKETCH_FAMILIES: [&str; 4] = ["Sketch", "Esquisse", "Skizze", "Esboço"];

/// A parameter a non-sketch feature owns is a feature parameter. F3D stores
/// no driven state for a feature parameter, so it is written driving. Sketch
/// dimensions keep their state.
pub(super) fn feature_parameter_driving(target: &mut CadIr, losses: &mut Vec<LossNote>) {
    let owners: std::collections::BTreeSet<_> = target
        .model
        .features
        .iter()
        .filter(|feature| {
            !matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
            ) && !feature
                .source_tag
                .as_deref()
                .is_some_and(|tag| SKETCH_FAMILIES.contains(&tag))
        })
        .map(|feature| feature.id.clone())
        .collect();
    for parameter in &mut target.model.parameters {
        if parameter.driving
            || !parameter
                .owner
                .as_ref()
                .is_some_and(|owner| owners.contains(owner))
        {
            continue;
        }
        parameter.driving = true;
        losses.push(F3dLossCode::WriterValueNormalized.note(format!(
            "parameter {}: a feature parameter is driving; it is written driving",
            parameter.id
        )));
    }
}

/// A sketch placement is stored as a rigid matrix. A frame whose axes the
/// matrix cannot hold is stored with unit axes.
pub(super) fn sketch_placements(target: &mut CadIr, losses: &mut Vec<LossNote>) {
    for sketch in &mut target.model.sketches {
        let Some((origin, normal, u_axis)) = sketch.resolved_placement() else {
            continue;
        };
        let (Some(unit_normal), Some(unit_u_axis)) = (normal.unit_nonzero(), u_axis.unit_nonzero())
        else {
            continue;
        };
        let (origin_raw, normal_raw, u_raw) = (origin.get(), normal.get(), u_axis.get());
        let v_raw = normal_raw.cross(u_raw);
        let stored = [
            [u_raw.x, v_raw.x, normal_raw.x, origin_raw.x / 10.0],
            [u_raw.y, v_raw.y, normal_raw.y, origin_raw.y / 10.0],
            [u_raw.z, v_raw.z, normal_raw.z, origin_raw.z / 10.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        if crate::records::sketch_placement::SketchPlacementMatrix::try_from(stored).is_ok() {
            continue;
        }
        let Ok(placement) = cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            origin.get(),
            unit_normal,
            unit_u_axis,
        ) else {
            continue;
        };
        sketch.placement = placement;
        losses.push(F3dLossCode::WriterValueNormalized.note(format!(
            "sketch {}: its placement axes are written as unit vectors",
            sketch.id
        )));
    }
}

/// The media type of an F3D image asset follows the extension of its entry
/// name: PNG and JPEG names carry their type, and any other name none.
pub(super) fn asset_media_types(target: &mut CadIr, losses: &mut Vec<LossNote>) {
    for asset in &mut target.model.assets {
        let derived = asset
            .name
            .as_ref()
            .and_then(|name| std::path::Path::new(name.as_str()).extension())
            .and_then(|extension| extension.to_str())
            .and_then(|extension| {
                if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
                    Some("image/jpeg")
                } else if extension.eq_ignore_ascii_case("png") {
                    Some("image/png")
                } else {
                    None
                }
            });
        if asset
            .media_type
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str)
            == derived
        {
            continue;
        }
        asset.media_type = derived.and_then(cadmpeg_core::text::NonBlankString::new);
        losses.push(F3dLossCode::WriterValueNormalized.note(format!(
            "asset {}: its media type follows its entry name",
            asset.id
        )));
    }
}

/// A mesh container's first corner is implicit, so a stored mesh references
/// its first and last vertex (spec §1.1.2). A mesh import whose corners miss
/// either one keeps only its referenced vertices, in order.
pub(super) fn mesh_import_vertices(target: &mut CadIr, losses: &mut Vec<LossNote>) {
    use cadmpeg_ir::tessellation::TessellationMesh;
    let imported: std::collections::BTreeSet<_> = target
        .model
        .features
        .iter()
        .filter_map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::MeshImport { tessellations }) => {
                Some(tessellations.iter().cloned())
            }
            _ => None,
        })
        .flatten()
        .collect();
    for tessellation in &mut target.model.tessellations {
        if !imported.contains(&tessellation.id)
            || !tessellation.feature_edges().is_empty()
            || !tessellation.texture_coordinates().is_empty()
        {
            continue;
        }
        let referenced: std::collections::BTreeSet<u32> =
            tessellation.triangles().into_iter().flatten().collect();
        let last = tessellation
            .vertex_count()
            .checked_sub(1)
            .and_then(|last| u32::try_from(last).ok());
        if referenced.first() == Some(&0) && referenced.last() == last.as_ref() {
            continue;
        }
        let Some(remap) = referenced
            .iter()
            .enumerate()
            .map(|(new, old)| u32::try_from(new).ok().map(|new| (*old, new)))
            .collect::<Option<std::collections::BTreeMap<u32, u32>>>()
        else {
            continue;
        };
        let triangles = |triangles: &[[u32; 3]]| {
            triangles
                .iter()
                .map(|triangle| {
                    let [a, b, c] = triangle.map(|corner| remap.get(&corner).copied());
                    Some([a?, b?, c?])
                })
                .collect::<Option<Vec<_>>>()
        };
        let mesh = match tessellation.mesh() {
            TessellationMesh::List {
                vertices,
                triangles: rows,
            } => triangles(rows).map(|triangles| TessellationMesh::List {
                vertices: referenced_rows(vertices, &referenced),
                triangles,
            }),
            TessellationMesh::ShadedList {
                vertices,
                triangles: rows,
            } => triangles(rows).map(|triangles| TessellationMesh::ShadedList {
                vertices: referenced_rows(vertices, &referenced),
                triangles,
            }),
            _ => None,
        };
        let Some(mesh) = mesh else {
            continue;
        };
        let channels = tessellation.channels().to_vec();
        let Ok(compacted) = tessellation.clone().with_mesh(mesh, channels) else {
            continue;
        };
        *tessellation = compacted;
        losses.push(F3dLossCode::WriterValueNormalized.note(format!(
            "tessellation {}: its unreferenced vertices are omitted so that its corners reach its first and last vertex",
            tessellation.id
        )));
    }
}

/// F3D stores no recompute flag, and a reader projects a sketch distance
/// only where its solved points agree with its value (spec §3.1 paired-locus
/// frames). A horizontal point distance in a sketch whose feature needs
/// recompute is written evaluated: its second point is placed at the
/// parameter's distance from the first, on the side it already lies.
pub(super) fn stale_sketch_distances(target: &mut CadIr, losses: &mut Vec<LossNote>) {
    use cadmpeg_ir::features::{Dimension, SketchFeatureBinding};
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput, SketchGeometryDefinition, SketchLocus,
    };
    let stale: std::collections::BTreeSet<_> = target
        .model
        .features
        .iter()
        .filter(|feature| feature.needs_recompute)
        .filter_map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch)),
            }) => Some(sketch.clone()),
            _ => None,
        })
        .collect();
    let tolerance = target.tolerances.linear_or_default().get();
    let point = |target: &CadIr, id: &cadmpeg_ir::sketches::SketchEntityId| {
        target
            .model
            .sketch_entities
            .iter()
            .find(|entity| entity.id() == id)
            .and_then(|entity| match entity.geometry.definition() {
                SketchGeometryDefinition::Point { position } => Some(position.get()),
                _ => None,
            })
    };
    let mut moves = Vec::new();
    for constraint in &target.model.sketch_constraints {
        if !stale.contains(&constraint.sketch) {
            continue;
        }
        let SketchConstraintDefinitionInput::HorizontalDistance {
            first: SketchLocus::Entity(first),
            second: SketchLocus::Entity(second),
            parameter: parameter @ Dimension::Parameter(_),
        } = constraint.definition.kind()
        else {
            continue;
        };
        let (Some(start), Some(end), Some(distance)) = (
            point(target, first),
            point(target, second),
            target
                .dimension_value(parameter)
                .map(|value| value.get().abs()),
        ) else {
            continue;
        };
        if ((end.u - start.u).abs() - distance).abs() <= tolerance {
            continue;
        }
        let u = if end.u >= start.u {
            start.u + distance
        } else {
            start.u - distance
        };
        moves.push((
            second.clone(),
            cadmpeg_ir::math::Point2::new(u, end.v),
            constraint.id.clone(),
        ));
    }
    for (entity_id, position, constraint) in moves {
        let Some(position) = cadmpeg_ir::units::FinitePoint2::new(position) else {
            continue;
        };
        let Ok(geometry) =
            cadmpeg_ir::sketches::SketchGeometry::from_parts(SketchGeometryDefinition::Point {
                position,
            })
        else {
            continue;
        };
        let Some(entity) = target
            .model
            .sketch_entities
            .iter_mut()
            .find(|entity| entity.id() == &entity_id)
        else {
            continue;
        };
        entity.geometry = geometry;
        losses.push(F3dLossCode::WriterValueNormalized.note(format!(
            "sketch entity {entity_id}: placed at the distance of stale sketch dimension {constraint}"
        )));
    }
}

fn referenced_rows<V: Clone>(rows: &[V], referenced: &std::collections::BTreeSet<u32>) -> Vec<V> {
    referenced
        .iter()
        .filter_map(|index| usize::try_from(*index).ok())
        .filter_map(|index| rows.get(index).cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::transform::Transform;

    use super::rigid_part;

    fn transform(rows: [[f64; 4]; 3]) -> Transform {
        Transform::affine(rows).expect("finite rows")
    }

    #[test]
    fn a_scaled_axis_keeps_its_direction_and_the_translation() {
        let scaled = transform([
            [2.0, 0.0, 0.0, 5.0],
            [0.0, 1.0, 0.0, 6.0],
            [0.0, 0.0, 1.0, 7.0],
        ]);
        assert_eq!(
            rigid_part(scaled).affine_rows(),
            [
                [1.0, 0.0, 0.0, 5.0],
                [0.0, 1.0, 0.0, 6.0],
                [0.0, 0.0, 1.0, 7.0]
            ]
        );
    }

    #[test]
    fn a_reflection_reverses_its_third_axis() {
        let mirrored = transform([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 3.0],
        ]);
        assert_eq!(
            rigid_part(mirrored).affine_rows(),
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 3.0]
            ]
        );
    }

    #[test]
    fn sheared_axes_fall_back_to_the_identity_rotation() {
        let sheared = transform([
            [1.0, 1.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 2.0],
            [0.0, 0.0, 1.0, 3.0],
        ]);
        assert_eq!(
            rigid_part(sheared).affine_rows(),
            [
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 2.0],
                [0.0, 0.0, 1.0, 3.0]
            ]
        );
    }
}
