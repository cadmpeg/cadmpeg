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
pub mod lex;
pub mod parse;
mod reader;
pub mod strings;
mod writer;

use std::collections::{BTreeSet, HashMap};
use std::io::Write;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerEntry, ContainerSummary, DecodeOptions, DecodeResult,
    Encoder, ReadSeek,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, Pcurve, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{OccurrenceId, ProductId};
use cadmpeg_ir::product::OccurrenceParent;
use cadmpeg_ir::report::{ExportReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::topology::{BodyKind, Coedge, Edge, Point, Sense, Vertex};
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
    /// Application protocol and edition declared by `FILE_SCHEMA`.
    pub schema: StepSchema,
    /// Handling of IR content the selected writer cannot represent exactly.
    pub unsupported: StepUnsupportedPolicy,
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
            schema: StepSchema::Ap214,
            unsupported: StepUnsupportedPolicy::Report,
            product_name: "cadmpeg_model".to_string(),
            author: String::new(),
            organization: String::new(),
            timestamp: String::new(),
            originating_system: "cadmpeg".to_string(),
        }
    }
}

/// Policy for semantic content not representable by the selected STEP target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StepUnsupportedPolicy {
    /// Emit the representable subset and return machine-readable loss notes.
    #[default]
    Report,
    /// Reject the document before writing any output byte.
    Reject,
}

/// STEP application-protocol targets supported by the Part 21 writer.
///
/// The AP242 edition number and the long-form schema revision are distinct:
/// editions 1, 2, and 3 use long-form revisions 1, 3, and 4 respectively.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StepSchema {
    /// AP203 edition 1 `CONFIG_CONTROL_DESIGN`.
    Ap203Edition1,
    /// AP203 edition 2 modular long form.
    Ap203Edition2,
    /// AP214 `AUTOMOTIVE_DESIGN`.
    #[default]
    Ap214,
    /// AP242 edition 1 modular long form.
    Ap242Edition1,
    /// AP242 edition 2 modular long form.
    Ap242Edition2,
    /// AP242 edition 3 modular long form.
    Ap242Edition3,
}

impl StepSchema {
    /// Exact schema identifier written in `FILE_SCHEMA`.
    pub const fn file_schema(self) -> &'static str {
        match self {
            Self::Ap203Edition1 => "CONFIG_CONTROL_DESIGN",
            Self::Ap203Edition2 => "AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES_MIM_LF { 1 0 10303 403 2 1 2 }",
            Self::Ap214 => "AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }",
            Self::Ap242Edition1 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }",
            Self::Ap242Edition2 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 3 1 4 }",
            Self::Ap242Edition3 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 4 1 4 }",
        }
    }

    const fn supports_tessellation(self) -> bool {
        matches!(
            self,
            Self::Ap242Edition1 | Self::Ap242Edition2 | Self::Ap242Edition3
        )
    }

    const fn application_protocol(self) -> (&'static str, &'static str, i32) {
        match self {
            Self::Ap203Edition1 => (
                "configuration controlled 3d designs of mechanical parts and assemblies",
                "config_control_design",
                1994,
            ),
            Self::Ap203Edition2 => (
                "configuration controlled 3d designs of mechanical parts and assemblies",
                "ap203_configuration_controlled_3d_design_of_mechanical_parts_and_assemblies",
                2011,
            ),
            Self::Ap214 => ("automotive design", "automotive_design", 2000),
            Self::Ap242Edition1 => (
                "managed model based 3d engineering",
                "ap242_managed_model_based_3d_engineering",
                2014,
            ),
            Self::Ap242Edition2 => (
                "managed model based 3d engineering",
                "ap242_managed_model_based_3d_engineering",
                2020,
            ),
            Self::Ap242Edition3 => (
                "managed model based 3d engineering",
                "ap242_managed_model_based_3d_engineering",
                2022,
            ),
        }
    }
}

/// Failure returned while streaming STEP output.
///
/// Unsupported or reduced IR content appears in [`ExportReport::losses`] after a
/// successful write.
#[derive(Debug, thiserror::Error)]
pub enum StepError {
    /// Strict writing found semantics that would be reduced or omitted.
    #[error("STEP target cannot represent the document without loss: {0}")]
    Unsupported(String),
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

/// Builds the DATA instance graph and accumulates export losses.
struct Builder<'a> {
    ir: &'a CadIr,
    schema: StepSchema,
    emitter: Emitter,
    losses: Vec<LossNote>,

