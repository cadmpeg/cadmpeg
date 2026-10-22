// SPDX-License-Identifier: Apache-2.0
//! Native Rhino 3DM archive writing.

use crate::mesh::FaceIndexWidth;
use std::io::{Seek, SeekFrom, Write};

mod attributes;
mod compounds;
mod loose;
mod model;
mod placement;
mod presentation;
mod reduce;
mod scopes;
mod tables;
pub(crate) mod target;
mod trim_iso;
mod trims;
use model::{
    WritableEdge, WritableEdgeCurve, WritableFaceSurface, WritableModel, WritableObjectCurve,
    WritablePcurve, WritableVertex,
};

use crate::loss::RhinoLossCode;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::LoopBoundaryRole;
use sha2::{Digest, Sha256};

use crate::chunks::{MAGIC, TCODE_ENDOFFILE, TCODE_SHORT};
use crate::RhinoArchiveVersion;

const EPS_WRITE_DEGENERATE: f64 = 1.0e-10;
const PLANE_PARAMETER_DOMAIN: [f64; 2] = [-1.0, 1.0];

/// Returns the IR-stated linear tolerance that governs `owner`.
///
/// An absent per-entity tolerance defers to the document linear tolerance.
/// Rhino Brep write routes compare evaluated geometry against this tolerance.
/// A stated tolerance below `EPS_WRITE_DEGENERATE` is finer than the write
/// route resolves, so the route refuses the document instead of widening the
/// stated value.
fn stated_write_tolerance(
    stated: Option<cadmpeg_ir::scalar::PositiveReal>,
    ir: &CadIr,
    owner: &str,
) -> Result<f64, CodecError> {
    let (field, value) = stated.map_or_else(
        || ("document linear tolerance", ir.tolerances.linear.get()),
        |tolerance| ("tolerance", tolerance.get()),
    );
    if value >= EPS_WRITE_DEGENERATE {
        return Ok(value);
    }
    Err(CodecError::UnsupportedContent(format!(
        "{field} {value} of {owner} is finer than the Rhino Brep write bound {EPS_WRITE_DEGENERATE}"
    )))
}

trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

const TCODE_PROPERTIES_TABLE: u32 = 0x1000_0014;
const TCODE_SETTINGS_TABLE: u32 = 0x1000_0015;
const TCODE_BITMAP_TABLE: u32 = 0x1000_0016;
const TCODE_TEXTURE_MAPPING_TABLE: u32 = 0x1000_0025;
const TCODE_MATERIAL_TABLE: u32 = 0x1000_0010;
const TCODE_LINETYPE_TABLE: u32 = 0x1000_0023;
const TCODE_LAYER_TABLE: u32 = 0x1000_0011;
const TCODE_GROUP_TABLE: u32 = 0x1000_0018;
const TCODE_FONT_TABLE: u32 = 0x1000_0019;
const TCODE_DIMSTYLE_TABLE: u32 = 0x1000_0020;
const TCODE_LIGHT_TABLE: u32 = 0x1000_0012;
const TCODE_HATCH_PATTERN_TABLE: u32 = 0x1000_0022;
const TCODE_INSTANCE_DEFINITION_TABLE: u32 = 0x1000_0021;
const TCODE_OBJECT_TABLE: u32 = 0x1000_0013;
const TCODE_HISTORY_RECORD_TABLE: u32 = 0x1000_0026;
const TCODE_ENDOFTABLE: u32 = 0xffff_ffff;
const TCODE_PROPERTIES_OPENNURBS_VERSION: u32 = 0xa000_0026;
const OPENNURBS_WRITER_VERSION: i64 = 0xa000_0026;
const TCODE_UNITS_AND_TOLERANCES: u32 = 0x2000_8031;
const TCODE_LAYER_RECORD: u32 = 0x2000_8050;
const TCODE_OBJECT_RECORD: u32 = 0x2000_8070;
const TCODE_OBJECT_RECORD_TYPE: u32 = 0x0200_0071;
const TCODE_OBJECT_RECORD_ATTRIBUTES: u32 = 0x0200_8072;
const TCODE_OBJECT_RECORD_END: u32 = 0x0200_007f;
const TCODE_CLASS_WRAPPER: u32 = 0x0002_7ffa;
const TCODE_CLASS_UUID: u32 = 0x0002_fffb;
const TCODE_CLASS_DATA: u32 = 0x0002_fffc;
const TCODE_CLASS_END: u32 = 0x0002_7fff;

