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
//! Review [`StepReport::losses`] before retaining the output. Export continues
//! when an IR fact has no representation in this writer. Unknown-surface faces
//! and edges without typed 3D curves are omitted; pcurves, presentation data,
//! source attributes, passthrough records, and parametric history are also
//! reported rather than emitted. Body transforms remain unapplied, leaving
//! affected coordinates in body-local space.
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

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;

use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::report::{LossCategory, LossNote, Severity};
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

/// Summary of a completed STEP export.
///
/// A report is returned only after the entire file reaches the output sink.
/// Loss notes describe omitted or reduced IR content and do not prevent export.
#[derive(Debug, Clone, PartialEq)]
pub struct StepReport {
    /// DATA instance counts keyed by entity keyword.
    ///
    /// Keys are sorted. Complex rational B-spline instances are counted under
    /// their `B_SPLINE_*_WITH_KNOTS` keyword.
    pub entity_counts: BTreeMap<String, usize>,
    /// Total DATA instances, including product, context, and geometry records.
    pub total_entities: usize,
    /// Omitted, normalized, or reduced IR content.
    ///
    /// Notes use [`cadmpeg_ir::LossNote`] categories and severities. Callers
    /// should inspect the complete list rather than relying only on
    /// [`Self::error_count`].
    pub losses: Vec<LossNote>,
}

impl StepReport {
    /// Counts loss notes whose severity is at least [`Severity::Error`].
    pub fn error_count(&self) -> usize {
        self.losses
            .iter()
            .filter(|l| l.severity >= Severity::Error)
            .count()
    }
}

/// Failure returned while streaming STEP output.
///
/// Unsupported or reduced IR content appears in [`StepReport::losses`] after a
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
    w: &mut impl Write,
    opts: &StepWriteOptions,
) -> Result<StepReport, StepError> {
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

fn write_header(w: &mut impl Write, opts: &StepWriteOptions) -> std::io::Result<()> {
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

        self.note_unrepresented();
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
        Some(self.emitter.emit(
            "ADVANCED_FACE",
            &format!("'',{},{surf_ref},{flag}", refs(&bound_refs)),
        ))
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
        if !self.ir.unknowns.is_empty() {
            self.loss(
                LossCategory::Metadata,
                Severity::Info,
                format!(
                    "{} uninterpreted passthrough record(s) were not represented in STEP",
                    self.ir.unknowns.len()
                ),
            );
        }
        // Colors carried on bodies/faces are not mapped to STEP presentation
        // (styled_item/colour_rgb) in this writer.
        let colored = self
            .ir
            .model
            .bodies
            .iter()
            .filter(|b| b.color.is_some())
            .count()
            + self
                .ir
                .model
                .faces
                .iter()
                .filter(|f| f.color.is_some())
                .count();
        if colored > 0 {
            self.loss(
                LossCategory::Attribute,
                Severity::Info,
                format!("{colored} display color(s) were not written to STEP presentation"),
            );
        }
        if !self.ir.model.appearances.is_empty() || !self.ir.model.appearance_bindings.is_empty() {
            self.loss(
                LossCategory::Material,
                Severity::Info,
                format!(
                    "{} appearance asset(s) and {} binding(s) were not written to STEP presentation",
                    self.ir.model.appearances.len(),
                    self.ir.model.appearance_bindings.len()
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
                ProceduralSurfaceDefinition::Extrusion { .. }
                | ProceduralSurfaceDefinition::Revolution { .. }
                | ProceduralSurfaceDefinition::Sum { .. }
                | ProceduralSurfaceDefinition::Sweep { .. }
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

    fn finish_report(&self) -> StepReport {
        StepReport {
            entity_counts: self.emitter.counts().clone(),
            total_entities: self.emitter.total(),
            losses: self.losses.clone(),
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
