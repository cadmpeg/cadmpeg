// SPDX-License-Identifier: Apache-2.0
//! STEP shape items, body placement, visibility, and standalone geometry.

use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::report::{LossCategory, LossCode, Severity};
use cadmpeg_ir::topology::BodyKind;

use crate::geometry;
use crate::writer::{refs, Ref};

use super::{is_identity, is_rigid_transform, Builder};

impl Builder<'_> {
    /// Emit one shape item per region; visibility is represented separately
    /// when the target application protocol supports `INVISIBILITY`.
    pub(super) fn emit_shape_items(&mut self, context: Ref) -> Vec<Ref> {
        let mut items = Vec::new();
        let ir = self.ir;
        for region in &ir.model.regions {
            let body_kind = self
                .index
                .bodies
                .get(region.body.as_str())
                .map_or(BodyKind::General, |body| body.kind);
            if body_kind == BodyKind::Wire {
                if let Some(item) = self.emit_wire_region(region) {
                    let shape_item = self.place_body_item(&region.body, item, context);
                    items.push(shape_item);
                    self.body_shape_refs
                        .entry(region.body.0.clone())
                        .or_insert(shape_item);
                    self.body_item_refs
                        .entry(region.body.0.clone())
                        .or_default()
                        .push(shape_item);
                    self.body_step_refs
                        .entry(region.body.0.clone())
                        .or_insert(item);
                }
                continue;
            }
            let closed = body_kind == BodyKind::Solid;
            let Some((outer_id, void_ids)) = region.shells.split_first() else {
                continue;
            };
            let Some(outer) = self.emit_shell(outer_id.as_str(), closed) else {
                self.loss(
                    LossCode::TopologyNotTransferred,
                    LossCategory::Topology,
                    Severity::Error,
                    format!("region {} has no writable outer shell", region.id),
                );
                continue;
            };
            let voids: Vec<Ref> = void_ids
                .iter()
                .filter_map(|sid| self.emit_shell(sid.as_str(), closed))
                .collect();
            let mut shell_refs = Vec::with_capacity(1 + voids.len());
            shell_refs.push(outer);
            shell_refs.extend_from_slice(&voids);
            let item = if !closed {
                self.emitter.emit(
                    "SHELL_BASED_SURFACE_MODEL",
                    &format!("'',{}", refs(&shell_refs)),
                )
            } else if voids.is_empty() {
                self.emitter
                    .emit("MANIFOLD_SOLID_BREP", &format!("'',{outer}"))
            } else {
                let void_refs: Vec<Ref> = voids
                    .iter()
                    .map(|s| {
                        self.emitter
                            .emit("ORIENTED_CLOSED_SHELL", &format!("'',*,{s},.F."))
                    })
                    .collect();
                self.emitter.emit(
                    "BREP_WITH_VOIDS",
                    &format!("'',{outer},{}", refs(&void_refs)),
                )
            };
            let shape_item = self.place_body_item(&region.body, item, context);
            items.push(shape_item);
            self.body_shape_refs
                .entry(region.body.0.clone())
                .or_insert(shape_item);
            self.body_item_refs
                .entry(region.body.0.clone())
                .or_default()
                .push(shape_item);
            self.body_step_refs
                .entry(region.body.0.clone())
                .or_insert(if closed { item } else { outer });
        }
        items
    }

    fn place_body_item(
        &mut self,
        body_id: &cadmpeg_ir::ids::BodyId,
        item: Ref,
        context: Ref,
    ) -> Ref {
        let transform = self
            .index
            .bodies
            .get(body_id.as_str())
            .and_then(|body| body.transform);
        let Some(transform) = transform.filter(|transform| !is_identity(&transform.rows)) else {
            return item;
        };
        if !is_rigid_transform(&transform.rows) {
            self.loss(
                LossCode::BodyTransformNotApplied,
                LossCategory::Geometry,
                Severity::Warning,
                format!("body '{body_id}' carries a non-rigid transform"),
            );
            return item;
        }
        let origin = geometry::placement(
            &mut self.emitter,
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        );
        let representation = self.emitter.emit(
            "SHAPE_REPRESENTATION",
            &format!("'body-local',({item}),{context}"),
        );
        let map = self
            .emitter
            .emit("REPRESENTATION_MAP", &format!("{origin},{representation}"));
        let rows = transform.rows;
        let target = geometry::placement(
            &mut self.emitter,
            cadmpeg_ir::math::Point3::new(rows[0][3], rows[1][3], rows[2][3]),
            cadmpeg_ir::math::Vector3::new(rows[0][2], rows[1][2], rows[2][2]),
            cadmpeg_ir::math::Vector3::new(rows[0][0], rows[1][0], rows[2][0]),
        );
        self.emitter.emit(
            "MAPPED_ITEM",
            &format!("'cadmpeg body placement',{map},{target}"),
        )
    }

    pub(super) fn emit_visibility(&mut self) {
        if !self.schema.supports_visibility() {
            let hidden = self
                .ir
                .model
                .bodies
                .iter()
                .filter(|body| body.visible == Some(false))
                .count();
            if hidden != 0 {
                self.loss(
                    LossCode::AttributesNotTransferred,
                    LossCategory::Metadata,
                    Severity::Warning,
                    format!(
                        "{hidden} hidden body visibility assignment(s) are unsupported by {}",
                        self.schema.file_schema()
                    ),
                );
            }
            return;
        }
        let hidden = self
            .ir
            .model
            .bodies
            .iter()
            .filter(|body| body.visible == Some(false))
            .filter_map(|body| self.body_step_refs.get(body.id.as_str()).copied())
            .collect::<Vec<_>>();
        if !hidden.is_empty() {
            self.emitter.emit("INVISIBILITY", &refs(&hidden));
        }
    }

    fn emit_wire_region(&mut self, region: &cadmpeg_ir::topology::Region) -> Option<Ref> {
        let shells = region
            .shells
            .iter()
            .filter_map(|shell_id| self.index.shells.get(shell_id.as_str()).copied().cloned())
            .collect::<Vec<_>>();
        let mut connected_sets = Vec::new();
        for shell in shells {
            if !shell.free_vertices.is_empty() {
                self.loss(
                    LossCode::TopologyNotTransferred,
                    LossCategory::Topology,
                    Severity::Warning,
                    format!(
                        "wire shell '{}' has {} free vertex/vertices without an edge-based STEP carrier",
                        shell.id,
                        shell.free_vertices.len()
                    ),
                );
            }
            let edges = shell
                .wire_edges
                .iter()
                .filter_map(|edge| self.emit_edge(edge.as_str()))
                .collect::<Vec<_>>();
            if !edges.is_empty() {
                connected_sets.push(
                    self.emitter
                        .emit("CONNECTED_EDGE_SET", &format!("'',{}", refs(&edges))),
                );
            }
        }
        if connected_sets.is_empty() {
            return None;
        }
        Some(self.emitter.emit(
            "EDGE_BASED_WIREFRAME_MODEL",
            &format!("'',{}", refs(&connected_sets)),
        ))
    }

    pub(super) fn emit_standalone_geometry(&mut self) -> Vec<Ref> {
        let surface_ids = self
            .ir
            .model
            .surfaces
            .iter()
            .filter(|surface| !self.surface_refs.contains_key(surface.id.as_str()))
            .map(|surface| surface.id.0.clone())
            .collect::<Vec<_>>();
        let mut members = Vec::new();
        let mut has_surfaces = false;
        for surface_id in surface_ids {
            if let Some(reference) = self.emit_surface(&surface_id) {
                members.push(reference);
                has_surfaces = true;
            } else {
                self.unsupported_standalone_geometry += 1;
            }
        }
        let curve_ids = self
            .ir
            .model
            .curves
            .iter()
            .filter(|curve| !self.curve_refs.contains_key(curve.id.as_str()))
            .map(|curve| curve.id.0.clone())
            .collect::<Vec<_>>();
        for curve_id in curve_ids {
            if self
                .index
                .curves
                .get(curve_id.as_str())
                .is_some_and(|curve| matches!(curve.geometry, CurveGeometry::Unknown { .. }))
            {
                self.unsupported_standalone_geometry += 1;
            } else if let Some(reference) = self.emit_curve(&curve_id) {
                members.push(reference);
            }
        }
        let point_ids = self
            .ir
            .model
            .points
            .iter()
            .filter(|point| !self.point_refs.contains_key(point.id.as_str()))
            .map(|point| point.id.0.clone())
            .collect::<Vec<_>>();
        for point_id in point_ids {
            let Some(point) = self.index.points.get(point_id.as_str()).copied() else {
                continue;
            };
            let reference = geometry::point(&mut self.emitter, point.position);
            self.point_refs.insert(point_id, reference);
            members.push(reference);
        }
        if members.is_empty() {
            Vec::new()
        } else {
            vec![self.emitter.emit(
                if has_surfaces {
                    "GEOMETRIC_SET"
                } else {
                    "GEOMETRIC_CURVE_SET"
                },
                &format!("'',{}", refs(&members)),
            )]
        }
    }
}