const POINT_CLASS: [u8; 16] = [
    0x1d, 0x1a, 0x10, 0xc3, 0x57, 0xf1, 0xd3, 0x11, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const POINT_CLOUD_CLASS: [u8; 16] = [
    0x47, 0xf3, 0x88, 0x24, 0xfa, 0xf8, 0xd3, 0x11, 0xbf, 0xec, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const LINE_CLASS: [u8; 16] = [
    0xdb, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const BREP_CLASS: [u8; 16] = [
    0xc5, 0xdb, 0xb5, 0x60, 0x60, 0xe6, 0xd3, 0x11, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const ARC_CLASS: [u8; 16] = [
    0x2a, 0xbe, 0x33, 0xcf, 0xb4, 0x09, 0xd4, 0x11, 0xbf, 0xfb, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const NURBS_CURVE_CLASS: [u8; 16] = [
    0xdd, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const NURBS_SURFACE_CLASS: [u8; 16] = [
    0xde, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const PLANE_SURFACE_CLASS: [u8; 16] = [
    0xdf, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const MESH_CLASS: [u8; 16] = [
    0xe4, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const LAYER_CLASS: [u8; 16] = [
    0x13, 0x98, 0x80, 0x95, 0x85, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];
const CHANNEL_UV: u32 = 0x5248_0001;
const CHANNEL_COLOR: u32 = 0x5248_0002;
const CHANNEL_SURFACE_PARAMETERS: u32 = 0x5248_0003;
const CHANNEL_CURVATURE: u32 = 0x5248_0004;
const DEFAULT_RELATIVE_TOLERANCE: f64 = 0.01;

fn is_support(source: Option<&cadmpeg_ir::SourceObjectAssociation>) -> bool {
    source
        .is_some_and(|source| source.geometry_role == Some(cadmpeg_ir::SourceGeometryRole::Support))
}

struct WriteOutcome {
    losses: Vec<LossNote>,
    objects: usize,
}

fn write(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    version: RhinoArchiveVersion,
    output: &mut dyn Write,
) -> Result<WriteOutcome, CodecError> {
    let mut staged = tempfile::tempfile()?;
    let losses = write_seekable(ctx, ir, version, &mut staged)?;
    staged.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut staged, output)?;
    Ok(losses)
}

fn write_seekable(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    version: RhinoArchiveVersion,
    output: &mut dyn WriteSeek,
) -> Result<WriteOutcome, CodecError> {
    let mut plan = prepare_write(ctx, ir, version)?;
    plan.losses.extend(write_archive_prefix(
        ctx,
        ir,
        version,
        &plan.written_bodies,
        output,
    )?);
    let table_start = output.stream_position()?;
    output.write_all(&TCODE_OBJECT_TABLE.to_le_bytes())?;
    output.write_all(&0_i64.to_le_bytes())?;
    let body_start = output.stream_position()?;

    plan.brep_records.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut plan.brep_records, output)?;
    for point in ir.model.points.iter().filter(|point| {
        !plan.topology_points.contains(point.id.as_str())
            && !is_support(point.source_object.as_ref())
    }) {
        let position = point.position().get();
        let mut payload = vec![0x10];
        payload.extend(position.x.to_le_bytes());
        payload.extend(position.y.to_le_bytes());
        payload.extend(position.z.to_le_bytes());
        let display = presentation::display(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Point(point.id.clone()),
            point.source_object.as_ref(),
        );
        ctx.charge_entities(1, "write Rhino object record")?;
        output.write_all(&attributed_object_record(
            ir,
            1,
            POINT_CLASS,
            &payload,
            point.id.as_str(),
            display,
        )?)?;
    }
    for (id, payload) in &plan.curves {
        let curve = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == *id)
            .ok_or_else(|| CodecError::malformed("prepared curve is missing"))?;
        let display = presentation::display(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Curve(curve.id.clone()),
            curve.source_object.as_ref(),
        );
        ctx.charge_entities(1, "write Rhino object record")?;
        output.write_all(&curve_object_record(ir, payload, id, display)?)?;
    }
    for (id, (class, payload)) in &plan.surfaces {
        let surface = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == *id)
            .ok_or_else(|| CodecError::malformed("prepared surface is missing"))?;
        let display = presentation::display(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Surface(surface.id.clone()),
            surface.source_object.as_ref(),
        );
        ctx.charge_entities(1, "write Rhino object record")?;
        output.write_all(&attributed_object_record(
            ir, 8, *class, payload, id, display,
        )?)?;
    }
    for mesh in &plan.meshes {
        let payload = mesh_payload(mesh, version)?;
        let display = presentation::display(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Tessellation(mesh.id.to_string()),
            mesh.source_object.as_ref(),
        );
        ctx.charge_entities(1, "write Rhino object record")?;
        output.write_all(&mesh_object_record(
            ir,
            &payload,
            mesh.id.as_str(),
            display,
        )?)?;
    }

    let (references, occurrence_losses) = tables::products::references(ir)?;
    plan.losses.extend(occurrence_losses);
    for record in &references {
        ctx.charge_entities(1, "write Rhino object record")?;
        output.write_all(record)?;
    }
    let (annotations, annotation_losses) = tables::annotations::records(ir)?;
    plan.losses.extend(annotation_losses);
    for record in &annotations {
        ctx.charge_entities(1, "write Rhino object record")?;
        output.write_all(record)?;
    }

    output.write_all(&short_chunk(TCODE_ENDOFTABLE, 0))?;
    let table_end = output.stream_position()?;
    let body_len = archive_body_len(table_end - body_start)?;
    output.seek(SeekFrom::Start(table_start + 4))?;
    output.write_all(&body_len.to_le_bytes())?;
    output.seek(SeekFrom::Start(table_end))?;
    output.write_all(&table(ctx, TCODE_HISTORY_RECORD_TABLE, &[])?)?;
    let final_size = archive_final_size(output.stream_position()?)?;
    output.write_all(&long_chunk(TCODE_ENDOFFILE, &final_size.to_le_bytes())?)?;
    let free_points = ir
        .model
        .points
        .iter()
        .filter(|point| {
            !plan.topology_points.contains(point.id.as_str())
                && !is_support(point.source_object.as_ref())
        })
        .count();
    Ok(WriteOutcome {
        objects: plan.brep_count
            + free_points
            + plan.curves.len()
            + plan.surfaces.len()
            + plan.meshes.len()
            + annotations.len()
            + references.len(),
        losses: plan.losses,
    })
}

fn write_archive_prefix(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    version: RhinoArchiveVersion,
    written_bodies: &std::collections::BTreeSet<String>,
    output: &mut dyn WriteSeek,
) -> Result<Vec<LossNote>, CodecError> {
    output.write_all(&header(version))?;
    output.write_all(&long_chunk(1, b"cadmpeg")?)?;
    let mut properties = vec![short_chunk(
        TCODE_PROPERTIES_OPENNURBS_VERSION,
        OPENNURBS_WRITER_VERSION,
    )];
    let documents = tables::document::properties(ir)?;
    properties.extend(documents.records);
    output.write_all(&table(ctx, TCODE_PROPERTIES_TABLE, &properties)?)?;
    let mut settings = vec![units_record(
        ir.tolerances.linear.get(),
        ir.tolerances.angular.get(),
    )?];
    settings.extend(tables::document::settings(ir)?);
    let views = tables::views::settings(ir)?;
    settings.extend(views.settings);
    output.write_all(&table(ctx, TCODE_SETTINGS_TABLE, &settings)?)?;
    for typecode in [
        TCODE_BITMAP_TABLE,
        TCODE_TEXTURE_MAPPING_TABLE,
        TCODE_MATERIAL_TABLE,
        TCODE_LINETYPE_TABLE,
        TCODE_GROUP_TABLE,
        TCODE_FONT_TABLE,
        TCODE_DIMSTYLE_TABLE,
        TCODE_LIGHT_TABLE,
        TCODE_HATCH_PATTERN_TABLE,
        TCODE_INSTANCE_DEFINITION_TABLE,
    ] {
        let records = match typecode {
            TCODE_BITMAP_TABLE => tables::images::images(ir)?,
            TCODE_LIGHT_TABLE => tables::lights::lights(ir)?,
            TCODE_FONT_TABLE => tables::fonts::styles(ir)?,
            TCODE_HATCH_PATTERN_TABLE => tables::hatches::patterns(ir)?,
            TCODE_INSTANCE_DEFINITION_TABLE => tables::products::definitions(ir, written_bodies)?,
            TCODE_MATERIAL_TABLE => tables::materials::materials(ir)?,
            TCODE_GROUP_TABLE => tables::native::groups(ir)?,
            TCODE_LINETYPE_TABLE => tables::native::linetypes(ir)?,
            _ => Vec::new(),
        };
        output.write_all(&table(ctx, typecode, &records)?)?;
        if typecode == TCODE_LINETYPE_TABLE {
            output.write_all(&table(ctx, TCODE_LAYER_TABLE, &tables::layers(ir)?)?)?;
        }
    }
    let mut losses = documents.losses;
    losses.extend(views.losses);
    Ok(losses)
}

struct BrepPayload {
    body: Vec<u8>,
    direct: Vec<u8>,
}

/// A native class identifier and the payload encoded for it.
type ClassPayload = ([u8; 16], Vec<u8>);

struct WritePlan<'a> {
    losses: Vec<LossNote>,
    meshes: Vec<cadmpeg_ir::tessellation::Tessellation>,
    /// Each free curve's identity and encoded payload.
    curves: Vec<(&'a str, compounds::CurvePayload)>,
    /// Each free surface's identity and encoded payload.
    surfaces: Vec<(&'a str, ClassPayload)>,
    brep_records: std::fs::File,
    brep_count: usize,
    written_bodies: std::collections::BTreeSet<String>,
    topology_points: std::collections::BTreeSet<String>,
}

fn content_limitation(error: &CodecError) -> bool {
    matches!(error, CodecError::UnsupportedContent(_))
}

fn prepare_write<'a>(
    ctx: &DecodeContext<'_>,
    ir: &'a CadIr,
    archive_version: RhinoArchiveVersion,
) -> Result<WritePlan<'a>, CodecError> {
    if ir.tolerances.angular.get() > std::f64::consts::PI {
        return Err(CodecError::InvalidInput(
            "Rhino angular tolerance must not exceed pi".into(),
        ));
    }
    let model = &ir.model;
    let mut losses = attributes::name_losses(ir);
    native_i32_count(
        model.points.len(),
        "point arena exceeds native counts".into(),
    )?;
    let scopes = scopes::collect(ir)?;
    let used_pcurves = scopes
        .iter()
        .flat_map(|scope| scope.pcurves.iter())
        .collect::<std::collections::BTreeSet<_>>();
    if used_pcurves.len() != model.pcurves.len() {
        losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
            "{} unowned pcurve record(s) omitted",
            model.pcurves.len() - used_pcurves.len()
        )));
    }
    let topology_curves = scopes
        .iter()
        .flat_map(|scope| scope.curves.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    let topology_surfaces = scopes
        .iter()
        .flat_map(|scope| scope.surfaces.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    let scoped_count = |select: fn(&cadmpeg_ir::document::Model) -> usize| {
        scopes
            .iter()
            .map(|scope| select(&scope.ir.model))
            .sum::<usize>()
    };
    if scoped_count(|model| model.regions.len()) != model.regions.len()
        || scoped_count(|model| model.shells.len()) != model.shells.len()
        || scoped_count(|model| model.faces.len()) != model.faces.len()
        || scoped_count(|model| model.loops.len()) != model.loops.len()
        || scoped_count(|model| model.coedges.len()) != model.coedges.len()
        || scoped_count(|model| model.edges.len()) != model.edges.len()
        || scoped_count(|model| model.vertices.len()) != model.vertices.len()
    {
        losses.push(
            RhinoLossCode::WriterEntityOmitted
                .note("topology outside owned body closures is omitted"),
        );
    }
    let topology_points = scopes
        .iter()
        .flat_map(|scope| scope.points.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    compounds::validate(ir)?;
    let compound_children = compounds::children(ir);
    let mut curves = Vec::new();
    let mut pending = model
        .curves
        .iter()
        .filter(|curve| {
            !compound_children.contains(&curve.id)
                && !topology_curves.contains(curve.id.as_str())
                && !is_support(curve.source_object.as_ref())
        })
        .collect::<std::collections::VecDeque<_>>();
    let mut covered = std::collections::BTreeSet::new();
    let mut attempted = std::collections::BTreeSet::new();
    while let Some(curve) = pending.pop_front() {
        if covered.contains(&curve.id) || !attempted.insert(curve.id.clone()) {
            continue;
        }
        match compounds::payload(ir, curve, &mut std::collections::BTreeSet::new()) {
            Ok((payload, reduced)) => {
                curves.push((curve.id.as_str(), payload));
                let mut represented = compounds::components(ir, curve);
                while let Some(child) = represented.pop() {
                    if covered.insert(child.id.clone()) {
                        represented.extend(compounds::components(ir, child));
                    }
                }
                if reduced {
                    losses.push(RhinoLossCode::WriterProceduralReduced.note(format!(
                        "curve {} compound construction was reduced to a held solved carrier",
                        curve.id
                    )));
                }
            }
            Err(error) if content_limitation(&error) => {
                losses.push(
                    RhinoLossCode::WriterEntityOmitted
                        .note(format!("curve {} omitted: {error}", curve.id)),
                );
                // Retain writable constituent curves when their enclosing
                // construction has no exact native representation or cache.
                pending.extend(
                    compounds::components(ir, curve)
                        .into_iter()
                        .filter(|child| !topology_curves.contains(child.id.as_str())),
                );
            }
            Err(error) => return Err(error),
        }
    }
    let mut surfaces = Vec::new();
    for surface in model
        .surfaces
        .iter()
        .filter(|surface| !topology_surfaces.contains(surface.id.as_str()))
    {
        match WritableFaceSurface::try_new(surface).and_then(model::WritableFaceSurface::payload) {
            Ok(payload) => surfaces.push((surface.id.as_str(), payload)),
            Err(error) if content_limitation(&error) => losses.push(
                RhinoLossCode::WriterEntityOmitted
                    .note(format!("surface {} omitted: {error}", surface.id)),
            ),
            Err(error) => return Err(error),
        }
    }
    let mut meshes = Vec::new();
    for source in &model.tessellations {
        let mut mesh = source.clone();
        if let Some(body_id) = &mesh.body {
            let mut body = model
                .bodies
                .iter()
                .find(|body| body.id == *body_id)
                .cloned()
                .ok_or_else(|| CodecError::malformed("tessellation references missing body"))?;
            if !body.transform.unwrap_or_default().is_proper_rigid() {
                losses.push(RhinoLossCode::WriterBodyTransformNotApplied.note(format!(
                    "mesh {} non-rigid body placement omitted; geometry is written in its local frame", mesh.id)));
                body.transform = None;
            }
            body.regions.clear();
            let mut placed = CadIr::empty();
            placed.model.bodies.push(body);
            placed.model.tessellations.push(mesh);
            match placement::bake(&mut placed) {
                Ok(()) => mesh = placed.model.tessellations.remove(0),
                Err(error) if content_limitation(&error) => {
                    losses.push(
                        RhinoLossCode::WriterEntityOmitted
                            .note(format!("mesh {} omitted: {error}", source.id)),
                    );
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        match check_mesh(&mesh) {
            Ok(()) => meshes.push(mesh),
            Err(error) if content_limitation(&error) => losses.push(
                RhinoLossCode::WriterEntityOmitted
                    .note(format!("mesh {} omitted: {error}", mesh.id)),
            ),
            Err(error) => return Err(error),
        }
    }
    let mut brep_records = tempfile::tempfile()?;
    let mut brep_count = 0;
    let mut written_bodies = std::collections::BTreeSet::new();
    for scope in &scopes {
        let mut baked = scope.ir.clone();
        let body = &mut baked.model.bodies[0];
        presentation::apply(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Body(body.id.clone()),
            &mut body.color,
            &mut body.visible,
        );
        let placement = body.transform.unwrap_or_default();
        if !placement.is_proper_rigid() {
            losses.push(RhinoLossCode::WriterBodyTransformNotApplied.note(format!(
                "body {} non-rigid placement omitted; geometry is written in its local frame",
                body.id
            )));
            body.transform = None;
        }
        match placement::bake(&mut baked) {
            Ok(()) => {}
            Err(error) if content_limitation(&error) => {
                losses.push(RhinoLossCode::WriterBodyOmitted.note(format!(
                    "body {} omitted: {error}",
                    baked.model.bodies[0].id
                )));
                continue;
            }
            Err(error) => return Err(error),
        }
        brep_count += loose::write(ctx, &baked, &mut brep_records, &mut losses)?;
        if baked.model.faces.is_empty() {
            continue;
        }
        let selected = baked
            .model
            .faces
            .iter()
            .map(|face| face.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let has_loose_members = baked
            .model
            .shells
            .iter()
            .any(|shell| !shell.wire_edges().is_empty() || !shell.free_vertices().is_empty());
        let general = baked.model.bodies[0].kind == cadmpeg_ir::topology::BodyKind::General;
        if has_loose_members || general {
            losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                "body {} dimensional grouping is reduced to a face Brep and independent wire/point objects",
                baked.model.bodies[0].id
            )));
        }
        baked = reduce::retain_faces(&baked, &selected)?;
        if general {
            baked.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Sheet;
        }
        let payload = match WritableModel::try_new(&baked)
            .and_then(|model| brep_payload(ctx, &model, archive_version))
        {
            Ok(payload) => payload,
            Err(error)
                if content_limitation(&error)
                    && baked.model.bodies[0].kind == cadmpeg_ir::topology::BodyKind::Sheet =>
            {
                let last_payload = std::cell::RefCell::new(None);
                let (reduced, reduction_losses) = reduce::sheet(&baked, |candidate| {
                    let payload = WritableModel::try_new(candidate)
                        .and_then(|model| brep_payload(ctx, &model, archive_version))?;
                    last_payload.replace(Some(payload));
                    Ok(())
                })?;
                losses.extend(reduction_losses);
                let Some(reduced) = reduced else {
                    continue;
                };
                baked = reduced;
                // (C) Successful admission must supply the staged payload.
                last_payload
                    .into_inner()
                    .ok_or_else(|| CodecError::malformed("sheet admission produced no payload"))?
            }
            Err(error) if content_limitation(&error) => {
                losses.push(RhinoLossCode::WriterBodyOmitted.note(format!(
                    "body {} omitted: {error}",
                    baked.model.bodies[0].id
                )));
                continue;
            }
            Err(error) => return Err(error),
        };
        let body = &baked.model.bodies[0];
        written_bodies.insert(body.id.to_string());
        brep_count += 1;
        ctx.charge_entities(1, "write Rhino object record")?;
        brep_records.write_all(&brep_object_record(
            ir,
            &payload,
            body.id.as_str(),
            body.name.as_deref(),
            body.color,
            body.visible,
        )?)?;
    }
    Ok(WritePlan {
        losses,
        meshes,
        curves,
        surfaces,
        brep_records,
        brep_count,
        written_bodies,
        topology_points,
    })
}

fn native_dispositions(
    ir: &CadIr,
) -> Result<cadmpeg_ir::codec::write::NativeDispositions, CodecError> {
    use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};
    let mut dispositions = NativeDispositions::for_namespace(
        "rhino",
        crate::native_registry::ARENAS,
        NativeDisposition::Dropped,
    );
    if let Some(records) = ir
        .native
        .namespace("rhino")
        .and_then(|namespace| namespace.arenas().get("layers"))
    {
        for record in records {
            if ir.model.presentation_layers.iter().any(|value| {
                value.native_ref.as_deref() == Some(record.id()) && value.description.is_none()
            }) && tables::native_layer_complete(record)
            {
                dispositions.set_record(
                    "rhino",
                    "layers",
                    record.id(),
                    NativeDisposition::Regenerated,
                );
            }
        }
    }
    if let Some(namespace) = ir.native.namespace("rhino") {
        for arena in ["groups", "linetypes", "materials", "text_styles"] {
            for record in namespace.arenas().get(arena).into_iter().flatten() {
                let disposition = match arena {
                    "groups" if tables::native::group_resolves(ir, record) => {
                        NativeDisposition::Written
                    }
                    "linetypes" => NativeDisposition::Written,
                    "text_styles" if tables::fonts::complete(record) => NativeDisposition::Written,
                    "materials"
                        if tables::materials::native_complete(record)
                            && ir
                                .model
                                .appearances
                                .iter()
                                .filter(|appearance| {
                                    appearance.native_ref.as_deref() == Some(record.id())
                                })
                                .all(|appearance| appearance.textures.is_empty()) =>
                    {
                        if ir
                            .model
                            .appearances
                            .iter()
                            .any(|appearance| appearance.native_ref.as_deref() == Some(record.id()))
                        {
                            NativeDisposition::Regenerated
                        } else {
                            NativeDisposition::Written
                        }
                    }
                    _ => NativeDisposition::Dropped,
                };
                dispositions.set_record("rhino", arena, record.id(), disposition);
            }
        }
    }
    let views = tables::views::settings(ir)?;
    for arena in ["views", "construction_planes"] {
        for record in ir
            .native
            .namespace("rhino")
            .and_then(|namespace| namespace.arenas().get(arena))
            .into_iter()
            .flatten()
        {
            if views.regenerated.contains(record.id()) {
                dispositions.set_record(
                    "rhino",
                    arena,
                    record.id(),
                    NativeDisposition::Regenerated,
                );
            } else if views.written.contains(record.id()) {
                dispositions.set_record("rhino", arena, record.id(), NativeDisposition::Written);
            }
        }
    }
    let properties = tables::document::properties(ir)?;
    for arena in ["revisions", "document_notes", "applications"] {
        for record in ir
            .native
            .namespace("rhino")
            .and_then(|namespace| namespace.arenas().get(arena))
            .into_iter()
            .flatten()
        {
            if properties.regenerated.contains(record.id()) {
                dispositions.set_record(
                    "rhino",
                    arena,
                    record.id(),
                    NativeDisposition::Regenerated,
                );
            } else if properties.written.contains(record.id()) {
                dispositions.set_record("rhino", arena, record.id(), NativeDisposition::Written);
            }
        }
    }
    Ok(dispositions)
}

fn brep_payload(
    ctx: &DecodeContext<'_>,
    model: &WritableModel<'_>,
    archive_version: RhinoArchiveVersion,
) -> Result<BrepPayload, CodecError> {
    use cadmpeg_ir::topology::{BodyKind, Sense};

    let version = if archive_version.uses_extended_brep_layout() {
        0x33
    } else {
        0x32
    };
    let mut payload = vec![version];
    let mut direct = vec![version];
    payload.extend(polymorphic_array(
        model.coedges.iter().map(|coedge| &coedge.c2),
    )?);
    let c3 = model
        .edges
        .iter()
        .map(|edge| brep_c3_curve(model, edge))
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(polymorphic_array(c3.iter())?);
    let surfaces = model
        .surfaces
        .iter()
        .map(|surface| surface.payload())
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(polymorphic_array(surfaces.iter())?);
    let vertices = model
        .vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| {
            let incident =
                wire_indexes(model.edges.iter().enumerate().flat_map(|(position, edge)| {
                    [edge.start, edge.end]
                        .into_iter()
                        .filter(move |endpoint| *endpoint == index)
                        .map(move |_| position)
                }))?;
            let mut record = wire_index(index)?.to_le_bytes().to_vec();
            for value in [vertex.point.x, vertex.point.y, vertex.point.z] {
                record.extend(value.to_le_bytes());
            }
            record.extend(indexes(&incident)?);
            record.extend(
                vertex
                    .source
                    .tolerance
                    .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get)
                    .to_le_bytes(),
            );
            Ok(record)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(raw_array(&vertices)?);
    let edges = model
        .edges
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            let index = wire_index(index)?;
            let mut record = index.to_le_bytes().to_vec();
            record.extend(index.to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            record.extend(
                edge.domain
                    .into_iter()
                    .flat_map(|value| value.get().to_le_bytes()),
            );
            record.extend(wire_index(edge.start)?.to_le_bytes());
            record.extend(wire_index(edge.end)?.to_le_bytes());
            record.extend(indexes(&wire_indexes(edge.uses.iter().copied())?)?);
            record.extend(
                edge.source
                    .tolerance
                    .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get)
                    .to_le_bytes(),
            );
            record.extend(
                edge.domain
                    .into_iter()
                    .flat_map(|value| value.get().to_le_bytes()),
            );
            Ok(record)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(raw_array(&edges)?);
    let trims = model
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| {
            let edge = &model.edges[coedge.edge];
            let (from, to) = model.endpoints(index);
            let index = wire_index(index)?;
            let mut record = index.to_le_bytes().to_vec();
            record.extend(index.to_le_bytes());
            record.extend(
                edge.domain
                    .into_iter()
                    .flat_map(|value| value.get().to_le_bytes()),
            );
            record.extend(wire_index(coedge.edge)?.to_le_bytes());
            record.extend(wire_index(from)?.to_le_bytes());
            record.extend(wire_index(to)?.to_le_bytes());
            record.extend(i32::from(coedge.source.sense == Sense::Reversed).to_le_bytes());
            let same_loop = edge.uses.len() == 2
                && model.coedges[edge.uses[0]].owner_loop == model.coedges[edge.uses[1]].owner_loop;
            record.extend(brep_trim_type(edge.uses.len(), same_loop).to_le_bytes());
            record.extend(trim_iso::classify(model, coedge)?.to_le_bytes());
            record.extend(wire_index(coedge.owner_loop)?.to_le_bytes());
            record.extend(
                [coedge.fit_tolerance, 0.0_f64]
                    .into_iter()
                    .flat_map(f64::to_le_bytes),
            );
            record.extend(
                edge.domain
                    .into_iter()
                    .flat_map(|value| value.get().to_le_bytes()),
            );
            record.push(0);
            record.extend([0_u8; 31]);
            record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
            Ok(record)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(raw_array(&trims)?);
    let loops = model
        .loops
        .iter()
        .enumerate()
        .map(|(index, loop_)| {
            let face = &model.faces[loop_.face];
            let mut record = wire_index(index)?.to_le_bytes().to_vec();
            record.extend(indexes(&wire_indexes(loop_.coedges.iter().copied())?)?);
            record.extend(
                brep_loop_type(
                    face.source.loop_role(&loop_.source.id),
                    face.loops.first() == Some(&index),
                )
                .to_le_bytes(),
            );
            record.extend(wire_index(loop_.face)?.to_le_bytes());
            Ok(record)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(raw_array(&loops)?);
    let faces = model
        .faces
        .iter()
        .enumerate()
        .map(|(index, face)| {
            let mut record = wire_index(index)?.to_le_bytes().to_vec();
            record.extend(indexes(&wire_indexes(face.loops.iter().copied())?)?);
            record.extend(wire_index(face.surface)?.to_le_bytes());
            record.extend(i32::from(face.source.sense == Sense::Reversed).to_le_bytes());
            record.extend(0_i32.to_le_bytes());
            Ok(record)
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    payload.extend(face_array(
        &faces,
        &model
            .faces
            .iter()
            .map(|face| face.source)
            .collect::<Vec<_>>(),
        archive_version,
    )?);
    let min = model.vertices.iter().fold([f64::INFINITY; 3], |a, vertex| {
        let p = vertex.point;
        [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
    });
    let max = model
        .vertices
        .iter()
        .fold([f64::NEG_INFINITY; 3], |a, vertex| {
            let p = vertex.point;
            [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
        });
    for value in min.into_iter().chain(max) {
        let bytes = value.to_le_bytes();
        payload.extend(bytes);
        direct.extend(bytes);
    }
    let mesh_presence = ctx.alloc_filled(model.faces.len(), 0_u8, "Rhino Brep mesh presence")?;
    payload.extend(crc_chunk(0x4000_8000, &mesh_presence)?);
    payload.extend(crc_chunk(0x4000_8000, &mesh_presence)?);
    let solid = if model.body.kind == BodyKind::Solid {
        planar_solid_orientation(model)
    } else {
        0
    }
    .to_le_bytes();
    payload.extend(solid);
    direct.extend(solid);
    if archive_version.uses_extended_brep_layout() {
        payload.extend(empty_region_wrapper()?);
    }
    Ok(BrepPayload {
        body: payload,
        direct,
    })
}

fn brep_trim_type(edge_use_count: usize, two_uses_in_one_loop: bool) -> i32 {
    if edge_use_count == 1 {
        1
    } else if two_uses_in_one_loop {
        3
    } else {
        2
    }
}

fn brep_loop_type(role: LoopBoundaryRole, first_on_face: bool) -> i32 {
    match role {
        LoopBoundaryRole::Outer => 1,
        LoopBoundaryRole::Inner => 2,
        LoopBoundaryRole::Unspecified if first_on_face => 1,
        LoopBoundaryRole::Unspecified => 2,
    }
}

fn planar_solid_orientation(model: &WritableModel<'_>) -> i32 {
    let Some(reference) = model.vertices.first().map(|vertex| vertex.point) else {
        return 0;
    };
    let scale = model
        .vertices
        .iter()
        .map(|vertex| {
            let delta = vertex.point.vector_from(reference);
            delta.x.abs().max(delta.y.abs()).max(delta.z.abs())
        })
        .fold(0.0, f64::max);
    if !scale.is_finite() || scale == 0.0 {
        return 0;
    }
    let local = |point: cadmpeg_ir::math::Point3| {
        let delta = point.vector_from(reference);
        cadmpeg_ir::math::Vector3::new(delta.x / scale, delta.y / scale, delta.z / scale)
    };
    let mut correction = 0.0;
    let mut volume6 = 0.0;
    for loop_ in &model.loops {
        let ring = loop_
            .coedges
            .iter()
            .map(|coedge| model.vertices[model.endpoints(*coedge).0].point)
            .collect::<Vec<_>>();
        if let Some(origin) = ring.first() {
            for triangle in ring[1..].windows(2) {
                let a = local(triangle[0]);
                let b = local(triangle[1]);
                let term = local(*origin).dot(a.cross(b));
                let adjusted = term - correction;
                let total = volume6 + adjusted;
                correction = (total - volume6) - adjusted;
                volume6 = total;
            }
        }
    }
    if volume6 > 0.0 {
        1
    } else if volume6 < 0.0 {
        2
    } else {
        0
    }
}

fn close_point(
    left: cadmpeg_ir::math::Point3,
    right: cadmpeg_ir::math::Point3,
    tolerance: f64,
) -> bool {
    (left.x - right.x).abs() <= tolerance
        && (left.y - right.y).abs() <= tolerance
        && (left.z - right.z).abs() <= tolerance
}

fn brep_c3_curve(
    model: &WritableModel<'_>,
    edge: &WritableEdge<'_>,
) -> Result<([u8; 16], Vec<u8>), CodecError> {
    Ok(match edge.curve {
        WritableEdgeCurve::Line(_) => {
            let from = model.vertices[edge.start].point;
            let to = model.vertices[edge.end].point;
            (
                LINE_CLASS,
                bounded_line_payload(
                    [from.x, from.y, from.z],
                    [to.x, to.y, to.z],
                    FiniteReal::raw_array(edge.domain),
                    3,
                )?,
            )
        }
        WritableEdgeCurve::Nurbs(nurbs) => (NURBS_CURVE_CLASS, nurbs_curve_payload(nurbs)?),
    })
}

fn generated_projected_brep_c2_curve(
    vertices: &[WritableVertex<'_>],
    edge: &WritableEdge<'_>,
    sense: cadmpeg_ir::topology::Sense,
    origin: cadmpeg_ir::math::Point3,
    u_axis: cadmpeg_ir::math::Vector3,
    v_axis: cadmpeg_ir::math::Vector3,
) -> Result<([u8; 16], Vec<u8>), CodecError> {
    use cadmpeg_ir::topology::Sense;

    Ok(match edge.curve {
        WritableEdgeCurve::Line(_) => {
            let (from, to) = if sense == Sense::Forward {
                (edge.start, edge.end)
            } else {
                (edge.end, edge.start)
            };
            let from = vertices[from].point;
            let to = vertices[to].point;
            (
                LINE_CLASS,
                bounded_line_payload(
                    plane_uv(from, origin, u_axis, v_axis),
                    plane_uv(to, origin, u_axis, v_axis),
                    FiniteReal::raw_array(edge.domain),
                    2,
                )?,
            )
        }
        WritableEdgeCurve::Nurbs(nurbs) => {
            let mut projected = nurbs.clone();
            projected
                .try_map_control_points(|_, point| {
                    let mut point = point.get();
                    let uv = plane_uv(point, origin, u_axis, v_axis);
                    point = cadmpeg_ir::math::Point3::new(uv[0], uv[1], 0.0);
                    cadmpeg_ir::features::FinitePoint3::new(point).ok_or_else(|| {
                        cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                            "control_points contains a non-finite point".into(),
                        )
                    })
                })
                .map_err(|error| CodecError::UnsupportedContent(error.to_string()))?;
            if sense == Sense::Reversed {
                let sum = projected.knots()[usize::try_from(projected.degree()).map_err(|_| {
                    CodecError::Malformed("Rhino count exceeds address space".into())
                })?] + projected.knots()[projected.pole_count()];
                projected.reverse_parameterization();
                projected
                    .edit_knots(|knots| {
                        for knot in knots {
                            *knot += sum;
                        }
                    })
                    .map_err(|error| CodecError::UnsupportedContent(error.to_string()))?;
                canonicalize_native_curve_knots(&mut projected, edge.curve_id)?;
            }
            (
                NURBS_CURVE_CLASS,
                nurbs_curve_payload_dimension(&projected, 2)?,
            )
        }
    })
}

fn canonicalize_native_curve_knots(
    curve: &mut cadmpeg_ir::geometry::nurbs::NurbsCurve,
    id: &str,
) -> Result<(), CodecError> {
    let order = usize::try_from(curve.degree())
        .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?
        + 1;
    let count = curve.control_points().len();
    let stored = curve.knots()[1..curve.knots().len() - 1].to_vec();
    let reconstructed = crate::surfaces::reconstruct_knots(&stored, order, count)
        .map_err(|error| CodecError::UnsupportedContent(format!("curve {id}: {error}")))?;
    curve
        .edit_knots(|knots| knots.copy_from_slice(&reconstructed))
        .map_err(|error| CodecError::UnsupportedContent(format!("curve {id}: {error}")))?;
    Ok(())
}

fn admit_pcurve<'a>(
    edge: &WritableEdge<'_>,
    pcurve: &'a cadmpeg_ir::geometry::pcurve::Pcurve,
) -> Result<WritablePcurve<'a>, CodecError> {
    if pcurve.wrapper_reversed() == Some(true)
        || pcurve.native_tail_flags().is_some()
        || pcurve
            .parameter_range()
            .is_some_and(|range| range.finite_components() != edge.domain)
    {
        return Err(CodecError::UnsupportedContent(format!(
            "pcurve {} has unsupported wrapper, tail, domain, or tolerance state",
            pcurve.id.as_str()
        )));
    }
    let domain = FiniteReal::raw_array(edge.domain);
    let payload = match &pcurve.geometry {
        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line) => {
            let origin = line.origin().as_raw();
            let direction = line.direction().as_raw();
            let from = [
                origin.u + direction.u * domain[0],
                origin.v + direction.v * domain[0],
                0.0,
            ];
            let to = [
                origin.u + direction.u * domain[1],
                origin.v + direction.v * domain[1],
                0.0,
            ];
            (LINE_CLASS, bounded_line_payload(from, to, domain, 2)?)
        }
        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } => {
            let curve = nurbs
                .lift(|point| cadmpeg_ir::math::Point3::new(point.u, point.v, 0.0))
                .map_err(|error| {
                    CodecError::UnsupportedContent(format!(
                        "pcurve {}: {error}",
                        pcurve.id.as_str()
                    ))
                })?;
            check_nurbs_curve(pcurve.id.as_str(), &curve)?;
            let count = curve.pole_count();
            if curve.periodic()
                || [
                    curve.knots()[usize::try_from(curve.degree()).map_err(|_| {
                        CodecError::Malformed("Rhino count exceeds address space".into())
                    })?],
                    curve.knots()[count],
                ] != domain
            {
                return Err(CodecError::UnsupportedContent(format!(
                    "pcurve {} is not a nonperiodic full-domain NURBS curve",
                    pcurve.id.as_str()
                )));
            }
            (NURBS_CURVE_CLASS, nurbs_curve_payload_dimension(&curve, 2)?)
        }
        _ => {
            return Err(CodecError::UnsupportedContent(format!(
                "pcurve {} geometry is not writable as Rhino Brep trim geometry",
                pcurve.id.as_str()
            )));
        }
    };
    Ok(WritablePcurve {
        source: pcurve,
        payload,
    })
}

/// Checks that an explicit pcurve tracks its directed edge curve through the
/// NURBS surface.
///
/// `face_tolerance` is an admitted IR-stated tolerance from
/// `stated_write_tolerance`.
fn validate_nurbs_trim(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    face_tolerance: f64,
    edge: &WritableEdge<'_>,
    sense: cadmpeg_ir::topology::Sense,
    explicit: &WritablePcurve<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::topology::Sense;

    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let u_domain = [
        surface.u_knots()[usize::try_from(surface.u_degree())
            .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?],
        surface.u_knots()[u_count],
    ];
    let v_domain = [
        surface.v_knots()[usize::try_from(surface.v_degree())
            .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?],
        surface.v_knots()[v_count],
    ];
    let pcurve = explicit.source;
    let domain = edge.domain;
    let inside_domain = |u: f64, v: f64| {
        cadmpeg_ir::math::parameter_in_domain(u, u_domain, EPS_WRITE_DEGENERATE)
            && cadmpeg_ir::math::parameter_in_domain(v, v_domain, EPS_WRITE_DEGENERATE)
    };
    let mut breaks = vec![domain[0].get(), domain[1].get()];
    if let WritableEdgeCurve::Nurbs(nurbs) = edge.curve {
        breaks.extend(
            nurbs
                .knots()
                .finite_knots()
                .filter(|value| *value > domain[0] && *value < domain[1])
                .map(|value| {
                    if sense == Sense::Forward {
                        Ok(value.get())
                    } else {
                        cadmpeg_ir::math::reflect_parameter(value, domain[0], domain[1])
                            .map(FiniteReal::get)
                            .ok_or_else(|| {
                                CodecError::malformed("reversed edge knot is non-finite")
                            })
                    }
                })
                .collect::<Result<Vec<_>, CodecError>>()?,
        );
    }
    if let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = &pcurve.geometry {
        breaks.extend(
            nurbs
                .knots()
                .iter()
                .copied()
                .filter(|value| *value > domain[0].get() && *value < domain[1].get()),
        );
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();

    let tolerance = face_tolerance
        .max(
            edge.source
                .tolerance
                .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get),
        )
        .max(
            pcurve
                .fit_tolerance()
                .map_or(0.0, cadmpeg_ir::geometry::FitTolerance::get),
        );
    for sample in trims::samples(&breaks, domain)? {
        let parameter = sample.get();
        // A non-finite pcurve point is refused by the domain test.
        let uv = match cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &pcurve.geometry,
            parameter,
        ) {
            Ok(uv) => uv.get(),
            Err(failure) => failure.non_finite()?.ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "pcurve {} cannot be evaluated over its edge domain",
                    pcurve.id.as_str()
                ))
            })?,
        };
        if !inside_domain(uv.u, uv.v) {
            return Err(CodecError::malformed(format_args!(
                "pcurve {} leaves its NURBS surface parameter domain",
                pcurve.id.as_str()
            )));
        }
        // A non-finite surface point is measured as a finite one is.
        let mapped = match cadmpeg_ir::eval::decode::nurbs_surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            surface,
            uv.u,
            uv.v,
        ) {
            Ok(point) => point.get(),
            Err(failure) => failure.non_finite()?.ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "pcurve {} cannot be evaluated through its NURBS surface",
                    pcurve.id.as_str()
                ))
            })?,
        };
        let curve_parameter = if sense == Sense::Forward {
            sample
        } else {
            cadmpeg_ir::math::reflect_parameter(sample, domain[0], domain[1])
                .ok_or_else(|| CodecError::malformed("reversed edge parameter is non-finite"))?
        };
        // A non-finite edge curve point is measured as a finite one is.
        let edge_point = match edge.curve.point(curve_parameter) {
            Ok(point) => point.get(),
            Err(failure) => failure.non_finite()?.ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "edge curve {} cannot be evaluated over its edge domain",
                    edge.curve_id
                ))
            })?,
        };
        let distance = cadmpeg_ir::math::Point3::distance(mapped, edge_point);
        if !distance.is_finite() || distance > tolerance {
            let reason = format!(
                "pcurve {} misses directed edge curve {} by {distance}",
                pcurve.id.as_str(),
                edge.curve_id
            );
            if distance.is_finite() && sample != domain[0] && sample != domain[1] {
                // (S-entity) Independent carrier parameters need not pair at
                // interior samples. This writer cannot establish their
                // correspondence, so it omits the face with an Error loss.
                return Err(CodecError::UnsupportedContent(reason));
            }
            // (C) Endpoint images disagree, or a held carrier is non-finite.
            return Err(CodecError::malformed(reason));
        }
    }

    Ok(())
}

