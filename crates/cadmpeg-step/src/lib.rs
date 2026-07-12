// SPDX-License-Identifier: Apache-2.0
//! Writes [`cadmpeg_ir::CadIr`] documents as ISO 10303-21 STEP AP214 exchange
//! files.
//!
//! [`write_step`] emits a Part 21 `AUTOMOTIVE_DESIGN` file containing an
//! `ADVANCED_BREP_SHAPE_REPRESENTATION`. It writes the product-definition and
//! representation-context records needed to connect the file metadata to the
//! boundary representation. Each reachable IR region becomes a
//! `MANIFOLD_SOLID_BREP` or, when the region has inner shells, a
//! `BREP_WITH_VOIDS`.
//!
//! # Export workflow
//!
//! Construct or decode a [`cadmpeg_ir::CadIr`], choose the header metadata in
//! [`StepWriteOptions`], then write to any [`std::io::Write`] sink:
//!
//! ```
//! use cadmpeg_ir::examples::unit_cube;
//! use cadmpeg_step::{write_step, StepWriteOptions};
//!
//! let ir = unit_cube();
//! let mut bytes = Vec::new();
//! let report = write_step(&ir, &mut bytes, &StepWriteOptions::default())?;
//!
//! assert!(bytes.starts_with(b"ISO-10303-21;"));
//! assert!(report.total_entities > 0);
//! # Ok::<(), cadmpeg_step::StepError>(())
//! ```
//!
//! Review [`cadmpeg_ir::ExportReport::losses`] before retaining the output. Export continues
//! when an IR fact has no representation in this writer. Unknown-surface faces
//! and edges without typed 3D curves are omitted; pcurves, source attributes,
//! passthrough records, and parametric history are reported rather than
//! emitted. Display colors on bodies and faces (direct or through appearance
//! bindings) become per-face `STYLED_ITEM` presentation on `ADVANCED_FACE`: a
//! body color is pushed down onto each of its faces so that viewers reading
//! only face colors still show it. Other appearance data is reduced to those
//! base colors. Body transforms remain unapplied, leaving affected coordinates
//! in body-local space.
//!
//! Coordinates are emitted unchanged under a millimetre length-unit context.
//! Callers must convert non-millimetre geometry before export. Analytic curves
//! and surfaces map to their corresponding STEP carriers. Rational and
//! non-rational NURBS use the `*_WITH_KNOTS` entities.
//!
//! [`StepError`] represents output-sink failures. Since the writer streams the
//! header and DATA section, such a failure can leave partial output.

mod geometry;
mod writer;

use std::collections::{BTreeSet, HashMap};
use std::io::Write;

