// SPDX-License-Identifier: Apache-2.0
//! Model-space frames resolved from feature-section datum references.

use crate::datum::DatumPlane;
use crate::feature::{BinaryFlag, FeatureDefinition};
use crate::surface::{OutlinePlane, PlaneLocalSystem};

/// A feature's right-handed section-to-model rigid frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSectionTransform {
    /// Owning feature-definition identifier.
    pub feature_id: u32,
    /// Model-space point corresponding to section coordinate `[0, 0, 0]`.
    pub origin: [f64; 3],
    /// Model-space direction of increasing section `u`.
    pub u_axis: [f64; 3],
    /// Model-space direction of increasing section `v`.
    pub v_axis: [f64; 3],
    /// Model-space normal of the section plane.
    pub normal: [f64; 3],
    /// Byte offset of the source `gsec3d_ptr` record.
    pub offset: usize,
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0].mul_add(right[0], left[1].mul_add(right[1], left[2] * right[2]))
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1].mul_add(right[2], -(left[2] * right[1])),
        left[2].mul_add(right[0], -(left[0] * right[2])),
        left[0].mul_add(right[1], -(left[1] * right[0])),
    ]
}

fn add(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn scale(vector: [f64; 3], factor: f64) -> [f64; 3] {
    vector.map(|value| value * factor)
}

/// Resolve feature frames whose sketch and orientation references are two
/// perpendicular, model-space `ActDatums` planes. Flipped and non-orthogonal
/// variants remain unresolved until their flag semantics are defined.
pub fn resolve(
    definitions: &[FeatureDefinition],
    datums: &[DatumPlane],
    model_planes: &[PlaneLocalSystem],
    outline_planes: &[OutlinePlane],
) -> Vec<FeatureSectionTransform> {
    let mut result = Vec::new();
    for definition in definitions {
        let Some(feature_id) = definition.owner_feature_id else {
            continue;
        };
        let Some(section) = &definition.section_3d else {
            continue;
        };
        if section.sketch_plane_flip == Some(BinaryFlag::Set)
            || section.orientation.section_flip == Some(BinaryFlag::Set)
            || section.orientation.reference_flip == Some(BinaryFlag::Set)
        {
            continue;
        }
        let Some(sketch_id) = section.sketch_plane_entity_id else {
            continue;
        };
        let Some(reference_id) = section.reference_plane_datum_geometry_id else {
            continue;
        };
        let sketch = datums
            .iter()
            .find(|datum| datum.id == sketch_id)
            .map(|datum| (datum.normal, datum.offset))
            .or_else(|| {
                let plane = model_planes
                    .iter()
                    .find(|plane| plane.surface_id == sketch_id)?;
                let normal = plane.normal?;
                let origin = plane.origin?;
                Some((normal, dot(normal, origin)))
            })
            .or_else(|| {
                let plane = outline_planes
                    .iter()
                    .find(|plane| plane.surface_id == sketch_id)?;
                Some((plane.normal, dot(plane.normal, plane.origin)))
            });
        let Some((sketch_normal, sketch_offset)) = sketch else {
            continue;
        };
        let Some(reference) = datums.iter().find(|datum| datum.id == reference_id) else {
            continue;
        };
        if dot(sketch_normal, reference.normal).abs() > 1e-12 {
            continue;
        }
        let normal = sketch_normal;
        let u_axis = reference.normal;
        let v_axis = cross(normal, u_axis);
        if (dot(v_axis, v_axis) - 1.0).abs() > 1e-12 {
            continue;
        }
        result.push(FeatureSectionTransform {
            feature_id,
            origin: add(
                scale(sketch_normal, sketch_offset),
                scale(reference.normal, reference.offset),
            ),
            u_axis,
            v_axis,
            normal,
            offset: section.offset,
        });
    }
    result.sort_by_key(|transform| transform.offset);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::{FeatureSection3d, FeatureSectionOrientation};

    fn datum(id: u32, normal: [f64; 3], offset: f64) -> DatumPlane {
        DatumPlane {
            id,
            feature_id: id.saturating_sub(1),
            normal,
            offset,
            corners: [[0.0; 3]; 2],
            offset_in_payload: usize::try_from(id).expect("fixture id fits usize"),
        }
    }

    #[test]
    fn resolves_perpendicular_datum_frame() {
        let definition = FeatureDefinition {
            id: 42,
            owner_feature_id: Some(42),
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: Some(FeatureSection3d {
                sketch_plane_entity_id: Some(2),
                sketch_plane_flip: Some(BinaryFlag::Clear),
                reference_plane_entity_ids: vec![4],
                reference_plane_datum_geometry_id: Some(4),
                orientation: FeatureSectionOrientation::default(),
                dimension_ids: Vec::new(),
                offset: 100,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 90,
        };
        assert_eq!(
            resolve(
                &[definition],
                &[
                    datum(2, [1.0, 0.0, 0.0], 2.0),
                    datum(4, [0.0, 0.0, 1.0], 3.0),
                ],
                &[],
                &[],
            ),
            vec![FeatureSectionTransform {
                feature_id: 42,
                origin: [2.0, 0.0, 3.0],
                u_axis: [0.0, 0.0, 1.0],
                v_axis: [0.0, -1.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                offset: 100,
            }]
        );
    }
}