fn plane_uv(
    point: cadmpeg_ir::math::Point3,
    origin: cadmpeg_ir::math::Point3,
    u: cadmpeg_ir::math::Vector3,
    v: cadmpeg_ir::math::Vector3,
) -> [f64; 3] {
    let delta = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
    [
        delta[0] * u.x + delta[1] * u.y + delta[2] * u.z,
        delta[0] * v.x + delta[1] * v.y + delta[2] * v.z,
        0.0,
    ]
}

/// A bounded line from its endpoints, which callers compute, and its domain.
fn bounded_line_payload(
    from: [f64; 3],
    to: [f64; 3],
    domain: [f64; 2],
    dimension: i32,
) -> Result<Vec<u8>, CodecError> {
    let mut payload = vec![0x10];
    for value in from.into_iter().chain(to) {
        payload.extend(
            computed(value, "line endpoint coordinate")?
                .get()
                .to_le_bytes(),
        );
    }
    for value in domain {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(dimension.to_le_bytes());
    Ok(payload)
}

/// Admit a number the writer computed for `field`. A 3DM archive states no
/// non-finite number.
fn computed(value: f64, field: &str) -> Result<FiniteReal, CodecError> {
    FiniteReal::new(value).ok_or_else(|| {
        CodecError::UnsupportedContent(format!(
            "Rhino writer computed the non-finite {field} {value}, which 3DM cannot state"
        ))
    })
}

/// A count the native field cannot store.
fn native_count_limit(limit: u64, count: u64) -> CodecError {
    CodecError::InvalidInput(format!(
        "Rhino native count {count} exceeds the format limit {limit}"
    ))
}

fn polymorphic_array<'a>(
    children: impl ExactSizeIterator<Item = &'a ([u8; 16], Vec<u8>)>,
) -> Result<Vec<u8>, CodecError> {
    let mut body = vec![0x10];
    body.extend(
        (i32::try_from(children.len()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(children.len()),
            )
        })?)
        .to_le_bytes(),
    );
    let mut direct = body.clone();
    for (class, payload) in children {
        body.extend(1_i32.to_le_bytes());
        direct.extend(1_i32.to_le_bytes());
        body.extend(class_wrapper(*class, payload)?);
    }
    crc_chunk_with_direct(0x4000_8000, &body, &direct)
}

