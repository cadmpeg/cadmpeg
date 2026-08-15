// SPDX-License-Identifier: Apache-2.0
//! IR → STEP Part 21 builder.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;

use cadmpeg_ir::appearance::{Appearance, AppearanceTarget};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, Pcurve, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::pmi::{
    DimensionKind, GeometricToleranceKind, PmiDefinition, PmiQuantity, PmiTarget,
};
use cadmpeg_ir::presentation::PresentationItem;
use cadmpeg_ir::products::{AssemblyGraph, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::report::{ExportReport, LossNote};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Sense, Shell, Vertex,
};
use cadmpeg_ir::CadIr;
use cadmpeg_ir::{FidelityResolution, WritePath};

use crate::error::StepError;
use crate::geometry;
use crate::loss::StepLossCode;
use crate::options::{StepSchema, StepUnsupportedPolicy, StepWriteOptions};
use crate::writer::{real, refs, string, Emitter, Ref};

const EPS_IDENTITY: f64 = 1e-12;

/// Serializes an IR document as an ISO 10303-21 STEP Part 21 file for the
/// schema selected by [`StepWriteOptions::schema`].
///
/// The output declares that schema and a millimetre length unit. Coordinate
/// values are not rescaled. The IR linear tolerance becomes the representation
/// context's uncertainty value.
///
/// Geometry conversion completes before this function writes the header. Under
/// [`StepUnsupportedPolicy::Reject`], unsupported content returns
/// [`StepError::Unsupported`] before any output byte is written. Otherwise the
/// function streams the header, DATA instances, and closing records to `w`. An
/// I/O error can therefore leave a partial file and returns no report.
///
/// On success, the report contains DATA entity counts and loss notes for
/// reductions that the selected schema cannot carry.
pub fn write_step(
    ir: &CadIr,
    w: &mut (impl Write + ?Sized),
    opts: &StepWriteOptions,
) -> Result<ExportReport, StepError> {
    let mut b = Builder::new(ir, opts.schema);
    b.build();
    let report = b.finish_report();
    let lines = b.emitter.into_lines();

    if opts.unsupported == StepUnsupportedPolicy::Reject && !report.losses.is_empty() {
        return Err(StepError::Unsupported(
            report
                .losses
                .iter()
                .map(|loss| loss.message.as_str())
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }

    write_header(w, opts)?;
    writeln!(w, "DATA;")?;
    for line in &lines {
        writeln!(w, "{line}")?;
    }
    writeln!(w, "ENDSEC;")?;
    writeln!(w, "END-ISO-10303-21;")?;
    Ok(report)
}

fn write_header(w: &mut (impl Write + ?Sized), opts: &StepWriteOptions) -> std::io::Result<()> {
    let ts = if opts.timestamp.is_empty() {
        "1970-01-01T00:00:00"
    } else {
        &opts.timestamp
    };
    writeln!(w, "ISO-10303-21;")?;
    writeln!(w, "HEADER;")?;
    writeln!(
        w,
        "FILE_DESCRIPTION(({}),'2;1');",
        string("CAD model exported by cadmpeg")
    )?;
    writeln!(
        w,
        "FILE_NAME({},{},({}),({}),{},{},{});",
        string(&opts.product_name),
        string(ts),
        string(&opts.author),
        string(&opts.organization),
        string("cadmpeg-step"),
        string(&opts.originating_system),
        string("")
    )?;
    writeln!(w, "FILE_SCHEMA(({}));", string(opts.schema.file_schema()))?;
    writeln!(w, "ENDSEC;")?;
    Ok(())
}

#[derive(Clone, Copy)]
struct ColorSpec<'a> {
    color: cadmpeg_ir::topology::Color,
    appearance: Option<&'a Appearance>,
    binding_id: Option<&'a str>,
}

struct LoopSegment {
    coedge_id: String,
    start_vertex: String,
    end_vertex: String,
}

pub(crate) struct Builder<'a> {
    ir: &'a CadIr,
    schema: StepSchema,
    emitter: Emitter,
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

    /// Individual enclosing topology relations that could not be emitted.
    topology_relation_losses: BTreeSet<String>,

    face_step_refs: HashMap<String, Ref>,
    /// First emitted exact solid or shell for each body, used by AP242 tessellation links.
    body_step_refs: HashMap<String, Ref>,
    default_product_definition_shape: Option<Ref>,
    body_shape_refs: HashMap<String, Ref>,
    pub(crate) body_item_refs: HashMap<String, Vec<Ref>>,
    body_step_item_refs: HashMap<String, Vec<Ref>>,
    product_step_refs: HashMap<String, Ref>,
    occurrence_step_refs: HashMap<String, Ref>,
    tessellation_step_refs: HashMap<String, Ref>,
    pmi_step_refs: HashMap<String, Ref>,
    written_appearance_bindings: BTreeSet<String>,
    unstyled_colors: usize,
    unwritten_geometry_carriers: BTreeSet<String>,
    unwritten_pcurve_carriers: BTreeSet<String>,
    missing_parent_products: BTreeSet<String>,
    empty_regions: BTreeSet<String>,
    empty_wire_regions: BTreeSet<String>,
    missing_wire_shells: BTreeSet<(String, String)>,
    hidden_bodies_without_items: BTreeSet<String>,
    dangling_appearance_bindings: BTreeSet<(String, String)>,
    colorless_appearance_bindings: BTreeSet<(String, String)>,
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
            topology_relation_losses: BTreeSet::new(),
            face_step_refs: HashMap::new(),
            body_step_refs: HashMap::new(),
            default_product_definition_shape: None,
            body_shape_refs: HashMap::new(),
            body_item_refs: HashMap::new(),
            body_step_item_refs: HashMap::new(),
            product_step_refs: HashMap::new(),
            occurrence_step_refs: HashMap::new(),
            tessellation_step_refs: HashMap::new(),
            pmi_step_refs: HashMap::new(),
            written_appearance_bindings: BTreeSet::new(),
            unstyled_colors: 0,
            unwritten_geometry_carriers: BTreeSet::new(),
            unwritten_pcurve_carriers: BTreeSet::new(),
            missing_parent_products: BTreeSet::new(),
            empty_regions: BTreeSet::new(),
            empty_wire_regions: BTreeSet::new(),
            missing_wire_shells: BTreeSet::new(),
            hidden_bodies_without_items: BTreeSet::new(),
            dangling_appearance_bindings: BTreeSet::new(),
            colorless_appearance_bindings: BTreeSet::new(),
            written_pmi: 0,
            length_unit: None,
            angle_unit: None,
            ratio_unit: None,
            geometry_emission_depth: 0,
        }
    }

    fn loss(&mut self, code: StepLossCode, message: String) {
        self.losses.push(code.note(message));
    }

    fn topology_relation_loss(&mut self, identity: String, code: StepLossCode, message: String) {
        if self.topology_relation_losses.insert(identity) {
            self.loss(code, message);
        }
    }

    pub(crate) fn build(&mut self) {
        let context = self.emit_context();

        let shape_items = self.emit_shape_items(context);
        let mut standalone_items = self.emit_standalone_geometry();
        let has_standalone_geometry = !standalone_items.is_empty();
        let mut emitted_items = shape_items;
        emitted_items.extend(standalone_items.iter().copied());
        if emitted_items.is_empty() && !self.ir.model.bodies.is_empty() {
            self.loss(
                StepLossCode::NoExportableSolids,
                "no exportable solids: the IR document contains no body/region/shell \
                          geometry, so the STEP representation is empty"
                    .to_string(),
            );
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

        if self.ir.model.product_definitions.is_empty() {
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
        self.emit_pmi(context);
        self.emit_layers();
        self.note_unrepresented();
    }

    fn emit_presentation(&mut self, context: Ref) {
        let ir = self.ir;
        let appearances: HashMap<&str, &Appearance> = ir
            .model
            .appearances
            .iter()
            .map(|appearance| (appearance.id.as_str(), appearance))
            .collect();
        let mut body_colors: HashMap<&str, ColorSpec<'_>> = HashMap::new();
        let mut face_colors: HashMap<&str, ColorSpec<'_>> = HashMap::new();
        let mut dangling_appearance_bindings = BTreeSet::new();
        let mut colorless_appearance_bindings = BTreeSet::new();
        for binding in &ir.model.appearance_bindings {
            let Some(appearance) = appearances.get(binding.appearance.as_str()).copied() else {
                dangling_appearance_bindings
                    .insert((binding.id.clone(), binding.appearance.0.clone()));
                continue;
            };
            let Some(color) = appearance.base_color else {
                colorless_appearance_bindings
                    .insert((binding.id.clone(), binding.appearance.0.clone()));
                continue;
            };
            let spec = ColorSpec {
                color,
                appearance: Some(appearance),
                binding_id: Some(&binding.id),
            };
            match &binding.target {
                AppearanceTarget::Body(id) => {
                    body_colors.entry(id.as_str()).or_insert(spec);
                }
                AppearanceTarget::Face(id) => {
                    face_colors.entry(id.as_str()).or_insert(spec);
                }
                AppearanceTarget::Surface(_)
                | AppearanceTarget::Curve(_)
                | AppearanceTarget::Point(_)
                | AppearanceTarget::Edge(_)
                | AppearanceTarget::Vertex(_)
                | AppearanceTarget::Tessellation(_)
                | AppearanceTarget::Source { .. } => {}
            }
        }
        self.dangling_appearance_bindings
            .extend(dangling_appearance_bindings);
        self.colorless_appearance_bindings
            .extend(colorless_appearance_bindings);
        for body in &ir.model.bodies {
            if let Some(color) = body.color {
                body_colors.entry(body.id.as_str()).or_insert(ColorSpec {
                    color,
                    appearance: None,
                    binding_id: None,
                });
            }
        }
        for face in &ir.model.faces {
            if let Some(color) = face.color {
                face_colors.entry(face.id.as_str()).or_insert(ColorSpec {
                    color,
                    appearance: None,
                    binding_id: None,
                });
            }
        }

        let mut face_body: HashMap<&str, &str> = HashMap::new();
        for region in &ir.model.regions {
            let body = region.body.0.as_str();
            for shell_id in &region.shells {
                let Some(shell) = self.shells.get(shell_id.as_str()).copied() else {
                    continue;
                };
                for face in &shell.faces {
                    face_body.insert(face.0.as_str(), body);
                }
            }
        }

        // Face-level STYLED_ITEM only: OCCT/VTK viewers read colors from
        // ADVANCED_FACE and ignore MANIFOLD_SOLID_BREP. Bodies with no face use
        // the shape carrier for whole-body color.
        let mut style_refs: HashMap<String, Ref> = HashMap::new();
        let mut styled = Vec::new();
        let mut faces: Vec<(String, Ref)> = self
            .face_step_refs
            .iter()
            .map(|(id, r)| (id.clone(), *r))
            .collect();
        faces.sort_by(|a, b| a.0.cmp(&b.0));
        let mut styled_bodies: BTreeSet<&str> = BTreeSet::new();
        for (face_id, face) in &faces {
            let own = face_colors.get(face_id.as_str()).copied();
            let body = face_body.get(face_id.as_str()).copied();
            let inherited = body.and_then(|b| body_colors.get(b).copied());
            let Some(spec) = own.or(inherited) else {
                continue;
            };
            // The body color is only counted as represented when a face without
            // its own override receives it.
            if own.is_none() {
                if let Some(b) = body {
                    styled_bodies.insert(b);
                }
            }
            if let Some(binding_id) = spec.binding_id {
                self.written_appearance_bindings
                    .insert(binding_id.to_string());
            }
            let name = spec
                .appearance
                .and_then(|appearance| appearance.name.as_deref())
                .unwrap_or("");
            let style = self.surface_style(spec.color, name, &mut style_refs);
            styled.push(
                self.emitter
                    .emit("STYLED_ITEM", &format!("'color',({style}),{face}")),
            );
        }
        let mut direct_unstyled = BTreeSet::new();
        for binding in &ir.model.appearance_bindings {
            if self.written_appearance_bindings.contains(&binding.id) {
                continue;
            }
            let Some(appearance) = appearances.get(binding.appearance.as_str()).copied() else {
                continue;
            };
            let Some(color) = appearance.base_color else {
                continue;
            };
            let (target, style_kind) = match &binding.target {
                AppearanceTarget::Face(id) => {
                    (self.face_step_refs.get(id.as_str()).copied(), "surface")
                }
                AppearanceTarget::Surface(id) => {
                    (self.surface_refs.get(id.as_str()).copied(), "surface")
                }
                AppearanceTarget::Curve(id) => (self.curve_refs.get(id.as_str()).copied(), "curve"),
                AppearanceTarget::Edge(id) => (self.edge_refs.get(id.as_str()).copied(), "curve"),
                AppearanceTarget::Point(id) => (self.point_refs.get(id.as_str()).copied(), "point"),
                AppearanceTarget::Vertex(id) => {
                    (self.vertex_refs.get(id.as_str()).copied(), "point")
                }
                AppearanceTarget::Tessellation(id) => {
                    (self.tessellation_step_refs.get(id).copied(), "surface")
                }
                AppearanceTarget::Body(_) | AppearanceTarget::Source { .. } => continue,
            };
            let Some(target) = target else {
                let target_id = match &binding.target {
                    AppearanceTarget::Face(id) => id.0.clone(),
                    AppearanceTarget::Surface(id) => id.0.clone(),
                    AppearanceTarget::Curve(id) => id.0.clone(),
                    AppearanceTarget::Edge(id) => id.0.clone(),
                    AppearanceTarget::Point(id) => id.0.clone(),
                    AppearanceTarget::Vertex(id) => id.0.clone(),
                    AppearanceTarget::Tessellation(id) => id.clone(),
                    AppearanceTarget::Body(_) | AppearanceTarget::Source { .. } => continue,
                };
                direct_unstyled.insert(target_id);
                continue;
            };
            let name = appearance.name.as_deref().unwrap_or("");
            let style = match style_kind {
                "surface" => self.surface_style(color, name, &mut style_refs),
                "curve" => self.curve_style(color, name, &mut style_refs),
                "point" => self.point_style(color, name, &mut style_refs),
                _ => unreachable!(),
            };
            self.written_appearance_bindings.insert(binding.id.clone());
            styled.push(
                self.emitter
                    .emit("STYLED_ITEM", &format!("'color',({style}),{target}")),
            );
        }
        for (body_id, spec) in &body_colors {
            if styled_bodies.contains(body_id) {
                continue;
            }
            let mut targets = self
                .body_item_refs
                .get(*body_id)
                .cloned()
                .unwrap_or_default();
            if targets.is_empty() {
                targets.extend(self.body_shape_refs.get(*body_id).copied());
            }
            if targets.is_empty() {
                continue;
            }
            if let Some(binding_id) = spec.binding_id {
                self.written_appearance_bindings
                    .insert(binding_id.to_string());
            }
            let name = spec
                .appearance
                .and_then(|appearance| appearance.name.as_deref())
                .unwrap_or("");
            let style = if self
                .bodies
                .get(*body_id)
                .is_some_and(|body| body.kind == BodyKind::Wire)
            {
                self.curve_style(spec.color, name, &mut style_refs)
            } else {
                self.surface_style(spec.color, name, &mut style_refs)
            };
            styled.extend(targets.into_iter().map(|target| {
                self.emitter
                    .emit("STYLED_ITEM", &format!("'color',({style}),{target}"))
            }));
            styled_bodies.insert(body_id);
        }
        // A color is unrepresented when no emitted ADVANCED_FACE could carry it:
        // a face override whose face was skipped, or a body whose faces were all
        // skipped (hidden bodies or faces without an explicit STEP surface),
        // and no whole-body carrier was emitted.
        let emitted: BTreeSet<&str> = self.face_step_refs.keys().map(String::as_str).collect();
        let mut unstyled_targets = face_colors
            .keys()
            .filter(|id| !emitted.contains(**id as &str))
            .map(|id| (*id).to_string())
            .collect::<BTreeSet<_>>();
        unstyled_targets.extend(
            body_colors
                .keys()
                .filter(|id| !styled_bodies.contains(**id as &str))
                .map(|id| (*id).to_string()),
        );
        unstyled_targets.extend(direct_unstyled);
        self.unstyled_colors = unstyled_targets.len();
        if styled.is_empty() {
            return;
        }
        self.emitter.emit(
            "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION",
            &format!("'',{},{context}", refs(&styled)),
        );
    }

    fn surface_style(
        &mut self,
        color: cadmpeg_ir::topology::Color,
        name: &str,
        cache: &mut HashMap<String, Ref>,
    ) -> Ref {
        let rgb = format!(
            "{},{},{}",
            real(f64::from(color.r)),
            real(f64::from(color.g)),
            real(f64::from(color.b))
        );
        let key = format!("surface:{name}:{rgb}");
        if let Some(style) = cache.get(&key) {
            return *style;
        }
        let colour = self
            .emitter
            .emit("COLOUR_RGB", &format!("{},{rgb}", string(name)));
        let fill_colour = self
            .emitter
            .emit("FILL_AREA_STYLE_COLOUR", &format!("'',{colour}"));
        let fill = self
            .emitter
            .emit("FILL_AREA_STYLE", &format!("'',({fill_colour})"));
        let style_fill = self
            .emitter
            .emit("SURFACE_STYLE_FILL_AREA", &fill.to_string());
        let side = self
            .emitter
            .emit("SURFACE_SIDE_STYLE", &format!("'',({style_fill})"));
        let usage = self
            .emitter
            .emit("SURFACE_STYLE_USAGE", &format!(".BOTH.,{side}"));
        let assignment = self
            .emitter
            .emit("PRESENTATION_STYLE_ASSIGNMENT", &format!("({usage})"));
        cache.insert(key, assignment);
        assignment
    }

    fn curve_style(
        &mut self,
        color: cadmpeg_ir::topology::Color,
        name: &str,
        cache: &mut HashMap<String, Ref>,
    ) -> Ref {
        let rgb = format!(
            "{},{},{}",
            real(f64::from(color.r)),
            real(f64::from(color.g)),
            real(f64::from(color.b))
        );
        let key = format!("curve:{name}:{rgb}");
        if let Some(style) = cache.get(&key) {
            return *style;
        }
        let colour = self
            .emitter
            .emit("COLOUR_RGB", &format!("{},{rgb}", string(name)));
        let font = self
            .emitter
            .emit("DRAUGHTING_PRE_DEFINED_CURVE_FONT", &string("continuous"));
        let curve = self.emitter.emit(
            "CURVE_STYLE",
            &format!("'',{font},POSITIVE_LENGTH_MEASURE(0.1),{colour}"),
        );
        let assignment = self
            .emitter
            .emit("PRESENTATION_STYLE_ASSIGNMENT", &format!("({curve})"));
        cache.insert(key, assignment);
        assignment
    }

    fn point_style(
        &mut self,
        color: cadmpeg_ir::topology::Color,
        name: &str,
        cache: &mut HashMap<String, Ref>,
    ) -> Ref {
        let rgb = format!(
            "{},{},{}",
            real(f64::from(color.r)),
            real(f64::from(color.g)),
            real(f64::from(color.b))
        );
        let key = format!("point:{name}:{rgb}");
        if let Some(style) = cache.get(&key) {
            return *style;
        }
        let colour = self
            .emitter
            .emit("COLOUR_RGB", &format!("{},{rgb}", string(name)));
        let point = self.emitter.emit(
            "POINT_STYLE",
            &format!("'',.DOT.,POSITIVE_LENGTH_MEASURE(1.),{colour}"),
        );
        let assignment = self
            .emitter
            .emit("PRESENTATION_STYLE_ASSIGNMENT", &format!("({point})"));
        cache.insert(key, assignment);
        assignment
    }

    fn emit_layers(&mut self) {
        for layer in self.ir.model.presentation_layers.clone() {
            let mut assigned = Vec::new();
            let mut unsupported = 0usize;
            for item in layer.items {
                let references = match item {
                    PresentationItem::Body { body } => self
                        .body_item_refs
                        .get(body.as_str())
                        .cloned()
                        .filter(|references| !references.is_empty())
                        .or_else(|| {
                            self.body_shape_refs
                                .get(body.as_str())
                                .copied()
                                .map(|reference| vec![reference])
                        })
                        .unwrap_or_default(),
                    PresentationItem::Face { face } => self
                        .face_step_refs
                        .get(face.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Edge { edge } => self
                        .edge_refs
                        .get(edge.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Vertex { vertex } => self
                        .vertex_refs
                        .get(vertex.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Curve { curve } => self
                        .curve_refs
                        .get(curve.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Surface { surface } => self
                        .surface_refs
                        .get(surface.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Product { product } => self
                        .product_step_refs
                        .get(product.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Occurrence { occurrence } => self
                        .occurrence_step_refs
                        .get(occurrence.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Pmi { annotation } => self
                        .pmi_step_refs
                        .get(annotation.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Point { point } => self
                        .point_refs
                        .get(point.as_str())
                        .copied()
                        .into_iter()
                        .collect(),
                    PresentationItem::Source { .. } => Vec::new(),
                    PresentationItem::Tessellation { tessellation } => self
                        .tessellation_step_refs
                        .get(&tessellation)
                        .copied()
                        .into_iter()
                        .collect(),
                };
                if references.is_empty() {
                    unsupported += 1;
                } else {
                    assigned.extend(references);
                }
            }
            if unsupported > 0 {
                self.loss(
                    StepLossCode::LayerItemWithoutCarrier,
                    format!(
                        "layer '{}' has {unsupported} item(s) without a writable STEP carrier",
                        layer.name
                    ),
                );
            }
            if !assigned.is_empty() {
                self.emitter.emit(
                    "PRESENTATION_LAYER_ASSIGNMENT",
                    &format!(
                        "{},{},{}",
                        string(&layer.name),
                        string(layer.description.as_deref().unwrap_or("")),
                        refs(&assigned)
                    ),
                );
            }
        }
    }

    fn emit_product_structure(&mut self) -> Ref {
        let name = self
            .ir
            .model
            .bodies
            .first()
            .and_then(|b| b.name.clone())
            .unwrap_or_else(|| "cadmpeg_model".to_string());

        let (application, protocol, year) = self.schema.application_protocol();
        let app_ctx = self
            .emitter
            .emit("APPLICATION_CONTEXT", &string(application));
        self.emitter.emit(
            "APPLICATION_PROTOCOL_DEFINITION",
            &format!(
                "{},{},{year},{app_ctx}",
                string("international standard"),
                string(protocol)
            ),
        );
        let prod_ctx = self.emitter.emit(
            "PRODUCT_CONTEXT",
            &format!("'',{app_ctx},{}", string("mechanical")),
        );
        let product = self.emitter.emit(
            "PRODUCT",
            &format!("{},{},'',({prod_ctx})", string(&name), string(&name)),
        );
        let formation = self
            .emitter
            .emit("PRODUCT_DEFINITION_FORMATION", &format!("'','',{product}"));
        let pd_ctx = self.emitter.emit(
            "PRODUCT_DEFINITION_CONTEXT",
            &format!(
                "{},{app_ctx},{}",
                string("part definition"),
                string("design")
            ),
        );
        let product_def = self.emitter.emit(
            "PRODUCT_DEFINITION",
            &format!("{},'',{formation},{pd_ctx}", string("design")),
        );
        self.emitter
            .emit("PRODUCT_DEFINITION_SHAPE", &format!("'','',{product_def}"))
    }

    fn emit_product_graph(&mut self, context: Ref) {
        let (application, protocol, year) = self.schema.application_protocol();
        let app_context = self
            .emitter
            .emit("APPLICATION_CONTEXT", &string(application));
        self.emitter.emit(
            "APPLICATION_PROTOCOL_DEFINITION",
            &format!(
                "{},{},{year},{app_context}",
                string("international standard"),
                string(protocol)
            ),
        );
        let product_context = self.emitter.emit(
            "PRODUCT_CONTEXT",
            &format!("'',{app_context},{}", string("mechanical")),
        );
        let definition_context = self.emitter.emit(
            "PRODUCT_DEFINITION_CONTEXT",
            &format!(
                "{},{app_context},{}",
                string("part definition"),
                string("design")
            ),
        );

        let ir = self.ir;
        let products = &ir.model.product_definitions;
        let occurrences = &ir.model.occurrences;
        let Ok(graph) = AssemblyGraph::new(occurrences) else {
            self.loss(
                StepLossCode::AssemblyGraphInvalid,
                "assembly occurrence graph is invalid".into(),
            );
            return;
        };
        let occurrence_products = occurrences
            .iter()
            .filter_map(|occurrence| match &occurrence.prototype {
                PrototypeReference::Local { definition } => {
                    Some((occurrence.id.clone(), definition.clone()))
                }
                PrototypeReference::External { .. } | PrototypeReference::Unresolved => None,
            })
            .collect::<HashMap<OccurrenceId, ProductDefinitionId>>();
        let mut product_origins = HashMap::<ProductDefinitionId, Ref>::new();
        for product in products {
            product_origins.insert(
                product.id.clone(),
                geometry::placement(
                    &mut self.emitter,
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                ),
            );
        }
        let mut representation_placements = HashMap::<ProductDefinitionId, Vec<Ref>>::new();
        let mut occurrence_placements = HashMap::<OccurrenceId, (Ref, Ref)>::new();
        for occurrence in occurrences {
            let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent else {
                continue;
            };
            let Some(parent_product) = occurrence_products.get(parent) else {
                continue;
            };
            let Some(child_product) = occurrence_products.get(&occurrence.id) else {
                continue;
            };
            let Some(&from) = product_origins.get(child_product) else {
                continue;
            };
            let transform = if occurrence.link_transform.unwrap_or(false) {
                occurrence.transform.compose(occurrence.prototype_transform)
            } else {
                occurrence.transform
            };
            if !transform.is_proper_rigid() || occurrence.scale != [1.0; 3] {
                continue;
            }
            let rows = transform.rows;
            let to = geometry::placement(
                &mut self.emitter,
                cadmpeg_ir::math::Point3::new(rows[0][3], rows[1][3], rows[2][3]),
                cadmpeg_ir::math::Vector3::new(rows[0][2], rows[1][2], rows[2][2]),
                cadmpeg_ir::math::Vector3::new(rows[0][0], rows[1][0], rows[2][0]),
            );
            representation_placements
                .entry(parent_product.clone())
                .or_default()
                .push(to);
            occurrence_placements.insert(occurrence.id.clone(), (from, to));
        }
        let mut definitions = HashMap::<ProductDefinitionId, Ref>::new();
        let mut representations = HashMap::<ProductDefinitionId, Ref>::new();
        for product in products {
            let product_id = product
                .part_number
                .as_deref()
                .or(product.source_name.as_deref())
                .unwrap_or(product.id.as_str());
            let name = product
                .label
                .as_deref()
                .or(product.source_name.as_deref())
                .unwrap_or(product_id);
            let description = product.description.as_deref().unwrap_or("");
            let product_ref = self.emitter.emit(
                "PRODUCT",
                &format!(
                    "{},{},{},({product_context})",
                    string(product_id),
                    string(name),
                    string(description)
                ),
            );
            self.product_step_refs
                .insert(product.id.0.clone(), product_ref);
            let formation = self.emitter.emit(
                "PRODUCT_DEFINITION_FORMATION",
                &format!("'','',{product_ref}"),
            );
            let definition = self.emitter.emit(
                "PRODUCT_DEFINITION",
                &format!(
                    "{},{},{formation},{definition_context}",
                    string(product_id),
                    string(description)
                ),
            );
            let shape = self
                .emitter
                .emit("PRODUCT_DEFINITION_SHAPE", &format!("'','',{definition}"));
            self.default_product_definition_shape.get_or_insert(shape);
            let mut body_items = product
                .bodies
                .iter()
                .flat_map(|body| {
                    self.body_item_refs
                        .get(body.as_str())
                        .into_iter()
                        .flatten()
                        .copied()
                })
                .collect::<Vec<_>>();
            if let Some(origin) = product_origins.get(&product.id) {
                body_items.push(*origin);
            }
            if let Some(placements) = representation_placements.get(&product.id) {
                body_items.extend(placements);
            }
            let representation = self.emitter.emit(
                "SHAPE_REPRESENTATION",
                &format!("{},{},{context}", string(name), refs(&body_items)),
            );
            self.emitter.emit(
                "SHAPE_DEFINITION_REPRESENTATION",
                &format!("{shape},{representation}"),
            );
            definitions.insert(product.id.clone(), definition);
            representations.insert(product.id.clone(), representation);
        }

        for occurrence in occurrences {
            let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent else {
                let transform = if occurrence.link_transform.unwrap_or(false) {
                    occurrence.transform.compose(occurrence.prototype_transform)
                } else {
                    occurrence.transform
                };
                if !is_identity(&transform.rows) || occurrence.scale != [1.0; 3] {
                    self.loss(
                        StepLossCode::RootOccurrencePlacementNotRepresentable,
                        format!(
                            "root occurrence '{}' has a placement or scale that is not representable",
                            occurrence.id
                        ),
                    );
                }
                continue;
            };
            let Some(parent_occurrence) = graph.occurrence(parent) else {
                self.loss(
                    StepLossCode::OccurrenceUnresolvedParent,
                    format!("occurrence '{}' has an unresolved parent", occurrence.id),
                );
                continue;
            };
            let Some(parent_product) = occurrence_products.get(&parent_occurrence.id) else {
                self.missing_parent_products.insert(occurrence.id.0.clone());
                continue;
            };
            let Some(child_product) = occurrence_products.get(&occurrence.id) else {
                self.loss(
                    StepLossCode::OccurrenceNoLocalProduct,
                    format!(
                        "occurrence '{}' has no local product definition",
                        occurrence.id
                    ),
                );
                continue;
            };
            let Some((
                &parent_definition,
                &child_definition,
                &parent_representation,
                &child_representation,
            )) = definitions
                .get(parent_product)
                .zip(definitions.get(child_product))
                .zip(representations.get(parent_product))
                .zip(representations.get(child_product))
                .map(|(((a, b), c), d)| (a, b, c, d))
            else {
                continue;
            };
            let transform = if occurrence.link_transform.unwrap_or(false) {
                occurrence.transform.compose(occurrence.prototype_transform)
            } else {
                occurrence.transform
            };
            if !transform.is_proper_rigid() || occurrence.scale != [1.0; 3] {
                self.loss(
                    StepLossCode::OccurrencePlacementNotRigid,
                    format!("occurrence '{}' placement is not rigid", occurrence.id),
                );
                continue;
            }
            let occurrence_name = occurrence.name.as_deref().unwrap_or(occurrence.id.as_str());
            let usage = self.emitter.emit(
                "NEXT_ASSEMBLY_USAGE_OCCURRENCE",
                &format!(
                    "{},{},'',{parent_definition},{child_definition},$",
                    string(occurrence.id.as_str()),
                    string(occurrence_name)
                ),
            );
            self.occurrence_step_refs
                .insert(occurrence.id.0.clone(), usage);
            let usage_shape = self
                .emitter
                .emit("PRODUCT_DEFINITION_SHAPE", &format!("'','',{usage}"));
            let Some(&(from, to)) = occurrence_placements.get(&occurrence.id) else {
                continue;
            };
            let transform = self
                .emitter
                .emit("ITEM_DEFINED_TRANSFORMATION", &format!("'','',{from},{to}"));
            let relationship = self.emitter.emit_raw(
                "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION",
                &format!(
                    "( REPRESENTATION_RELATIONSHIP('','',{child_representation},{parent_representation}) REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION({transform}) SHAPE_REPRESENTATION_RELATIONSHIP() )"
                ),
            );
            self.emitter.emit(
                "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION",
                &format!("{relationship},{usage_shape}"),
            );
        }
    }

    fn emit_context(&mut self) -> Ref {
        let len = self.emit_length_unit();
        let angle = self.emit_angle_unit();
        let solid = self.emitter.emit_raw(
            "SOLID_ANGLE_UNIT",
            "( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() )",
        );
        let unc = self.emitter.emit(
            "UNCERTAINTY_MEASURE_WITH_UNIT",
            &format!(
                "LENGTH_MEASURE({}),{len},{},{}",
                real(self.ir.tolerances.linear),
                string("distance_accuracy_value"),
                string("maximum model space distance")
            ),
        );
        self.emitter.emit_raw(
            "GEOMETRIC_REPRESENTATION_CONTEXT",
            &format!(
                "( GEOMETRIC_REPRESENTATION_CONTEXT(3) \
                 GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT(({unc})) \
                 GLOBAL_UNIT_ASSIGNED_CONTEXT(({len},{angle},{solid})) \
                 REPRESENTATION_CONTEXT('Context','3D') )"
            ),
        )
    }

    fn emit_length_unit(&mut self) -> Ref {
        if let Some(unit) = self.length_unit {
            return unit;
        }
        let unit = self.emitter.emit_raw(
            "LENGTH_UNIT",
            "( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) )",
        );
        self.length_unit = Some(unit);
        unit
    }

    fn emit_angle_unit(&mut self) -> Ref {
        if let Some(unit) = self.angle_unit {
            return unit;
        }
        let unit = self.emitter.emit_raw(
            "PLANE_ANGLE_UNIT",
            "( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) )",
        );
        self.angle_unit = Some(unit);
        unit
    }

    fn emit_ratio_unit(&mut self) -> Ref {
        if let Some(unit) = self.ratio_unit {
            return unit;
        }
        let unit = self
            .emitter
            .emit_raw("RATIO_UNIT", "( NAMED_UNIT(*) RATIO_UNIT() )");
        self.ratio_unit = Some(unit);
        unit
    }

    /// Emit one shape item per region; visibility is represented separately
    /// when the target application protocol supports `INVISIBILITY`.
    fn emit_shape_items(&mut self, context: Ref) -> Vec<Ref> {
        let mut items = Vec::new();
        let ir = self.ir;
        for region in &ir.model.regions {
            let body_kind = self
                .bodies
                .get(region.body.as_str())
                .map_or(BodyKind::General, |body| body.kind);
            let has_surface_topology = region.shells.iter().any(|shell_id| {
                self.shells
                    .get(shell_id.as_str())
                    .is_some_and(|shell| !shell.faces.is_empty())
            });
            let has_wire_topology = region.shells.iter().any(|shell_id| {
                self.shells.get(shell_id.as_str()).is_some_and(|shell| {
                    !shell.wire_edges.is_empty() || !shell.free_vertices.is_empty()
                })
            });
            let mixed_wire = body_kind == BodyKind::General && has_wire_topology;
            if body_kind == BodyKind::Wire
                || (body_kind == BodyKind::General && !has_surface_topology)
            {
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
                    self.body_step_item_refs
                        .entry(region.body.0.clone())
                        .or_default()
                        .push(item);
                    self.body_step_refs
                        .entry(region.body.0.clone())
                        .or_insert(item);
                } else {
                    self.empty_wire_regions.insert(region.id.0.clone());
                }
                continue;
            }
            let closed = body_kind == BodyKind::Solid;
            let Some((outer_id, void_ids)) = region.shells.split_first() else {
                self.empty_regions.insert(region.id.0.clone());
                continue;
            };
            let Some(outer) = self.emit_shell(outer_id.as_str(), closed) else {
                self.topology_relation_loss(
                    format!("region:{}:outer-shell:{}", region.id, outer_id),
                    StepLossCode::RegionNoWritableOuterShell,
                    format!("region {} has no writable outer shell", region.id),
                );
                continue;
            };
            let mut voids = Vec::new();
            for sid in void_ids {
                if let Some(void) = self.emit_shell(sid.as_str(), closed) {
                    voids.push(void);
                } else {
                    self.topology_relation_loss(
                        format!("region:{}:void-shell:{}", region.id, sid),
                        StepLossCode::RegionOmittedVoidShell,
                        format!(
                            "region {} omitted void shell {} because it has no writable faces",
                            region.id, sid
                        ),
                    );
                }
            }
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
            self.body_step_item_refs
                .entry(region.body.0.clone())
                .or_default()
                .push(item);
            self.body_step_refs
                .entry(region.body.0.clone())
                .or_insert(if closed { item } else { outer });
            if mixed_wire {
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
                    self.body_step_item_refs
                        .entry(region.body.0.clone())
                        .or_default()
                        .push(item);
                    self.body_step_refs
                        .entry(region.body.0.clone())
                        .or_insert(item);
                } else {
                    self.empty_wire_regions.insert(region.id.0.clone());
                }
            }
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
            .bodies
            .get(body_id.as_str())
            .and_then(|body| body.transform);
        let Some(transform) = transform.filter(|transform| !is_identity(&transform.rows)) else {
            return item;
        };
        if !is_rigid_transform(&transform.rows) {
            self.loss(
                StepLossCode::BodyNonRigidTransform,
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

    fn emit_visibility(&mut self) {
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
                    StepLossCode::HiddenBodyVisibilityUnsupported,
                    format!(
                        "{hidden} hidden body visibility assignment(s) are unsupported by {}",
                        self.schema.file_schema()
                    ),
                );
            }
            return;
        }
        let mut hidden = Vec::new();
        let mut hidden_without_items = Vec::new();
        for body in &self.ir.model.bodies {
            if body.visible != Some(false) {
                continue;
            }
            if let Some(references) = self.body_step_item_refs.get(body.id.as_str()) {
                if !references.is_empty() {
                    hidden.extend(references.iter().copied());
                    continue;
                }
            }
            if let Some(reference) = self.body_step_refs.get(body.id.as_str()).copied() {
                hidden.push(reference);
            } else {
                hidden_without_items.push(body.id.0.clone());
            }
        }
        self.hidden_bodies_without_items
            .extend(hidden_without_items);
        if !hidden.is_empty() {
            self.emitter.emit("INVISIBILITY", &refs(&hidden));
        }
    }

    fn emit_wire_region(&mut self, region: &cadmpeg_ir::topology::Region) -> Option<Ref> {
        let mut shells = Vec::new();
        for shell_id in &region.shells {
            if let Some(shell) = self.shells.get(shell_id.as_str()).copied().cloned() {
                shells.push(shell);
            } else {
                self.missing_wire_shells
                    .insert((region.id.0.clone(), shell_id.0.clone()));
            }
        }
        let mut connected_sets = Vec::new();
        for shell in shells {
            if !shell.free_vertices.is_empty() {
                self.loss(
                    StepLossCode::WireShellFreeVertices,
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

    fn emit_standalone_geometry(&mut self) -> Vec<Ref> {
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
                self.unwritten_geometry_carriers.insert(surface_id);
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
            if let Some(reference) = self.emit_curve(&curve_id) {
                members.push(reference);
            } else {
                self.unwritten_geometry_carriers.insert(curve_id);
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
            let Some(point) = self.points.get(point_id.as_str()).copied() else {
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

    fn emit_tessellations(&mut self, context: Ref) {
        if self.ir.model.tessellations.is_empty() {
            return;
        }
        if !self.schema.supports_tessellation() {
            self.loss(
                StepLossCode::TessellationRequiresAp242,
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
            if !mesh.feature_edges.is_empty() {
                self.loss(
                    StepLossCode::TessellationFeatureEdges,
                    format!(
                        "tessellation '{}' feature-edge classification is not represented",
                        mesh.id
                    ),
                );
            }
            if !mesh.corner_normals.is_empty() {
                self.loss(
                    StepLossCode::TessellationCornerNormals,
                    format!(
                        "tessellation '{}' corner normals are not represented",
                        mesh.id
                    ),
                );
            }
            if !mesh.triangle_groups.is_empty() {
                self.loss(
                    StepLossCode::TessellationTriangleGroups,
                    format!(
                        "tessellation '{}' triangle groups are not represented",
                        mesh.id
                    ),
                );
            }
            if !mesh.texture_assignments.is_empty() {
                self.loss(
                    StepLossCode::TessellationTextureAssignments,
                    format!(
                        "tessellation '{}' texture assignments are not represented",
                        mesh.id
                    ),
                );
            }
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
                    StepLossCode::TessellationInvalidCardinality,
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
                "COORDINATES_LIST",
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
                let link = self.body_step_refs.get(body.as_str()).copied()?;
                let kind = self.bodies.get(body.as_str())?.kind;
                matches!(kind, BodyKind::Solid | BodyKind::Sheet).then_some((kind, link))
            });
            if let Some(body) = &mesh.body {
                if linked_body.is_none() {
                    self.loss(
                        StepLossCode::TessellationBodyLinkUnwritable,
                        format!(
                            "tessellation '{}' body '{}' has no writable AP242 tessellation link",
                            mesh.id, body
                        ),
                    );
                }
            }
            let mut reduced_fields = Vec::new();
            if !mesh.faces.is_empty() {
                reduced_fields.push(format!("{} face ownership link(s)", mesh.faces.len()));
            }
            if mesh.chordal_deflection.is_some() {
                reduced_fields.push("chordal deflection".to_string());
            }
            if !mesh.channels.is_empty() {
                reduced_fields.push(format!("{} data channel(s)", mesh.channels.len()));
            }
            if !reduced_fields.is_empty() {
                self.loss(
                    StepLossCode::TessellationMetadataReduced,
                    format!(
                        "tessellation '{}' reduced unsupported metadata: {}",
                        mesh.id,
                        reduced_fields.join(", ")
                    ),
                );
            }
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
                    "TRIANGULATED_FACE",
                    &format!(
                        "{},{coordinates},{},{normals},$,({point_indices}),({triangles})",
                        string(&mesh.id),
                        mesh.vertices.len()
                    ),
                );
                self.emitter.emit(
                    if kind == BodyKind::Solid {
                        "TESSELLATED_SOLID"
                    } else {
                        "TESSELLATED_SHELL"
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
                    "TRIANGULATED_SURFACE_SET",
                    &format!(
                        "{},{coordinates},{},{normals},({point_indices}),({triangles})",
                        string(&mesh.id),
                        mesh.vertices.len()
                    ),
                )
            };
            self.tessellation_step_refs.insert(mesh.id.clone(), item);
            representation_items.push(item);
        }
        if !representation_items.is_empty() {
            self.emitter.emit(
                "TESSELLATED_SHAPE_REPRESENTATION",
                &format!("'',{},{context}", refs(&representation_items)),
            );
        }
    }

    fn emit_shell(&mut self, shell_id: &str, closed: bool) -> Option<Ref> {
        let shell = self.shells.get(shell_id).copied()?;
        let face_ids: Vec<String> = shell.faces.iter().map(|f| f.0.clone()).collect();
        let mut face_refs = Vec::new();
        for fid in &face_ids {
            if let Some(r) = self.emit_face(fid) {
                face_refs.push(r);
            } else {
                let outer = self.faces.get(fid.as_str()).is_some_and(|face| {
                    face.loops.iter().any(|loop_id| {
                        self.loops
                            .get(loop_id.as_str())
                            .is_some_and(|loop_| loop_.boundary_role == LoopBoundaryRole::Outer)
                    }) || face.loops.first().is_some_and(|loop_id| {
                        !face.loops.iter().any(|candidate| {
                            self.loops
                                .get(candidate.as_str())
                                .is_some_and(|loop_| loop_.boundary_role == LoopBoundaryRole::Outer)
                        }) && self.loops.contains_key(loop_id.as_str())
                    })
                });
                self.topology_relation_loss(
                    format!("shell:{shell_id}:face:{fid}"),
                    if outer {
                        StepLossCode::ShellOmittedOuterFace
                    } else {
                        StepLossCode::ShellOmittedInnerFace
                    },
                    format!(
                        "shell {shell_id} omitted face {fid} because the face has no writable topology"
                    ),
                );
            }
        }
        if face_refs.is_empty() {
            return None;
        }
        Some(self.emitter.emit(
            if closed { "CLOSED_SHELL" } else { "OPEN_SHELL" },
            &format!("'',{}", refs(&face_refs)),
        ))
    }

    fn emit_face(&mut self, face_id: &str) -> Option<Ref> {
        let face = self.faces.get(face_id).copied()?;
        let surface_id = face.surface.0.clone();
        // A face resting on an unknown (opaque) surface cannot become an
        // ADVANCED_FACE: STEP requires a real surface. Skip it and aggregate the
        // loss rather than fabricate placeholder geometry.
        if let Some(surf) = self.surfaces.get(surface_id.as_str()) {
            if !geometry::surface_is_supported(&surf.geometry) {
                self.unknown_surface_faces.insert(face_id.to_string());
                return None;
            }
        }
        let loop_ids: Vec<String> = face.loops.iter().map(|l| l.0.clone()).collect();
        let same_sense = matches!(face.sense, Sense::Forward);

        let Some(surf_ref) = self.emit_surface(&surface_id) else {
            self.unknown_surface_faces.insert(face_id.to_string());
            return None;
        };

        let mut bound_refs = Vec::new();
        for (i, lid) in loop_ids.iter().enumerate() {
            if let Some(loop_ref) = self.emit_loop(lid) {
                let kind = if matches!(
                    self.loops
                        .get(lid.as_str())
                        .map(|loop_| loop_.boundary_role),
                    Some(LoopBoundaryRole::Outer)
                ) || (i == 0
                    && !loop_ids.iter().any(|id| {
                        self.loops
                            .get(id.as_str())
                            .is_some_and(|loop_| loop_.boundary_role == LoopBoundaryRole::Outer)
                    })) {
                    "FACE_OUTER_BOUND"
                } else {
                    "FACE_BOUND"
                };
                let b = self.emitter.emit(kind, &format!("'',{loop_ref},.T."));
                bound_refs.push(b);
            } else {
                let outer = self
                    .loops
                    .get(lid.as_str())
                    .is_some_and(|loop_| loop_.boundary_role == LoopBoundaryRole::Outer)
                    || (i == 0
                        && !loop_ids.iter().any(|id| {
                            self.loops
                                .get(id.as_str())
                                .is_some_and(|loop_| loop_.boundary_role == LoopBoundaryRole::Outer)
                        }));
                self.topology_relation_loss(
                    format!("face:{face_id}:loop:{lid}"),
                    if outer {
                        StepLossCode::FaceOmittedOuterLoop
                    } else {
                        StepLossCode::FaceOmittedInnerLoop
                    },
                    format!(
                        "face {face_id} omitted loop {lid} because the loop has no writable topology"
                    ),
                );
            }
        }
        if bound_refs.is_empty() {
            self.topology_relation_loss(
                format!("face:{face_id}:bound-list"),
                StepLossCode::FaceNoWritableBounds,
                format!("face {face_id} has no writable bounds"),
            );
            return None;
        }
        let flag = if same_sense { ".T." } else { ".F." };
        let advanced_face = self.emitter.emit(
            "ADVANCED_FACE",
            &format!(
                "{},{},{surf_ref},{flag}",
                string(face.name.as_deref().unwrap_or("")),
                refs(&bound_refs)
            ),
        );
        self.face_step_refs
            .insert(face_id.to_string(), advanced_face);
        Some(advanced_face)
    }

    fn emit_loop(&mut self, loop_id: &str) -> Option<Ref> {
        let Some(lp) = self.loops.get(loop_id).copied() else {
            self.topology_relation_loss(
                format!("loop:{loop_id}:record"),
                StepLossCode::LoopRecordMissing,
                format!("loop {loop_id} was omitted because its record is missing"),
            );
            return None;
        };
        if lp.coedges.is_empty() && lp.vertex_uses.len() == 1 {
            let vertex_id = lp.vertex_uses[0].vertex.as_str();
            let Some(vertex) = self.emit_vertex(vertex_id) else {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:vertex:{vertex_id}"),
                    StepLossCode::LoopVertexMissing,
                    format!("loop {loop_id} was omitted because vertex {vertex_id} is missing"),
                );
                return None;
            };
            return Some(self.emitter.emit("VERTEX_LOOP", &format!("'',{vertex}")));
        }
        let coedge_ids = self.ordered_loop_coedges(loop_id, lp)?;
        let mut oe_refs = Vec::new();
        for cid in &coedge_ids {
            let Some(coedge) = self.coedges.get(cid.as_str()).copied() else {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:coedge:{cid}"),
                    StepLossCode::LoopCoedgeRecordMissing,
                    format!("loop {loop_id} omitted coedge {cid} because its record is missing"),
                );
                return None;
            };
            let orientation = matches!(coedge.sense, Sense::Forward);
            let Some(edge_ref) = self.emit_edge(coedge.edge.as_str()) else {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:edge:{}", coedge.edge),
                    StepLossCode::LoopEdgeNotWritable,
                    format!(
                        "loop {loop_id} omitted edge {} because the edge is not writable",
                        coedge.edge
                    ),
                );
                return None;
            };
            let flag = if orientation { ".T." } else { ".F." };
            let oe = self
                .emitter
                .emit("ORIENTED_EDGE", &format!("'',*,*,{edge_ref},{flag}"));
            oe_refs.push(oe);
        }
        if oe_refs.is_empty() {
            self.topology_relation_loss(
                format!("loop:{loop_id}:edge-list"),
                StepLossCode::LoopNoWritableEdges,
                format!("loop {loop_id} has no writable edges"),
            );
            return None;
        }
        Some(
            self.emitter
                .emit("EDGE_LOOP", &format!("'',{}", refs(&oe_refs))),
        )
    }

    fn ordered_loop_coedges(&mut self, loop_id: &str, lp: &Loop) -> Option<Vec<String>> {
        let mut segments = Vec::with_capacity(lp.coedges.len());
        let mut seen = BTreeSet::new();

        for coedge_id in &lp.coedges {
            let coedge_key = coedge_id.as_str();
            if !seen.insert(coedge_key) {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:duplicate-coedge:{coedge_id}"),
                    StepLossCode::LoopDuplicateCoedge,
                    format!(
                        "loop {loop_id} cannot be ordered because coedge {coedge_id} occurs more than once"
                    ),
                );
                return None;
            }
            let Some(coedge) = self.coedges.get(coedge_key).copied() else {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:coedge:{coedge_id}"),
                    StepLossCode::LoopCoedgeMissingForOrder,
                    format!(
                        "loop {loop_id} cannot be ordered because coedge {coedge_id} is missing"
                    ),
                );
                return None;
            };
            let edge_key = coedge.edge.as_str();
            let Some(edge) = self.edges.get(edge_key).copied() else {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:edge:{edge_key}"),
                    StepLossCode::LoopEdgeMissingForOrder,
                    format!("loop {loop_id} cannot be ordered because edge {edge_key} is missing"),
                );
                return None;
            };
            let Some(curve_id) = edge.curve.as_ref() else {
                self.curveless_edges.insert(edge_key.to_string());
                self.topology_relation_loss(
                    format!("edge:{edge_key}:curve"),
                    StepLossCode::EdgeNo3dCurve,
                    format!("edge {edge_key} was omitted because it has no 3D curve"),
                );
                continue;
            };
            let Some(curve) = self.curves.get(curve_id.as_str()).copied() else {
                self.topology_relation_loss(
                    format!("edge:{edge_key}:curve:{curve_id}"),
                    StepLossCode::EdgeCurveNotWritable,
                    format!("edge {edge_key} was omitted because curve {curve_id} is not writable"),
                );
                continue;
            };
            if !geometry::curve_is_supported(&curve.geometry) {
                self.curveless_edges.insert(edge_key.to_string());
                self.topology_relation_loss(
                    format!("edge:{edge_key}:unsupported-curve:{curve_id}"),
                    StepLossCode::EdgeCurveUnsupported,
                    format!("edge {edge_key} was omitted because curve {curve_id} is unsupported"),
                );
                continue;
            }
            let (start_vertex, end_vertex) = match coedge.sense {
                Sense::Forward => (&edge.start, &edge.end),
                Sense::Reversed => (&edge.end, &edge.start),
            };
            segments.push(LoopSegment {
                coedge_id: coedge_id.0.clone(),
                start_vertex: start_vertex.0.clone(),
                end_vertex: end_vertex.0.clone(),
            });
        }

        let mut outgoing: HashMap<String, Vec<usize>> = HashMap::new();
        let mut incoming: HashMap<&str, usize> = HashMap::new();
        for (index, segment) in segments.iter().enumerate() {
            outgoing
                .entry(segment.start_vertex.clone())
                .or_default()
                .push(index);
            *incoming.entry(segment.end_vertex.as_str()).or_default() += 1;
        }
        if segments.iter().any(|segment| {
            outgoing.get(&segment.start_vertex).map_or(0, Vec::len)
                != incoming
                    .get(segment.start_vertex.as_str())
                    .copied()
                    .unwrap_or(0)
        }) {
            self.topology_relation_loss(
                format!("loop:{loop_id}:continuity"),
                StepLossCode::LoopNoContinuousOrdering,
                format!("loop {loop_id} has no continuous vertex-to-vertex coedge ordering"),
            );
            return None;
        }

        // The IR coedge list and its next/previous links are not sufficient to
        // establish the STEP path order. Build an Eulerian circuit from the
        // oriented edge endpoints instead; this also handles repeated vertices
        // without choosing a geometric edge by position in the source list.
        for candidates in outgoing.values_mut() {
            candidates.reverse();
        }
        let first_start = segments.first()?.start_vertex.clone();
        let mut vertex_stack = vec![first_start.clone()];
        let mut edge_stack = Vec::new();
        let mut circuit = Vec::with_capacity(segments.len());
        while let Some(vertex) = vertex_stack.last().cloned() {
            let next_edge = outgoing.get_mut(&vertex).and_then(Vec::pop);
            if let Some(edge_index) = next_edge {
                vertex_stack.push(segments[edge_index].end_vertex.clone());
                edge_stack.push(edge_index);
            } else {
                vertex_stack.pop();
                if let Some(edge_index) = edge_stack.pop() {
                    circuit.push(edge_index);
                }
            }
        }
        circuit.reverse();

        if circuit.len() != segments.len() {
            self.topology_relation_loss(
                format!("loop:{loop_id}:continuity"),
                StepLossCode::LoopNoContinuousOrdering,
                format!("loop {loop_id} has no continuous vertex-to-vertex coedge ordering"),
            );
            return None;
        }

        if let Some(first_index) = circuit.iter().position(|index| *index == 0) {
            circuit.rotate_left(first_index);
        }
        let mut current_vertex = segments.first()?.start_vertex.as_str();
        for edge_index in &circuit {
            let segment = &segments[*edge_index];
            if segment.start_vertex != current_vertex {
                self.topology_relation_loss(
                    format!("loop:{loop_id}:continuity"),
                    StepLossCode::LoopNoContinuousOrdering,
                    format!("loop {loop_id} has no continuous vertex-to-vertex coedge ordering"),
                );
                return None;
            }
            current_vertex = segment.end_vertex.as_str();
        }
        if current_vertex != first_start {
            self.topology_relation_loss(
                format!("loop:{loop_id}:continuity"),
                StepLossCode::LoopNoContinuousOrdering,
                format!("loop {loop_id} has no continuous vertex-to-vertex coedge ordering"),
            );
            return None;
        }

        Some(
            circuit
                .into_iter()
                .map(|index| segments[index].coedge_id.clone())
                .collect(),
        )
    }

    fn emit_edge(&mut self, edge_id: &str) -> Option<Ref> {
        if let Some(r) = self.edge_refs.get(edge_id) {
            return Some(*r);
        }
        let Some(edge) = self.edges.get(edge_id).copied() else {
            self.topology_relation_loss(
                format!("edge:{edge_id}:record"),
                StepLossCode::EdgeRecordMissing,
                format!("edge {edge_id} was omitted because its record is missing"),
            );
            return None;
        };
        let Some(v1) = self.emit_vertex(edge.start.as_str()) else {
            self.topology_relation_loss(
                format!("edge:{edge_id}:start-vertex:{}", edge.start),
                StepLossCode::EdgeStartVertexMissing,
                format!(
                    "edge {edge_id} was omitted because start vertex {} is missing",
                    edge.start
                ),
            );
            return None;
        };
        let Some(v2) = self.emit_vertex(edge.end.as_str()) else {
            self.topology_relation_loss(
                format!("edge:{edge_id}:end-vertex:{}", edge.end),
                StepLossCode::EdgeEndVertexMissing,
                format!(
                    "edge {edge_id} was omitted because end vertex {} is missing",
                    edge.end
                ),
            );
            return None;
        };
        let Some(curve_id) = &edge.curve else {
            self.curveless_edges.insert(edge_id.to_string());
            self.topology_relation_loss(
                format!("edge:{edge_id}:curve"),
                StepLossCode::EdgeNo3dCurve,
                format!("edge {edge_id} was omitted because it has no 3D curve"),
            );
            return None;
        };
        if self
            .curves
            .get(curve_id.as_str())
            .is_some_and(|curve| !geometry::curve_is_supported(&curve.geometry))
        {
            self.curveless_edges.insert(edge_id.to_string());
            self.topology_relation_loss(
                format!("edge:{edge_id}:unsupported-curve:{curve_id}"),
                StepLossCode::EdgeCurveUnsupported,
                format!("edge {edge_id} was omitted because curve {curve_id} is unsupported"),
            );
            return None;
        }
        let Some(basis_curve) = self.emit_curve(curve_id.as_str()) else {
            self.topology_relation_loss(
                format!("edge:{edge_id}:curve:{curve_id}"),
                StepLossCode::EdgeCurveNotWritable,
                format!("edge {edge_id} was omitted because curve {curve_id} is not writable"),
            );
            return None;
        };
        let associated = self.edge_coedges.get(edge_id).cloned().unwrap_or_default();
        let mut pcurve_refs = Vec::new();
        for (pcurve_id, surface_id) in associated {
            if let Some(pcurve) = self.emit_pcurve(pcurve_id, surface_id) {
                pcurve_refs.push(pcurve);
            } else if self.pcurves.contains_key(pcurve_id) {
                self.unwritten_pcurve_carriers.insert(pcurve_id.to_string());
            }
        }
        let curve_ref = if pcurve_refs.is_empty() {
            basis_curve
        } else {
            self.emitter.emit(
                "SURFACE_CURVE",
                &format!("'',{basis_curve},{},.CURVE_3D.", refs(&pcurve_refs)),
            )
        };
        // same_sense = .T.: the edge runs start→end along the curve's own
        // parameterization, the convention IR curves follow.
        let r = self
            .emitter
            .emit("EDGE_CURVE", &format!("'',{v1},{v2},{curve_ref},.T."));
        self.edge_refs.insert(edge_id.to_string(), r);
        Some(r)
    }

    fn emit_pcurve(&mut self, pcurve_id: &str, surface_id: &str) -> Option<Ref> {
        let pcurve = self.pcurves.get(pcurve_id).copied()?;
        let surface = self.emit_surface(surface_id)?;
        let curve = geometry::pcurve(&mut self.emitter, &pcurve.geometry)?;
        let context = if let Some(context) = self.pcurve_context {
            context
        } else {
            let context = self.emitter.emit_raw(
                "GEOMETRIC_REPRESENTATION_CONTEXT",
                "( GEOMETRIC_REPRESENTATION_CONTEXT(2) PARAMETRIC_REPRESENTATION_CONTEXT() REPRESENTATION_CONTEXT('uv','2D') )",
            );
            self.pcurve_context = Some(context);
            context
        };
        let representation = self.emitter.emit(
            "DEFINITIONAL_REPRESENTATION",
            &format!("'',({curve}),{context}"),
        );
        Some(
            self.emitter
                .emit("PCURVE", &format!("'',{surface},{representation}")),
        )
    }

    fn emit_vertex(&mut self, vertex_id: &str) -> Option<Ref> {
        if let Some(r) = self.vertex_refs.get(vertex_id) {
            return Some(*r);
        }
        let vertex = self.vertices.get(vertex_id).copied()?;
        let pt = self.points.get(vertex.point.as_str()).copied()?;
        let cp = geometry::point(&mut self.emitter, pt.position);
        self.point_refs.insert(vertex.point.0.clone(), cp);
        let r = self.emitter.emit("VERTEX_POINT", &format!("'',{cp}"));
        self.vertex_refs.insert(vertex_id.to_string(), r);
        Some(r)
    }

    fn emit_surface(&mut self, surface_id: &str) -> Option<Ref> {
        if let Some(r) = self.surface_refs.get(surface_id) {
            return Some(*r);
        }
        if self.geometry_emission_depth >= 256
            || !self.active_surfaces.insert(surface_id.to_string())
        {
            return None;
        }
        self.geometry_emission_depth += 1;
        let result = (|| {
            let surf = self.surfaces.get(surface_id).copied()?;
            let procedural = self
                .procedural_surfaces
                .get(surface_id)
                .map(|procedural| (procedural.id.0.clone(), procedural.definition.clone()));
            let emitted = procedural.and_then(|(id, definition)| {
                self.emit_procedural_surface(&surf.geometry, &definition)
                    .map(|reference| (id, reference))
            });
            let r = if let Some((id, reference)) = emitted {
                self.written_procedural_surfaces.insert(id);
                reference
            } else if !geometry::surface_is_supported(&surf.geometry) {
                return None;
            } else {
                geometry::surface(&mut self.emitter, &surf.geometry)?
            };
            Some(r)
        })();
        self.active_surfaces.remove(surface_id);
        self.geometry_emission_depth -= 1;
        if let Some(r) = result {
            self.surface_refs.insert(surface_id.to_string(), r);
        }
        result
    }

    fn emit_procedural_surface(
        &mut self,
        solved: &SurfaceGeometry,
        definition: &ProceduralSurfaceDefinition,
    ) -> Option<Ref> {
        let logical = |value: Option<bool>| match value {
            Some(true) => ".T.",
            Some(false) => ".F.",
            None => ".U.",
        };
        match definition {
            ProceduralSurfaceDefinition::LinearSweep {
                directrix,
                direction,
            } => {
                let directrix = self.emit_curve(directrix.as_str())?;
                let direction_ref = geometry::direction(&mut self.emitter, *direction);
                let vector = self.emitter.emit(
                    "VECTOR",
                    &format!("'',{direction_ref},{}", real(direction.norm())),
                );
                Some(self.emitter.emit(
                    "SURFACE_OF_LINEAR_EXTRUSION",
                    &format!("'',{directrix},{vector}"),
                ))
            }
            ProceduralSurfaceDefinition::AxisRevolution {
                directrix,
                axis_origin,
                axis_direction,
            } => {
                let directrix = self.emit_curve(directrix.as_str())?;
                let origin = geometry::point(&mut self.emitter, *axis_origin);
                let direction = geometry::direction(&mut self.emitter, *axis_direction);
                let axis = self
                    .emitter
                    .emit("AXIS1_PLACEMENT", &format!("'',{origin},{direction}"));
                Some(
                    self.emitter
                        .emit("SURFACE_OF_REVOLUTION", &format!("'',{directrix},{axis}")),
                )
            }
            ProceduralSurfaceDefinition::ParallelOffset {
                support,
                distance,
                self_intersect,
            } => {
                let support = self.emit_surface(support.as_str())?;
                Some(self.emitter.emit(
                    "OFFSET_SURFACE",
                    &format!(
                        "'',{support},{},{}",
                        real(*distance),
                        logical(*self_intersect)
                    ),
                ))
            }
            ProceduralSurfaceDefinition::Subset {
                support,
                parameter_ranges,
                u_sense: Some(u_sense),
                v_sense: Some(v_sense),
            } => {
                let support = self.emit_surface(support.as_str())?;
                Some(self.emitter.emit(
                    "RECTANGULAR_TRIMMED_SURFACE",
                    &format!(
                        "'',{support},{},{},{},{},{},{}",
                        real(parameter_ranges[0][0]),
                        real(parameter_ranges[0][1]),
                        real(parameter_ranges[1][0]),
                        real(parameter_ranges[1][1]),
                        if *u_sense { ".T." } else { ".F." },
                        if *v_sense { ".T." } else { ".F." },
                    ),
                ))
            }
            ProceduralSurfaceDefinition::Replica { source, transform } => {
                let source = self.emit_surface(source.as_str())?;
                let operator = geometry::transformation_operator(&mut self.emitter, *transform);
                Some(
                    self.emitter
                        .emit("SURFACE_REPLICA", &format!("'',{source},{operator}")),
                )
            }
            ProceduralSurfaceDefinition::DegenerateTorus { select_outer } => {
                let SurfaceGeometry::Torus {
                    center,
                    axis,
                    ref_direction,
                    major_radius,
                    minor_radius,
                } = solved
                else {
                    return None;
                };
                let placement =
                    geometry::placement(&mut self.emitter, *center, *axis, *ref_direction);
                Some(self.emitter.emit(
                    "DEGENERATE_TOROIDAL_SURFACE",
                    &format!(
                        "'',{placement},{},{},{}",
                        real(major_radius.abs()),
                        real(minor_radius.abs()),
                        if *select_outer { ".T." } else { ".F." }
                    ),
                ))
            }
            _ => None,
        }
    }

    pub(crate) fn emit_curve(&mut self, curve_id: &str) -> Option<Ref> {
        if let Some(r) = self.curve_refs.get(curve_id) {
            return Some(*r);
        }
        if self.geometry_emission_depth >= 256 || !self.active_curves.insert(curve_id.to_string()) {
            return None;
        }
        self.geometry_emission_depth += 1;
        let result = (|| {
            let geometry = self.curves.get(curve_id)?.geometry.clone();
            let procedural = self
                .procedural_curves
                .get(curve_id)
                .map(|procedural| (procedural.id.0.clone(), procedural.definition.clone()));
            let emitted = procedural.and_then(|(id, definition)| {
                self.emit_procedural_curve(&definition)
                    .map(|reference| (id, reference))
            });
            let r = if let Some((id, reference)) = emitted {
                self.written_procedural_curves.insert(id);
                reference
            } else if let CurveGeometry::Composite {
                segments,
                self_intersect,
            } = &geometry
            {
                let mut segment_refs = Vec::with_capacity(segments.len());
                for segment in segments {
                    let curve = self.emit_curve(segment.curve.as_str())?;
                    let transition = match segment.transition {
                    cadmpeg_ir::geometry::CompositeCurveTransition::Discontinuous => {
                        ".DISCONTINUOUS."
                    }
                    cadmpeg_ir::geometry::CompositeCurveTransition::Continuous => ".CONTINUOUS.",
                    cadmpeg_ir::geometry::CompositeCurveTransition::ContSameGradient => {
                        ".CONTSAMEGRADIENT."
                    }
                    cadmpeg_ir::geometry::CompositeCurveTransition::ContSameGradientSameCurvature => {
                        ".CONTSAMEGRADIENTSAMECURVATURE."
                    }
                };
                    segment_refs.push(self.emitter.emit(
                        "COMPOSITE_CURVE_SEGMENT",
                        &format!(
                            "{transition},{},{curve}",
                            if segment.same_sense { ".T." } else { ".F." }
                        ),
                    ));
                }
                self.emitter.emit(
                    "COMPOSITE_CURVE",
                    &format!(
                        "'',{},{}",
                        refs(&segment_refs),
                        match self_intersect {
                            Some(true) => ".T.",
                            Some(false) => ".F.",
                            None => ".U.",
                        }
                    ),
                )
            } else if !geometry::curve_is_supported(&geometry) {
                return None;
            } else {
                geometry::curve(&mut self.emitter, &geometry)?
            };
            Some(r)
        })();
        self.active_curves.remove(curve_id);
        self.geometry_emission_depth -= 1;
        if let Some(r) = result {
            self.curve_refs.insert(curve_id.to_string(), r);
        }
        result
    }

    fn emit_procedural_curve(&mut self, definition: &ProceduralCurveDefinition) -> Option<Ref> {
        match definition {
            ProceduralCurveDefinition::Subset {
                source,
                parameter_range: [start, end],
                sense,
            } => {
                let source = self.emit_curve(source.as_str())?;
                let (start, end) = if *sense {
                    (*start, *end)
                } else {
                    (*end, *start)
                };
                Some(self.emitter.emit(
                    "TRIMMED_CURVE",
                    &format!(
                        "'',{source},(PARAMETER_VALUE({})),(PARAMETER_VALUE({})),{},.PARAMETER.",
                        real(start),
                        real(end),
                        if *sense { ".T." } else { ".F." }
                    ),
                ))
            }
            ProceduralCurveDefinition::Replica { source, transform } => {
                let source = self.emit_curve(source.as_str())?;
                let operator = geometry::transformation_operator(&mut self.emitter, *transform);
                Some(
                    self.emitter
                        .emit("CURVE_REPLICA", &format!("'',{source},{operator}")),
                )
            }
            ProceduralCurveDefinition::SpatialOffset {
                source,
                distance,
                reference_direction,
                self_intersect,
            } => {
                let source = self.emit_curve(source.as_str())?;
                let direction = geometry::direction(&mut self.emitter, *reference_direction);
                let self_intersect = match self_intersect {
                    Some(true) => ".T.",
                    Some(false) => ".F.",
                    None => ".U.",
                };
                Some(self.emitter.emit(
                    "OFFSET_CURVE_3D",
                    &format!(
                        "'',{source},{},{self_intersect},{direction}",
                        real(*distance)
                    ),
                ))
            }
            _ => None,
        }
    }

    fn pmi_target_ref(&self, target: &PmiTarget) -> Option<Ref> {
        match target {
            PmiTarget::Body { body } => self
                .body_step_refs
                .get(body.as_str())
                .copied()
                .or_else(|| self.body_shape_refs.get(body.as_str()).copied()),
            PmiTarget::Face { face } => self.face_step_refs.get(face.as_str()).copied(),
            PmiTarget::Edge { edge } => self.edge_refs.get(edge.as_str()).copied(),
            PmiTarget::Vertex { vertex } => self.vertex_refs.get(vertex.as_str()).copied(),
            PmiTarget::Product { .. }
            | PmiTarget::Occurrence { .. }
            | PmiTarget::ShapeAspect { .. } => None,
        }
    }

    fn emit_pmi(&mut self, context: Ref) {
        if self.ir.model.pmi.is_empty() || !self.schema.supports_semantic_pmi() {
            return;
        }
        let annotations = self.ir.model.pmi.clone();
        let Some(pds) = self.default_product_definition_shape else {
            return;
        };
        let mut annotation_refs = HashMap::new();
        let mut aspects = HashMap::<String, Ref>::new();
        for annotation in &annotations {
            for target in &annotation.targets {
                let PmiTarget::ShapeAspect { source_id } = target else {
                    continue;
                };
                aspects.entry(source_id.clone()).or_insert_with(|| {
                    self.emitter.emit(
                        "SHAPE_ASPECT",
                        &format!("{},'',{pds},.T.", string(source_id)),
                    )
                });
            }
        }
        let fallback_aspect = self
            .emitter
            .emit("SHAPE_ASPECT", &format!("'PMI target','',{pds},.T."));
        let target_ref = |annotation: &cadmpeg_ir::PmiAnnotation| {
            annotation.targets.iter().find_map(|target| {
                if let PmiTarget::ShapeAspect { source_id } = target {
                    aspects.get(source_id).copied()
                } else {
                    None
                }
            })
        };
        let mut target_items = Vec::new();
        for annotation in &annotations {
            if matches!(&annotation.definition, PmiDefinition::Presentation { .. }) {
                continue;
            }
            for target in &annotation.targets {
                if let Some(target_ref) = self.pmi_target_ref(target) {
                    if !target_items.contains(&target_ref) {
                        target_items.push(target_ref);
                    }
                }
            }
        }
        let target_representation = (!target_items.is_empty()).then(|| {
            self.emitter.emit(
                "SHAPE_REPRESENTATION",
                &format!("'PMI geometric targets',{},{context}", refs(&target_items)),
            )
        });
        let mut targets_exact_by_annotation = HashMap::new();
        for annotation in &annotations {
            let semantic = !matches!(&annotation.definition, PmiDefinition::Presentation { .. });
            let definition = target_ref(annotation).unwrap_or(fallback_aspect);
            let mut exact = true;
            for target in &annotation.targets {
                let Some(identified_item) = self.pmi_target_ref(target) else {
                    if !matches!(target, PmiTarget::ShapeAspect { .. }) {
                        exact = false;
                    }
                    continue;
                };
                let Some(used_representation) = target_representation else {
                    exact = false;
                    continue;
                };
                if semantic {
                    self.emitter.emit(
                        "GEOMETRIC_ITEM_SPECIFIC_USAGE",
                        &format!("'','',{definition},{used_representation},{identified_item}"),
                    );
                } else {
                    exact = false;
                }
            }
            targets_exact_by_annotation.insert(annotation.id.clone(), exact);
        }
        let targets_exact = |annotation: &cadmpeg_ir::PmiAnnotation| {
            targets_exact_by_annotation
                .get(&annotation.id)
                .copied()
                .unwrap_or(false)
        };

        for annotation in &annotations {
            if let PmiDefinition::Datum { identification } = &annotation.definition {
                let datum = self.emitter.emit(
                    "DATUM",
                    &format!(
                        "{},$,{pds},.F.,{}",
                        string(annotation.name.as_deref().unwrap_or("")),
                        string(identification)
                    ),
                );
                annotation_refs.insert(annotation.id.clone(), datum);
                self.written_pmi += usize::from(targets_exact(annotation));
            }
        }
        for annotation in &annotations {
            if let PmiDefinition::DatumSystem { references } = &annotation.definition {
                let mut groups = BTreeMap::<(u32, Option<u32>), Vec<_>>::new();
                for reference in references {
                    groups
                        .entry((reference.precedence, reference.common_group))
                        .or_default()
                        .push(reference);
                }
                let compartments = groups
                    .values()
                    .filter_map(|group| {
                        let datum_refs = group
                            .iter()
                            .map(|reference| annotation_refs.get(&reference.datum).copied())
                            .collect::<Option<Vec<_>>>()?;
                        if group[0].common_group.is_none() && group.len() != 1 {
                            return None;
                        }
                        let (datum, modifiers) = if group[0].common_group.is_some() {
                            let elements = group
                                .iter()
                                .zip(datum_refs)
                                .map(|(reference, datum)| {
                                    let modifiers =
                                        self.emit_datum_modifiers(&reference.modifiers)?;
                                    Some(self.emitter.emit(
                                        "DATUM_REFERENCE_ELEMENT",
                                        &format!("'',$,{pds},.F.,{datum},({modifiers})"),
                                    ))
                                })
                                .collect::<Option<Vec<_>>>()?;
                            (
                                format!("COMMON_DATUM_LIST({})", refs(&elements)),
                                String::new(),
                            )
                        } else {
                            (
                                datum_refs[0].to_string(),
                                self.emit_datum_modifiers(&group[0].modifiers)?,
                            )
                        };
                        Some(self.emitter.emit(
                            "DATUM_REFERENCE_COMPARTMENT",
                            &format!("'',$,{pds},.F.,{datum},({modifiers})"),
                        ))
                    })
                    .collect::<Vec<_>>();
                let complete = compartments.len() == groups.len();
                if compartments.is_empty() {
                    continue;
                }
                let system = self.emitter.emit(
                    "DATUM_SYSTEM",
                    &format!(
                        "{},'',{pds},.F.,{}",
                        string(annotation.name.as_deref().unwrap_or("")),
                        refs(&compartments)
                    ),
                );
                annotation_refs.insert(annotation.id.clone(), system);
                self.written_pmi += usize::from(targets_exact(annotation) && complete);
            }
        }
        for annotation in &annotations {
            match &annotation.definition {
                PmiDefinition::Dimension {
                    dimension,
                    nominal,
                    lower_deviation,
                    upper_deviation,
                    limits_and_fits,
                } => {
                    let aspect = target_ref(annotation).unwrap_or(fallback_aspect);
                    let name = annotation.name.as_deref().unwrap_or("");
                    let (entity, kind_exact) = match dimension {
                        DimensionKind::Size => ("DIMENSIONAL_SIZE", true),
                        DimensionKind::Location => ("DIMENSIONAL_LOCATION", true),
                        DimensionKind::Angular => ("ANGULAR_SIZE", true),
                        // AP242 represents diameter and radius as a
                        // DIMENSIONAL_SIZE whose name identifies the size
                        // category; DIAMETER_SIZE and RADIUS_SIZE are not
                        // entity types.
                        DimensionKind::Diameter | DimensionKind::Radius => {
                            ("DIMENSIONAL_SIZE", true)
                        }
                        DimensionKind::Other(_) => ("DIMENSIONAL_SIZE", false),
                    };
                    let characteristic_name = match dimension {
                        DimensionKind::Diameter => "diameter",
                        DimensionKind::Radius => "radius",
                        _ => name,
                    };
                    let parameters = match dimension {
                        DimensionKind::Location => {
                            format!("{},$,{aspect},{aspect}", string(characteristic_name))
                        }
                        DimensionKind::Angular => {
                            format!("{aspect},{},.SMALL.", string(characteristic_name))
                        }
                        _ => format!("{aspect},{}", string(characteristic_name)),
                    };
                    let characteristic = self.emitter.emit(entity, &parameters);
                    if let Some(value) = nominal {
                        let measure = self.emit_pmi_measure_representation_item(*value, name);
                        let representation = self.emitter.emit(
                            "SHAPE_DIMENSION_REPRESENTATION",
                            &format!("'',({measure}),{context}"),
                        );
                        self.emitter.emit(
                            "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION",
                            &format!("{characteristic},{representation}"),
                        );
                    }
                    if let (Some(lower), Some(upper)) = (lower_deviation, upper_deviation) {
                        let lower = self.emit_pmi_measure(*lower);
                        let upper = self.emit_pmi_measure(*upper);
                        let tolerance = self
                            .emitter
                            .emit("TOLERANCE_VALUE", &format!("{lower},{upper}"));
                        self.emitter.emit(
                            "PLUS_MINUS_TOLERANCE",
                            &format!("{tolerance},{characteristic}"),
                        );
                    }
                    if let Some(fit) = limits_and_fits {
                        let fit = self.emitter.emit(
                            "LIMITS_AND_FITS",
                            &format!(
                                "{},{},{},{}",
                                string(&fit.form_variance),
                                string(&fit.zone_variance),
                                string(&fit.grade),
                                string(&fit.source)
                            ),
                        );
                        self.emitter
                            .emit("PLUS_MINUS_TOLERANCE", &format!("{fit},{characteristic}"));
                    }
                    annotation_refs.insert(annotation.id.clone(), characteristic);
                    let deviations_exact = lower_deviation.is_some() == upper_deviation.is_some();
                    self.written_pmi +=
                        usize::from(targets_exact(annotation) && deviations_exact && kind_exact);
                }
                PmiDefinition::GeometricTolerance {
                    tolerance,
                    magnitude,
                    defined_unit,
                    defined_area_unit,
                    defined_area_second_unit,
                    datum_system,
                    modifiers,
                } => {
                    let kind_exact = !matches!(tolerance, GeometricToleranceKind::Other(value) if value != "geometric_tolerance");
                    let entity = match tolerance {
                        GeometricToleranceKind::Straightness => "STRAIGHTNESS_TOLERANCE",
                        GeometricToleranceKind::Flatness => "FLATNESS_TOLERANCE",
                        GeometricToleranceKind::Roundness => "ROUNDNESS_TOLERANCE",
                        GeometricToleranceKind::Cylindricity => "CYLINDRICITY_TOLERANCE",
                        GeometricToleranceKind::Coaxiality => "COAXIALITY_TOLERANCE",
                        GeometricToleranceKind::LineProfile => "LINE_PROFILE_TOLERANCE",
                        GeometricToleranceKind::SurfaceProfile => "SURFACE_PROFILE_TOLERANCE",
                        GeometricToleranceKind::Angularity => "ANGULARITY_TOLERANCE",
                        GeometricToleranceKind::Perpendicularity => "PERPENDICULARITY_TOLERANCE",
                        GeometricToleranceKind::Parallelism => "PARALLELISM_TOLERANCE",
                        GeometricToleranceKind::Position => "POSITION_TOLERANCE",
                        GeometricToleranceKind::Concentricity => "CONCENTRICITY_TOLERANCE",
                        GeometricToleranceKind::Symmetry => "SYMMETRY_TOLERANCE",
                        GeometricToleranceKind::CircularRunout => "CIRCULAR_RUNOUT_TOLERANCE",
                        GeometricToleranceKind::TotalRunout => "TOTAL_RUNOUT_TOLERANCE",
                        GeometricToleranceKind::Other(_) => continue,
                    };
                    let aspect = target_ref(annotation).unwrap_or(fallback_aspect);
                    let modifier_values = Self::emit_geometric_tolerance_modifiers(modifiers);
                    if !modifiers.is_empty() && modifier_values.is_none() {
                        continue;
                    }
                    let area_unit =
                        Self::emit_geometric_tolerance_area_unit(defined_area_unit.as_deref());
                    if defined_area_unit.is_some() && area_unit.is_none()
                        || defined_area_second_unit.is_some() && area_unit.is_none()
                    {
                        continue;
                    }
                    let datum_ref = datum_system
                        .as_ref()
                        .and_then(|id| annotation_refs.get(id).copied());
                    if datum_system.is_some() && datum_ref.is_none() {
                        continue;
                    }
                    let measure = self.emit_pmi_measure(*magnitude);
                    let tolerance_ref = if datum_ref.is_none()
                        && modifiers.is_empty()
                        && defined_unit.is_none()
                        && area_unit.is_none()
                    {
                        self.emitter.emit(
                            entity,
                            &format!(
                                "{},'',{measure},{aspect}",
                                string(annotation.name.as_deref().unwrap_or(""))
                            ),
                        )
                    } else {
                        let mut parts = vec![format!(
                            "GEOMETRIC_TOLERANCE({},{},{measure},{aspect})",
                            string(annotation.name.as_deref().unwrap_or("")),
                            "''"
                        )];
                        if area_unit.is_some() || defined_unit.is_some() {
                            let defined_unit = defined_unit.as_ref().map_or_else(
                                || "$".into(),
                                |unit| self.emit_pmi_measure(*unit).to_string(),
                            );
                            parts.push(format!(
                                "GEOMETRIC_TOLERANCE_WITH_DEFINED_UNIT({defined_unit})"
                            ));
                        }
                        if let Some(area_unit) = area_unit {
                            let second_unit = defined_area_second_unit.as_ref().map_or_else(
                                || "$".into(),
                                |unit| self.emit_pmi_measure(*unit).to_string(),
                            );
                            parts.push(format!(
                                "GEOMETRIC_TOLERANCE_WITH_DEFINED_AREA_UNIT(.{area_unit}.,{second_unit})"
                            ));
                        }
                        if let Some(datum) = datum_ref {
                            parts.push(format!(
                                "GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE(({datum}))"
                            ));
                        }
                        if let Some(modifiers) = modifier_values {
                            parts
                                .push(format!("GEOMETRIC_TOLERANCE_WITH_MODIFIERS(({modifiers}))"));
                        }
                        parts.push(format!("{entity}()"));
                        self.emitter
                            .emit_raw("GEOMETRIC_TOLERANCE", &format!("({})", parts.join(" ")))
                    };
                    annotation_refs.insert(annotation.id.clone(), tolerance_ref);
                    self.written_pmi += usize::from(targets_exact(annotation) && kind_exact);
                }
                PmiDefinition::Datum { .. }
                | PmiDefinition::DatumSystem { .. }
                | PmiDefinition::DatumTarget { .. }
                | PmiDefinition::Presentation { .. } => {}
            }
        }
        let mut presentation_items = Vec::new();
        let mut presentation_semantics = Vec::new();
        for annotation in &annotations {
            let PmiDefinition::Presentation {
                text,
                placement,
                semantics,
            } = &annotation.definition
            else {
                continue;
            };
            let (Some(text), Some(placement)) = (text.as_deref(), placement.as_ref()) else {
                continue;
            };
            if !annotation.targets.is_empty() || !is_rigid_transform(&placement.rows) {
                continue;
            }
            let rows = placement.rows;
            let placement = geometry::placement(
                &mut self.emitter,
                cadmpeg_ir::math::Point3::new(rows[0][3], rows[1][3], rows[2][3]),
                cadmpeg_ir::math::Vector3::new(rows[0][2], rows[1][2], rows[2][2]),
                cadmpeg_ir::math::Vector3::new(rows[0][0], rows[1][0], rows[2][0]),
            );
            let font_source = self.emitter.emit("EXTERNAL_SOURCE", "'ISO 3098'");
            let font = self.emitter.emit(
                "EXTERNALLY_DEFINED_TEXT_FONT",
                &format!("IDENTIFIER('ISO 3098'),{font_source}"),
            );
            let literal = self.emitter.emit(
                "TEXT_LITERAL",
                &format!("{},{placement},'left',.RIGHT.,{font}", string(text)),
            );
            let semantic_refs = semantics
                .iter()
                .filter_map(|semantic| annotation_refs.get(semantic).copied())
                .collect::<Vec<_>>();
            if semantic_refs.len() != semantics.len() {
                continue;
            }
            let style = self
                .emitter
                .emit("PRESENTATION_STYLE_ASSIGNMENT", "(.NULL.)");
            let occurrence = self.emitter.emit(
                "ANNOTATION_TEXT_OCCURRENCE",
                &format!(
                    "{},{},{literal}",
                    string(annotation.name.as_deref().unwrap_or("")),
                    refs(&[style])
                ),
            );
            presentation_items.push(occurrence);
            presentation_semantics.push((occurrence, semantic_refs));
            annotation_refs.insert(annotation.id.clone(), occurrence);
            self.written_pmi += 1;
        }
        if !presentation_items.is_empty() {
            let model = self.emitter.emit(
                "DRAUGHTING_MODEL",
                &format!(
                    "'PMI presentation',{}, {context}",
                    refs(&presentation_items)
                ),
            );
            for (occurrence, semantics) in presentation_semantics {
                for semantic in semantics {
                    self.emitter.emit(
                        "DRAUGHTING_MODEL_ITEM_ASSOCIATION",
                        &format!("'','',{semantic},{model},{occurrence}"),
                    );
                }
            }
        }
        for (annotation, reference) in annotation_refs {
            self.pmi_step_refs.insert(annotation.0, reference);
        }
    }

    fn emit_datum_modifiers(&mut self, source: &[String]) -> Option<String> {
        const SIMPLE: &[&str] = &[
            "free_state",
            "basic",
            "translation",
            "least_material_requirement",
            "maximum_material_requirement",
            "point",
            "line",
            "plane",
            "orientation",
            "any_cross_section",
            "any_longitudinal_section",
            "contacting_feature",
            "distance_variable",
            "degree_of_freedom_constraint_x",
            "degree_of_freedom_constraint_y",
            "degree_of_freedom_constraint_z",
            "degree_of_freedom_constraint_u",
            "degree_of_freedom_constraint_v",
            "degree_of_freedom_constraint_w",
            "minor_diameter",
            "major_diameter",
            "pitch_diameter",
        ];
        const WITH_VALUE: &[&str] = &[
            "circular_or_cylindrical",
            "spherical",
            "distance",
            "projected",
        ];
        enum Modifier {
            Simple(String),
            WithValue { kind: String, value: f64 },
        }
        let parsed = source
            .iter()
            .map(|modifier| {
                if let Some((kind, value)) = modifier.split_once(':') {
                    let kind = kind.to_ascii_lowercase();
                    let value = value.parse::<f64>().ok()?;
                    if !value.is_finite() {
                        return None;
                    }
                    WITH_VALUE
                        .contains(&kind.as_str())
                        .then_some(Modifier::WithValue { kind, value })
                } else {
                    let modifier = modifier.to_ascii_lowercase();
                    SIMPLE
                        .contains(&modifier.as_str())
                        .then_some(Modifier::Simple(modifier))
                }
            })
            .collect::<Option<Vec<_>>>()?;
        let mut modifiers = Vec::with_capacity(source.len());
        for modifier in parsed {
            match modifier {
                Modifier::WithValue { kind, value } => {
                    let measure = self.emit_pmi_measure(cadmpeg_ir::PmiValue {
                        value,
                        quantity: cadmpeg_ir::PmiQuantity::Length,
                    });
                    modifiers.push(
                        self.emitter
                            .emit(
                                "DATUM_REFERENCE_MODIFIER_WITH_VALUE",
                                &format!(".{}.,{measure}", kind.to_ascii_uppercase()),
                            )
                            .to_string(),
                    );
                }
                Modifier::Simple(modifier) => {
                    modifiers.push(format!(".{}.", modifier.to_ascii_uppercase()));
                }
            }
        }
        Some(modifiers.join(","))
    }

    fn emit_geometric_tolerance_modifiers(source: &[String]) -> Option<String> {
        const SUPPORTED: &[&str] = &[
            "any_cross_section",
            "associated_least_square_feature",
            "associated_maximum_inscribed_feature",
            "associated_minimum_inscribed_feature",
            "associated_minmax_feature",
            "associated_tangent_feature",
            "circle_a",
            "common_zone",
            "continuous_features",
            "derived_feature",
            "each_element",
            "each_radial_element",
            "free_state",
            "individually",
            "least_material_requirement",
            "line_element",
            "major_diameter",
            "maximum_material_requirement",
            "minor_diameter",
            "not_convex",
            "offset_zone",
            "peak_height",
            "pitch_diameter",
            "reciprocity_requirement",
            "reference_least_square_feature_with_external_material_constraint",
            "reference_least_square_feature_with_internal_material_constraint",
            "reference_least_square_feature_without_constraint",
            "reference_maximum_inscribed_feature",
            "reference_minimax_feature_with_external_material_constraint",
            "reference_minimax_feature_with_internal_material_constraint",
            "reference_minimax_feature_without_constraint",
            "reference_minimum_circumscribed_feature",
            "separate_requirement",
            "separate_zones",
            "standard_deviation",
            "statistical_tolerance",
            "stock",
            "tangent_plane",
            "total_range_deviations",
            "united_feature",
            "unspecified_angular_tolerance_zone_offset",
            "unspecified_linear_tolerance_zone_offset",
            "valley_depth",
            "variable_angle",
        ];
        source
            .iter()
            .map(|modifier| {
                let normalized = modifier.to_ascii_lowercase();
                SUPPORTED
                    .contains(&normalized.as_str())
                    .then(|| format!(".{}.", normalized.to_ascii_uppercase()))
            })
            .collect::<Option<Vec<_>>>()
            .map(|modifiers| modifiers.join(","))
    }

    fn emit_geometric_tolerance_area_unit(source: Option<&str>) -> Option<String> {
        let source = source?;
        matches!(
            source.to_ascii_lowercase().as_str(),
            "circular" | "square" | "rectangular" | "cylindrical" | "spherical"
        )
        .then(|| source.to_ascii_lowercase())
    }

    fn emit_pmi_measure(&mut self, value: cadmpeg_ir::PmiValue) -> Ref {
        let (entity, typed, unit) = match value.quantity {
            PmiQuantity::Length => (
                "LENGTH_MEASURE_WITH_UNIT",
                "LENGTH_MEASURE",
                self.emit_length_unit(),
            ),
            PmiQuantity::Angle => (
                "PLANE_ANGLE_MEASURE_WITH_UNIT",
                "PLANE_ANGLE_MEASURE",
                self.emit_angle_unit(),
            ),
            PmiQuantity::Ratio => ("MEASURE_WITH_UNIT", "RATIO_MEASURE", self.emit_ratio_unit()),
        };
        self.emitter
            .emit(entity, &format!("{typed}({}),{unit}", real(value.value)))
    }

    fn emit_pmi_measure_representation_item(
        &mut self,
        value: cadmpeg_ir::PmiValue,
        name: &str,
    ) -> Ref {
        let (typed, unit) = match value.quantity {
            PmiQuantity::Length => ("LENGTH_MEASURE", self.emit_length_unit()),
            PmiQuantity::Angle => ("PLANE_ANGLE_MEASURE", self.emit_angle_unit()),
            PmiQuantity::Ratio => ("RATIO_MEASURE", self.emit_ratio_unit()),
        };
        self.emitter.emit(
            "MEASURE_REPRESENTATION_ITEM",
            &format!("{},{typed}({}),{unit}", string(name), real(value.value)),
        )
    }

    fn note_unrepresented(&mut self) {
        let mut referenced_shells = BTreeSet::new();
        let mut referenced_faces = BTreeSet::new();
        let mut referenced_loops = BTreeSet::new();
        let mut referenced_coedges = BTreeSet::new();
        let mut referenced_edges = BTreeSet::new();
        let mut referenced_vertices = BTreeSet::new();
        for region in &self.ir.model.regions {
            for shell_id in &region.shells {
                if !referenced_shells.insert(shell_id.as_str()) {
                    continue;
                }
                let Some(shell) = self.shells.get(shell_id.as_str()).copied() else {
                    continue;
                };
                for edge_id in &shell.wire_edges {
                    referenced_edges.insert(edge_id.as_str());
                    if let Some(edge) = self.edges.get(edge_id.as_str()).copied() {
                        referenced_vertices.insert(edge.start.as_str());
                        referenced_vertices.insert(edge.end.as_str());
                    }
                }
                referenced_vertices.extend(
                    shell
                        .free_vertices
                        .iter()
                        .map(cadmpeg_ir::ids::VertexId::as_str),
                );
                for face_id in &shell.faces {
                    if !referenced_faces.insert(face_id.as_str()) {
                        continue;
                    }
                    let Some(face) = self.faces.get(face_id.as_str()).copied() else {
                        continue;
                    };
                    for loop_id in &face.loops {
                        if !referenced_loops.insert(loop_id.as_str()) {
                            continue;
                        }
                        let Some(loop_) = self.loops.get(loop_id.as_str()).copied() else {
                            continue;
                        };
                        referenced_vertices
                            .extend(loop_.vertex_uses.iter().map(|use_| use_.vertex.as_str()));
                        for vertex_use in &loop_.vertex_uses {
                            if let Some(after) = &vertex_use.after {
                                referenced_coedges.insert(after.as_str());
                            }
                        }
                        for coedge_id in &loop_.coedges {
                            if !referenced_coedges.insert(coedge_id.as_str()) {
                                continue;
                            }
                            let Some(coedge) = self.coedges.get(coedge_id.as_str()).copied() else {
                                continue;
                            };
                            referenced_edges.insert(coedge.edge.as_str());
                            if let Some(edge) = self.edges.get(coedge.edge.as_str()).copied() {
                                referenced_vertices.insert(edge.start.as_str());
                                referenced_vertices.insert(edge.end.as_str());
                            }
                        }
                    }
                }
            }
        }
        let omitted_bodies = self
            .ir
            .model
            .bodies
            .iter()
            .filter(|body| !self.body_shape_refs.contains_key(body.id.as_str()))
            .count();
        let omitted_shells = self
            .ir
            .model
            .shells
            .iter()
            .filter(|shell| !referenced_shells.contains(shell.id.as_str()))
            .count();
        let omitted_faces = self
            .ir
            .model
            .faces
            .iter()
            .filter(|face| !referenced_faces.contains(face.id.as_str()))
            .count();
        let omitted_loops = self
            .ir
            .model
            .loops
            .iter()
            .filter(|loop_| !referenced_loops.contains(loop_.id.as_str()))
            .count();
        let omitted_coedges = self
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| !referenced_coedges.contains(coedge.id.as_str()))
            .count();
        let omitted_edges = self
            .ir
            .model
            .edges
            .iter()
            .filter(|edge| !referenced_edges.contains(edge.id.as_str()))
            .count();
        let omitted_vertices = self
            .ir
            .model
            .vertices
            .iter()
            .filter(|vertex| !referenced_vertices.contains(vertex.id.as_str()))
            .count();
        let omitted_topology = [
            ("body", omitted_bodies),
            ("shell", omitted_shells),
            ("face", omitted_faces),
            ("loop", omitted_loops),
            ("coedge", omitted_coedges),
            ("edge", omitted_edges),
            ("vertex", omitted_vertices),
        ];
        if omitted_topology.iter().any(|(_, count)| *count > 0) {
            let details = omitted_topology
                .iter()
                .filter(|(_, count)| *count > 0)
                .map(|(kind, count)| format!("{count} {kind}(s)"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::TopologyUnreachableFromRegion,
                format!("topology not reachable from any emitted region shape item: {details}"),
            );
        }
        let nonstandard_analytic_surfaces = self
            .ir
            .model
            .surfaces
            .iter()
            .filter(|surface| match &surface.geometry {
                SurfaceGeometry::Sphere { radius, .. } => *radius < 0.0,
                SurfaceGeometry::Torus {
                    major_radius,
                    minor_radius,
                    ..
                } => {
                    *major_radius < 0.0
                        || *minor_radius < 0.0
                        || (minor_radius.abs() > major_radius.abs()
                            && !self.ir.model.procedural_surfaces.iter().any(|procedural| {
                                procedural.surface == surface.id
                                    && self.written_procedural_surfaces.contains(&procedural.id.0)
                                    && matches!(
                                        procedural.definition,
                                        ProceduralSurfaceDefinition::DegenerateTorus { .. }
                                    )
                            }))
                }
                _ => false,
            })
            .count();
        if nonstandard_analytic_surfaces > 0 {
            self.loss(
                StepLossCode::AnalyticSurfaceNormalized,
                format!(
                    "{nonstandard_analytic_surfaces} signed or self-intersecting analytic \
                     surface(s) were normalized to positive STEP radii"
                ),
            );
        }
        let elliptical_cones = self
            .ir
            .model
            .surfaces
            .iter()
            .filter(|surface| {
                matches!(
                    surface.geometry,
                    SurfaceGeometry::Cone { ratio, .. } if ratio != 1.0
                )
            })
            .count();
        if elliptical_cones > 0 {
            self.loss(
                StepLossCode::EllipticalConeReduced,
                format!(
                    "{elliptical_cones} elliptical cone surface(s) were reduced to circular STEP CONICAL_SURFACE carriers"
                ),
            );
        }
        if !self.curveless_edges.is_empty() {
            self.loss(
                StepLossCode::CurvelessEdgeOmitted,
                format!(
                    "{} edge(s) have no typed 3D curve or carry a STEP-unsupported transform and were omitted from \
                     their edge loops (STEP EDGE_CURVE requires a 3D curve)",
                    self.curveless_edges.len()
                ),
            );
        }
        if !self.unknown_surface_faces.is_empty() {
            self.loss(
                StepLossCode::UnknownSurfaceFaceOmitted,
                format!(
                    "{} face(s) rest on an unknown or STEP-unsupported surface and were omitted \
                     from the STEP shell (an ADVANCED_FACE requires a surface); their \
                     topology remains in the IR",
                    self.unknown_surface_faces.len()
                ),
            );
        }
        if !self.unwritten_geometry_carriers.is_empty() {
            let carriers = self
                .unwritten_geometry_carriers
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::GeometryCarrierNotWritten,
                format!(
                    "{} geometry carrier(s) were not written: {carriers}",
                    self.unwritten_geometry_carriers.len()
                ),
            );
        }
        if !self.unwritten_pcurve_carriers.is_empty() {
            let pcurves = self
                .unwritten_pcurve_carriers
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::PcurveCarrierUnwritable,
                format!(
                    "{} coedge pcurve carrier(s) use geometry or surface references that were not writable: {pcurves}",
                    self.unwritten_pcurve_carriers.len()
                ),
            );
        }
        if !self.missing_parent_products.is_empty() {
            let occurrences = self
                .missing_parent_products
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::AssemblyOccurrenceOmittedNoParentProduct,
                format!(
                    "{} assembly occurrence(s) were omitted because their parent has no local product definition: {occurrences}",
                    self.missing_parent_products.len()
                ),
            );
        }
        if !self.empty_regions.is_empty() {
            let regions = self
                .empty_regions
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::RegionNoShellList,
                format!(
                    "{} region(s) have no shell list and were not written to STEP: {regions}",
                    self.empty_regions.len()
                ),
            );
        }
        if !self.empty_wire_regions.is_empty() {
            let regions = self
                .empty_wire_regions
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::WireRegionNoConnectedEdgeSet,
                format!(
                    "{} wire region(s) had no writable connected edge set and were not written to STEP: {regions}",
                    self.empty_wire_regions.len()
                ),
            );
        }
        if !self.missing_wire_shells.is_empty() {
            let shells = self
                .missing_wire_shells
                .iter()
                .map(|(region, shell)| format!("'{region}' -> '{shell}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::WireRegionMissingShell,
                format!(
                    "{} wire region/shell relation(s) referenced missing shell records: {shells}",
                    self.missing_wire_shells.len()
                ),
            );
        }
        if !self.hidden_bodies_without_items.is_empty() {
            let bodies = self
                .hidden_bodies_without_items
                .iter()
                .map(|id| format!("'{id}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::HiddenBodyOmitted,
                format!(
                    "{} hidden body/bodies had no emitted STEP item and were omitted from INVISIBILITY: {bodies}",
                    self.hidden_bodies_without_items.len()
                ),
            );
        }
        if !self.dangling_appearance_bindings.is_empty() {
            let bindings = self
                .dangling_appearance_bindings
                .iter()
                .map(|(binding, appearance)| format!("'{binding}' -> '{appearance}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::AppearanceBindingMissingAsset,
                format!(
                    "{} appearance binding(s) reference missing appearance assets and were not written: {bindings}",
                    self.dangling_appearance_bindings.len()
                ),
            );
        }
        if !self.colorless_appearance_bindings.is_empty() {
            let bindings = self
                .colorless_appearance_bindings
                .iter()
                .map(|(binding, appearance)| format!("'{binding}' -> '{appearance}'"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::AppearanceBindingNoBaseColor,
                format!(
                    "{} appearance binding(s) reference appearances without a base color and were not written: {bindings}",
                    self.colorless_appearance_bindings.len()
                ),
            );
        }
        let missing_pcurve_count = self
            .ir
            .model
            .coedges
            .iter()
            .flat_map(|coedge| &coedge.pcurves)
            .filter(|use_| !self.pcurves.contains_key(use_.pcurve.as_str()))
            .count();
        if missing_pcurve_count > 0 {
            self.loss(
                StepLossCode::CoedgePcurveNoGeometry,
                format!(
                    "{missing_pcurve_count} coedge pcurve reference(s) have no geometry and were not written"
                ),
            );
        }
        let reduced_pcurve_count = self
            .ir
            .model
            .coedges
            .iter()
            .flat_map(|coedge| &coedge.pcurves)
            .filter_map(|use_| self.pcurves.get(use_.pcurve.as_str()))
            .filter(|pcurve| {
                pcurve.wrapper_reversed.is_some()
                    || pcurve.native_tail_flags.is_some()
                    || pcurve.parameter_range.is_some()
                    || pcurve.fit_tolerance.is_some()
            })
            .count();
        if reduced_pcurve_count > 0 {
            self.loss(
                StepLossCode::CoedgePcurveNativeMetadata,
                format!(
                    "{reduced_pcurve_count} emitted coedge pcurve(s) carry native-only metadata not represented in STEP"
                ),
            );
        }
        let pcurve_use_metadata_count = self
            .ir
            .model
            .coedges
            .iter()
            .flat_map(|coedge| &coedge.pcurves)
            .chain(
                self.ir
                    .model
                    .loops
                    .iter()
                    .flat_map(|loop_| &loop_.vertex_uses)
                    .flat_map(|vertex_use| &vertex_use.pcurves),
            )
            .filter(|use_| use_.isoparametric.is_some() || use_.parameter_range.is_some())
            .count();
        if pcurve_use_metadata_count > 0 {
            self.loss(
                StepLossCode::PcurveUseNativeMetadata,
                format!(
                    "{pcurve_use_metadata_count} pcurve use(s) carry native-only parameter metadata not represented in STEP"
                ),
            );
        }
        let coedge_use_curve_metadata_count = self
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| {
                coedge.use_curve.is_some() || coedge.use_curve_parameter_range.is_some()
            })
            .count();
        if coedge_use_curve_metadata_count > 0 {
            self.loss(
                StepLossCode::CoedgeUseCurveNotRepresented,
                format!(
                    "{coedge_use_curve_metadata_count} coedge-local 3D curve use(s) were not represented in STEP"
                ),
            );
        }
        // EDGE_CURVE carries its bounded domain through the two vertex
        // endpoints. The IR parameter interval is a derived parameterization
        // detail, not a separate STEP attribute, so it is not a loss here.
        let topology_metadata = [
            (
                "face tolerance",
                self.ir
                    .model
                    .faces
                    .iter()
                    .filter(|face| face.tolerance.is_some())
                    .count(),
            ),
            (
                "edge tolerance",
                self.ir
                    .model
                    .edges
                    .iter()
                    .filter(|edge| edge.tolerance.is_some())
                    .count(),
            ),
            (
                "vertex tolerance",
                self.ir
                    .model
                    .vertices
                    .iter()
                    .filter(|vertex| vertex.tolerance.is_some())
                    .count(),
            ),
        ];
        let topology_metadata_count = topology_metadata
            .iter()
            .map(|(_, count)| count)
            .sum::<usize>();
        if topology_metadata_count > 0 {
            let details = topology_metadata
                .iter()
                .filter(|(_, count)| *count > 0)
                .map(|(kind, count)| format!("{kind}={count}"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::TopologyMetadataNotRepresented,
                format!(
                    "{topology_metadata_count} topology metadata value(s) were not represented in STEP: {details}"
                ),
            );
        }
        if !self.ir.model.subds.is_empty() {
            self.loss(
                StepLossCode::SubdOmitted,
                format!(
                    "{} subdivision surface(s) were omitted because this STEP writer \
                     does not encode SubD control cages",
                    self.ir.model.subds.len()
                ),
            );
        }
        let design_arenas = [
            ("feature", self.ir.model.features.len()),
            (
                "feature input topology",
                self.ir.model.feature_input_topologies.len(),
            ),
            (
                "feature result topology",
                self.ir.model.feature_result_topologies.len(),
            ),
            ("configuration", self.ir.model.configurations.len()),
            ("parameter", self.ir.model.parameters.len()),
            ("sketch", self.ir.model.sketches.len()),
            ("sketch entity", self.ir.model.sketch_entities.len()),
            ("sketch constraint", self.ir.model.sketch_constraints.len()),
            ("spatial sketch", self.ir.model.spatial_sketches.len()),
            (
                "spatial sketch entity",
                self.ir.model.spatial_sketch_entities.len(),
            ),
            (
                "spatial sketch constraint",
                self.ir.model.spatial_sketch_constraints.len(),
            ),
            ("spreadsheet", self.ir.model.spreadsheets.len()),
        ];
        let design_record_count = design_arenas.iter().map(|(_, count)| count).sum::<usize>();
        if design_record_count > 0 {
            let details = design_arenas
                .iter()
                .filter(|(_, count)| *count > 0)
                .map(|(kind, count)| format!("{kind}={count}"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::ParametricDesignRecordsOmitted,
                format!(
                    "{design_record_count} parametric/design record(s) were not represented in STEP: {details}"
                ),
            );
        }
        let presentation_arenas = [
            ("drawing", self.ir.model.drawings.len()),
            (
                "presentation document",
                self.ir.model.presentation_documents.len(),
            ),
            ("view presentation", self.ir.model.view_presentations.len()),
        ];
        let presentation_record_count = presentation_arenas
            .iter()
            .map(|(_, count)| count)
            .sum::<usize>();
        if presentation_record_count > 0 {
            let details = presentation_arenas
                .iter()
                .filter(|(_, count)| *count > 0)
                .map(|(kind, count)| format!("{kind}={count}"))
                .collect::<Vec<_>>()
                .join(", ");
            self.loss(
                StepLossCode::DrawingPresentationRecordsOmitted,
                format!(
                    "{presentation_record_count} drawing/presentation record(s) were not represented in STEP: {details}"
                ),
            );
        }
        if !self.ir.model.semantic_annotations.is_empty() {
            self.loss(
                StepLossCode::SemanticAnnotationOmitted,
                format!(
                    "{} semantic annotation(s) were not represented in STEP",
                    self.ir.model.semantic_annotations.len()
                ),
            );
        }
        if !self.ir.model.assets.is_empty() {
            self.loss(
                StepLossCode::DocumentAssetOmitted,
                format!(
                    "{} document asset(s) were not represented in STEP",
                    self.ir.model.assets.len()
                ),
            );
        }
        if !self.ir.model.assembly_joints.is_empty() {
            self.loss(
                StepLossCode::AssemblyJointOmitted,
                format!(
                    "{} assembly joint(s) were not represented in STEP",
                    self.ir.model.assembly_joints.len()
                ),
            );
        }
        let non_part_products = self
            .ir
            .model
            .product_definitions
            .iter()
            .filter(|product| {
                !matches!(
                    &product.kind,
                    cadmpeg_ir::products::ProductDefinitionKind::Part
                )
            })
            .count();
        if non_part_products > 0 {
            self.loss(
                StepLossCode::ProductNonPartKind,
                format!(
                    "{non_part_products} product definition(s) use a non-part kind not represented in STEP"
                ),
            );
        }
        let bom_property_count = self
            .ir
            .model
            .product_definitions
            .iter()
            .map(|product| product.bom_properties.len())
            .sum::<usize>();
        if bom_property_count > 0 {
            self.loss(
                StepLossCode::ProductBomPropertyOmitted,
                format!(
                    "{bom_property_count} product BOM property value(s) were not represented in STEP"
                ),
            );
        }
        let external_occurrences = self
            .ir
            .model
            .occurrences
            .iter()
            .filter(|occurrence| {
                matches!(
                    &occurrence.prototype,
                    cadmpeg_ir::products::PrototypeReference::External { .. }
                )
            })
            .count();
        if external_occurrences > 0 {
            self.loss(
                StepLossCode::OccurrenceExternalProduct,
                format!(
                    "{external_occurrences} occurrence(s) reference external product definitions and were not represented in STEP"
                ),
            );
        }
        let unresolved_occurrences = self
            .ir
            .model
            .occurrences
            .iter()
            .filter(|occurrence| {
                matches!(
                    &occurrence.prototype,
                    cadmpeg_ir::products::PrototypeReference::Unresolved
                )
            })
            .count();
        if unresolved_occurrences > 0 {
            self.loss(
                StepLossCode::OccurrenceNoWritableProduct,
                format!(
                    "{unresolved_occurrences} occurrence(s) have no writable product definition"
                ),
            );
        }
        let occurrence_metadata = self
            .ir
            .model
            .occurrences
            .iter()
            .map(|occurrence| {
                usize::from(!occurrence.linked_subelements.is_empty())
                    + usize::from(occurrence.visible.is_some())
                    + usize::from(occurrence.element_component.is_some())
                    + usize::from(occurrence.claim_child.is_some())
                    + usize::from(occurrence.copy_on_change.is_some())
                    + usize::from(occurrence.copy_on_change_source.is_some())
                    + usize::from(occurrence.copy_on_change_group.is_some())
                    + usize::from(occurrence.copy_on_change_touched.is_some())
            })
            .sum::<usize>();
        if occurrence_metadata > 0 {
            self.loss(
                StepLossCode::OccurrenceMetadataOmitted,
                format!(
                    "{occurrence_metadata} occurrence metadata value(s) were not represented in STEP"
                ),
            );
        }
        let unwritten_pmi = self.ir.model.pmi.len().saturating_sub(self.written_pmi);
        if unwritten_pmi > 0 {
            self.loss(
                StepLossCode::PmiAnnotationNotWritten,
                format!("{unwritten_pmi} PMI annotation(s) were not written to STEP"),
            );
        }
        // STEP-native source associations identify records already represented
        // by the writer's own STEP graph. They are not lossy foreign-source
        // metadata. Keep strict-mode rejection for associations from other
        // codecs, which this writer cannot reproduce.
        let source_object_count = self
            .ir
            .model
            .surfaces
            .iter()
            .filter(|surface| {
                surface
                    .source_object
                    .as_ref()
                    .is_some_and(|source| source.format != "step")
            })
            .count()
            + self
                .ir
                .model
                .curves
                .iter()
                .filter(|curve| {
                    curve
                        .source_object
                        .as_ref()
                        .is_some_and(|source| source.format != "step")
                })
                .count()
            + self
                .ir
                .model
                .subds
                .iter()
                .filter(|subd| {
                    subd.source_object
                        .as_ref()
                        .is_some_and(|source| source.format != "step")
                })
                .count()
            + self
                .ir
                .model
                .tessellations
                .iter()
                .filter(|tessellation| {
                    tessellation
                        .source_object
                        .as_ref()
                        .is_some_and(|source| source.format != "step")
                })
                .count();
        let source_object_count = source_object_count
            + self
                .ir
                .model
                .points
                .iter()
                .filter(|point| {
                    point
                        .source_object
                        .as_ref()
                        .is_some_and(|source| source.format != "step")
                })
                .count();
        if source_object_count > 0 {
            self.loss(
                StepLossCode::SourceAssociationOmitted,
                format!(
                    "{source_object_count} source-object association(s) were not represented in STEP"
                ),
            );
        }
        let unknown_count = self
            .ir
            .native
            .loss_counts()
            .into_iter()
            .filter(|count| count.kind == "unknowns")
            .map(|count| count.count)
            .sum::<usize>();
        if unknown_count > 0 {
            self.loss(
                StepLossCode::PassthroughRecordOmitted,
                format!("{unknown_count} uninterpreted passthrough record(s) were not represented in STEP"),
            );
        }
        if self.unstyled_colors > 0 {
            self.loss(
                StepLossCode::DisplayColorUnstyled,
                format!(
                    "{} display color(s) had no emitted STEP item and were not written \
                     to STEP presentation",
                    self.unstyled_colors
                ),
            );
        }
        let lossy_appearances = self
            .ir
            .model
            .appearances
            .iter()
            .filter(|appearance| {
                let bindings = self
                    .ir
                    .model
                    .appearance_bindings
                    .iter()
                    .filter(|binding| binding.appearance == appearance.id)
                    .collect::<Vec<_>>();
                appearance.asset_guid.is_some()
                    || appearance.visual_guid.is_some()
                    || appearance.physical_token.is_some()
                    || appearance
                        .schema
                        .as_deref()
                        .is_some_and(|schema| schema != "step_surface_style")
                    || appearance.category.is_some()
                    || !appearance.properties.is_empty()
                    || appearance.base_color.is_none_or(|color| color.a != 1.0)
                    || bindings.is_empty()
                    || bindings
                        .iter()
                        .any(|binding| !self.written_appearance_bindings.contains(&binding.id))
            })
            .count();
        if lossy_appearances > 0 {
            self.loss(
                StepLossCode::AppearanceReducedToBaseColor,
                format!(
                    "{lossy_appearances} appearance asset(s) were reduced to STYLED_ITEM base colors; \
                     schemas, textures, and shader properties were not written to STEP"
                ),
            );
        }
        let lossy_binding_metadata = self
            .ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| binding.object_type.is_some() || !binding.channels.is_empty())
            .count();
        if lossy_binding_metadata > 0 {
            self.loss(
                StepLossCode::AppearanceBindingMetadataReduced,
                format!(
                    "{lossy_binding_metadata} appearance binding(s) carry source object or channel metadata not represented in STEP"
                ),
            );
        }
        if !self.ir.model.attributes.is_empty() {
            self.loss(
                StepLossCode::SourceAttributeNotWritten,
                format!(
                    "{} source attribute record(s) were not written to STEP",
                    self.ir.model.attributes.len()
                ),
            );
        }
        let procedural_surface_count = self
            .ir
            .model
            .procedural_surfaces
            .iter()
            .filter(|procedural| !self.written_procedural_surfaces.contains(&procedural.id.0))
            .count();
        let procedural_curve_count = self
            .ir
            .model
            .procedural_curves
            .iter()
            .filter(|procedural| !self.written_procedural_curves.contains(&procedural.id.0))
            .count();
        if procedural_surface_count > 0 || procedural_curve_count > 0 {
            self.loss(
                StepLossCode::ProceduralReducedToCarrier,
                format!(
                    "{procedural_surface_count} procedural surface definition(s) and {procedural_curve_count} procedural curve definition(s) were reduced to their solved STEP carriers"
                ),
            );
        }
        let source_native_records: usize = self
            .ir
            .native
            .loss_counts()
            .iter()
            .filter(|loss| loss.kind != "unknowns")
            .map(|loss| loss.count)
            .sum();
        if source_native_records > 0 {
            self.loss(
                StepLossCode::SourceNativeRecordOmitted,
                format!(
                    "{source_native_records} source-native record(s) were not represented in STEP"
                ),
            );
        }
    }

    fn finish_report(&self) -> ExportReport {
        ExportReport {
            format: "step".into(),
            census: cadmpeg_ir::EntityCensus {
                basis: cadmpeg_ir::CensusBasis::TargetRecords,
                counts: self.emitter.counts(),
            },
            fidelity: FidelityResolution::NotProvided,
            // STEP is a target-only format here: every record is emitted from
            // the neutral IR, with no source container to replay or patch.
            write_path: WritePath::Synthesized,
            losses: self.losses.clone(),
            notes: self.notes.clone(),
        }
    }
}

fn is_identity(rows: &[[f64; 4]; 4]) -> bool {
    for (i, row) in rows.iter().enumerate() {
        for (j, &v) in row.iter().enumerate() {
            let expect = if i == j { 1.0 } else { 0.0 };
            if (v - expect).abs() > EPS_IDENTITY {
                return false;
            }
        }
    }
    true
}

pub(crate) fn is_rigid_transform(rows: &[[f64; 4]; 4]) -> bool {
    cadmpeg_ir::transform::Transform { rows: *rows }.is_proper_rigid()
}
