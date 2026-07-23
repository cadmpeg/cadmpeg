// SPDX-License-Identifier: Apache-2.0
//! AP242 indexed tessellation emission.

use cadmpeg_ir::report::{LossCategory, LossCode, Severity};
use cadmpeg_ir::topology::BodyKind;

use crate::writer::{real, refs, string, Ref};

use super::Builder;
use crate::vocab::{
    COORDINATES_LIST, TESSELLATED_SHAPE_REPRESENTATION, TESSELLATED_SHELL, TESSELLATED_SOLID,
    TRIANGULATED_FACE, TRIANGULATED_SURFACE_SET,
};

impl Builder<'_> {
    pub(super) fn emit_tessellations(&mut self, context: Ref) {
        if self.ir.model.tessellations.is_empty() {
            return;
        }
        if !self.schema.supports_tessellation() {
            self.loss(
                LossCode::TessellationOmitted,
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{} tessellation(s) require an AP242 target",
                    self.ir.model.tessellations.len()
                ),
            );
            return;
        }

        let ir = self.ir;
        let mut representation_items = Vec::new();
        for mesh in &ir.model.tessellations {
            if mesh.vertices.is_empty()
                || mesh.triangles.is_empty()
                || mesh
                    .triangles
                    .iter()
                    .flatten()
                    .any(|index| *index as usize >= mesh.vertices.len())
                || (!mesh.normals.is_empty() && mesh.normals.len() != mesh.vertices.len())
            {
                self.loss(
                    LossCode::TessellationOmitted,
                    LossCategory::Geometry,
                    Severity::Warning,
                    format!(
                        "tessellation '{}' has invalid vertex/index/normal cardinality",
                        mesh.id
                    ),
                );
                continue;
            }
            let coordinates = mesh
                .vertices
                .iter()
                .map(|point| format!("({},{},{})", real(point.x), real(point.y), real(point.z)))
                .collect::<Vec<_>>()
                .join(",");
            let coordinates = self.emitter.emit(
                COORDINATES_LIST,
                &format!(
                    "{}, {},({coordinates})",
                    string(&mesh.id),
                    mesh.vertices.len()
                ),
            );
            let normals = if mesh.normals.is_empty() {
                "$".to_string()
            } else {
                format!(
                    "({})",
                    mesh.normals
                        .iter()
                        .map(|normal| format!(
                            "({},{},{})",
                            real(normal.x),
                            real(normal.y),
                            real(normal.z)
                        ))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            let point_indices = (1..=mesh.vertices.len())
                .map(|index| index.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let linked_body = mesh.body.as_ref().and_then(|body| {
                let link = self.links.body_step_refs.get(body.as_str()).copied()?;
                let kind = self.index.bodies.get(body.as_str())?.kind;
                matches!(kind, BodyKind::Solid | BodyKind::Sheet).then_some((kind, link))
            });
            let item = if let Some((kind, link)) = linked_body {
                let triangles = mesh
                    .triangles
                    .iter()
                    .map(|triangle| {
                        format!(
                            "({},{},{})",
                            triangle[0] + 1,
                            triangle[1] + 1,
                            triangle[2] + 1
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let face = self.emitter.emit(
                    TRIANGULATED_FACE,
                    &format!(
                        "{},{coordinates},{},{normals},$,({point_indices}),({triangles})",
                        string(&mesh.id),
                        mesh.vertices.len()
                    ),
                );
                self.emitter.emit(
                    if kind == BodyKind::Solid {
                        TESSELLATED_SOLID
                    } else {
                        TESSELLATED_SHELL
                    },
                    &format!("{},({face}),{link}", string(&mesh.id)),
                )
            } else {
                let triangles = mesh
                    .triangles
                    .iter()
                    .map(|triangle| {
                        format!(
                            "({},{},{})",
                            triangle[0] + 1,
                            triangle[1] + 1,
                            triangle[2] + 1
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                self.emitter.emit(
                    TRIANGULATED_SURFACE_SET,
                    &format!(
                        "{},{coordinates},{},{normals},({point_indices}),({triangles})",
                        string(&mesh.id),
                        mesh.vertices.len()
                    ),
                )
            };
            self.links
                .tessellation_step_refs
                .insert(mesh.id.clone(), item);
            representation_items.push(item);
        }
        if !representation_items.is_empty() {
            self.emitter.emit(
                TESSELLATED_SHAPE_REPRESENTATION,
                &format!("'',{},{context}", refs(&representation_items)),
            );
        }
    }
}