fn raw_array(records: &[Vec<u8>]) -> Result<Vec<u8>, CodecError> {
    let mut body = vec![0x10];
    body.extend(
        (i32::try_from(records.len()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(records.len()),
            )
        })?)
        .to_le_bytes(),
    );
    body.extend(records.concat());
    crc_chunk(0x4000_8000, &body)
}

fn face_array(
    records: &[Vec<u8>],
    faces: &[&cadmpeg_ir::topology::Face],
    archive_version: RhinoArchiveVersion,
) -> Result<Vec<u8>, CodecError> {
    let version_two = archive_version.uses_face_array_v2();
    let minor = if version_two { 2 } else { 1 };
    let mut body = vec![0x10 | minor];
    body.extend(
        (i32::try_from(records.len()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(records.len()),
            )
        })?)
        .to_le_bytes(),
    );
    body.extend(records.concat());
    for face in faces {
        body.extend(&Sha256::digest(face.id.as_str().as_bytes())[..16]);
    }
    if version_two {
        body.push(0);
    }
    crc_chunk(0x4000_8000, &body)
}

fn empty_region_wrapper() -> Result<Vec<u8>, CodecError> {
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(1_i32.to_le_bytes());
    body.push(0);
    crc_chunk(0x4000_8000, &body)
}

