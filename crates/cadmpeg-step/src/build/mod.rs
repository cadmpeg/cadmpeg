// SPDX-License-Identifier: Apache-2.0
//! STEP writer `Builder` state and the top-level build orchestration.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_ir::geometry::{Curve, Pcurve, ProceduralCurve, ProceduralSurface, Surface};
use cadmpeg_ir::report::{ExportReport, LossCategory, LossCode, LossNote, Severity};
use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Edge, Face, Loop, Point, Shell, Vertex};
use cadmpeg_ir::CadIr;

use crate::geometry;
use crate::writer::{refs, string, Emitter, Ref};
use crate::StepSchema;

mod accounting;
mod context;
mod pmi;
mod presentation;
mod product;
mod shape;
mod tessellation;
mod topology;

pub(crate) struct Builder<'a> {
    ir: &'a CadIr,
    schema: StepSchema,
    pub(crate) emitter: Emitter,
    losses: Vec<LossNote>,
    notes: Vec<String>,

    points: HashMap<&'a str, &'a Point>,
    bodies: HashMap<&'a str, &'a Body>,
    shells: HashMap<&'a str, &'a Shell>,
    faces: HashMap<&'a str, &'a Face>,
    loops: HashMap<&'a str, &'a Loop>,
    vertices: HashMap<&'a str, &'a Vertex>,
    edges: HashMap<&'a str, &'a Edge>,
    coedges: HashMap<&'a str, &'a Coedge>,
    surfaces: HashMap<&'a str, &'a Surface>,
    curves: HashMap<&'a str, &'a Curve>,
    pcurves: HashMap<&'a str, &'a Pcurve>,
    procedural_surfaces: HashMap<&'a str, &'a ProceduralSurface>,
    procedural_curves: HashMap<&'a str, &'a ProceduralCurve>,
    edge_coedges: HashMap<&'a str, Vec<(&'a str, &'a str)>>,

    surface_refs: HashMap<String, Ref>,
    curve_refs: HashMap<String, Ref>,
    edge_refs: HashMap<String, Ref>,
    vertex_refs: HashMap<String, Ref>,
    point_refs: HashMap<String, Ref>,
    pcurve_context: Option<Ref>,
    active_surfaces: BTreeSet<String>,
    pub(crate) active_curves: BTreeSet<String>,
    written_procedural_surfaces: BTreeSet<String>,
    written_procedural_curves: BTreeSet<String>,

    /// Edges skipped because they carry no attributed 3D curve, deduplicated
    /// (a shared edge is reached once per coedge) and aggregated into a single
    /// counted loss note.
    curveless_edges: BTreeSet<String>,

    /// Faces skipped because their surface geometry is unknown (opaque), so no
    /// STEP surface exists to build an `ADVANCED_FACE` on. Deduplicated (a face
    /// is reached once per shell) and aggregated into a single counted loss.
    unknown_surface_faces: BTreeSet<String>,

    face_step_refs: HashMap<String, Ref>,
    /// First emitted exact solid or shell for each body, used by AP242 tessellation links.
    body_step_refs: HashMap<String, Ref>,
    default_product_definition_shape: Option<Ref>,
    body_shape_refs: HashMap<String, Ref>,
    pub(crate) body_item_refs: HashMap<String, Vec<Ref>>,
    tessellation_step_refs: HashMap<String, Ref>,
    written_appearance_bindings: BTreeSet<String>,
    unstyled_colors: usize,
    unsupported_standalone_geometry: usize,
    written_pmi: usize,
    length_unit: Option<Ref>,
    angle_unit: Option<Ref>,
    ratio_unit: Option<Ref>,
    geometry_emission_depth: usize,
}