use cadmpeg_ir::codec::{CodecError, Encoder};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::report::{ExportReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::topology::{Coedge, Edge, Point, Sense, Vertex};
use cadmpeg_ir::CadIr;

use writer::{real, refs, string, Emitter, Ref};

/// Metadata written to the STEP `FILE_NAME` header record.
///
/// Default values produce deterministic output. They identify the file as
/// `cadmpeg_model`, leave the author and organization empty, use `cadmpeg` as
/// the originating system, and substitute `1970-01-01T00:00:00` for the empty
/// timestamp.
#[derive(Debug, Clone)]
pub struct StepWriteOptions {
    /// The `FILE_NAME` name field.
    ///
    /// The STEP `PRODUCT` id and name come from the first IR body name, or
    /// `cadmpeg_model` when that body has no name.
    pub product_name: String,
    /// The sole entry in the `FILE_NAME` author list.
    pub author: String,
    /// The sole entry in the `FILE_NAME` organization list.
    pub organization: String,
    /// The `FILE_NAME` timestamp.
    ///
    /// Supply an ISO 8601 value. An empty string is written as
    /// `1970-01-01T00:00:00`.
    pub timestamp: String,
    /// The `FILE_NAME` originating-system field.
    pub originating_system: String,
}

impl Default for StepWriteOptions {
    fn default() -> Self {
        StepWriteOptions {
            product_name: "cadmpeg_model".to_string(),
            author: String::new(),
            organization: String::new(),
            timestamp: String::new(),
            originating_system: "cadmpeg".to_string(),
        }
    }
}

/// Failure returned while streaming STEP output.
///
/// Unsupported or reduced IR content appears in [`ExportReport::losses`] after a
/// successful write.
#[derive(Debug, thiserror::Error)]
pub enum StepError {
    /// The output sink rejected a write.
    #[error("failed to write STEP output: {0}")]
    Io(#[from] std::io::Error),
}

/// Serializes an IR document as an ISO 10303-21 STEP AP214 file.
///
/// The output declares the `AUTOMOTIVE_DESIGN` schema and a millimetre length
/// unit. Coordinate values are not rescaled. The IR linear tolerance becomes
/// the representation context's uncertainty value.
///
/// Geometry conversion completes before this function writes the header. It
/// then streams the header, DATA instances, and closing records to `w`. An I/O
/// error can therefore leave a partial file and returns no report.
///
/// On success, the report contains DATA entity counts and loss notes for
/// omitted or reduced content.
pub fn write_step(
    ir: &CadIr,
    w: &mut (impl Write + ?Sized),
    opts: &StepWriteOptions,
) -> Result<ExportReport, StepError> {
    let mut b = Builder::new(ir);
    b.build();
    let report = b.finish_report();
    let lines = b.emitter.into_lines();

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
    writeln!(
        w,
        "FILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));"
    )?;
    writeln!(w, "ENDSEC;")?;
    Ok(())
}

/// Builds the DATA instance graph and accumulates export losses.
struct Builder<'a> {
    ir: &'a CadIr,
    emitter: Emitter,
    losses: Vec<LossNote>,

    // Lookup indices from the flat IR arenas.
    points: HashMap<&'a str, &'a Point>,
    vertices: HashMap<&'a str, &'a Vertex>,
    edges: HashMap<&'a str, &'a Edge>,
    coedges: HashMap<&'a str, &'a Coedge>,
    surfaces: HashMap<&'a str, &'a Surface>,
    curves: HashMap<&'a str, &'a Curve>,

    // Emitted-instance caches keyed by IR id, so shared carriers emit once.
    surface_refs: HashMap<String, Ref>,
    curve_refs: HashMap<String, Ref>,
    edge_refs: HashMap<String, Ref>,
    vertex_refs: HashMap<String, Ref>,

    /// Edges skipped because they carry no attributed 3D curve, deduplicated
    /// (a shared edge is reached once per coedge) and aggregated into a single
    /// counted loss note.
    curveless_edges: BTreeSet<String>,

    /// Faces skipped because their surface geometry is unknown (opaque), so no
    /// STEP surface exists to build an `ADVANCED_FACE` on. Deduplicated (a face
    /// is reached once per shell) and aggregated into a single counted loss.
    unknown_surface_faces: BTreeSet<String>,

    /// Emitted `ADVANCED_FACE` instances keyed by IR face id, for presentation
    /// styling.
    face_step_refs: HashMap<String, Ref>,
    /// Display colors that could not be attached to an emitted STEP item.
    unstyled_colors: usize,
}

impl<'a> Builder<'a> {
    fn new(ir: &'a CadIr) -> Self {
        Builder {
            ir,
            emitter: Emitter::new(),
            losses: Vec::new(),
            points: ir.model.points.iter().map(|p| (p.id.as_str(), p)).collect(),
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
            surface_refs: HashMap::new(),
            curve_refs: HashMap::new(),
            edge_refs: HashMap::new(),
            vertex_refs: HashMap::new(),
            curveless_edges: BTreeSet::new(),
            unknown_surface_faces: BTreeSet::new(),
            face_step_refs: HashMap::new(),
            unstyled_colors: 0,
        }
    }

    fn loss(&mut self, category: LossCategory, severity: Severity, message: String) {
        self.losses.push(LossNote {
            category,
            severity,
            message,
            provenance: None,
        });
    }

    fn build(&mut self) {
        // Product structure and unit-bearing context first; the representation
        // instance that ties them to the geometry is emitted last.
        let product_def_shape = self.emit_product_structure();
        let context = self.emit_context();

        let solids = self.emit_solids();
        if solids.is_empty() {
            self.loss(
                LossCategory::Topology,
                Severity::Warning,
                "no exportable solids: the IR document contains no body/region/shell \
                 geometry, so the STEP representation is empty"
                    .to_string(),
            );
        }

        let mut items = solids;
        // A representation-space origin placement is conventional and harmless.
        let origin = geometry::placement(
            &mut self.emitter,
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        );
        items.push(origin);

        let absr = self.emitter.emit(
            "ADVANCED_BREP_SHAPE_REPRESENTATION",
            &format!("'',{},{context}", refs(&items)),
        );
        self.emitter.emit(
            "SHAPE_DEFINITION_REPRESENTATION",
            &format!("{product_def_shape},{absr}"),
        );

        self.emit_presentation(context);
        self.note_unrepresented();
    }

    /// Emit a per-face `STYLED_ITEM` surface color for every colored face.
    ///
    /// A face's color is its own `color` field or face appearance binding when
    /// present; otherwise it inherits the color of the body that owns it. Body
    /// colors are pushed down onto each face rather than styled on the solid,
    /// because common OCCT/VTK-based viewers read surface colors only from
    /// `ADVANCED_FACE` and ignore `MANIFOLD_SOLID_BREP`. Each distinct color
    /// shares one `PRESENTATION_STYLE_ASSIGNMENT`; the styled items are gathered
    /// into one `MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION` in the
    /// geometric context.
    fn emit_presentation(&mut self, context: Ref) {
        use cadmpeg_ir::appearance::AppearanceTarget;
        use cadmpeg_ir::topology::Color;

        let ir = self.ir;
        let appearances: HashMap<&str, Option<Color>> = ir
            .model
            .appearances
            .iter()
            .map(|appearance| (appearance.id.as_str(), appearance.base_color))
            .collect();
        let mut body_colors: HashMap<&str, Color> = HashMap::new();
        let mut face_colors: HashMap<&str, Color> = HashMap::new();
        for binding in &ir.model.appearance_bindings {
            let Some(color) = appearances
                .get(binding.appearance.as_str())
                .copied()
                .flatten()
            else {
                continue;
            };
            match &binding.target {
                AppearanceTarget::Body(id) => {
                    body_colors.entry(id.as_str()).or_insert(color);
                }
                AppearanceTarget::Face(id) => {
                    face_colors.entry(id.as_str()).or_insert(color);
                }
            }
        }
        for body in &ir.model.bodies {
            if let Some(color) = body.color {
                body_colors.insert(body.id.as_str(), color);
            }
        }
        for face in &ir.model.faces {
            if let Some(color) = face.color {
                face_colors.insert(face.id.as_str(), color);
            }
        }

        // Map each face to the body that owns it, so a body-level color can be
        // pushed down onto the body's individual faces.
        let mut face_body: HashMap<&str, &str> = HashMap::new();
        for region in &ir.model.regions {
            let body = region.body.0.as_str();
            for shell_id in &region.shells {
                let Some(shell) = ir
                    .model
                    .shells
                    .iter()
                    .find(|s| s.id.as_str() == shell_id.as_str())
                else {
                    continue;
                };
                for face in &shell.faces {
                    face_body.insert(face.0.as_str(), body);
                }
            }
        }

        // Every colored face carries its own face-level STYLED_ITEM. A face's
        // color is its own override when present, otherwise the color of the
        // body that owns it. Whole-solid styling is intentionally not emitted:
        // common OCCT/VTK-based viewers (f3d, CAD Assistant) read STEP surface
        // colors only from ADVANCED_FACE and ignore MANIFOLD_SOLID_BREP, so a
        // body color left at the solid level renders as the viewer default.
        let mut style_refs: HashMap<String, Ref> = HashMap::new();
        let mut styled = Vec::new();
        let mut faces: Vec<(String, Ref)> = self
            .face_step_refs
            .iter()
            .map(|(id, r)| (id.clone(), *r))
            .collect();
        faces.sort_by(|a, b| a.0.cmp(&b.0));
        // Bodies whose color actually reached at least one face, for loss
        // accounting.
        let mut styled_bodies: BTreeSet<&str> = BTreeSet::new();
        for (face_id, face) in &faces {
            let own = face_colors.get(face_id.as_str()).copied();
            let body = face_body.get(face_id.as_str()).copied();
            let inherited = body.and_then(|b| body_colors.get(b).copied());
            let Some(color) = own.or(inherited) else {
                continue;
            };
            // The body color is only counted as represented when a face without
            // its own override receives it.
            if own.is_none() {
                if let Some(b) = body {
                    styled_bodies.insert(b);
                }
            }
            let style = self.surface_style(color, &mut style_refs);
            styled.push(
                self.emitter
                    .emit("STYLED_ITEM", &format!("'color',({style}),{face}")),
            );
        }
        // A color is unrepresented when no emitted ADVANCED_FACE could carry it:
        // a face override whose face was skipped, or a body whose faces were all
        // skipped (hidden bodies or faces on unknown surfaces).
        let emitted: BTreeSet<&str> = self.face_step_refs.keys().map(String::as_str).collect();
        self.unstyled_colors = face_colors
            .keys()
            .filter(|id| !emitted.contains(**id as &str))
            .count()
            + body_colors
                .keys()
                .filter(|id| !styled_bodies.contains(**id as &str))
                .count();
        if styled.is_empty() {
            return;
        }
        self.emitter.emit(
            "MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION",
            &format!("'',{},{context}", refs(&styled)),
        );
    }

    /// Emit (or reuse) the `PRESENTATION_STYLE_ASSIGNMENT` chain for one
    /// surface color.
    fn surface_style(
        &mut self,
        color: cadmpeg_ir::topology::Color,
        cache: &mut HashMap<String, Ref>,
    ) -> Ref {
        let rgb = format!(
            "{},{},{}",
            real(f64::from(color.r)),
            real(f64::from(color.g)),
            real(f64::from(color.b))
        );
        if let Some(style) = cache.get(&rgb) {
            return *style;
        }
        let colour = self.emitter.emit("COLOUR_RGB", &format!("'',{rgb}"));
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
        cache.insert(rgb, assignment);
        assignment
    }

    /// Emit the `PRODUCT` → `PRODUCT_DEFINITION_SHAPE` chain, returning the
    /// `PRODUCT_DEFINITION_SHAPE` reference.
    fn emit_product_structure(&mut self) -> Ref {
        let name = self
            .ir
            .model
            .bodies
            .first()
            .and_then(|b| b.name.clone())
            .unwrap_or_else(|| "cadmpeg_model".to_string());

        let app_ctx = self
            .emitter
            .emit("APPLICATION_CONTEXT", &string("automotive design"));
        self.emitter.emit(
            "APPLICATION_PROTOCOL_DEFINITION",
            &format!(
                "{},{},2000,{app_ctx}",
                string("international standard"),
                string("automotive_design")
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

    /// Emit the units and the geometric representation context, returning the
    /// context reference.
    fn emit_context(&mut self) -> Ref {
        let len = self.emit_length_unit();
        let angle = self.emitter.emit_raw(
            "PLANE_ANGLE_UNIT",
            "( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) )",
        );
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

    /// Emit the millimetre length unit used by the representation context.
    ///
    /// Coordinate values are written unchanged.
    fn emit_length_unit(&mut self) -> Ref {
        self.emitter.emit_raw(
            "LENGTH_UNIT",
            "( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) )",
        )
    }

    /// Emit one solid per region across all displayed bodies; returns the
    /// solid references. Bodies the source document hides are omitted.
    fn emit_solids(&mut self) -> Vec<Ref> {
        let mut solids = Vec::new();
        // `ir` is a shared `&CadIr`; binding it locally lets us read the arenas
        // while still calling `&mut self` helpers (loss/emit).
        let ir = self.ir;
        let hidden: BTreeSet<&str> = ir
            .model
            .bodies
            .iter()
            .filter(|body| body.visible == Some(false))
            .map(|body| body.id.0.as_str())
            .collect();
        if !hidden.is_empty() {
            self.loss(
                LossCategory::Metadata,
                Severity::Info,
                format!(
                    "{} hidden body(ies) were omitted from STEP output",
                    hidden.len()
                ),
            );
        }
        for body in &ir.model.bodies {
            if hidden.contains(body.id.0.as_str()) {
                continue;
            }
            if let Some(t) = &body.transform {
                if !is_identity(&t.rows) {
                    self.loss(
                        LossCategory::Geometry,
                        Severity::Warning,
                        format!(
                            "body '{}' carries a non-identity transform that was not \
                             applied to the exported geometry; coordinates are written \
                             in body-local space",
                            body.id
                        ),
                    );
                }
            }
        }

        for region in &ir.model.regions {
            if hidden.contains(region.body.0.as_str()) {
                continue;
            }
            let shell_refs: Vec<Ref> = region
                .shells
                .iter()
                .filter_map(|sid| self.emit_shell(sid.as_str()))
                .collect();
            let Some((outer, voids)) = shell_refs.split_first() else {
                continue;
            };
            let solid = if voids.is_empty() {
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
            solids.push(solid);
        }
        solids
    }

    fn emit_shell(&mut self, shell_id: &str) -> Option<Ref> {
        let shell = self
            .ir
            .model
            .shells
            .iter()
            .find(|s| s.id.as_str() == shell_id)?;
        let face_ids: Vec<String> = shell.faces.iter().map(|f| f.0.clone()).collect();
        let mut face_refs = Vec::new();
        for fid in &face_ids {
            if let Some(r) = self.emit_face(fid) {
                face_refs.push(r);
            }
        }
        if face_refs.is_empty() {
            return None;
        }
        Some(
            self.emitter
                .emit("CLOSED_SHELL", &format!("'',{}", refs(&face_refs))),
        )
    }

    fn emit_face(&mut self, face_id: &str) -> Option<Ref> {
        let face = self
            .ir
            .model
            .faces
            .iter()
            .find(|f| f.id.as_str() == face_id)?;
        let surface_id = face.surface.0.clone();
        // A face resting on an unknown (opaque) surface cannot become an
        // ADVANCED_FACE: STEP requires a real surface. Skip it and aggregate the
        // loss rather than fabricate placeholder geometry.
        if let Some(surf) = self.surfaces.get(surface_id.as_str()) {
            if matches!(surf.geometry, SurfaceGeometry::Unknown { .. }) {
                self.unknown_surface_faces.insert(face_id.to_string());
                return None;
            }
        }
        let loop_ids: Vec<String> = face.loops.iter().map(|l| l.0.clone()).collect();
        let same_sense = matches!(face.sense, Sense::Forward);

        let surf_ref = self.emit_surface(&surface_id)?;

        let mut bound_refs = Vec::new();
        for (i, lid) in loop_ids.iter().enumerate() {
            if let Some(loop_ref) = self.emit_loop(lid) {
                // The first loop is the outer bound by IR convention.
                let kind = if i == 0 {
                    "FACE_OUTER_BOUND"
                } else {
                    "FACE_BOUND"
                };
                let b = self.emitter.emit(kind, &format!("'',{loop_ref},.T."));
                bound_refs.push(b);
            }
        }
        if bound_refs.is_empty() {
            return None;
        }
        let flag = if same_sense { ".T." } else { ".F." };
        let advanced_face = self.emitter.emit(
            "ADVANCED_FACE",
            &format!("'',{},{surf_ref},{flag}", refs(&bound_refs)),
        );
        self.face_step_refs
            .insert(face_id.to_string(), advanced_face);
        Some(advanced_face)
    }

    fn emit_loop(&mut self, loop_id: &str) -> Option<Ref> {
        let lp = self
            .ir
            .model
            .loops
            .iter()
            .find(|l| l.id.as_str() == loop_id)?;
        let coedge_ids: Vec<String> = lp.coedges.iter().map(|c| c.0.clone()).collect();
        let mut oe_refs = Vec::new();
        for cid in &coedge_ids {
            let Some(coedge) = self.coedges.get(cid.as_str()).copied() else {
                continue;
            };
            let orientation = matches!(coedge.sense, Sense::Forward);
            // Pcurves (coedge.pcurve) are intentionally dropped; the aggregate
            // loss note is recorded in `note_unrepresented`.
            let Some(edge_ref) = self.emit_edge(coedge.edge.as_str()) else {
                continue;
            };
            let flag = if orientation { ".T." } else { ".F." };
            let oe = self
                .emitter
                .emit("ORIENTED_EDGE", &format!("'',*,*,{edge_ref},{flag}"));
            oe_refs.push(oe);
        }
        if oe_refs.is_empty() {
            return None;
        }
        Some(
            self.emitter
                .emit("EDGE_LOOP", &format!("'',{}", refs(&oe_refs))),
        )
    }

    fn emit_edge(&mut self, edge_id: &str) -> Option<Ref> {
        if let Some(r) = self.edge_refs.get(edge_id) {
            return Some(*r);
        }
        let edge = self.edges.get(edge_id).copied()?;
        let v1 = self.emit_vertex(edge.start.as_str())?;
        let v2 = self.emit_vertex(edge.end.as_str())?;
        let Some(curve_id) = &edge.curve else {
            self.curveless_edges.insert(edge_id.to_string());
            return None;
        };
        if self
            .curves
            .get(curve_id.as_str())
            .is_some_and(|curve| matches!(curve.geometry, CurveGeometry::Unknown { .. }))
        {
            self.curveless_edges.insert(edge_id.to_string());
            return None;
        }
        let curve_ref = self.emit_curve(curve_id.as_str())?;
        // same_sense = .T.: the edge runs start→end along the curve's own
        // parameterization, the convention IR curves follow.
        let r = self
            .emitter
            .emit("EDGE_CURVE", &format!("'',{v1},{v2},{curve_ref},.T."));
        self.edge_refs.insert(edge_id.to_string(), r);
        Some(r)
    }

    fn emit_vertex(&mut self, vertex_id: &str) -> Option<Ref> {
        if let Some(r) = self.vertex_refs.get(vertex_id) {
            return Some(*r);
        }
        let vertex = self.vertices.get(vertex_id).copied()?;
        let pt = self.points.get(vertex.point.as_str()).copied()?;
        let cp = geometry::point(&mut self.emitter, pt.position);
        let r = self.emitter.emit("VERTEX_POINT", &format!("'',{cp}"));
        self.vertex_refs.insert(vertex_id.to_string(), r);
        Some(r)
    }

    fn emit_surface(&mut self, surface_id: &str) -> Option<Ref> {
        if let Some(r) = self.surface_refs.get(surface_id) {
            return Some(*r);
        }
        let surf = self.surfaces.get(surface_id).copied()?;
        let r = geometry::surface(&mut self.emitter, &surf.geometry);
        self.surface_refs.insert(surface_id.to_string(), r);
        Some(r)
    }

    fn emit_curve(&mut self, curve_id: &str) -> Option<Ref> {
        if let Some(r) = self.curve_refs.get(curve_id) {
            return Some(*r);
        }
        let crv = self.curves.get(curve_id).copied()?;
        let r = geometry::curve(&mut self.emitter, &crv.geometry);
        self.curve_refs.insert(curve_id.to_string(), r);
        Some(r)
    }

    /// Record aggregate loss notes for IR content the writer does not carry.
    fn note_unrepresented(&mut self) {
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
                } => *minor_radius < 0.0 || minor_radius.abs() > major_radius.abs(),
                _ => false,
            })
            .count();
        if nonstandard_analytic_surfaces > 0 {
            self.loss(
                LossCategory::Geometry,
                Severity::Warning,
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
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{elliptical_cones} elliptical cone surface(s) were reduced to circular STEP CONICAL_SURFACE carriers"
                ),
            );
        }
        if !self.curveless_edges.is_empty() {
            self.loss(
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{} edge(s) have no typed 3D curve and were omitted from \
                     their edge loops (STEP EDGE_CURVE requires a 3D curve)",
                    self.curveless_edges.len()
                ),
            );
        }
        if !self.unknown_surface_faces.is_empty() {
            self.loss(
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{} face(s) rest on an unknown (undecoded) surface and were omitted \
                     from the STEP shell (an ADVANCED_FACE requires a surface); their \
                     topology remains in the IR",
                    self.unknown_surface_faces.len()
                ),
            );
        }
        let pcurve_count = self
            .ir
            .model
            .coedges
            .iter()
            .filter(|c| c.pcurve.is_some())
            .count();
        if pcurve_count > 0 {
            self.loss(
                LossCategory::Geometry,
                Severity::Info,
                format!(
                    "{pcurve_count} coedge pcurve(s) were not written; parameter-space \
                     trims are omitted, and consumers recompute them from the 3D \
                     edge/surface geometry"
                ),
            );
        }
        if !self.ir.model.pcurves.is_empty() {
            self.loss(
                LossCategory::Geometry,
                Severity::Info,
                format!(
                    "{} pcurve carrier(s) in the IR were not emitted",
                    self.ir.model.pcurves.len()
                ),
            );
        }
        if !self.ir.model.subds.is_empty() {
            self.loss(
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{} subdivision surface(s) were omitted because this STEP writer \
                     does not encode SubD control cages",
                    self.ir.model.subds.len()
                ),
            );
        }
        if !self.ir.model.tessellations.is_empty() {
            self.loss(
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{} tessellation(s) were omitted because this STEP writer emits \
                     exact B-rep geometry only",
                    self.ir.model.tessellations.len()
                ),
            );
        }
        let source_object_count = self
            .ir
            .model
            .surfaces
            .iter()
            .filter(|surface| surface.source_object.is_some())
            .count()
            + self
                .ir
                .model
                .curves
                .iter()
                .filter(|curve| curve.source_object.is_some())
                .count()
            + self
                .ir
                .model
                .subds
                .iter()
                .filter(|subd| subd.source_object.is_some())
                .count()
            + self
                .ir
                .model
                .tessellations
                .iter()
                .filter(|tessellation| tessellation.source_object.is_some())
                .count();
        if source_object_count > 0 {
            self.loss(
                LossCategory::Metadata,
                Severity::Info,
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
                LossCategory::Metadata,
                Severity::Info,
                format!("{unknown_count} uninterpreted passthrough record(s) were not represented in STEP"),
            );
        }
        if self.unstyled_colors > 0 {
            self.loss(
                LossCategory::Attribute,
                Severity::Info,
                format!(
                    "{} display color(s) had no emitted STEP item and were not written \
                     to STEP presentation",
                    self.unstyled_colors
                ),
            );
        }
        if !self.ir.model.appearances.is_empty() {
            self.loss(
                LossCategory::Material,
                Severity::Info,
                format!(
                    "{} appearance asset(s) were reduced to STYLED_ITEM base colors; \
                     schemas, textures, and shader properties were not written to STEP",
                    self.ir.model.appearances.len()
                ),
            );
        }
        if !self.ir.model.attributes.is_empty() {
            self.loss(
                LossCategory::Attribute,
                Severity::Info,
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
            .filter(|procedural| match &procedural.definition {
                ProceduralSurfaceDefinition::Exact { .. }
                | ProceduralSurfaceDefinition::Compound { .. }
                | ProceduralSurfaceDefinition::Taper { .. }
                | ProceduralSurfaceDefinition::Loft { .. }
                | ProceduralSurfaceDefinition::CompoundLoft { .. }
                | ProceduralSurfaceDefinition::ScaledCompoundLoft { .. }
                | ProceduralSurfaceDefinition::Skin { .. }
                | ProceduralSurfaceDefinition::Net { .. }
                | ProceduralSurfaceDefinition::G2Blend { .. }
                | ProceduralSurfaceDefinition::VariableBlend { .. }
                | ProceduralSurfaceDefinition::VertexBlend { .. }
                | ProceduralSurfaceDefinition::Extrusion { .. }
                | ProceduralSurfaceDefinition::Revolution { .. }
                | ProceduralSurfaceDefinition::Sum { .. }
                | ProceduralSurfaceDefinition::Sweep { .. }
                | ProceduralSurfaceDefinition::Helix { .. }
                | ProceduralSurfaceDefinition::Deformable { .. }
                | ProceduralSurfaceDefinition::TSpline { .. }
                | ProceduralSurfaceDefinition::Offset { .. }
                | ProceduralSurfaceDefinition::Ruled { .. }
                | ProceduralSurfaceDefinition::Blend { .. }
                | ProceduralSurfaceDefinition::Unknown { .. } => true,
            })
            .count();
        if procedural_surface_count > 0 || !self.ir.model.procedural_curves.is_empty() {
            self.loss(
                LossCategory::Geometry,
                Severity::Info,
                format!(
                    "{} procedural surface definition(s) and {} procedural curve definition(s) were reduced to their solved STEP carriers",
                    procedural_surface_count,
                    self.ir.model.procedural_curves.len()
                ),
            );
        }
        let parametric_records: usize = self
            .ir
            .native
            .loss_counts()
            .iter()
            .map(|loss| loss.count)
            .sum();
        if parametric_records > 0 {
            self.loss(
                LossCategory::Metadata,
                Severity::Info,
                format!(
                    "{parametric_records} parametric design/history record(s) were not represented in STEP"
                ),
            );
        }
    }

    fn finish_report(&self) -> ExportReport {
        ExportReport {
            format: "step".into(),
            entity_counts: self.emitter.counts(),
            total_entities: self.emitter.total(),
            losses: self.losses.clone(),
            notes: Vec::new(),
        }
    }
}

/// STEP encoder with per-export header options.
#[derive(Debug, Clone, Default)]
pub struct StepCodec {
    /// Header metadata and deterministic writer options.
    pub options: StepWriteOptions,
}

impl Encoder for StepCodec {
    fn id(&self) -> &'static str {
        "step"
    }

    fn encode(&self, ir: &CadIr, writer: &mut dyn Write) -> Result<ExportReport, CodecError> {
        write_step(ir, writer, &self.options).map_err(CodecError::from)
    }
}

impl From<StepError> for CodecError {
    fn from(error: StepError) -> Self {
        match error {
            StepError::Io(error) => Self::Io(error),
        }
    }
}

/// Whether a 4×4 row-major matrix is (numerically) the identity.
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

#[cfg(test)]
mod tests;