/// Converts one arena position into the native index the Brep records store.
fn wire_index(position: usize) -> Result<i32, CodecError> {
    native_i32_count(
        position,
        "Brep record index exceeds the native index range".into(),
    )
}

fn native_i32_count(value: usize, message: String) -> Result<i32, CodecError> {
    i32::try_from(value).map_err(|_| CodecError::InvalidInput(message))
}

fn archive_body_len(value: u64) -> Result<i64, CodecError> {
    i64::try_from(value)
        .map_err(|_| CodecError::InvalidInput("3DM object table size overflow".into()))
}

fn archive_final_size(position: u64) -> Result<u64, CodecError> {
    position
        .checked_add(20)
        .ok_or_else(|| CodecError::InvalidInput("3DM output size overflow".into()))
}

/// Converts a list of arena positions into the native indexes they store as.
fn wire_indexes(positions: impl IntoIterator<Item = usize>) -> Result<Vec<i32>, CodecError> {
    positions.into_iter().map(wire_index).collect()
}

fn indexes(values: &[i32]) -> Result<Vec<u8>, CodecError> {
    let mut bytes = wire_index(values.len())?.to_le_bytes().to_vec();
    bytes.extend(values.iter().flat_map(|value| value.to_le_bytes()));
    Ok(bytes)
}

fn class_wrapper(class_uuid: [u8; 16], payload: &[u8]) -> Result<Vec<u8>, CodecError> {
    class_wrapper_with_direct(class_uuid, payload, payload)
}

fn class_wrapper_with_direct(
    class_uuid: [u8; 16],
    payload: &[u8],
    direct: &[u8],
) -> Result<Vec<u8>, CodecError> {
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(TCODE_CLASS_UUID, &uuid_body)?;
    let data = crc_chunk_with_direct(TCODE_CLASS_DATA, payload, direct)?;
    let end = short_chunk(TCODE_CLASS_END, 0);
    long_chunk(TCODE_CLASS_WRAPPER, &[uuid, data, end].concat())
}

fn check_mesh(mesh: &cadmpeg_ir::tessellation::Tessellation) -> Result<(), CodecError> {
    let vertex_count = mesh.vertices().len();
    if vertex_count == 0 || vertex_count > (1 << 24) || mesh.triangles().len() > (1 << 24) {
        return Err(CodecError::InvalidInput(format!(
            "mesh {} counts are outside Rhino's native range",
            mesh.id
        )));
    }
    if mesh.vertices().iter().any(|p| {
        cadmpeg_core::convert::f32_from_f64(p.x).is_none()
            || cadmpeg_core::convert::f32_from_f64(p.y).is_none()
            || cadmpeg_core::convert::f32_from_f64(p.z).is_none()
    }) || mesh.vertex_normals().iter().any(|n| {
        cadmpeg_core::convert::f32_from_f64(n.x).is_none()
            || cadmpeg_core::convert::f32_from_f64(n.y).is_none()
            || cadmpeg_core::convert::f32_from_f64(n.z).is_none()
    }) {
        return Err(CodecError::InvalidInput(format!(
            "mesh {} values exceed Rhino's native finite range",
            mesh.id
        )));
    }
    let mut kinds = std::collections::BTreeSet::new();
    for channel in mesh.channels() {
        let expected = match channel.kind() {
            CHANNEL_UV => 8,
            CHANNEL_COLOR => 4,
            CHANNEL_SURFACE_PARAMETERS | CHANNEL_CURVATURE => 16,
            _ => continue,
        };
        if !kinds.insert(channel.kind())
            || channel.flags() != 0
            || channel.item_size() != expected
            || usize::try_from(channel.count())
                .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?
                != vertex_count
        {
            return Err(CodecError::malformed(format_args!(
                "mesh {} channel {:#x} has invalid metadata",
                mesh.id,
                channel.kind()
            )));
        }
    }
    Ok(())
}

