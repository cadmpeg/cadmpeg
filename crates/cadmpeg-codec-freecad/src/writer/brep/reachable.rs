// SPDX-License-Identifier: Apache-2.0
//! Reachable topology tables and independent geometry roots.
use cadmpeg_ir::{topology::Body, CadIr};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct Reach {
    pub(super) faces: HashSet<String>,
    pub(super) loops: HashSet<String>,
    pub(super) coedges: HashSet<String>,
    pub(super) edges: HashSet<String>,
    pub(super) vertices: HashSet<String>,
    pub(super) curves: HashSet<String>,
    pub(super) surfaces: HashSet<String>,
    pub(super) pcurves: HashSet<String>,
}
impl Reach {
    pub(super) fn bodies(
        ir: &CadIr,
        bodies: &[&Body],
        row_owners: &std::collections::BTreeMap<String, String>,
    ) -> Self {
        let payloads: HashSet<_> = bodies
            .iter()
            .filter_map(|body| body.native_ref.as_deref())
            .collect();
        let regions: HashSet<_> = bodies.iter().flat_map(|body| &body.regions).collect();
        let shells: HashSet<_> = ir
            .model
            .regions
            .iter()
            .filter(|region| regions.contains(&region.id))
            .flat_map(|region| &region.shells)
            .collect();
        let mut output = Self::default();
        for shell in ir
            .model
            .shells
            .iter()
            .filter(|shell| shells.contains(&shell.id))
        {
            output
                .faces
                .extend(shell.faces().iter().map(ToString::to_string));
            output
                .edges
                .extend(shell.wire_edges().iter().map(ToString::to_string));
            output
                .vertices
                .extend(shell.free_vertices().iter().map(ToString::to_string));
        }
        output.complete(ir);
        // A source object's standalone carrier records are held independently
        // of its rooted topology. Keep those rows in that object's table too.
        let owners: HashSet<_> = ir
            .model
            .curves
            .iter()
            .filter(|curve| output.curves.contains(curve.id.as_str()))
            .filter_map(|curve| {
                curve
                    .source_object
                    .as_ref()
                    .map(|source| source.object_id.as_str().to_owned())
            })
            .chain(
                ir.model
                    .surfaces
                    .iter()
                    .filter(|surface| output.surfaces.contains(surface.id.as_str()))
                    .filter_map(|surface| {
                        surface
                            .source_object
                            .as_ref()
                            .map(|source| source.object_id.as_str().to_owned())
                    }),
            )
            .collect();
        output.curves.extend(
            ir.model
                .curves
                .iter()
                .filter(|curve| {
                    curve.native_ref.as_deref().map_or_else(
                        || {
                            curve
                                .source_object
                                .as_ref()
                                .is_some_and(|source| owners.contains(source.object_id.as_str()))
                        },
                        |native| {
                            payloads.contains(row_owners.get(native).map_or(native, String::as_str))
                        },
                    )
                })
                .map(|curve| curve.id.to_string()),
        );
        output.surfaces.extend(
            ir.model
                .surfaces
                .iter()
                .filter(|surface| {
                    surface.native_ref.as_deref().map_or_else(
                        || {
                            surface
                                .source_object
                                .as_ref()
                                .is_some_and(|source| owners.contains(source.object_id.as_str()))
                        },
                        |native| {
                            payloads.contains(row_owners.get(native).map_or(native, String::as_str))
                        },
                    )
                })
                .map(|surface| surface.id.to_string()),
        );
        output.remove_inline(ir);
        output
    }
    fn remove_inline(&mut self, ir: &CadIr) {
        let mut inline_curves = HashSet::new();
        let mut inline_surfaces = HashSet::new();
        for surface in ir
            .model
            .surfaces
            .iter()
            .filter(|surface| self.surfaces.contains(surface.id.as_str()))
        {
            if surface.geometry.procedural_construction().is_some()
                && super::super::geometry::surface_record(ir, surface).is_some_and(|row| {
                    ["6 ", "7 ", "10 ", "11 "]
                        .iter()
                        .any(|prefix| row.starts_with(prefix))
                })
            {
                if let Some(record) = surface.geometry.procedural_construction().and_then(|id| {
                    ir.model
                        .procedural_surfaces
                        .iter()
                        .find(|record| record.id == *id)
                }) {
                    references(
                        &serde_json::to_value(record.definition()).unwrap_or_default(),
                        ir,
                        &mut inline_curves,
                        &mut inline_surfaces,
                    );
                }
            }
        }
        for curve in ir
            .model
            .curves
            .iter()
            .filter(|curve| self.curves.contains(curve.id.as_str()))
        {
            if curve.geometry.procedural_construction().is_some()
                && super::super::geometry::curve_record(ir, curve)
                    .is_some_and(|row| row.starts_with("8 ") || row.starts_with("9 "))
            {
                if let Some(record) = curve.geometry.procedural_construction().and_then(|id| {
                    ir.model
                        .procedural_curves
                        .iter()
                        .find(|record| record.id == *id)
                }) {
                    references(
                        &serde_json::to_value(record.definition()).unwrap_or_default(),
                        ir,
                        &mut inline_curves,
                        &mut inline_surfaces,
                    );
                }
            }
        }
        self.curves.retain(|id| {
            !inline_curves.contains(id)
                || ir.model.edges.iter().any(|edge| {
                    self.edges.contains(edge.id.as_str())
                        && edge.curve().is_some_and(|curve| curve.as_str() == id)
                })
        });
        self.surfaces.retain(|id| {
            !inline_surfaces.contains(id)
                || ir.model.faces.iter().any(|face| {
                    self.faces.contains(face.id.as_str()) && face.surface.as_str() == id
                })
        });
    }
    /// Select the actual current graph of one canonical native payload.
    pub(super) fn payload(
        ir: &CadIr,
        bodies: &[&Body],
        payload: &str,
        row_owners: &std::collections::BTreeMap<String, String>,
    ) -> Self {
        let mut output = Self::bodies(ir, bodies, row_owners);
        let owns = |native: Option<&str>| {
            native.is_some_and(|id| row_owners.get(id).map_or(id, String::as_str) == payload)
        };
        macro_rules! select {
            ($arena:ident) => {
                output.$arena.extend(
                    ir.model
                        .$arena
                        .iter()
                        .filter(|row| owns(row.native_ref.as_deref()))
                        .map(|row| row.id.to_string()),
                );
            };
        }
        select!(faces);
        select!(loops);
        select!(coedges);
        select!(edges);
        select!(vertices);
        select!(curves);
        select!(surfaces);
        select!(pcurves);
        output.complete(ir);
        output.remove_inline(ir);
        output
    }
    pub(super) fn unowned(
        ir: &CadIr,
        row_owners: &std::collections::BTreeMap<String, String>,
    ) -> Self {
        let owned = Self::bodies(ir, &ir.model.bodies.iter().collect::<Vec<_>>(), row_owners);
        let mut output = Self::default();
        output.faces.extend(
            ir.model
                .faces
                .iter()
                .filter(|face| !owned.faces.contains(face.id.as_str()))
                .map(|face| face.id.to_string()),
        );
        output.loops.extend(
            ir.model
                .loops
                .iter()
                .filter(|wire| !owned.loops.contains(wire.id.as_str()))
                .map(|wire| wire.id.to_string()),
        );
        output.edges.extend(
            ir.model
                .edges
                .iter()
                .filter(|edge| !owned.edges.contains(edge.id.as_str()))
                .map(|edge| edge.id.to_string()),
        );
        output.vertices.extend(
            ir.model
                .vertices
                .iter()
                .filter(|vertex| !owned.vertices.contains(vertex.id.as_str()))
                .map(|vertex| vertex.id.to_string()),
        );
        output.complete(ir);
        let mut inline_curves = HashSet::new();
        let mut inline_surfaces = HashSet::new();
        // An inline procedural basis is already emitted as part of that row.
        // Its independent neutral carrier is not a second top-level table row.
        for curve in &ir.model.curves {
            if matches!(
                curve.geometry,
                cadmpeg_ir::geometry::CurveGeometry::Procedural { .. }
            ) && super::super::geometry::curve_record(ir, curve)
                .is_some_and(|row| row.starts_with("8 ") || row.starts_with("9 "))
            {
                if let Some(record) = curve.geometry.procedural_construction().and_then(|id| {
                    ir.model
                        .procedural_curves
                        .iter()
                        .find(|record| record.id == *id)
                }) {
                    references(
                        &serde_json::to_value(record.definition()).unwrap_or_default(),
                        ir,
                        &mut inline_curves,
                        &mut inline_surfaces,
                    );
                }
            }
        }
        for surface in &ir.model.surfaces {
            if matches!(
                surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Procedural { .. }
            ) && super::super::geometry::surface_record(ir, surface).is_some_and(|row| {
                ["6 ", "7 ", "10 ", "11 "]
                    .iter()
                    .any(|prefix| row.starts_with(prefix))
            }) {
                if let Some(record) = surface.geometry.procedural_construction().and_then(|id| {
                    ir.model
                        .procedural_surfaces
                        .iter()
                        .find(|record| record.id == *id)
                }) {
                    references(
                        &serde_json::to_value(record.definition()).unwrap_or_default(),
                        ir,
                        &mut inline_curves,
                        &mut inline_surfaces,
                    );
                }
            }
        }
        output.curves.extend(
            ir.model
                .curves
                .iter()
                .filter(|curve| {
                    !owned.curves.contains(curve.id.as_str())
                        && !inline_curves.contains(curve.id.as_str())
                })
                .map(|curve| curve.id.to_string()),
        );
        output.surfaces.extend(
            ir.model
                .surfaces
                .iter()
                .filter(|surface| {
                    !owned.surfaces.contains(surface.id.as_str())
                        && !inline_surfaces.contains(surface.id.as_str())
                })
                .map(|surface| surface.id.to_string()),
        );
        output
    }
    fn complete(&mut self, ir: &CadIr) {
        self.loops.extend(
            ir.model
                .faces
                .iter()
                .filter(|face| self.faces.contains(face.id.as_str()))
                .flat_map(|face| face.loops.iter().map(ToString::to_string)),
        );
        self.coedges.extend(
            ir.model
                .loops
                .iter()
                .filter(|wire| self.loops.contains(wire.id.as_str()))
                .flat_map(|wire| wire.coedges().iter().map(ToString::to_string)),
        );
        self.edges.extend(
            ir.model
                .coedges
                .iter()
                .filter(|coedge| self.coedges.contains(coedge.id.as_str()))
                .map(|coedge| coedge.edge.to_string()),
        );
        self.vertices.extend(
            ir.model
                .edges
                .iter()
                .filter(|edge| self.edges.contains(edge.id.as_str()))
                .flat_map(|edge| [edge.start.to_string(), edge.end.to_string()]),
        );
        self.curves.extend(
            ir.model
                .edges
                .iter()
                .filter(|edge| self.edges.contains(edge.id.as_str()))
                .filter_map(|edge| edge.curve().map(ToString::to_string)),
        );
        self.surfaces.extend(
            ir.model
                .faces
                .iter()
                .filter(|face| self.faces.contains(face.id.as_str()))
                .map(|face| face.surface.to_string()),
        );
        self.pcurves.extend(
            ir.model
                .coedges
                .iter()
                .filter(|coedge| self.coedges.contains(coedge.id.as_str()))
                .flat_map(|coedge| coedge.pcurves.iter().map(|usage| usage.pcurve.to_string())),
        );
    }
}
fn references(
    value: &serde_json::Value,
    ir: &CadIr,
    curves: &mut HashSet<String>,
    surfaces: &mut HashSet<String>,
) {
    match value {
        serde_json::Value::String(id) => {
            if ir.model.curves.iter().any(|curve| curve.id.as_str() == id) {
                curves.insert(id.clone());
            }
            if ir
                .model
                .surfaces
                .iter()
                .any(|surface| surface.id.as_str() == id)
            {
                surfaces.insert(id.clone());
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                references(value, ir, curves, surfaces);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                references(value, ir, curves, surfaces);
            }
        }
        _ => {}
    }
}