impl<'a> Builder<'a> {
    pub(crate) fn new(ir: &'a CadIr, schema: StepSchema) -> Self {
        let loop_surfaces = ir
            .model
            .faces
            .iter()
            .flat_map(|face| {
                face.loops
                    .iter()
                    .map(move |loop_id| (loop_id.as_str(), face.surface.as_str()))
            })
            .collect::<HashMap<_, _>>();
        let coedge_surfaces: HashMap<&str, &str> = ir
            .model
            .loops
            .iter()
            .filter_map(|loop_| {
                loop_surfaces
                    .get(loop_.id.as_str())
                    .map(|surface| (loop_, *surface))
            })
            .flat_map(|(loop_, surface)| {
                loop_
                    .coedges
                    .iter()
                    .map(move |coedge| (coedge.as_str(), surface))
            })
            .collect();
        let mut edge_coedges = HashMap::<&str, Vec<(&str, &str)>>::new();
        for coedge in &ir.model.coedges {
            let Some(surface) = coedge_surfaces.get(coedge.id.as_str()) else {
                continue;
            };
            for pcurve in &coedge.pcurves {
                edge_coedges
                    .entry(coedge.edge.as_str())
                    .or_default()
                    .push((pcurve.pcurve.as_str(), *surface));
            }
        }
        Builder {
            ir,
            schema,
            emitter: Emitter::new(),
            losses: Vec::new(),
            notes: Vec::new(),
            points: ir.model.points.iter().map(|p| (p.id.as_str(), p)).collect(),
            bodies: ir
                .model
                .bodies
                .iter()
                .map(|body| (body.id.as_str(), body))
                .collect(),
            shells: ir
                .model
                .shells
                .iter()
                .map(|shell| (shell.id.as_str(), shell))
                .collect(),
            faces: ir
                .model
                .faces
                .iter()
                .map(|face| (face.id.as_str(), face))
                .collect(),
            loops: ir
                .model
                .loops
                .iter()
                .map(|loop_| (loop_.id.as_str(), loop_))
                .collect(),
            vertices: ir
                .model
                .vertices
                .iter()
                .map(|v| (v.id.as_str(), v))
                .collect(),
            edges: ir.model.edges.iter().map(|e| (e.id.as_str(), e)).collect(),
            coedges: ir
                .model
                .coedges
                .iter()
                .map(|c| (c.id.as_str(), c))
                .collect(),
            surfaces: ir
                .model
                .surfaces
                .iter()
                .map(|s| (s.id.as_str(), s))
                .collect(),
            curves: ir.model.curves.iter().map(|c| (c.id.as_str(), c)).collect(),
            pcurves: ir
                .model
                .pcurves
                .iter()
                .map(|p| (p.id.as_str(), p))
                .collect(),
            procedural_surfaces: ir
                .model
                .procedural_surfaces
                .iter()
                .map(|surface| (surface.surface.as_str(), surface))
                .collect(),
            procedural_curves: ir
                .model
                .procedural_curves
                .iter()
                .map(|curve| (curve.curve.as_str(), curve))
                .collect(),
            edge_coedges,
            surface_refs: HashMap::new(),
            curve_refs: HashMap::new(),
            edge_refs: HashMap::new(),
            vertex_refs: HashMap::new(),
            point_refs: HashMap::new(),
            pcurve_context: None,
            active_surfaces: BTreeSet::new(),
            active_curves: BTreeSet::new(),
            written_procedural_surfaces: BTreeSet::new(),
            written_procedural_curves: BTreeSet::new(),
            curveless_edges: BTreeSet::new(),
            unknown_surface_faces: BTreeSet::new(),
            face_step_refs: HashMap::new(),
            body_step_refs: HashMap::new(),
            default_product_definition_shape: None,
            body_shape_refs: HashMap::new(),
            body_item_refs: HashMap::new(),
            tessellation_step_refs: HashMap::new(),
            written_appearance_bindings: BTreeSet::new(),
            unstyled_colors: 0,
            unsupported_standalone_geometry: 0,
            written_pmi: 0,
            length_unit: None,
            angle_unit: None,
            ratio_unit: None,
            geometry_emission_depth: 0,
        }
    }

    fn loss(
        &mut self,
        code: LossCode,
        category: LossCategory,
        severity: Severity,
        message: String,
    ) {
        self.losses.push(LossNote {
            code,
            category,
            severity,
            message,
            provenance: None,
        });
    }