fn check_frame(
    id: &str,
    frame: cadmpeg_ir::units::OrthonormalFrame3,
    family: &str,
) -> Result<(), CodecError> {
    let normal = frame.axis().as_raw();
    let x = frame.reference().as_raw();
    let dot = normal.x * x.x + normal.y * x.y + normal.z * x.z;
    if (normal.norm() - 1.0).abs() > EPS_WRITE_DEGENERATE
        || (x.norm() - 1.0).abs() > EPS_WRITE_DEGENERATE
        || dot.abs() > EPS_WRITE_DEGENERATE
    {
        return Err(CodecError::UnsupportedContent(format!(
            "{family} {id} frame is not orthonormal to Rhino's tighter bound {EPS_WRITE_DEGENERATE}"
        )));
    }
    Ok(())
}

/// Admits a NURBS surface the Rhino archive can state, returning its pole
/// count at the archive's own lane width.
///
/// The pole lane is an `i32`, so the rule
/// [`cadmpeg_core::decode::id_from_index`] states applies at that width: a
/// count the lane cannot state is a refusal naming the surface. The proven
/// figure is returned rather than discarded, so the writer states the lane
/// from this admission instead of narrowing the population again.
fn check_nurbs_surface(
    id: &str,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
) -> Result<i32, CodecError> {
    let u_order = usize::try_from(surface.u_degree())
        .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?
        + 1;
    let v_order = usize::try_from(surface.v_degree())
        .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?
        + 1;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let count_error = || format!("surface {id} cannot be represented by Rhino NURBS counts");
    let pole_count = native_i32_count(surface.poles().len(), count_error())?;
    if u_order < 2 || v_order < 2 {
        return Err(CodecError::UnsupportedContent(count_error()));
    }
    native_i32_count(u_order, count_error())?;
    native_i32_count(v_order, count_error())?;
    native_i32_count(u_count, count_error())?;
    native_i32_count(v_count, count_error())?;
    check_knot_roundtrip(
        id,
        "surface U",
        surface.u_knots(),
        u_order,
        u_count,
        surface.u_periodic(),
    )?;
    check_knot_roundtrip(
        id,
        "surface V",
        surface.v_knots(),
        v_order,
        v_count,
        surface.v_periodic(),
    )?;
    Ok(pole_count)
}

fn check_nurbs_curve(
    id: &str,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<(), CodecError> {
    let order = usize::try_from(curve.degree())
        .map_err(|_| CodecError::Malformed("Rhino count exceeds address space".into()))?
        + 1;
    let count = curve.control_points().len();
    let count_error = || format!("curve {id} cannot be represented by Rhino NURBS counts");
    if order < 2 {
        return Err(CodecError::UnsupportedContent(count_error()));
    }
    native_i32_count(order, count_error())?;
    native_i32_count(count, count_error())?;
    check_knot_roundtrip(id, "curve", curve.knots(), order, count, curve.periodic())?;
    Ok(())
}

fn check_knot_roundtrip(
    id: &str,
    direction: &str,
    full: &[f64],
    order: usize,
    count: usize,
    declared_periodic: bool,
) -> Result<(), CodecError> {
    let stored = &full[1..full.len() - 1];
    if stored[order - 2] >= stored[count - 1] {
        return Err(CodecError::UnsupportedContent(format!(
            "{direction} {id} has a non-increasing native NURBS domain"
        )));
    }
    let reconstructed =
        crate::surfaces::reconstruct_knots(stored, order, count).map_err(|error| match error {
            crate::curves::GeometryError::Codec(CodecError::ResourceLimit(limit)) => {
                CodecError::ResourceLimit(limit)
            }
            error => CodecError::UnsupportedContent(format!("{direction} {id}: {error}")),
        })?;
    let periodic = crate::surfaces::periodic_knots(stored, order, count);
    if reconstructed != full || periodic != declared_periodic {
        return Err(CodecError::UnsupportedContent(format!(
            "{direction} {id} knot endpoints or periodic flag are not native-canonical"
        )));
    }
    Ok(())
}

fn header(version: RhinoArchiveVersion) -> Vec<u8> {
    let text = version.value().to_string();
    let mut bytes = MAGIC.to_vec();
    bytes.extend(std::iter::repeat_with(|| b' ').take(8 - text.len()));
    bytes.extend(text.bytes());
    bytes
}

fn long_chunk(typecode: u32, body: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut bytes = typecode.to_le_bytes().to_vec();
    bytes.extend(
        (i64::try_from(body.len()).map_err(|_| {
            native_count_limit(
                9_223_372_036_854_775_807,
                cadmpeg_core::decode::u64_from_index(body.len()),
            )
        })?)
        .to_le_bytes(),
    );
    bytes.extend(body);
    Ok(bytes)
}

fn crc_chunk(typecode: u32, body: &[u8]) -> Result<Vec<u8>, CodecError> {
    crc_chunk_with_direct(typecode, body, body)
}

fn crc_chunk_with_direct(typecode: u32, body: &[u8], direct: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut payload = body.to_vec();
    payload.extend(crc32fast::hash(direct).to_le_bytes());
    long_chunk(typecode, &payload)
}

fn zero_crc_chunk(typecode: u32, body: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut payload = body.to_vec();
    payload.extend(0_u32.to_le_bytes());
    long_chunk(typecode, &payload)
}

fn short_chunk(typecode: u32, value: i64) -> Vec<u8> {
    let mut bytes = (typecode | TCODE_SHORT).to_le_bytes().to_vec();
    bytes.extend(value.to_le_bytes());
    bytes
}

fn table(
    ctx: &DecodeContext<'_>,
    typecode: u32,
    records: &[Vec<u8>],
) -> Result<Vec<u8>, CodecError> {
    let mut body = Vec::new();
    for record in records {
        ctx.charge_entities(1, "write Rhino table record")?;
        body.extend(record);
    }
    body.extend(short_chunk(TCODE_ENDOFTABLE, 0));
    long_chunk(typecode, &body)
}

fn units_record(linear: f64, angular: f64) -> Result<Vec<u8>, CodecError> {
    let mut body = 100_i32.to_le_bytes().to_vec();
    body.extend(2_i32.to_le_bytes()); // millimeters
    body.extend(linear.to_le_bytes());
    body.extend(angular.to_le_bytes());
    body.extend(DEFAULT_RELATIVE_TOLERANCE.to_le_bytes());
    crc_chunk(TCODE_UNITS_AND_TOLERANCES, &body)
}

fn point_cloud_payload(points: &[cadmpeg_ir::math::Point3]) -> Result<Vec<u8>, CodecError> {
    if points.is_empty() {
        return Err(CodecError::InvalidInput(
            "Rhino point cloud requires at least one point".into(),
        ));
    }
    let mut payload = vec![0x10];
    payload.extend(
        (i32::try_from(points.len()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(points.len()),
            )
        })?)
        .to_le_bytes(),
    );
    for point in points {
        payload.extend(point.x.to_le_bytes());
        payload.extend(point.y.to_le_bytes());
        payload.extend(point.z.to_le_bytes());
    }
    for value in [
        0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0,
    ] {
        payload.extend(value.to_le_bytes());
    }
    let min = points.iter().fold([f64::INFINITY; 3], |a, p| {
        [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
    });
    let max = points.iter().fold([f64::NEG_INFINITY; 3], |a, p| {
        [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
    });
    for value in min.into_iter().chain(max) {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(0_i32.to_le_bytes());
    Ok(payload)
}

fn circle_payload(
    center: cadmpeg_ir::math::Point3,
    axis: cadmpeg_ir::math::Vector3,
    x: cadmpeg_ir::math::Vector3,
    radius: f64,
) -> Result<Vec<u8>, CodecError> {
    let y = cadmpeg_ir::math::Vector3::new(
        axis.y * x.z - axis.z * x.y,
        axis.z * x.x - axis.x * x.z,
        axis.x * x.y - axis.y * x.x,
    );
    let equation_d = computed(
        -(axis.x * center.x + axis.y * center.y + axis.z * center.z),
        "circle plane equation constant",
    )?;
    let arc_point = |value: f64| computed(value, "circle arc point coordinate");
    let mut payload = vec![0x10];
    for value in [
        center.x,
        center.y,
        center.z,
        x.x,
        x.y,
        x.z,
        y.x,
        y.y,
        y.z,
        axis.x,
        axis.y,
        axis.z,
        axis.x,
        axis.y,
        axis.z,
        equation_d.get(),
        radius,
        arc_point(center.x + radius * x.x)?.get(),
        arc_point(center.y + radius * x.y)?.get(),
        arc_point(center.z + radius * x.z)?.get(),
        arc_point(center.x + radius * y.x)?.get(),
        arc_point(center.y + radius * y.y)?.get(),
        arc_point(center.z + radius * y.z)?.get(),
        arc_point(center.x - radius * x.x)?.get(),
        arc_point(center.y - radius * x.y)?.get(),
        arc_point(center.z - radius * x.z)?.get(),
        0.0,
        std::f64::consts::TAU,
        0.0,
        std::f64::consts::TAU,
    ] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(3_i32.to_le_bytes());
    Ok(payload)
}

fn nurbs_curve_payload(
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<Vec<u8>, CodecError> {
    nurbs_curve_payload_dimension(curve, 3)
}

fn nurbs_curve_payload_dimension(
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    dimension: i32,
) -> Result<Vec<u8>, CodecError> {
    let rational = i32::from(curve.weights().is_some());
    let order = i32::try_from(curve.degree() + 1)
        .map_err(|_| native_count_limit(2_147_483_647, u64::from(curve.degree() + 1)))?;
    let count = i32::try_from(curve.control_points().len()).map_err(|_| {
        native_count_limit(
            2_147_483_647,
            cadmpeg_core::decode::u64_from_index(curve.control_points().len()),
        )
    })?;
    let mut payload = vec![0x10];
    for value in [dimension, rational, order, count, 0, 0] {
        payload.extend(value.to_le_bytes());
    }
    let min = curve
        .control_points()
        .iter()
        .fold([f64::INFINITY; 3], |a, p| {
            [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
        });
    let max = curve
        .control_points()
        .iter()
        .fold([f64::NEG_INFINITY; 3], |a, p| {
            [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
        });
    for value in min.into_iter().chain(max) {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(
        (i32::try_from(curve.knots().len() - 2).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(curve.knots().len() - 2),
            )
        })?)
        .to_le_bytes(),
    );
    for knot in &curve.knots()[1..curve.knots().len() - 1] {
        payload.extend(knot.to_le_bytes());
    }
    payload.extend(count.to_le_bytes());
    let homogeneous = |value: f64| computed(value, "NURBS curve homogeneous pole coordinate");
    for (index, point) in curve.control_points().iter().enumerate() {
        let weight = curve.weights().map_or(1.0, |weights| weights[index].get());
        payload.extend(homogeneous(point.x * weight)?.get().to_le_bytes());
        payload.extend(homogeneous(point.y * weight)?.get().to_le_bytes());
        if dimension == 3 {
            payload.extend(homogeneous(point.z * weight)?.get().to_le_bytes());
        }
        if rational != 0 {
            payload.extend(weight.to_le_bytes());
        }
    }
    Ok(payload)
}

fn plane_surface_payload(
    origin: cadmpeg_ir::math::Point3,
    normal: cadmpeg_ir::math::Vector3,
    x: cadmpeg_ir::math::Vector3,
) -> Result<Vec<u8>, CodecError> {
    let y = normal.cross(x);
    let d = computed(
        -(normal.x * origin.x + normal.y * origin.y + normal.z * origin.z),
        "plane equation constant",
    )?;
    let mut payload = vec![0x10];
    for value in [
        origin.x,
        origin.y,
        origin.z,
        x.x,
        x.y,
        x.z,
        y.x,
        y.y,
        y.z,
        normal.x,
        normal.y,
        normal.z,
        normal.x,
        normal.y,
        normal.z,
        d.get(),
    ]
    .into_iter()
    .chain(PLANE_PARAMETER_DOMAIN)
    .chain(PLANE_PARAMETER_DOMAIN)
    {
        payload.extend(value.to_le_bytes());
    }
    Ok(payload)
}

/// Serializes a NURBS surface.
///
/// `pole_count` is the figure `check_nurbs_surface` proved when the surface
/// was admitted; the population is never narrowed again here.
fn nurbs_surface_payload(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    pole_count: i32,
) -> Result<Vec<u8>, CodecError> {
    let rational = i32::from(surface.weights().is_some());
    let mut payload = vec![0x10];
    for value in [
        3,
        rational,
        i32::try_from(surface.u_degree() + 1)
            .map_err(|_| native_count_limit(2_147_483_647, u64::from(surface.u_degree() + 1)))?,
        i32::try_from(surface.v_degree() + 1)
            .map_err(|_| native_count_limit(2_147_483_647, u64::from(surface.v_degree() + 1)))?,
        i32::try_from(surface.u_count()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(surface.u_count()),
            )
        })?,
        i32::try_from(surface.v_count()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(surface.v_count()),
            )
        })?,
        0,
        0,
    ] {
        payload.extend(value.to_le_bytes());
    }
    let poles = surface.poles();
    let min = poles.iter().fold([f64::INFINITY; 3], |a, p| {
        [a[0].min(p.x), a[1].min(p.y), a[2].min(p.z)]
    });
    let max = poles.iter().fold([f64::NEG_INFINITY; 3], |a, p| {
        [a[0].max(p.x), a[1].max(p.y), a[2].max(p.z)]
    });
    for value in min.into_iter().chain(max) {
        payload.extend(value.to_le_bytes());
    }
    for knots in [surface.u_knots(), surface.v_knots()] {
        payload.extend(
            (i32::try_from(knots.len() - 2).map_err(|_| {
                native_count_limit(
                    2_147_483_647,
                    cadmpeg_core::decode::u64_from_index(knots.len() - 2),
                )
            })?)
            .to_le_bytes(),
        );
        for knot in &knots[1..knots.len() - 1] {
            payload.extend(knot.to_le_bytes());
        }
    }
    payload.extend(pole_count.to_le_bytes());
    let pole_weights = surface.pole_weights();
    let homogeneous = |value: f64| computed(value, "NURBS surface homogeneous pole coordinate");
    for (index, point) in poles.iter().enumerate() {
        let weight = pole_weights
            .as_ref()
            .map_or(1.0, |weights| weights[index].get());
        payload.extend(homogeneous(point.x * weight)?.get().to_le_bytes());
        payload.extend(homogeneous(point.y * weight)?.get().to_le_bytes());
        payload.extend(homogeneous(point.z * weight)?.get().to_le_bytes());
        if rational != 0 {
            payload.extend(weight.to_le_bytes());
        }
    }
    Ok(payload)
}

