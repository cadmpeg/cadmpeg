// SPDX-License-Identifier: Apache-2.0
//! Reads and writes [`cadmpeg_ir::CadIr`] documents as ISO 10303-21 STEP Part
//! 21 exchange structures for AP203, AP214, and AP242.
//!
//! [`write_step`] emits the application protocol selected by
//! [`StepWriteOptions::schema`]. It writes product and representation context,
//! connected exact shape, product occurrences, tessellation, presentation,
//! and PMI when the target schema carries those domains.
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
//! Review [`cadmpeg_ir::ExportReport::losses`] before retaining report-mode
//! output. [`StepUnsupportedPolicy::Reject`] rejects all such losses before any
//! output byte is written. Opaque records, source attributes, unsupported
//! procedural definitions, and target-schema incompatibilities are reported or
//! rejected rather than silently discarded. Body and face colors become
//! per-face `STYLED_ITEM` presentation; direct geometry and tessellation
//! bindings retain their native presentation targets.
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

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;

use cadmpeg_ir::appearance::Appearance;
use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerEntry, ContainerSummary, DecodeOptions, DecodeResult,
    Encoder, ReadSeek,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, Pcurve, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{OccurrenceId, ProductId};
use cadmpeg_ir::product::OccurrenceParent;
use cadmpeg_ir::report::{ExportReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Sense, Shell, Vertex,
};
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

    const fn supports_semantic_pmi(self) -> bool {
        self.supports_tessellation()
    }

    const fn supports_visibility(self) -> bool {
        !matches!(self, Self::Ap203Edition1)
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

#[derive(Clone, Copy)]
struct ColorSpec<'a> {
    color: cadmpeg_ir::topology::Color,
    appearance: Option<&'a Appearance>,
    binding_id: Option<&'a str>,
}