    pub(crate) fn build(&mut self) {
        let context = self.emit_context();

        let shape_items = self.emit_shape_items(context);
        let mut standalone_items = self.emit_standalone_geometry();
        let has_standalone_geometry = !standalone_items.is_empty();
        let mut emitted_items = shape_items;
        emitted_items.extend(standalone_items.iter().copied());
        if emitted_items.is_empty() && !self.ir.model.bodies.is_empty() {
            self.losses.push(LossNote {
                code: LossCode::NoExportableSolids,
                category: LossCategory::Topology,
                severity: Severity::Warning,
                message: "no exportable solids: the IR document contains no body/region/shell \
                          geometry, so the STEP representation is empty"
                    .to_string(),
                provenance: None,
            });
            emitted_items.clear();
        }
        let mut items = emitted_items;
        let origin = geometry::placement(
            &mut self.emitter,
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        );
        items.push(origin);

        if self.ir.model.products.is_empty() {
            let product_def_shape = self.emit_product_structure();
            self.default_product_definition_shape = Some(product_def_shape);
            let representation_kind = if !has_standalone_geometry
                && !self.ir.model.bodies.is_empty()
                && self.ir.model.bodies.iter().all(|body| {
                    body.transform
                        .is_none_or(|transform| is_identity(&transform.rows))
                })
                && self
                    .ir
                    .model
                    .bodies
                    .iter()
                    .all(|body| body.kind == BodyKind::Solid)
            {
                "ADVANCED_BREP_SHAPE_REPRESENTATION"
            } else {
                "SHAPE_REPRESENTATION"
            };
            let representation = self.emitter.emit(
                representation_kind,
                &format!("'',{},{context}", refs(&items)),
            );
            self.emitter.emit(
                "SHAPE_DEFINITION_REPRESENTATION",
                &format!("{product_def_shape},{representation}"),
            );
        } else {
            self.emit_product_graph(context);
            if has_standalone_geometry {
                standalone_items.push(origin);
                self.emitter.emit(
                    "SHAPE_REPRESENTATION",
                    &format!(
                        "{},{},{context}",
                        string("standalone geometry"),
                        refs(&standalone_items)
                    ),
                );
            }
        }

        self.emit_visibility();
        self.emit_tessellations(context);
        self.emit_presentation(context);
        self.emit_layers();
        self.emit_pmi(context);
        self.note_unrepresented();
    }

    pub(crate) fn finish_report(&self) -> ExportReport {
        ExportReport {
            format: "step".into(),
            entity_counts: self.emitter.counts(),
            total_entities: self.emitter.total(),
            losses: self.losses.clone(),
            notes: self.notes.clone(),
        }
    }
}

fn is_identity(rows: &[[f64; 4]; 4]) -> bool {
    for (i, row) in rows.iter().enumerate() {
        for (j, &v) in row.iter().enumerate() {
            let expect = if i == j { 1.0 } else { 0.0 };
            if (v - expect).abs() > 1e-12 {
                return false;
            }
        }
    }
    true
}

pub(crate) fn is_rigid_transform(rows: &[[f64; 4]; 4]) -> bool {
    const EPSILON: f64 = 1.0e-9;
    if rows.iter().flatten().any(|value| !value.is_finite())
        || rows[3]
            .iter()
            .zip([0.0, 0.0, 0.0, 1.0])
            .any(|(actual, expected)| (*actual - expected).abs() > EPSILON)
    {
        return false;
    }
    let columns = (0..3)
        .map(|column| [rows[0][column], rows[1][column], rows[2][column]])
        .collect::<Vec<_>>();
    for left in 0..3 {
        for right in 0..3 {
            let dot = (0..3)
                .map(|row| columns[left][row] * columns[right][row])
                .sum::<f64>();
            let expected = if left == right { 1.0 } else { 0.0 };
            if (dot - expected).abs() > EPSILON {
                return false;
            }
        }
    }
    let determinant = columns[0][0]
        * (columns[1][1] * columns[2][2] - columns[1][2] * columns[2][1])
        - columns[1][0] * (columns[0][1] * columns[2][2] - columns[0][2] * columns[2][1])
        + columns[2][0] * (columns[0][1] * columns[1][2] - columns[0][2] * columns[1][1]);
    (determinant - 1.0).abs() <= EPSILON
}