struct MeshPayload {
    body: Vec<u8>,
    direct: Vec<u8>,
}

fn mesh_payload(
    mesh: &cadmpeg_ir::tessellation::Tessellation,
    archive_version: RhinoArchiveVersion,
) -> Result<MeshPayload, CodecError> {
    let writes_double_vertices = archive_version.stores_mesh_vertices_as_f64();
    let payload_version = if writes_double_vertices { 0x38 } else { 0x35 };
    let mut payload = vec![payload_version];
    payload.extend(
        (i32::try_from(mesh.vertices().len()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(mesh.vertices().len()),
            )
        })?)
        .to_le_bytes(),
    );
    payload.extend(
        (i32::try_from(mesh.triangles().len()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(mesh.triangles().len()),
            )
        })?)
        .to_le_bytes(),
    );
    for _ in 0..4 {
        payload.extend(0.0_f64.to_le_bytes());
        payload.extend(1.0_f64.to_le_bytes());
    }
    payload.extend([0_u8; 16]);
    payload.extend([0_u8; 16 * 4]);
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0_u8; 5]);

    let width = if mesh.vertices().len() < 256 {
        FaceIndexWidth::One
    } else if mesh.vertices().len() < 65_536 {
        FaceIndexWidth::Two
    } else {
        FaceIndexWidth::Four
    };
    payload.extend(
        (i32::try_from(width.bytes()).map_err(|_| {
            native_count_limit(
                2_147_483_647,
                cadmpeg_core::decode::u64_from_index(width.bytes()),
            )
        })?)
        .to_le_bytes(),
    );
    for triangle in mesh.triangles() {
        for index in [triangle[0], triangle[1], triangle[2], triangle[2]] {
            match width {
                FaceIndexWidth::One => payload.push(u8::try_from(index).map_err(|_| {
                    CodecError::Malformed("mesh face index exceeds chosen width".into())
                })?),
                FaceIndexWidth::Two => payload.extend(
                    (u16::try_from(index).map_err(|_| {
                        CodecError::Malformed("mesh face index exceeds chosen width".into())
                    })?)
                    .to_le_bytes(),
                ),
                FaceIndexWidth::Four => payload.extend(index.to_le_bytes()),
            }
        }
    }

    let mut float_vertices = Vec::new();
    for point in mesh.vertices() {
        for value in [point.x, point.y, point.z] {
            let value = cadmpeg_core::convert::f32_from_f64(value)
                .ok_or_else(|| CodecError::Malformed("mesh vertex exceeds f32 range".into()))?;
            float_vertices.extend(value.to_le_bytes());
        }
    }
    let mut normals = Vec::new();
    for normal in mesh.vertex_normals() {
        for value in [normal.x, normal.y, normal.z] {
            let value = cadmpeg_core::convert::f32_from_f64(value)
                .ok_or_else(|| CodecError::Malformed("mesh normal exceeds f32 range".into()))?;
            normals.extend(value.to_le_bytes());
        }
    }
    for data in [
        &float_vertices[..],
        &normals[..],
        mesh_channel(mesh, CHANNEL_UV),
        mesh_channel(mesh, CHANNEL_CURVATURE),
        mesh_channel(mesh, CHANNEL_COLOR),
    ] {
        payload.extend(mesh_buffer(data)?);
    }
    payload.extend(0_i32.to_le_bytes());
    payload.extend([0_u8; 16]);
    payload.extend(mesh_buffer(mesh_channel(mesh, CHANNEL_SURFACE_PARAMETERS))?);
    let mut direct = payload.clone();
    payload.extend(mesh_mapping_tag()?);
    payload.extend([0_u8; 3]);
    direct.extend([0_u8; 3]);
    if writes_double_vertices {
        payload.push(0);
        direct.push(0);
        payload.push(1);
        direct.push(1);
        let doubles = mesh
            .vertices()
            .iter()
            .flat_map(|point| {
                [point.x, point.y, point.z]
                    .into_iter()
                    .flat_map(f64::to_le_bytes)
            })
            .collect::<Vec<_>>();
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(0_i32.to_le_bytes());
        body.extend(
            (u32::try_from(mesh.vertices().len()).map_err(|_| {
                native_count_limit(
                    4_294_967_295,
                    cadmpeg_core::decode::u64_from_index(mesh.vertices().len()),
                )
            })?)
            .to_le_bytes(),
        );
        body.extend(mesh_buffer(&doubles)?);
        payload.extend(crc_chunk(0x4000_8000, &body)?);
        let min = mesh.vertices().iter().fold([f64::INFINITY; 3], |a, point| {
            [a[0].min(point.x), a[1].min(point.y), a[2].min(point.z)]
        });
        let max = mesh
            .vertices()
            .iter()
            .fold([f64::NEG_INFINITY; 3], |a, point| {
                [a[0].max(point.x), a[1].max(point.y), a[2].max(point.z)]
            });
        let bounding_box = min
            .into_iter()
            .chain(max)
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        payload.extend(&bounding_box);
        direct.extend(bounding_box);
    }
    Ok(MeshPayload {
        body: payload,
        direct,
    })
}

fn mesh_mapping_tag() -> Result<Vec<u8>, CodecError> {
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(0_i32.to_le_bytes());
    body.extend([0_u8; 16]);
    body.extend(0_i32.to_le_bytes());
    for value in [
        1.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ] {
        body.extend(value.to_le_bytes());
    }
    crc_chunk(0x4000_8000, &body)
}