    // Lookup indices from the flat IR arenas.
    points: HashMap<&'a str, &'a Point>,
    vertices: HashMap<&'a str, &'a Vertex>,
    edges: HashMap<&'a str, &'a Edge>,
    coedges: HashMap<&'a str, &'a Coedge>,
    surfaces: HashMap<&'a str, &'a Surface>,
    curves: HashMap<&'a str, &'a Curve>,
    pcurves: HashMap<&'a str, &'a Pcurve>,
    coedge_surfaces: HashMap<&'a str, &'a str>,

    // Emitted-instance caches keyed by IR id, so shared carriers emit once.
    surface_refs: HashMap<String, Ref>,
    curve_refs: HashMap<String, Ref>,
    edge_refs: HashMap<String, Ref>,
    vertex_refs: HashMap<String, Ref>,
    pcurve_context: Option<Ref>,

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
    /// First emitted exact solid for each body, used by AP242 tessellation links.
    body_step_refs: HashMap<String, Ref>,
    /// Emitted representation item for each body.
    body_shape_refs: HashMap<String, Ref>,
    /// Display colors that could not be attached to an emitted STEP item.
    unstyled_colors: usize,
}

impl<'a> Builder<'a> {
    fn new(ir: &'a CadIr, schema: StepSchema) -> Self {
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
        let coedge_surfaces = ir
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
        Builder {
            ir,
            schema,
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
            pcurves: ir
                .model
                .pcurves
                .iter()
                .map(|p| (p.id.as_str(), p))
                .collect(),
            coedge_surfaces,
            surface_refs: HashMap::new(),
            curve_refs: HashMap::new(),
            edge_refs: HashMap::new(),
            vertex_refs: HashMap::new(),
            pcurve_context: None,
            curveless_edges: BTreeSet::new(),
            unknown_surface_faces: BTreeSet::new(),
            face_step_refs: HashMap::new(),
            body_step_refs: HashMap::new(),
            body_shape_refs: HashMap::new(),
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
        let context = self.emit_context();

        let shape_items = self.emit_shape_items();
        if shape_items.is_empty() {
            self.loss(
                LossCategory::Topology,
                Severity::Warning,
                "no exportable solids: the IR document contains no body/region/shell \
                 geometry, so the STEP representation is empty"
                    .to_string(),
            );
        }

        let mut items = shape_items;
        // A representation-space origin placement is conventional and harmless.
        let origin = geometry::placement(
            &mut self.emitter,
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        );
        items.push(origin);

        if self.ir.model.products.is_empty() {
            let product_def_shape = self.emit_product_structure();
            let representation_kind = if self
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
        }

        self.emit_presentation(context);
        self.emit_tessellations(context);
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
                AppearanceTarget::Surface(_)
                | AppearanceTarget::Curve(_)
                | AppearanceTarget::Point(_)
                | AppearanceTarget::Edge(_)
                | AppearanceTarget::Tessellation(_)
                | AppearanceTarget::Source { .. } => {}
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

        let products = self.ir.model.products.clone();
        let mut definitions = HashMap::<ProductId, Ref>::new();
        let mut representations = HashMap::<ProductId, Ref>::new();
        for product in products {
            let name = product.name.as_deref().unwrap_or(&product.product_id);
            let product_ref = self.emitter.emit(
                "PRODUCT",
                &format!(
                    "{},{},'',({product_context})",
                    string(&product.product_id),
                    string(name)
                ),
            );
            let formation = self.emitter.emit(
                "PRODUCT_DEFINITION_FORMATION",
                &format!("'','',{product_ref}"),
            );
            let definition = self.emitter.emit(
                "PRODUCT_DEFINITION",
                &format!(
                    "{},'',{formation},{definition_context}",
                    string(&product.product_id)
                ),
            );
            let shape = self
                .emitter
                .emit("PRODUCT_DEFINITION_SHAPE", &format!("'','',{definition}"));
            let body_items = product
                .bodies
                .iter()
                .filter_map(|body| self.body_shape_refs.get(body.as_str()).copied())
                .collect::<Vec<_>>();
            let representation = self.emitter.emit(
                "SHAPE_REPRESENTATION",
                &format!("{},{},{context}", string(name), refs(&body_items)),
            );
            self.emitter.emit(
                "SHAPE_DEFINITION_REPRESENTATION",
                &format!("{shape},{representation}"),
            );
            definitions.insert(product.id.clone(), definition);
            representations.insert(product.id, representation);
        }

        let occurrences = self.ir.model.occurrences.clone();
        let occurrence_products = occurrences
            .iter()
            .map(|occurrence| (occurrence.id.clone(), occurrence.product.clone()))
            .collect::<HashMap<OccurrenceId, ProductId>>();
        for occurrence in occurrences {
            let OccurrenceParent::Occurrence { occurrence: parent } = occurrence.parent else {
                if !is_identity(&occurrence.transform.rows) {
                    self.loss(
                        LossCategory::Topology,
                        Severity::Warning,
                        format!(
                            "root occurrence '{}' has a non-identity placement",
                            occurrence.id
                        ),
                    );
                }
                continue;
            };
            let Some(parent_product) = occurrence_products.get(&parent) else {
                self.loss(
                    LossCategory::Topology,
                    Severity::Warning,
                    format!("occurrence '{}' has an unresolved parent", occurrence.id),
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
                .zip(definitions.get(&occurrence.product))
                .zip(representations.get(parent_product))
                .zip(representations.get(&occurrence.product))
                .map(|(((a, b), c), d)| (a, b, c, d))
            else {
                continue;
            };
            if !is_rigid_transform(&occurrence.transform.rows) {
                self.loss(
                    LossCategory::Topology,
                    Severity::Warning,
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
            let usage_shape = self
                .emitter
                .emit("PRODUCT_DEFINITION_SHAPE", &format!("'','',{usage}"));
            let from = geometry::placement(
                &mut self.emitter,
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            );
            let rows = occurrence.transform.rows;
            let to = geometry::placement(
                &mut self.emitter,
                cadmpeg_ir::math::Point3::new(rows[0][3], rows[1][3], rows[2][3]),
                cadmpeg_ir::math::Vector3::new(rows[0][2], rows[1][2], rows[2][2]),
                cadmpeg_ir::math::Vector3::new(rows[0][0], rows[1][0], rows[2][0]),
            );
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
    fn emit_shape_items(&mut self) -> Vec<Ref> {
        let mut items = Vec::new();
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
            let body_kind = ir
                .model
                .bodies
                .iter()
                .find(|body| body.id == region.body)
                .map(|body| body.kind)
                .unwrap_or(BodyKind::General);
            if body_kind == BodyKind::Wire {
                self.loss(
                    LossCategory::Topology,
                    Severity::Warning,
                    format!(
                        "wire body '{}' is not yet writable as STEP topology",
                        region.body
                    ),
                );
                continue;
            }
            let closed = body_kind == BodyKind::Solid;
            let shell_refs: Vec<Ref> = region
                .shells
                .iter()
                .filter_map(|sid| self.emit_shell(sid.as_str(), closed))
                .collect();
            let Some((outer, voids)) = shell_refs.split_first() else {
                continue;
            };
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
            items.push(item);
            self.body_shape_refs
                .entry(region.body.0.clone())
                .or_insert(item);
            self.body_step_refs
                .entry(region.body.0.clone())
                .or_insert(if closed { item } else { *outer });
        }
        items
    }

    fn emit_tessellations(&mut self, context: Ref) {
        if self.ir.model.tessellations.is_empty() {
            return;
        }
        if !self.schema.supports_tessellation() {
            self.loss(
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{} tessellation(s) require an AP242 target",
                    self.ir.model.tessellations.len()
                ),
            );
            return;
        }

        let meshes = self.ir.model.tessellations.clone();
        let mut representation_items = Vec::new();
        for mesh in meshes {
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
            let item = self.emitter.emit(
                "TRIANGULATED_SURFACE_SET",
                &format!(
                    "{},{coordinates},{},{normals},(),({triangles})",
                    string(&mesh.id),
                    mesh.vertices.len()
                ),
            );
            let item = mesh
                .body
                .as_ref()
                .and_then(|body| self.body_step_refs.get(body.as_str()).copied())
                .map(|solid| {
                    self.emitter.emit(
                        "TESSELLATED_SOLID",
                        &format!("{},({item}),{solid}", string(&mesh.id)),
                    )
                })
                .unwrap_or(item);
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
        Some(self.emitter.emit(
            if closed { "CLOSED_SHELL" } else { "OPEN_SHELL" },
            &format!("'',{}", refs(&face_refs)),
        ))
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
        if let Some(vertex) = &lp.vertex {
            let vertex = self.emit_vertex(vertex.as_str())?;
            return Some(self.emitter.emit("VERTEX_LOOP", &format!("'',{vertex}")));
        }
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
        let basis_curve = self.emit_curve(curve_id.as_str())?;
        let associated = self
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge.as_str() == edge_id)
            .filter_map(|coedge| {
                Some((
                    coedge.pcurve.as_ref()?.0.clone(),
                    self.coedge_surfaces.get(coedge.id.as_str())?.to_string(),
                ))
            })
            .collect::<Vec<_>>();
        let mut pcurve_refs = Vec::new();
        for (pcurve_id, surface_id) in associated {
            if let Some(pcurve) = self.emit_pcurve(&pcurve_id, &surface_id) {
                pcurve_refs.push(pcurve);
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
        let curve = geometry::pcurve(&mut self.emitter, &pcurve.geometry);
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
        let geometry = self.curves.get(curve_id)?.geometry.clone();
        let r = if let CurveGeometry::Composite {
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
        } else {
            geometry::curve(&mut self.emitter, &geometry)
        };
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
            .filter_map(|coedge| coedge.pcurve.as_ref())
            .filter(|id| {
                self.pcurves.get(id.as_str()).is_none_or(|pcurve| {
                    pcurve.wrapper_reversed.is_some()
                        || pcurve.native_tail_flags.is_some()
                        || pcurve.parameter_range.is_some()
                        || pcurve.fit_tolerance.is_some()
                })
            })
            .count();
        if pcurve_count > 0 {
            self.loss(
                LossCategory::Geometry,
                Severity::Info,
                format!(
                    "{pcurve_count} coedge pcurve(s) use unsupported geometry or native-only metadata and were not written"
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
                | ProceduralSurfaceDefinition::LinearSweep { .. }
                | ProceduralSurfaceDefinition::Revolution { .. }
                | ProceduralSurfaceDefinition::AxisRevolution { .. }
                | ProceduralSurfaceDefinition::Sum { .. }
                | ProceduralSurfaceDefinition::Sweep { .. }
                | ProceduralSurfaceDefinition::Helix { .. }
                | ProceduralSurfaceDefinition::Deformable { .. }
                | ProceduralSurfaceDefinition::TSpline { .. }
                | ProceduralSurfaceDefinition::Offset { .. }
                | ProceduralSurfaceDefinition::ParallelOffset { .. }
                | ProceduralSurfaceDefinition::DegenerateTorus { .. }
                | ProceduralSurfaceDefinition::CurveBounded { .. }
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

impl Codec for StepCodec {
    fn id(&self) -> &'static str {
        "step"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        if prefix.starts_with(b"ISO-10303-21;") {
            Confidence::High
        } else if is_part28_xml(prefix) {
            Confidence::Medium
        } else {
            Confidence::No
        }
    }

    fn inspect(&self, reader: &mut dyn ReadSeek) -> Result<ContainerSummary, CodecError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        refuse_alternate_encoding(&bytes)?;
        if self.detect(&bytes) == Confidence::No {
            return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
        }
        let exchange =
            parse::parse(&bytes).map_err(|error| CodecError::Malformed(error.to_string()))?;
        let decoded = reader::decode(&bytes, &DecodeOptions::default())?;
        let opaque_offsets = decoded
            .ir
            .native_unknowns("step")?
            .iter()
            .map(|record| record.offset as usize)
            .collect::<std::collections::BTreeSet<_>>();
        let mut entries = vec![ContainerEntry {
            name: "HEADER".into(),
            role: "metadata".into(),
            compression: "none".into(),
            compressed_size: 0,
            uncompressed_size: 0,
            attributes: Default::default(),
        }];
        if !exchange.anchors.is_empty() {
            let mut attributes = std::collections::BTreeMap::new();
            attributes.insert("anchor_count".into(), exchange.anchors.len().to_string());
            entries.push(ContainerEntry {
                name: "ANCHOR".into(),
                role: "in_file_anchors".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes,
            });
        }
        if !exchange.references.is_empty() {
            let mut attributes = std::collections::BTreeMap::new();
            attributes.insert(
                "external_count".into(),
                exchange.references.len().to_string(),
            );
            attributes.insert(
                "external_uris".into(),
                exchange
                    .references
                    .iter()
                    .map(|entry| entry.uri.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            entries.push(ContainerEntry {
                name: "REFERENCE".into(),
                role: "external_references".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes,
            });
        }
        for (index, section) in exchange.data.iter().enumerate() {
            let mut counts = std::collections::BTreeMap::<String, usize>::new();
            for id in &section.records {
                if !opaque_offsets.contains(&exchange.records[id].span.start) {
                    continue;
                }
                for partial in &exchange.records[id].partials {
                    *counts.entry(partial.name.clone()).or_default() += 1;
                }
            }
            let unknown = counts
                .iter()
                .map(|(name, count)| format!("{name}:{count}"))
                .collect::<Vec<_>>()
                .join(",");
            let mut attributes = std::collections::BTreeMap::new();
            attributes.insert("entity_count".into(), section.records.len().to_string());
            attributes.insert("unknown_entities".into(), unknown);
            entries.push(ContainerEntry {
                name: format!("DATA[{index}]"),
                role: "entity_records".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes,
            });
        }
        let external_dependencies = decoded
            .report
            .notes
            .iter()
            .filter(|note| {
                note.starts_with("external document ") || note.starts_with("external source ")
            })
            .cloned()
            .collect::<Vec<_>>();
        if !external_dependencies.is_empty() {
            let mut attributes = std::collections::BTreeMap::new();
            attributes.insert(
                "dependency_count".into(),
                external_dependencies.len().to_string(),
            );
            attributes.insert("dependencies".into(), external_dependencies.join(","));
            entries.push(ContainerEntry {
                name: "EXTERNAL_DEPENDENCIES".into(),
                role: "external_references".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes,
            });
        }
        if exchange.signature.is_some() {
            entries.push(ContainerEntry {
                name: "SIGNATURE".into(),
                role: "signature".into(),
                compression: "none".into(),
                compressed_size: 0,
                uncompressed_size: 0,
                attributes: Default::default(),
            });
        }
        let schema = exchange
            .header
            .iter()
            .find(|record| record.name == "FILE_SCHEMA")
            .map(|record| {
                fn strings(value: &parse::Value, out: &mut Vec<String>) {
                    match value {
                        parse::Value::String(bytes) => {
                            if let Ok(value) = strings::decode(bytes) {
                                out.push(value);
                            }
                        }
                        parse::Value::List(values) => {
                            values.iter().for_each(|value| strings(value, out));
                        }
                        parse::Value::Typed(_, value) => strings(value, out),
                        _ => {}
                    }
                }
                let mut names = Vec::new();
                record
                    .parameters
                    .iter()
                    .for_each(|value| strings(value, &mut names));
                names.join(",")
            })
            .unwrap_or_else(|| "unspecified".into());
        let edition = if schema.contains("442 4") {
            "edition 3"
        } else if schema.contains("442 3") {
            "edition 2"
        } else if schema.contains("442 1") {
            "edition 1"
        } else {
            "edition unspecified"
        };
        Ok(ContainerSummary {
            format: "step".into(),
            container_kind: "iso-10303-21-clear-text".into(),
            entries,
            notes: vec![format!("schema {schema}; {edition}")],
        })
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, CodecError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        refuse_alternate_encoding(&bytes)?;
        if self.detect(&bytes) == Confidence::No {
            return Err(CodecError::WrongFormat("missing ISO-10303-21 magic".into()));
        }
        reader::decode(&bytes, options)
    }
}

fn refuse_alternate_encoding(bytes: &[u8]) -> Result<(), CodecError> {
    if bytes.starts_with(b"PK\x03\x04") {
        return Err(CodecError::NotImplemented(
            "STEP Part 21 ZIP container".into(),
        ));
    }
    if bytes.starts_with(b"\x89HDF\r\n\x1a\n") {
        return Err(CodecError::NotImplemented(
            "STEP Part 26 binary/HDF5 encoding".into(),
        ));
    }
    if is_part28_xml(bytes) {
        return Err(CodecError::NotImplemented(
            "STEP Part 28 XML encoding".into(),
        ));
    }
    let lower = bytes
        .iter()
        .take(4096)
        .map(u8::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if lower.starts_with(b"<?xml")
        && (lower
            .windows(21)
            .any(|window| window == b"business_object_model")
            || lower.windows(14).any(|window| window == b"ap242_bo_model"))
    {
        return Err(CodecError::NotImplemented(
            "AP242 BO-Model XML sidecar".into(),
        ));
    }
    Ok(())
}

fn is_part28_xml(bytes: &[u8]) -> bool {
    let lower = bytes
        .iter()
        .take(4096)
        .map(u8::to_ascii_lowercase)
        .collect::<Vec<_>>();
    lower.starts_with(b"<?xml")
        && (lower.windows(12).any(|window| window == b"iso_10303_28")
            || lower
                .windows(21)
                .any(|window| window == b"iso:std:iso:10303:-28"))
}

impl From<StepError> for CodecError {
    fn from(error: StepError) -> Self {
        match error {
            StepError::Unsupported(message) => Self::NotImplemented(message),
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

fn is_rigid_transform(rows: &[[f64; 4]; 4]) -> bool {
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
    true
}

#[cfg(test)]
mod tests;