/// Builds the DATA instance graph and accumulates export losses.
struct Builder<'a> {
    ir: &'a CadIr,
    schema: StepSchema,
    emitter: Emitter,
    losses: Vec<LossNote>,
    notes: Vec<String>,

    // Lookup indices from the flat IR arenas.
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

    // Emitted-instance caches keyed by IR id, so shared carriers emit once.
    surface_refs: HashMap<String, Ref>,
    curve_refs: HashMap<String, Ref>,
    edge_refs: HashMap<String, Ref>,
    vertex_refs: HashMap<String, Ref>,
    point_refs: HashMap<String, Ref>,
    pcurve_context: Option<Ref>,
    active_surfaces: BTreeSet<String>,
    active_curves: BTreeSet<String>,
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

    /// Emitted `ADVANCED_FACE` instances keyed by IR face id, for presentation
    /// styling.
    face_step_refs: HashMap<String, Ref>,
    /// First emitted exact solid or shell for each body, used by AP242 tessellation links.
    body_step_refs: HashMap<String, Ref>,
    default_product_definition_shape: Option<Ref>,
    /// Emitted representation item for each body.
    body_shape_refs: HashMap<String, Ref>,
    body_item_refs: HashMap<String, Vec<Ref>>,
    tessellation_step_refs: HashMap<String, Ref>,
    written_appearance_bindings: BTreeSet<String>,
    /// Display colors that could not be attached to an emitted STEP item.
    unstyled_colors: usize,
    unsupported_standalone_geometry: usize,
    written_pmi: usize,
    length_unit: Option<Ref>,
    angle_unit: Option<Ref>,
    ratio_unit: Option<Ref>,
    geometry_emission_depth: usize,
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

        let shape_items = self.emit_shape_items(context);
        let mut standalone_items = self.emit_standalone_geometry();
        let has_standalone_geometry = !standalone_items.is_empty();
        if shape_items.is_empty() && standalone_items.is_empty() {
            self.notes
                .push("STEP representation contains no shape items".to_string());
        }

        let mut items = shape_items;
        items.extend(standalone_items.iter().copied());
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

        let ir = self.ir;
        let appearances: HashMap<&str, &Appearance> = ir
            .model
            .appearances
            .iter()
            .map(|appearance| (appearance.id.as_str(), appearance))
            .collect();
        let mut body_colors: HashMap<&str, ColorSpec<'_>> = HashMap::new();
        let mut face_colors: HashMap<&str, ColorSpec<'_>> = HashMap::new();
        for binding in &ir.model.appearance_bindings {
            let Some(appearance) = appearances.get(binding.appearance.as_str()).copied() else {
                continue;
            };
            let Some(color) = appearance.base_color else {
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

        // Map each face to the body that owns it, so a body-level color can be
        // pushed down onto the body's individual faces.
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
                AppearanceTarget::Tessellation(id) => {
                    (self.tessellation_step_refs.get(id).copied(), "surface")
                }
                AppearanceTarget::Body(_)
                | AppearanceTarget::Vertex(_)
                | AppearanceTarget::Source { .. } => continue,
            };
            let Some(target) = target else {
                let target_id = match &binding.target {
                    AppearanceTarget::Face(id) => id.0.clone(),
                    AppearanceTarget::Surface(id) => id.0.clone(),
                    AppearanceTarget::Curve(id) => id.0.clone(),
                    AppearanceTarget::Edge(id) => id.0.clone(),
                    AppearanceTarget::Point(id) => id.0.clone(),
                    AppearanceTarget::Tessellation(id) => id.clone(),
                    AppearanceTarget::Body(_)
                    | AppearanceTarget::Vertex(_)
                    | AppearanceTarget::Source { .. } => continue,
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
        // A color is unrepresented when no emitted ADVANCED_FACE could carry it:
        // a face override whose face was skipped, or a body whose faces were all
        // skipped (hidden bodies or faces without an explicit STEP surface).
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

    /// Emit (or reuse) the `PRESENTATION_STYLE_ASSIGNMENT` chain for one
    /// surface color.
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
        use cadmpeg_ir::presentation::PresentationItem;

        for layer in self.ir.model.presentation_layers.clone() {
            let mut assigned = Vec::new();
            let mut unsupported = 0usize;
            for item in layer.items {
                let reference = match item {
                    PresentationItem::Body { body } => {
                        self.body_shape_refs.get(body.as_str()).copied()
                    }
                    PresentationItem::Face { face } => {
                        self.face_step_refs.get(face.as_str()).copied()
                    }
                    PresentationItem::Edge { edge } => self.edge_refs.get(edge.as_str()).copied(),
                    PresentationItem::Vertex { vertex } => {
                        self.vertex_refs.get(vertex.as_str()).copied()
                    }
                    PresentationItem::Curve { curve } => {
                        self.curve_refs.get(curve.as_str()).copied()
                    }
                    PresentationItem::Surface { surface } => {
                        self.surface_refs.get(surface.as_str()).copied()
                    }
                    PresentationItem::Point { .. }
                    | PresentationItem::Product { .. }
                    | PresentationItem::Occurrence { .. }
                    | PresentationItem::Pmi { .. }
                    | PresentationItem::Tessellation { .. }
                    | PresentationItem::Source { .. } => None,
                };
                if let Some(reference) = reference {
                    assigned.push(reference);
                } else {
                    unsupported += 1;
                }
            }
            if unsupported > 0 {
                self.loss(
                    LossCategory::Attribute,
                    Severity::Warning,
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

        let ir = self.ir;
        let products = &ir.model.products;
        let occurrences = &ir.model.product_occurrences;
        let occurrence_products = occurrences
            .iter()
            .map(|occurrence| (occurrence.id.clone(), occurrence.product.clone()))
            .collect::<HashMap<OccurrenceId, ProductId>>();
        let mut product_origins = HashMap::<ProductId, Ref>::new();
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
        let mut representation_placements = HashMap::<ProductId, Vec<Ref>>::new();
        let mut occurrence_placements = HashMap::<OccurrenceId, (Ref, Ref)>::new();
        for occurrence in occurrences {
            let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent else {
                continue;
            };
            let Some(parent_product) = occurrence_products.get(parent) else {
                continue;
            };
            let Some(&from) = product_origins.get(&occurrence.product) else {
                continue;
            };
            if !is_rigid_transform(&occurrence.transform.rows) {
                continue;
            }
            let rows = occurrence.transform.rows;
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
            let Some(parent_product) = occurrence_products.get(parent) else {
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

    /// Emit the units and the geometric representation context, returning the
    /// context reference.
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

    /// Emit the millimetre length unit used by the representation context.
    ///
    /// Coordinate values are written unchanged.
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
        // `ir` is a shared `&CadIr`; binding it locally lets us read the arenas
        // while still calling `&mut self` helpers (loss/emit).
        let ir = self.ir;
        for region in &ir.model.regions {
            let body_kind = self
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
            .bodies
            .get(body_id.as_str())
            .and_then(|body| body.transform);
        let Some(transform) = transform.filter(|transform| !is_identity(&transform.rows)) else {
            return item;
        };
        if !is_rigid_transform(&transform.rows) {
            self.loss(
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
            .filter_map(|shell_id| self.shells.get(shell_id.as_str()).copied().cloned())
            .collect::<Vec<_>>();
        let mut connected_sets = Vec::new();
        for shell in shells {
            if !shell.free_vertices.is_empty() {
                self.loss(
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
            let point_indices = (1..=mesh.vertices.len())
                .map(|index| index.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let linked_body = mesh.body.as_ref().and_then(|body| {
                let link = self.body_step_refs.get(body.as_str()).copied()?;
                let kind = self.bodies.get(body.as_str())?.kind;
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
        let lp = self.loops.get(loop_id).copied()?;
        if lp.coedges.is_empty() && lp.vertex_uses.len() == 1 {
            let vertex = self.emit_vertex(lp.vertex_uses[0].vertex.as_str())?;
            return Some(self.emitter.emit("VERTEX_LOOP", &format!("'',{vertex}")));
        }
        let coedge_ids: Vec<String> = lp.coedges.iter().map(|c| c.0.clone()).collect();
        let mut oe_refs = Vec::new();
        for cid in &coedge_ids {
            let Some(coedge) = self.coedges.get(cid.as_str()).copied() else {
                continue;
            };
            let orientation = matches!(coedge.sense, Sense::Forward);
            // The edge carrier emits the coedge pcurve through SURFACE_CURVE.
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
            .is_some_and(|curve| !geometry::curve_is_supported(&curve.geometry))
        {
            self.curveless_edges.insert(edge_id.to_string());
            return None;
        }
        let basis_curve = self.emit_curve(curve_id.as_str())?;
        let associated = self.edge_coedges.get(edge_id).cloned().unwrap_or_default();
        let mut pcurve_refs = Vec::new();
        for (pcurve_id, surface_id) in associated {
            if let Some(pcurve) = self.emit_pcurve(pcurve_id, surface_id) {
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
                geometry::surface(&mut self.emitter, &surf.geometry)
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

    fn emit_curve(&mut self, curve_id: &str) -> Option<Ref> {
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
                geometry::curve(&mut self.emitter, &geometry)
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
            } => {
                let source = self.emit_curve(source.as_str())?;
                Some(self.emitter.emit(
                    "TRIMMED_CURVE",
                    &format!(
                        "'',{source},(PARAMETER_VALUE({})),(PARAMETER_VALUE({})),.T.,.PARAMETER.",
                        real(*start),
                        real(*end)
                    ),
                ))
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

    fn emit_pmi(&mut self, context: Ref) {
        use cadmpeg_ir::pmi::{DimensionKind, GeometricToleranceKind, PmiDefinition, PmiTarget};

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
        let targets_exact = |annotation: &cadmpeg_ir::PmiAnnotation| {
            annotation
                .targets
                .iter()
                .all(|target| matches!(target, PmiTarget::ShapeAspect { .. }))
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
                    datum_system,
                } => {
                    let kind_exact = !matches!(tolerance, GeometricToleranceKind::Other(value) if value != "geometric_tolerance");
                    let entity = match tolerance {
                        GeometricToleranceKind::Straightness => "STRAIGHTNESS_TOLERANCE",
                        GeometricToleranceKind::Flatness => "FLATNESS_TOLERANCE",
                        GeometricToleranceKind::Roundness => "ROUNDNESS_TOLERANCE",
                        GeometricToleranceKind::Cylindricity => "CYLINDRICITY_TOLERANCE",
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
                    let measure = self.emit_pmi_measure(*magnitude);
                    let aspect = target_ref(annotation).unwrap_or(fallback_aspect);
                    // Datum references are carried by the complex
                    // GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE subtype. Until
                    // that complex entity is modeled, refuse it through the
                    // unwritten-PMI accounting instead of emitting an invalid
                    // fifth parameter on the simple tolerance entity.
                    if datum_system.is_some() {
                        continue;
                    }
                    let tolerance_ref = self.emitter.emit(
                        entity,
                        &format!(
                            "{},'',{measure},{aspect}",
                            string(annotation.name.as_deref().unwrap_or(""))
                        ),
                    );
                    annotation_refs.insert(annotation.id.clone(), tolerance_ref);
                    self.written_pmi += usize::from(targets_exact(annotation) && kind_exact);
                }
                PmiDefinition::Datum { .. }
                | PmiDefinition::DatumSystem { .. }
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
    }

    fn emit_datum_modifiers(&mut self, source: &[String]) -> Option<String> {
        let mut modifiers = Vec::with_capacity(source.len());
        for modifier in source {
            if let Some((kind, value)) = modifier.split_once(':') {
                let value = value.parse::<f64>().ok()?;
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
            } else {
                modifiers.push(format!(".{}.", modifier.to_ascii_uppercase()));
            }
        }
        Some(modifiers.join(","))
    }

    fn emit_pmi_measure(&mut self, value: cadmpeg_ir::PmiValue) -> Ref {
        use cadmpeg_ir::pmi::PmiQuantity;
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
        use cadmpeg_ir::pmi::PmiQuantity;
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
                    "{} edge(s) have no typed 3D curve or carry a STEP-unsupported transform and were omitted from \
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
                    "{} face(s) rest on an unknown or STEP-unsupported surface and were omitted \
                     from the STEP shell (an ADVANCED_FACE requires a surface); their \
                     topology remains in the IR",
                    self.unknown_surface_faces.len()
                ),
            );
        }
        if self.unsupported_standalone_geometry > 0 {
            self.loss(
                LossCategory::Geometry,
                Severity::Warning,
                format!(
                    "{} standalone unknown geometry carrier(s) were not written",
                    self.unsupported_standalone_geometry
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
                LossCategory::Geometry,
                Severity::Warning,
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
                LossCategory::Metadata,
                Severity::Info,
                format!(
                    "{reduced_pcurve_count} emitted coedge pcurve(s) carry native-only metadata not represented in STEP"
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
        let unwritten_pmi = self.ir.model.pmi.len().saturating_sub(self.written_pmi);
        if unwritten_pmi > 0 {
            self.loss(
                LossCategory::Attribute,
                Severity::Warning,
                format!("{unwritten_pmi} PMI annotation(s) were not written to STEP"),
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
                LossCategory::Material,
                Severity::Info,
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
                LossCategory::Metadata,
                Severity::Info,
                format!(
                    "{lossy_binding_metadata} appearance binding(s) carry source object or channel metadata not represented in STEP"
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
            .filter(|procedural| !self.written_procedural_surfaces.contains(&procedural.id.0))
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
                | ProceduralSurfaceDefinition::Subset { .. }
                | ProceduralSurfaceDefinition::ParallelOffset { .. }
                | ProceduralSurfaceDefinition::DegenerateTorus { .. }
                | ProceduralSurfaceDefinition::CurveBounded { .. }
                | ProceduralSurfaceDefinition::Ruled { .. }
                | ProceduralSurfaceDefinition::Blend { .. }
                | ProceduralSurfaceDefinition::Unknown { .. } => true,
            })
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
                LossCategory::Geometry,
                Severity::Info,
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
                LossCategory::Metadata,
                Severity::Info,
                format!(
                    "{source_native_records} source-native record(s) were not represented in STEP"
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
            notes: self.notes.clone(),
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
        let (decoded, opaque_offsets) = reader::inspect_exchange(&bytes, &exchange);
        let mut entries = vec![ContainerEntry {
            name: "HEADER".into(),
            role: "metadata".into(),
            compression: "none".into(),
            compressed_size: 0,
            uncompressed_size: 0,
            attributes: BTreeMap::default(),
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
                attributes: BTreeMap::default(),
            });
        }
        let schema = exchange
            .header
            .iter()
            .find(|record| record.name == "FILE_SCHEMA")
            .map_or_else(
                || "unspecified".into(),
                |record| {
                    fn strings(value: &parse::Value, out: &mut Vec<String>) {
                        match value {
                            parse::Value::String(bytes) => {
                                if let Ok(value) = strings::decode(bytes) {
                                    out.push(value);
                                }
                            }
                            parse::Value::List(values) => {
                                for value in values {
                                    strings(value, out);
                                }
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
                },
            );
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
        reader::decode(&bytes, *options)
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
    let determinant = columns[0][0]
        * (columns[1][1] * columns[2][2] - columns[1][2] * columns[2][1])
        - columns[1][0] * (columns[0][1] * columns[2][2] - columns[0][2] * columns[2][1])
        + columns[2][0] * (columns[0][1] * columns[1][2] - columns[0][2] * columns[1][1]);
    (determinant - 1.0).abs() <= EPSILON
}

#[cfg(test)]
mod tests;