fn mesh_channel(mesh: &cadmpeg_ir::tessellation::Tessellation, kind: u32) -> &[u8] {
    mesh.channels()
        .iter()
        .find(|channel| channel.kind() == kind)
        .map_or(&[], |channel| channel.data())
}

fn mesh_buffer(data: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut result = (u32::try_from(data.len()).map_err(|_| {
        native_count_limit(
            4_294_967_295,
            cadmpeg_core::decode::u64_from_index(data.len()),
        )
    })?)
    .to_le_bytes()
    .to_vec();
    if !data.is_empty() {
        result.extend(crc32fast::hash(data).to_le_bytes());
        result.push(0);
        result.extend(data);
    }
    Ok(result)
}

fn attributed_object_record(
    ir: &CadIr,
    object_type: i64,
    class_uuid: [u8; 16],
    payload: &[u8],
    identity: &str,
    display: presentation::Display<'_>,
) -> Result<Vec<u8>, CodecError> {
    framed_object_record(
        object_type,
        class_uuid,
        payload,
        None,
        Some(object_attributes_payload(
            ir,
            identity,
            display.name,
            display.color,
            display.visible,
        )?),
    )
}

fn curve_object_record(
    ir: &CadIr,
    payload: &compounds::CurvePayload,
    identity: &str,
    display: presentation::Display<'_>,
) -> Result<Vec<u8>, CodecError> {
    framed_object_record(
        4,
        payload.class,
        &payload.body,
        Some(&payload.direct),
        Some(object_attributes_payload(
            ir,
            identity,
            display.name,
            display.color,
            display.visible,
        )?),
    )
}

fn mesh_object_record(
    ir: &CadIr,
    payload: &MeshPayload,
    identity: &str,
    display: presentation::Display<'_>,
) -> Result<Vec<u8>, CodecError> {
    framed_object_record(
        0x20,
        MESH_CLASS,
        &payload.body,
        Some(&payload.direct),
        Some(object_attributes_payload(
            ir,
            identity,
            display.name,
            display.color,
            display.visible,
        )?),
    )
}

fn brep_object_record(
    ir: &CadIr,
    payload: &BrepPayload,
    identity: &str,
    name: Option<&str>,
    color: Option<cadmpeg_ir::topology::Color>,
    visible: Option<bool>,
) -> Result<Vec<u8>, CodecError> {
    framed_object_record(
        0x10,
        BREP_CLASS,
        &payload.body,
        Some(&payload.direct),
        Some(object_attributes_payload(
            ir, identity, name, color, visible,
        )?),
    )
}

fn framed_object_record(
    object_type: i64,
    class_uuid: [u8; 16],
    payload: &[u8],
    direct_class_data: Option<&[u8]>,
    attributes: Option<Vec<u8>>,
) -> Result<Vec<u8>, CodecError> {
    let object_type = short_chunk(TCODE_OBJECT_RECORD_TYPE, object_type);
    let mut uuid_body = class_uuid.to_vec();
    uuid_body.extend(crc32fast::hash(&class_uuid).to_le_bytes());
    let uuid = long_chunk(TCODE_CLASS_UUID, &uuid_body)?;
    let class_data = if let Some(direct) = direct_class_data {
        crc_chunk_with_direct(TCODE_CLASS_DATA, payload, direct)?
    } else {
        crc_chunk(TCODE_CLASS_DATA, payload)?
    };
    let class_end = short_chunk(TCODE_CLASS_END, 0);
    let class = long_chunk(TCODE_CLASS_WRAPPER, &[uuid, class_data, class_end].concat())?;
    let object_end = short_chunk(TCODE_OBJECT_RECORD_END, 0);
    let mut body = [object_type, class].concat();
    if let Some(attributes) = attributes {
        body.extend(crc_chunk(TCODE_OBJECT_RECORD_ATTRIBUTES, &attributes)?);
    }
    body.extend(object_end);
    zero_crc_chunk(TCODE_OBJECT_RECORD, &body)
}

/// Reuse the document's native UUID identity when present. Foreign identities
/// receive a deterministic UUID; no retained bytes are consulted.
fn neutral_object_uuid(identity: &str) -> [u8; 16] {
    if identity.starts_with("rhino:object:") {
        if let Some((_, key)) = identity.split_once('#') {
            if key.len() == 36
                && [8, 13, 18, 23]
                    .iter()
                    .all(|index| key.as_bytes()[*index] == b'-')
            {
                let digits = key.bytes().filter(|byte| *byte != b'-').collect::<Vec<_>>();
                let mut canonical = [0; 16];
                let mut valid = true;
                for (slot, pair) in canonical.iter_mut().zip(digits.chunks_exact(2)) {
                    let nibble = |byte: u8| {
                        char::from(byte)
                            .to_digit(16)
                            .and_then(|v| u8::try_from(v).ok())
                    };
                    if let (Some(high), Some(low)) = (nibble(pair[0]), nibble(pair[1])) {
                        *slot = (high << 4) | low;
                    } else {
                        valid = false;
                    }
                }
                if valid {
                    return crate::wire::Uuid::from_canonical(canonical).to_wire();
                }
            }
        }
    }
    let digest = Sha256::digest(identity.as_bytes());
    let mut uuid = [0; 16];
    uuid.copy_from_slice(&digest[..16]);
    uuid
}

fn object_attributes_payload(
    ir: &CadIr,
    identity: &str,
    name: Option<&str>,
    color: Option<cadmpeg_ir::topology::Color>,
    visible: Option<bool>,
) -> Result<Vec<u8>, CodecError> {
    let uuid = neutral_object_uuid(identity);
    let material = tables::materials::material_index(ir, identity);
    // Rhino can state an object display colour independently of its material.
    // Keep that channel absent when the effective colour comes from material.
    let direct_color = ir
        .model
        .bodies
        .iter()
        .find(|body| body.id.as_str() == identity)
        .and_then(|body| body.color)
        .or_else(|| {
            ir.model
                .faces
                .iter()
                .find(|face| face.id.as_str() == identity)
                .and_then(|face| face.color)
        });
    let color = if material.is_some() {
        direct_color
    } else {
        color
    };
    let visible = ir
        .model
        .bodies
        .iter()
        .find(|body| body.id.as_str() == identity)
        .map(|body| body.visible)
        .or_else(|| {
            ir.model
                .occurrences
                .iter()
                .find(|occurrence| occurrence.id.as_str() == identity)
                .map(|occurrence| occurrence.visible)
        })
        .unwrap_or(visible);
    let mut payload = vec![0x20];
    payload.extend(uuid);
    payload.extend(tables::layer_index(ir, identity)?.to_le_bytes());
    // Attribute tags are an ascending stream, not an unordered map.
    if let Some(name) = name.filter(|name| !name.contains('\0')) {
        payload.push(1);
        payload.extend(utf16(name)?);
    }
    let native = tables::own_object(ir, identity);
    if let Some(index) = native
        .as_ref()
        .and_then(|fields| fields.get("linetype_index"))
        .and_then(serde_json::Value::as_i64)
    {
        if let Some(index) = ir
            .native
            .namespace("rhino")
            .and_then(|namespace| namespace.arenas().get("linetypes"))
            .and_then(|records| {
                records.iter().position(|record| {
                    record
                        .field("archive_index")
                        .and_then(|value| value.as_i64())
                        == Some(index)
                })
            })
        {
            payload.push(3);
            payload.extend(wire_index(index)?.to_le_bytes());
        }
    }
    if let Some(material) = material {
        payload.push(4);
        payload.extend(material.to_le_bytes());
    }
    if let Some(color) = color {
        payload.push(6);
        payload.extend([
            unit_color_channel(color.r())?,
            unit_color_channel(color.g())?,
            unit_color_channel(color.b())?,
            unit_color_channel(1.0 - color.a())?,
        ]);
    }
    if let Some(visible) = visible {
        payload.extend([11, u8::from(visible)]);
    }
    if tables::products::is_member(ir, identity) {
        payload.extend([12, 2]);
    }
    if color.is_some() {
        payload.extend([13, 1]);
    } else if material.is_some() {
        payload.extend([13, 2]);
    }
    if material.is_some() {
        payload.extend([16, 1]);
    }
    if native
        .as_ref()
        .and_then(|fields| fields.get("linetype_index"))
        .and_then(serde_json::Value::as_i64)
        .is_some_and(|index| index >= 0)
    {
        payload.extend([17, 1]);
    }
    let groups = tables::native::group_indices(ir, identity)?;
    if !groups.is_empty() {
        payload.push(18);
        payload.extend(indexes(&groups)?);
    }
    if let Some(order) = native
        .as_ref()
        .and_then(|fields| fields.get("display_order"))
        .and_then(serde_json::Value::as_i64)
    {
        payload.push(22);
        payload.extend(
            i32::try_from(order)
                .map_err(CodecError::malformed)?
                .to_le_bytes(),
        );
    }
    payload.push(0);
    Ok(payload)
}

fn default_layer_payload() -> Result<Vec<u8>, CodecError> {
    let mut payload = vec![0x15];
    payload.extend(0_i32.to_le_bytes());
    payload.extend(0_i32.to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend((-1_i32).to_le_bytes());
    payload.extend([0_u8, 0, 0, 0]);
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0_i16.to_le_bytes());
    payload.extend(0.0_f64.to_le_bytes());
    payload.extend(1.0_f64.to_le_bytes());
    payload.extend(utf16("Default")?);
    payload.push(1);
    payload.extend((-1_i32).to_le_bytes());
    payload.extend([0_u8, 0, 0, 0]);
    payload.extend(0.0_f64.to_le_bytes());
    payload.push(0);
    payload.extend(&Sha256::digest(b"cadmpeg:default-layer")[..16]);
    Ok(payload)
}

fn unit_color_channel(value: f32) -> Result<u8, CodecError> {
    cadmpeg_core::convert::truncate_f64_to_u8(f64::from((value * 255.0).round()))
        .ok_or_else(|| CodecError::Malformed("color channel is outside byte range".into()))
}

fn utf16(value: &str) -> Result<Vec<u8>, CodecError> {
    let mut units = value.encode_utf16().collect::<Vec<_>>();
    if !units.is_empty() {
        units.push(0);
    }
    let mut bytes = (u32::try_from(units.len()).map_err(|_| {
        native_count_limit(
            4_294_967_295,
            cadmpeg_core::decode::u64_from_index(units.len()),
        )
    })?)
    .to_le_bytes()
    .to_vec();
    for unit in units {
        bytes.extend(unit.to_le_bytes());
    }
    Ok(bytes)
}

#[cfg(test)]
pub(crate) mod tests;
