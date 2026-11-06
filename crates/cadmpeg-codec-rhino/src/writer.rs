// SPDX-License-Identifier: Apache-2.0
//! Native Rhino 3DM archive writing.

use crate::mesh::FaceIndexWidth;

mod attributes;
mod compounds;
mod conics;
mod extrusion;
mod loose;
mod model;
mod placement;
mod plane_domain;
mod polyline;
mod presentation;
mod reduce;
mod revolution;
mod salvage;
mod scopes;
mod subd;
mod tables;
pub(crate) mod target;
mod trim_iso;
mod trims;
mod wire;
use model::{
    ReducedCarrier, WritableEdge, WritableEdgeCurve, WritableFaceSurface, WritableModel,
    WritableObjectCurve, WritablePcurve, WritableVertex,
};

use crate::loss::RhinoLossCode;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{EncodeBuffer, EncodeSession};
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
) -> Result<f64, WriteError> {
    let (field, value) = stated.map_or_else(
        || {
            (
                "document linear tolerance",
                ir.tolerances.linear_or_default().get(),
            )
        },
        |tolerance| ("tolerance", tolerance.get()),
    );
    if value >= EPS_WRITE_DEGENERATE {
        return Ok(value);
    }
    Err(WriteError::Omit(format!(
        "{field} {value} of {owner} is finer than the Rhino Brep write bound {EPS_WRITE_DEGENERATE}"
    )))
}

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

use tables::materials::MaterialPlan;

struct WriteOutcome {
    losses: Vec<LossNote>,
    geometry_objects: usize,
    subds: usize,
    history_features: std::collections::BTreeSet<String>,
    face_colors: std::collections::BTreeSet<String>,
    written_curves: std::collections::BTreeSet<String>,
}

/// Build the archive and hand over its staged bytes with no copy.
fn write<'budget>(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    version: RhinoArchiveVersion,
    session: &'budget EncodeSession,
) -> Result<(EncodeBuffer<'budget>, WriteOutcome), cadmpeg_ir::codec::write::EncodeError> {
    let mut staged = session.buffer();
    let outcome = write_buffered(ctx, ir, version, session, &mut staged);
    session.finish()?;
    match outcome {
        Ok(outcome) => Ok((staged, outcome)),
        Err(WriteError::Omit(message)) => Err(cadmpeg_ir::codec::write::transitional_from_codec(
            CodecError::UnsupportedContent(message),
        )),
        Err(WriteError::Fail(error)) => {
            Err(cadmpeg_ir::codec::write::transitional_from_codec(error))
        }
        Err(WriteError::Encode(error)) => Err(error),
    }
}

fn write_buffered<'budget>(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    version: RhinoArchiveVersion,
    session: &'budget EncodeSession,
    output: &mut EncodeBuffer<'budget>,
) -> Result<WriteOutcome, WriteError> {
    // Degrade invalidated records to per-record losses before admission so
    // the plan names the record instead of refusing the document. A
    // consistent document passes through borrowed and unchanged.
    let (admitted, admit_losses) = scopes::admit(ir);
    let ir = admitted.as_ref();
    let extrusion = if ir.model.procedural_surfaces.iter().any(|surface| {
        matches!(
            surface.definition(),
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(_)
        )
    }) {
        let materials = MaterialPlan::new(ir)?;
        extrusion::admit(session, ir, version, &materials)?
    } else {
        None
    };
    let reduced = extrusion.as_ref().map(|_| {
        let mut reduced = ir.clone();
        reduced.model.bodies.clear();
        reduced.model.regions.clear();
        reduced.model.shells.clear();
        reduced.model.faces.clear();
        reduced.model.loops.clear();
        reduced.model.coedges.clear();
        reduced.model.edges.clear();
        reduced.model.vertices.clear();
        reduced.model.points.clear();
        reduced.model.surfaces.clear();
        reduced.model.curves.clear();
        reduced.model.pcurves.clear();
        reduced.model.procedural_curves.clear();
        reduced.model.procedural_surfaces.clear();
        reduced
    });
    let mut plan = prepare_write(ctx, reduced.as_ref().unwrap_or(ir), version, session)?;
    if !ir.scope.model {
        plan.losses.push(RhinoLossCode::WriterScopeExpanded.note(
            "the 3DM archive represents an empty model where the input model scope is absent",
        ));
    }
    if let Some((candidate, record)) = extrusion {
        ctx.charge_entities(
            1 + cadmpeg_core::decode::u64_from_index(candidate.profile_count),
            "write Rhino extrusion records",
        )?;
        plan.brep_records
            .append(&record, "retain Rhino extrusion record")?;
        plan.brep_count += 1;
        plan.written_bodies.insert(candidate.body_id);
        plan.topology_points
            .extend(ir.model.points.iter().map(|point| point.id.to_string()));
        plan.written_curves
            .extend(ir.model.curves.iter().map(|curve| curve.id.to_string()));
    }
    plan.losses.splice(0..0, admit_losses);
    // The B-rep records are staged before the prefix, which names their
    // bodies. The prefix and the object table header are appended after the
    // records in the same buffer and rotated in front of them, so the
    // records are not held twice.
    debug_assert!(output.is_empty(), "the Rhino archive starts empty");
    std::mem::swap(output, &mut plan.brep_records);
    let brep_len = output.len();
    plan.losses.extend(write_archive_prefix(
        ctx,
        ir,
        version,
        &plan.written_bodies,
        &plan.materials,
        output,
    )?);
    let table_start = output.len() - brep_len;
    output.append(
        &TCODE_OBJECT_TABLE.to_le_bytes(),
        "retain Rhino archive bytes",
    )?;
    output.append(&0_i64.to_le_bytes(), "retain Rhino archive bytes")?;
    let body_start = output.len() - brep_len;
    let rotated = output.rotate_tail_to_front(brep_len);
    debug_assert!(rotated, "the staged B-rep records precede the prefix");
    for point in ir
        .model
        .points
        .iter()
        .filter(|point| !plan.topology_points.contains(point.id.as_str()))
    {
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
        output.append(
            &attributed_object_record(
                ir,
                1,
                POINT_CLASS,
                &payload,
                point.id.as_str(),
                display,
                &plan.materials,
            )?,
            "retain Rhino archive bytes",
        )?;
        if is_support(point.source_object.as_ref()) {
            plan.losses
                .push(RhinoLossCode::WriterCarrierResolved.note(format!(
                    "support point {} is written as an independent point object",
                    point.id
                )));
        }
    }
    for (id, payload) in &plan.curves {
        let Some(curve) = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == *id)
        else {
            // Rule 1 (#217 §5.0): the plan prepared this curve from the same
            // document, so the lookup cannot fail on a valid document.
            debug_assert!(false, "prepared curve {id} is missing");
            continue;
        };
        let display = presentation::display(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Curve(curve.id.clone()),
            curve.source_object.as_ref(),
        );
        ctx.charge_entities(payload.record_count, "write Rhino curve records")?;
        output.append(
            &curve_object_record(ir, payload, id, display, &plan.materials)?,
            "retain Rhino archive bytes",
        )?;
    }
    for (id, payload) in &plan.surfaces {
        let Some(surface) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == *id)
        else {
            // Rule 1 (#217 §5.0): the plan prepared this surface from the same
            // document, so the lookup cannot fail on a valid document.
            debug_assert!(false, "prepared surface {id} is missing");
            continue;
        };
        let display = presentation::display(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Surface(surface.id.clone()),
            surface.source_object.as_ref(),
        );
        ctx.charge_entities(
            cadmpeg_core::decode::u64_from_index(payload.record_count),
            "write Rhino surface records",
        )?;
        output.append(
            &surface_object_record(ir, payload, id, display, &plan.materials)?,
            "retain Rhino archive bytes",
        )?;
    }
    let mut written_meshes = 0;
    for mesh in &plan.meshes {
        let payload = match mesh_payload(mesh, version) {
            Ok(payload) => payload,
            Err(WriteError::Omit(reason)) => {
                plan.losses.push(
                    RhinoLossCode::WriterEntityOmitted
                        .note(format!("mesh {} omitted: {reason}", mesh.id)),
                );
                continue;
            }
            Err(error) => return Err(error),
        };
        let metadata_dropped = mesh.body.is_some()
            || !mesh.feature_edges().is_empty()
            || !mesh.per_corner_normals().is_empty()
            || !mesh.triangle_groups().is_empty()
            || !mesh.texture_assignments().is_empty();
        if metadata_dropped {
            plan.losses
                .push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                    "mesh {} auxiliary metadata omitted; triangle geometry is retained",
                    mesh.id
                )));
        }
        let mut seen = std::collections::BTreeSet::new();
        let omitted_channels = mesh
            .channels()
            .iter()
            .filter(|channel| {
                !mesh_channel_supported(channel, mesh.vertex_count())
                    || !seen.insert(channel.kind())
            })
            .map(|channel| format!("{:#x}", channel.kind()))
            .collect::<Vec<_>>();
        if !omitted_channels.is_empty() {
            plan.losses
                .push(RhinoLossCode::WriterAttributesOmitted.note(format!(
                    "mesh {} channels omitted: {}",
                    mesh.id,
                    omitted_channels.join(", ")
                )));
        }
        if !version.stores_mesh_vertices_as_f64()
            && mesh.vertices().iter().any(|point| {
                [point.x, point.y, point.z].into_iter().any(|value| {
                    cadmpeg_core::convert::f32_from_f64(value).map(f64::from) != Some(value)
                })
            })
        {
            plan.losses
                .push(RhinoLossCode::MeshVertexPrecisionReduced.note(format!(
                    "mesh {} vertices are rounded to f32 in archive version 50",
                    mesh.id
                )));
        }
        if mesh.vertex_normals().iter().any(|normal| {
            [normal.x, normal.y, normal.z].into_iter().any(|value| {
                cadmpeg_core::convert::f32_from_f64(value).map(f64::from) != Some(value)
            })
        }) {
            plan.losses.push(
                RhinoLossCode::MeshNormalPrecisionReduced
                    .note(format!("mesh {} normals are rounded to f32", mesh.id)),
            );
        }
        let display = presentation::display(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Tessellation(mesh.id.to_string()),
            mesh.source_object.as_ref(),
        );
        ctx.charge_entities(1, "write Rhino object record")?;
        output.append(
            &mesh_object_record(ir, &payload, mesh.id.as_str(), display, &plan.materials)?,
            "retain Rhino archive bytes",
        )?;
        written_meshes += 1;
    }

    let subds = subd::write(
        ctx,
        ir,
        version,
        session,
        output,
        &plan.materials,
        &mut plan.losses,
    )?;

    let references_exported = tables::products::references(ir, &plan.materials)?;
    plan.losses.extend(references_exported.losses);
    for record in &references_exported.records {
        ctx.charge_entities(1, "write Rhino object record")?;
        output.append(record, "retain Rhino archive bytes")?;
    }
    let annotations_exported = tables::annotations::records(ir, &plan.materials)?;
    plan.losses.extend(annotations_exported.losses);
    for record in &annotations_exported.records {
        ctx.charge_entities(1, "write Rhino object record")?;
        output.append(record, "retain Rhino archive bytes")?;
    }
    let history_exported = tables::history::records(ir)?;
    plan.losses.extend(history_exported.losses);

    output.append(
        &short_chunk(TCODE_ENDOFTABLE, 0),
        "retain Rhino archive bytes",
    )?;
    let table_end = output.len();
    let body_len = archive_body_len(cadmpeg_core::decode::u64_from_index(table_end - body_start))?;
    let patched = output.overwrite(table_start + 4, &body_len.to_le_bytes());
    debug_assert!(patched, "the staged Rhino object table header exists");
    output.append(
        &table(ctx, TCODE_HISTORY_RECORD_TABLE, &history_exported.records)?,
        "retain Rhino archive bytes",
    )?;
    let final_size = archive_final_size(cadmpeg_core::decode::u64_from_index(output.len()))?;
    output.append(
        &long_chunk(TCODE_ENDOFFILE, &final_size.to_le_bytes())?,
        "retain Rhino archive bytes",
    )?;
    let free_points = ir
        .model
        .points
        .iter()
        .filter(|point| !plan.topology_points.contains(point.id.as_str()))
        .count();
    Ok(WriteOutcome {
        geometry_objects: plan.brep_count
            + free_points
            + plan.curves.len()
            + plan.surfaces.len()
            + written_meshes
            + subds,
        losses: plan.losses,
        subds,
        history_features: history_exported.written_features,
        face_colors: plan.face_colors,
        written_curves: plan.written_curves,
    })
}

fn write_archive_prefix(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    version: RhinoArchiveVersion,
    written_bodies: &std::collections::BTreeSet<String>,
    materials: &MaterialPlan,
    output: &mut EncodeBuffer<'_>,
) -> Result<Vec<LossNote>, WriteError> {
    output.append(&header(version), "retain Rhino archive bytes")?;
    output.append(&long_chunk(1, b"cadmpeg")?, "retain Rhino archive bytes")?;
    let mut properties = vec![short_chunk(
        TCODE_PROPERTIES_OPENNURBS_VERSION,
        OPENNURBS_WRITER_VERSION,
    )];
    let documents = tables::document::properties(ir)?;
    properties.extend(documents.records);
    output.append(
        &table(ctx, TCODE_PROPERTIES_TABLE, &properties)?,
        "retain Rhino archive bytes",
    )?;
    let angular = ir.tolerances.angular.get();
    let mut setting_losses = Vec::new();
    if angular > std::f64::consts::PI {
        setting_losses.push(RhinoLossCode::WriterAngularToleranceReduced.note(format!(
            "document angular tolerance {angular} is reduced to pi"
        )));
    }
    let mut settings = vec![units_record(
        ir.tolerances.linear_or_default().get(),
        angular.min(std::f64::consts::PI),
    )?];
    let exported = tables::document::settings(ir, materials, &mut setting_losses)?;
    settings.extend(exported.records);
    let table_settings = tables::document::table_settings(ir, version)?;
    setting_losses.extend(table_settings.losses);
    settings.extend(table_settings.records);
    let views = tables::views::settings(ir)?;
    settings.extend(views.settings);
    output.append(
        &table(ctx, TCODE_SETTINGS_TABLE, &settings)?,
        "retain Rhino archive bytes",
    )?;
    let mut image_losses = Vec::new();
    let mut dimstyle_losses = Vec::new();
    let mut texture_losses = Vec::new();
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
            TCODE_BITMAP_TABLE => {
                let exported = tables::images::images(ir)?;
                image_losses.extend(exported.losses);
                exported.records
            }
            TCODE_LIGHT_TABLE => tables::lights::lights(ir)?,
            TCODE_FONT_TABLE => tables::fonts::styles(ir)?,
            TCODE_HATCH_PATTERN_TABLE => tables::hatches::patterns(ir)?,
            TCODE_INSTANCE_DEFINITION_TABLE => {
                tables::products::definitions(ir, written_bodies, version)?
            }
            TCODE_MATERIAL_TABLE => materials.encode(ir)?,
            TCODE_GROUP_TABLE => tables::native::groups(ir)?,
            TCODE_LINETYPE_TABLE => tables::native::linetypes(ir)?,
            TCODE_DIMSTYLE_TABLE => {
                let exported = tables::dimstyles::styles(ir, version)?;
                dimstyle_losses.extend(exported.losses);
                exported.records
            }
            TCODE_TEXTURE_MAPPING_TABLE => {
                let exported = tables::texture_mappings::mappings(ir, version)?;
                texture_losses.extend(exported.losses);
                exported.records
            }
            _ => Vec::new(),
        };
        output.append(
            &table(ctx, typecode, &records)?,
            "retain Rhino archive bytes",
        )?;
        if typecode == TCODE_LINETYPE_TABLE {
            output.append(
                &table(ctx, TCODE_LAYER_TABLE, &tables::layers(ir)?)?,
                "retain Rhino archive bytes",
            )?;
        }
    }
    let mut losses = documents.losses;
    losses.extend(setting_losses);
    losses.extend(views.losses);
    losses.extend(image_losses);
    losses.extend(dimstyle_losses);
    losses.extend(texture_losses);
    losses.extend(tables::unwritten::losses(ir));
    Ok(losses)
}

struct BrepPayload {
    body: Vec<u8>,
    direct: Vec<u8>,
    record_count: u64,
}

/// A native class identifier and the payload encoded for it.
type ClassPayload = ([u8; 16], Vec<u8>);
type SurfacePointAt = Box<dyn Fn(f64, f64) -> [f64; 3]>;

#[derive(Clone)]
struct SurfacePayload {
    class: [u8; 16],
    body: Vec<u8>,
    direct: Option<Vec<u8>>,
    record_count: usize,
}

struct WritePlan<'a, 'budget> {
    losses: Vec<LossNote>,
    meshes: Vec<cadmpeg_ir::tessellation::Tessellation>,
    /// Each free curve's identity and encoded payload.
    curves: Vec<(&'a str, compounds::CurvePayload)>,
    /// Each free surface's identity and encoded payload.
    surfaces: Vec<(&'a str, SurfacePayload)>,
    brep_records: EncodeBuffer<'budget>,
    brep_count: usize,
    written_bodies: std::collections::BTreeSet<String>,
    face_colors: std::collections::BTreeSet<String>,
    written_curves: std::collections::BTreeSet<String>,
    topology_points: std::collections::BTreeSet<String>,
    materials: MaterialPlan,
}

/// One record the target cannot carry, or a genuine encode failure (#217 §5.0).
///
/// `Omit` is control flow: the record boundary reports the loss and continues.
#[derive(Debug)]
pub(super) enum WriteError {
    /// Content the target cannot express; the message names the record.
    Omit(String),
    /// A genuine failure that fails the write.
    Fail(CodecError),
    /// A typed encode failure.
    Encode(cadmpeg_ir::codec::write::EncodeError),
}

impl From<cadmpeg_ir::codec::write::EncodeError> for WriteError {
    fn from(error: cadmpeg_ir::codec::write::EncodeError) -> Self {
        Self::Encode(error)
    }
}

impl From<CodecError> for WriteError {
    fn from(error: CodecError) -> Self {
        Self::Fail(error)
    }
}

impl From<cadmpeg_core::decode::ResourceLimit> for WriteError {
    fn from(limit: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Encode(limit.into())
    }
}

impl From<cadmpeg_ir::native::NativeConvertError> for WriteError {
    fn from(error: cadmpeg_ir::native::NativeConvertError) -> Self {
        Self::Fail(error.into())
    }
}

/// One carrier resolution naming every wrapper it skipped, so no dropped
/// wrapper stays silent when its edge is written on the resolved leaf.
fn reduction_loss(carrier: &ReducedCarrier<'_>) -> LossNote {
    let coedges = (!carrier.coedges.is_empty())
        .then(|| format!("; affecting coedge(s): {}", carrier.coedges.join(", ")));
    match carrier.leaf {
        Some(leaf) => RhinoLossCode::WriterCarrierResolved.note(format!(
            "edge {} composite carrier {} was reduced to held solved carrier {}{}",
            carrier.edge,
            carrier.skipped.join(", "),
            leaf,
            coedges.unwrap_or_default()
        )),
        None => RhinoLossCode::WriterCarrierResolved.note(format!(
            "edge {} transformed carrier {} was resolved to an explicit carrier{}",
            carrier.edge,
            carrier.skipped.join(", "),
            coedges.unwrap_or_default()
        )),
    }
}

/// One body omission naming every member of the dropped closure, so no
/// sub-record drops silently when its body does.
fn omitted_body(
    baked: &CadIr,
    reason: &str,
    carried: &std::collections::BTreeSet<String>,
) -> LossNote {
    let members = reduce::body_closure(baked)
        .difference(carried)
        .cloned()
        .collect::<Vec<_>>();
    RhinoLossCode::WriterBodyOmitted.note(format!(
        "body {} omitted: {reason}; {} member record(s): {}",
        baked.model.bodies[0].id,
        members.len(),
        members.join(", ")
    ))
}

fn prepare_write<'a, 'budget>(
    ctx: &DecodeContext<'_>,
    ir: &'a CadIr,
    archive_version: RhinoArchiveVersion,
    session: &'budget EncodeSession,
) -> Result<WritePlan<'a, 'budget>, WriteError> {
    let model = &ir.model;
    let materials = MaterialPlan::new(ir)?;
    let mut losses = attributes::name_losses(ir);
    losses.extend(materials.losses().iter().cloned());
    let (scopes, scope_losses) = scopes::collect(ir)?;
    losses.extend(scope_losses);
    let used_pcurves = scopes
        .iter()
        .flat_map(|scope| scope.pcurves.iter())
        .collect::<std::collections::BTreeSet<_>>();
    let outside_pcurves = model
        .pcurves
        .iter()
        .filter(|row| !used_pcurves.iter().any(|id| id.as_str() == row.id.as_str()))
        .map(|row| row.id.as_str())
        .collect::<Vec<_>>();
    if !outside_pcurves.is_empty() {
        losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
            "{} unowned pcurve record(s) omitted: {}",
            outside_pcurves.len(),
            outside_pcurves.join(", ")
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
    let scoped_ids = |select: fn(&cadmpeg_ir::document::Model) -> Vec<&str>| {
        scopes
            .iter()
            .flat_map(|scope| select(&scope.ir.model))
            .map(str::to_owned)
            .collect::<std::collections::BTreeSet<_>>()
    };
    let mut outside_topology = Vec::new();
    for (owned, scoped) in [
        (
            model
                .regions
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            scoped_ids(|model| model.regions.iter().map(|row| row.id.as_str()).collect()),
        ),
        (
            model
                .shells
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            scoped_ids(|model| model.shells.iter().map(|row| row.id.as_str()).collect()),
        ),
        (
            model
                .faces
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            scoped_ids(|model| model.faces.iter().map(|row| row.id.as_str()).collect()),
        ),
        (
            model
                .loops
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            scoped_ids(|model| model.loops.iter().map(|row| row.id.as_str()).collect()),
        ),
        (
            model
                .coedges
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            scoped_ids(|model| model.coedges.iter().map(|row| row.id.as_str()).collect()),
        ),
        (
            model
                .edges
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            scoped_ids(|model| model.edges.iter().map(|row| row.id.as_str()).collect()),
        ),
        (
            model
                .vertices
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            scoped_ids(|model| model.vertices.iter().map(|row| row.id.as_str()).collect()),
        ),
    ] {
        for id in owned {
            if !scoped.contains(id) {
                outside_topology.push(id.to_owned());
            }
        }
    }
    if !outside_topology.is_empty() {
        losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
            "topology outside owned body closures is omitted: {}",
            outside_topology.join(", ")
        )));
    }
    let topology_points = scopes
        .iter()
        .flat_map(|scope| scope.points.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    let mut revolutions = std::collections::BTreeMap::new();
    let mut extrusions = std::collections::BTreeMap::new();
    let mut revolution_profiles = std::collections::BTreeSet::new();
    for surface in model
        .surfaces
        .iter()
        .filter(|surface| !topology_surfaces.contains(surface.id.as_str()))
    {
        let native_extrusion = match extrusion::separate(session, ir, surface, archive_version)? {
            Some(value) => Some(value),
            None => extrusion::sum(session, ir, surface, archive_version)?,
        };
        if let Some((profile, payload)) = native_extrusion {
            let independently_presented = model.appearance_bindings.iter().any(|binding| matches!(&binding.target, cadmpeg_ir::appearance::AppearanceTarget::Curve(id) if *id == profile))
                || model.attributes.iter().any(|attribute| matches!(&attribute.target, cadmpeg_ir::attributes::AttributeTarget::Curve(id) if *id == profile))
                || model.presentation_layers.iter().any(|layer| layer.items.iter().any(|item| matches!(item, cadmpeg_ir::presentation::PresentationItem::Curve { curve } if *curve == profile)))
                || model.view_presentations.iter().any(|view| view.object.as_deref() == Some(profile.as_str()));
            if !independently_presented && !topology_curves.contains(profile.as_str()) {
                revolution_profiles.insert(profile.clone());
            }
            let is_sum = payload.class == crate::surfaces::SUM_SURFACE.to_wire();
            let carrier_kind = if is_sum { "sum surface" } else { "extrusion" };
            losses.push(RhinoLossCode::WriterCarrierResolved.note(format!(
                "surface {} and profile {} are written as an independent {carrier_kind} object",
                surface.id, profile
            )));
            if is_sum {
                if let Some(construction) = surface.geometry.procedural_construction() {
                    losses.push(RhinoLossCode::WriterProceduralReduced.note(format!(
                        "surface {} extrusion construction {} is converted to an exact sum-surface carrier",
                        surface.id, construction
                    )));
                }
            }
            extrusions.insert(surface.id.as_str(), payload);
            continue;
        }
        if let Some((profile, payload)) = revolution::held_revolution(ir, surface)? {
            let independently_presented = model.appearance_bindings.iter().any(|binding| matches!(&binding.target, cadmpeg_ir::appearance::AppearanceTarget::Curve(id) if *id == profile))
                || model.attributes.iter().any(|attribute| matches!(&attribute.target, cadmpeg_ir::attributes::AttributeTarget::Curve(id) if *id == profile))
                || model.presentation_layers.iter().any(|layer| layer.items.iter().any(|item| matches!(item, cadmpeg_ir::presentation::PresentationItem::Curve { curve } if *curve == profile)))
                || model.view_presentations.iter().any(|view| view.object.as_deref() == Some(profile.as_str()));
            if !independently_presented
                && model
                    .curves
                    .iter()
                    .find(|curve| curve.id == profile)
                    .is_some_and(|curve| {
                        curve.source_object.is_some()
                            && curve.source_object == surface.source_object
                    })
            {
                revolution_profiles.insert(profile);
            }
            revolutions.insert(surface.id.as_str(), payload);
        }
    }
    let compound_children = compounds::children(ir);
    let unrooted_curves = compounds::unrooted_members(ir);
    let mut curves = Vec::new();
    let mut written_curves = revolution_profiles
        .iter()
        .map(ToString::to_string)
        .collect::<std::collections::BTreeSet<_>>();
    let mut pending = model
        .curves
        .iter()
        .filter(|curve| {
            (!compound_children.contains(&curve.id) || unrooted_curves.contains(&curve.id))
                && !revolution_profiles.contains(&curve.id)
                && !topology_curves.contains(curve.id.as_str())
        })
        .collect::<std::collections::VecDeque<_>>();
    let mut covered = std::collections::BTreeSet::new();
    let mut attempted = std::collections::BTreeSet::new();
    while let Some(curve) = pending.pop_front() {
        if covered.contains(&curve.id) || !attempted.insert(curve.id.clone()) {
            continue;
        }
        match compounds::payload(ir, curve, &mut std::collections::BTreeSet::new()) {
            Ok((payload, reduction_notes)) => {
                for child in compounds::unrepresented_children(ir, curve, &payload.represented) {
                    if !topology_curves.contains(child.id.as_str()) {
                        pending.push_back(child);
                    }
                }
                if is_support(curve.source_object.as_ref()) {
                    losses.push(RhinoLossCode::WriterCarrierResolved.note(format!(
                        "support curve {} is written as an independent curve object",
                        curve.id
                    )));
                }
                covered.extend(payload.represented.iter().cloned());
                written_curves.extend(payload.represented.iter().map(ToString::to_string));
                curves.push((curve.id.as_str(), payload));
                losses.extend(reduction_notes);
            }
            Err(WriteError::Omit(reason)) => {
                losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                        "curve {} omitted: {reason}{}",
                        curve.id,
                        curve
                            .geometry
                            .procedural_construction()
                            .map(|construction| format!(
                                "; procedural construction {construction} omitted"
                            ))
                            .unwrap_or_default()
                    )));
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
        let written_native = extrusions
            .get(surface.id.as_str())
            .or_else(|| revolutions.get(surface.id.as_str()));
        match written_native.cloned().map_or_else(
            || {
                WritableFaceSurface::try_new(surface, ir).and_then(|(face, loss)| {
                    losses.extend(loss);
                    face.payload()
                })
            },
            Ok,
        ) {
            Ok(payload) => {
                surfaces.push((surface.id.as_str(), payload));
                if let Some(construction) = surface
                    .geometry
                    .procedural_construction()
                    .filter(|_| written_native.is_none())
                {
                    losses.push(RhinoLossCode::WriterProceduralReduced.note(format!(
                        "surface {} procedural construction {} was reduced to a held solved carrier",
                        surface.id, construction
                    )));
                }
            }
            Err(WriteError::Omit(reason)) => {
                losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                    "surface {} omitted: {reason}{}",
                    surface.id,
                    surface
                        .geometry
                        .procedural_construction()
                        .map(|construction| format!(
                            "; procedural construction {construction} omitted"
                        ))
                        .unwrap_or_default()
                )));
            }
            Err(error) => return Err(error),
        }
    }
    let referenced_procedural_curves = model
        .curves
        .iter()
        .filter_map(|curve| curve.geometry.procedural_construction())
        .collect::<std::collections::BTreeSet<_>>();
    let referenced_procedural_surfaces = model
        .surfaces
        .iter()
        .filter_map(|surface| surface.geometry.procedural_construction())
        .collect::<std::collections::BTreeSet<_>>();
    for construction in &model.procedural_curves {
        if !referenced_procedural_curves.contains(&construction.id) {
            losses.push(RhinoLossCode::WriterProceduralReduced.note(format!(
                "procedural curve {} is omitted: no held curve uses its construction",
                construction.id
            )));
        }
    }
    for construction in &model.procedural_surfaces {
        if !referenced_procedural_surfaces.contains(&construction.id) {
            losses.push(RhinoLossCode::WriterProceduralReduced.note(format!(
                "procedural surface {} is omitted: no held surface uses its construction",
                construction.id
            )));
        }
    }
    let mut meshes = Vec::new();
    for source in &model.tessellations {
        let mut mesh = source.clone();
        if let Some(body_id) = &mesh.body {
            let Some(mut body) = model
                .bodies
                .iter()
                .find(|body| body.id == *body_id)
                .cloned()
            else {
                losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                    "mesh {} omitted: body {} is missing",
                    mesh.id, body_id
                )));
                continue;
            };
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
                Err(WriteError::Omit(reason)) => {
                    losses.push(
                        RhinoLossCode::WriterEntityOmitted
                            .note(format!("mesh {} omitted: {reason}", source.id)),
                    );
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        match check_mesh(&mesh) {
            Ok(()) => meshes.push(mesh),
            Err(WriteError::Omit(reason)) => losses.push(
                RhinoLossCode::WriterEntityOmitted
                    .note(format!("mesh {} omitted: {reason}", mesh.id)),
            ),
            Err(error) => return Err(error),
        }
    }
    let mut brep_records = session.buffer();
    let mut brep_count = 0;
    let mut written_bodies = std::collections::BTreeSet::new();
    let mut face_colors = std::collections::BTreeSet::new();
    let mut failed_bodies = Vec::new();
    let mut carried = std::collections::BTreeSet::new();
    for scope in &scopes {
        let mut baked = scope.ir.clone();
        for face in &mut baked.model.faces {
            if face.color.is_none() {
                face.color = tables::materials::bound_appearance(ir, face.id.as_str())
                    .and_then(|appearance| appearance.base_color);
            }
        }
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
            Err(WriteError::Omit(reason)) => {
                losses.push(omitted_body(&baked, &reason, &carried));
                continue;
            }
            Err(error) => return Err(error),
        }
        if let Some((wire, curve_id, conversion)) = wire::single_curve(&baked, archive_version)? {
            let body = &baked.model.bodies[0];
            let edge = &baked.model.edges[0];
            let display = presentation::display(
                ir,
                &cadmpeg_ir::appearance::AppearanceTarget::Edge(edge.id.clone()),
                None,
            );
            ctx.charge_entities(wire.record_count, "write Rhino Brep records")?;
            brep_records.append(
                &brep_object_record(
                    ir,
                    &wire,
                    body.id.as_str(),
                    display.name.or(body.name.as_deref()),
                    display.color.or(body.color),
                    display.visible.or(body.visible),
                    &materials,
                )?,
                "retain Rhino Brep records",
            )?;
            losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                "body {} wire ownership is written as a 3DM Brep without shell membership: region {}, shell {}",
                body.id, baked.model.regions[0].id, baked.model.shells[0].id
            )));
            losses.extend(conversion);
            if let Some(curve) = baked.model.curves.iter().find(|curve| curve.id == curve_id) {
                if matches!(
                    curve.geometry.solved(),
                    Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Circle(_))
                ) {
                    losses.push(RhinoLossCode::WriterCarrierResolved.note(format!(
                        "curve {} is written as its exact bounded 3DM arc",
                        curve.id
                    )));
                }
                if !is_support(curve.source_object.as_ref()) {
                    losses.push(RhinoLossCode::WriterCarrierResolved.note(format!(
                        "curve {} is written as a Brep edge carrier",
                        curve.id
                    )));
                }
                if curve
                    .parameter_range
                    .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
                    != edge.param_range().map(cadmpeg_ir::units::FiniteVector::get)
                {
                    losses.push(RhinoLossCode::WriterCarrierResolved.note(format!(
                        "curve {} is restricted to the exact span of wire edge {}",
                        curve.id, edge.id
                    )));
                }
            }
            written_curves.insert(curve_id.to_string());
            written_bodies.insert(body.id.to_string());
            brep_count += 1;
            continue;
        }
        let loose_written = loose::write(
            ctx,
            &baked,
            ir,
            &mut brep_records,
            &mut losses,
            &materials,
            &mut written_curves,
        )?;
        brep_count += loose_written;
        if baked.model.faces.is_empty() {
            if loose_written == 0 {
                losses.push(omitted_body(
                    &baked,
                    "no face remains after loose-member separation",
                    &carried,
                ));
            }
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
        let payload = match WritableModel::try_new(&baked).and_then(|model| {
            let payload = brep_payload(ctx, &model, archive_version)?;
            Ok((
                payload,
                model.reduced_carriers,
                model.surface_conversions,
                model.edge_conversions,
            ))
        }) {
            Ok((payload, reduced, converted, edge_converted)) => {
                for carrier in &reduced {
                    losses.push(reduction_loss(carrier));
                }
                losses.extend(converted);
                losses.extend(edge_converted);
                payload
            }
            Err(WriteError::Omit(reason))
                if matches!(
                    baked.model.bodies[0].kind,
                    cadmpeg_ir::topology::BodyKind::Sheet | cadmpeg_ir::topology::BodyKind::Solid
                ) =>
            {
                let solid = baked.model.bodies[0].kind == cadmpeg_ir::topology::BodyKind::Solid;
                let candidate = solid.then(|| {
                    let mut candidate = baked.clone();
                    candidate.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Sheet;
                    candidate
                });
                let last_payload = std::cell::RefCell::new(None);
                let (reduced, reduction_losses) =
                    reduce::sheet(candidate.as_ref().unwrap_or(&baked), |candidate| {
                        let payload = WritableModel::try_new(candidate)
                            .and_then(|model| brep_payload(ctx, &model, archive_version))?;
                        last_payload.replace(Some(payload));
                        Ok(())
                    })?;
                losses.extend(reduction_losses);
                let Some(reduced) = reduced else {
                    failed_bodies.push((
                        baked,
                        Some("no sheet face admits a Brep payload".to_owned()),
                    ));
                    continue;
                };
                if solid {
                    let members = reduce::body_closure(&baked).into_iter().collect::<Vec<_>>();
                    losses.push(RhinoLossCode::WriterEntityOmitted.note(format!(
                        "body {} solid is reduced to a sheet: {reason}; source body members: {}",
                        baked.model.bodies[0].id,
                        members.join(", ")
                    )));
                }
                if reduced.model.faces.len() < baked.model.faces.len() {
                    failed_bodies.push((std::mem::replace(&mut baked, reduced), None));
                } else {
                    baked = reduced;
                }
                // Rule 1 (#217 §5.0): a successful admission runs the callback
                // that stages the payload, so it cannot be absent here.
                let Some(payload) = last_payload.into_inner() else {
                    debug_assert!(false, "sheet admission produced no payload");
                    continue;
                };
                // The admitted candidate re-resolves exactly the carriers the
                // admission used, so its reductions are the emitted ones.
                if let Ok(model) = WritableModel::try_new(&baked) {
                    for carrier in &model.reduced_carriers {
                        losses.push(reduction_loss(carrier));
                    }
                    losses.extend(model.surface_conversions);
                    losses.extend(model.edge_conversions);
                }
                payload
            }
            Err(WriteError::Omit(reason)) => {
                failed_bodies.push((baked, Some(reason)));
                continue;
            }
            Err(error) => return Err(error),
        };
        for surface in &baked.model.surfaces {
            if let Some(construction) = surface.geometry.procedural_construction() {
                losses.push(RhinoLossCode::WriterProceduralReduced.note(format!(
                    "surface {} procedural construction {} was reduced to a held solved carrier",
                    surface.id, construction
                )));
            }
        }
        let body = &baked.model.bodies[0];
        if archive_version.uses_face_array_v2() {
            if let Some(colors) =
                encoded_face_colors(&baked.model.faces.iter().collect::<Vec<_>>())?
            {
                face_colors.extend(
                    baked
                        .model
                        .faces
                        .iter()
                        .zip(colors)
                        .filter(|(face, color)| face.color.is_some() && *color != [u8::MAX; 4])
                        .map(|(face, _)| face.id.to_string()),
                );
            }
        }
        carried.extend(reduce::body_closure(&baked));
        written_bodies.insert(body.id.to_string());
        brep_count += 1;
        ctx.charge_entities(payload.record_count, "write Rhino Brep records")?;
        brep_records.append(
            &brep_object_record(
                ir,
                &payload,
                body.id.as_str(),
                body.name.as_deref(),
                body.color,
                body.visible,
                &materials,
            )?,
            "retain Rhino Brep records",
        )?;
    }
    for (baked, reason) in failed_bodies {
        let mut retained_body = false;
        if reason.is_some() {
            if let Some((payload, retained)) = wire::vertex_body(&baked, archive_version)? {
                let body = &baked.model.bodies[0];
                ctx.charge_entities(payload.record_count, "write Rhino Brep records")?;
                brep_records.append(
                    &brep_object_record(
                        ir,
                        &payload,
                        body.id.as_str(),
                        body.name.as_deref(),
                        body.color,
                        body.visible,
                        &materials,
                    )?,
                    "retain Rhino Brep records",
                )?;
                carried.extend(retained);
                carried.insert(body.id.to_string());
                written_bodies.insert(body.id.to_string());
                brep_count += 1;
                retained_body = true;
            }
        }
        brep_count += salvage::write(
            ctx,
            ir,
            &baked,
            &mut brep_records,
            &materials,
            salvage::SalvageRecords {
                carried: &mut carried,
                curves: &mut written_curves,
            },
            &mut losses,
        )?;
        if let Some(reason) = reason {
            if retained_body {
                let members = reduce::body_closure(&baked)
                    .difference(&carried)
                    .cloned()
                    .collect::<Vec<_>>();
                losses.push(RhinoLossCode::WriterBodyReduced.note(format!(
                    "body {} is reduced to a vertex Brep: {reason}; {} omitted member record(s): {}",
                    baked.model.bodies[0].id,
                    members.len(),
                    members.join(", ")
                )));
            } else {
                losses.push(omitted_body(&baked, &reason, &carried));
            }
        }
    }
    Ok(WritePlan {
        losses,
        meshes,
        curves,
        surfaces,
        brep_records,
        brep_count,
        written_bodies,
        face_colors,
        written_curves,
        topology_points,
        materials,
    })
}

fn native_dispositions(
    ir: &CadIr,
    version: RhinoArchiveVersion,
) -> Result<cadmpeg_ir::codec::write::NativeDispositions, WriteError> {
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
                        if ir.model.appearances.iter().any(|appearance| {
                            appearance.native_ref.as_deref() == Some(record.id())
                        }) =>
                    {
                        NativeDisposition::Regenerated
                    }
                    "materials" if tables::materials::native_complete(record) => {
                        NativeDisposition::Written
                    }
                    _ => NativeDisposition::Dropped,
                };
                dispositions.set_record("rhino", arena, record.id(), disposition);
            }
        }
    }
    let emitted_history = tables::history::records(ir)?;
    for record in records_in(ir, "history_records") {
        if emitted_history.written.contains(record.id()) {
            dispositions.set_record(
                "rhino",
                "history_records",
                record.id(),
                NativeDisposition::Written,
            );
        }
    }
    let emitted_styles = tables::dimstyles::styles(ir, version)?;
    for record in records_in(ir, "dimension_styles") {
        if emitted_styles.written.contains(record.id()) {
            dispositions.set_record(
                "rhino",
                "dimension_styles",
                record.id(),
                NativeDisposition::Written,
            );
        }
    }
    let emitted_mappings = tables::texture_mappings::mappings(ir, version)?;
    for record in records_in(ir, "texture_mappings") {
        if emitted_mappings.written.contains(record.id()) {
            dispositions.set_record(
                "rhino",
                "texture_mappings",
                record.id(),
                NativeDisposition::Written,
            );
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
    for record in records_in(ir, "previews") {
        if properties.previews_written.contains(record.id()) {
            dispositions.set_record(
                "rhino",
                "previews",
                record.id(),
                NativeDisposition::Regenerated,
            );
        }
    }
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
    for record in records_in(ir, "document_settings") {
        if properties.settings_written.contains(record.id()) {
            dispositions.set_record(
                "rhino",
                "document_settings",
                record.id(),
                NativeDisposition::Written,
            );
        }
    }
    let mut setting_losses = Vec::new();
    let materials = MaterialPlan::new(ir)?;
    let document_settings = tables::document::settings(ir, &materials, &mut setting_losses)?;
    for record in records_in(ir, "document_settings") {
        if document_settings.regenerated.contains(record.id()) {
            dispositions.set_record(
                "rhino",
                "document_settings",
                record.id(),
                NativeDisposition::Regenerated,
            );
        } else if document_settings.written.contains(record.id()) {
            dispositions.set_record(
                "rhino",
                "document_settings",
                record.id(),
                NativeDisposition::Written,
            );
        }
    }
    for arena in ["lights", "hatch_patterns"] {
        for record in records_in(ir, arena) {
            dispositions.set_record("rhino", arena, record.id(), NativeDisposition::Written);
        }
    }
    let table_settings = tables::document::table_settings(ir, version)?;
    for arena in ["annotation_settings", "grid_defaults", "render_settings"] {
        for record in records_in(ir, arena) {
            if table_settings.written.contains(record.id()) {
                dispositions.set_record("rhino", arena, record.id(), NativeDisposition::Written);
            }
        }
    }
    let emitted = tables::annotations::records(ir, &materials)?;
    let linked_annotations = ir
        .model
        .semantic_annotations
        .iter()
        .filter(|annotation| emitted.written.contains(annotation.id.as_str()))
        .map(|annotation| annotation.native_ref.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for arena in [
        "annotations",
        "text_dots",
        "annotation_arrows",
        "legacy_v1_records",
    ] {
        for record in records_in(ir, arena) {
            if linked_annotations.contains(record.id()) {
                dispositions.set_record(
                    "rhino",
                    arena,
                    record.id(),
                    NativeDisposition::Regenerated,
                );
            }
        }
    }
    let referenced = tables::products::referenced_definitions(ir);
    let referenced_native = ir
        .model
        .product_definitions
        .iter()
        .filter(|definition| tables::products::definition_written(ir, &referenced, definition))
        .filter_map(|definition| definition.native_ref.as_deref())
        .collect::<std::collections::BTreeSet<_>>();
    for record in records_in(ir, "product_definitions") {
        if referenced_native.contains(record.id()) {
            dispositions.set_record(
                "rhino",
                "product_definitions",
                record.id(),
                NativeDisposition::Regenerated,
            );
        }
    }
    let emitted_references = tables::products::references(ir, &materials)?;
    let linked_occurrences = ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| emitted_references.written.contains(occurrence.id.as_str()))
        .filter_map(|occurrence| occurrence.native_ref.as_deref())
        .collect::<std::collections::BTreeSet<_>>();
    for record in records_in(ir, "product_occurrences") {
        if linked_occurrences.contains(record.id()) {
            dispositions.set_record(
                "rhino",
                "product_occurrences",
                record.id(),
                NativeDisposition::Regenerated,
            );
        }
    }
    let embedded = tables::images::images(ir)?;
    for record in records_in(ir, "embedded_images") {
        if embedded.regenerated.contains(record.id()) {
            dispositions.set_record(
                "rhino",
                "embedded_images",
                record.id(),
                NativeDisposition::Regenerated,
            );
        }
    }
    Ok(dispositions)
}

/// Held native records of one arena, or an empty slice when absent.
fn records_in<'a>(ir: &'a CadIr, arena: &str) -> &'a [cadmpeg_ir::NativeRecord] {
    ir.native
        .namespace("rhino")
        .and_then(|namespace| namespace.arenas().get(arena))
        .map_or(&[], Vec::as_slice)
}

fn brep_payload(
    ctx: &DecodeContext<'_>,
    model: &WritableModel<'_>,
    archive_version: RhinoArchiveVersion,
) -> Result<BrepPayload, WriteError> {
    use cadmpeg_ir::topology::{BodyKind, Sense};

    let version = if archive_version.uses_extended_brep_layout() {
        0x33
    } else {
        0x32
    };
    let mut payload = vec![version];
    let mut direct = vec![version];
    let mut record_count = 1_u64;
    let mut count_records = |count: usize| -> Result<(), WriteError> {
        record_count = record_count
            .checked_add(cadmpeg_core::decode::u64_from_index(count))
            .ok_or_else(|| WriteError::Omit("Brep record count exceeds u64".into()))?;
        Ok(())
    };
    let mut next_edge = 0_usize;
    let edge_indices = model
        .edges
        .iter()
        .map(|edge| {
            if matches!(edge.curve, WritableEdgeCurve::Singular) {
                None
            } else {
                let index = next_edge;
                next_edge += 1;
                Some(index)
            }
        })
        .collect::<Vec<_>>();
    let mut pcurve_slots = std::collections::BTreeMap::new();
    let mut c2 = Vec::new();
    let mut c2_indices = Vec::new();
    for coedge in &model.coedges {
        let key = coedge
            .explicit_pcurve
            .map(|pcurve| (pcurve.id.as_str(), coedge.c2.0, coedge.c2.1.as_slice()));
        let index = if let Some(index) = key.and_then(|key| pcurve_slots.get(&key).copied()) {
            index
        } else {
            let index = c2.len();
            c2.push(&coedge.c2);
            if let Some(key) = key {
                pcurve_slots.insert(key, index);
            }
            index
        };
        c2_indices.push(index);
    }
    count_records(c2.len())?;
    payload.extend(polymorphic_array(c2.into_iter())?);
    let c3 = model
        .edges
        .iter()
        .filter(|edge| !matches!(edge.curve, WritableEdgeCurve::Singular))
        .map(brep_c3_curve)
        .collect::<Result<Vec<_>, WriteError>>()?;
    count_records(c3.len())?;
    payload.extend(polymorphic_array(c3.iter())?);
    let surfaces = model
        .surfaces
        .iter()
        .map(WritableFaceSurface::payload)
        .collect::<Result<Vec<_>, WriteError>>()?;
    for surface in &surfaces {
        count_records(surface.record_count)?;
    }
    payload.extend(polymorphic_records(surfaces.iter().map(|surface| {
        (
            surface.class,
            surface.body.as_slice(),
            surface.direct.as_deref(),
        )
    }))?);
    let vertices = model
        .vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| {
            let incident = wire_indexes(
                model
                    .edges
                    .iter()
                    .enumerate()
                    .filter_map(|(position, edge)| {
                        edge_indices[position].map(|index| (index, edge))
                    })
                    .flat_map(|(position, edge)| {
                        [edge.start, edge.end]
                            .into_iter()
                            .filter(move |endpoint| *endpoint == index)
                            .map(move |_| position)
                    }),
            )?;
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
    count_records(vertices.len())?;
    payload.extend(raw_array(&vertices)?);
    let edges = model
        .edges
        .iter()
        .filter(|edge| !matches!(edge.curve, WritableEdgeCurve::Singular))
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
    count_records(edges.len())?;
    payload.extend(raw_array(&edges)?);
    let trims = model
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| {
            let edge = &model.edges[coedge.edge];
            let (from, to) = model.endpoints(index);
            let c2_index = wire_index(c2_indices[index])?;
            let index = wire_index(index)?;
            let mut record = index.to_le_bytes().to_vec();
            record.extend(c2_index.to_le_bytes());
            record.extend(
                coedge
                    .trim_domain(edge)
                    .into_iter()
                    .flat_map(|value| value.get().to_le_bytes()),
            );
            record.extend(
                match edge_indices[coedge.edge] {
                    Some(index) => wire_index(index)?,
                    None => -1,
                }
                .to_le_bytes(),
            );
            record.extend(wire_index(from)?.to_le_bytes());
            record.extend(wire_index(to)?.to_le_bytes());
            record.extend(i32::from(coedge.source.sense == Sense::Reversed).to_le_bytes());
            let same_loop = edge.uses.len() == 2
                && model.coedges[edge.uses[0]].owner_loop == model.coedges[edge.uses[1]].owner_loop;
            record.extend(
                if matches!(edge.curve, WritableEdgeCurve::Singular) {
                    4_i32
                } else {
                    brep_trim_type(edge.uses.len(), same_loop)
                }
                .to_le_bytes(),
            );
            record.extend(trim_iso::classify(model, coedge)?.to_le_bytes());
            record.extend(wire_index(coedge.owner_loop)?.to_le_bytes());
            record.extend(
                [
                    coedge.fit_tolerance,
                    if matches!(edge.curve, WritableEdgeCurve::Singular) {
                        edge.source
                            .tolerance
                            .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get)
                    } else {
                        0.0
                    },
                ]
                .into_iter()
                .flat_map(f64::to_le_bytes),
            );
            record.extend(
                coedge
                    .trim_domain(edge)
                    .into_iter()
                    .flat_map(|value| value.get().to_le_bytes()),
            );
            record.push(0);
            record.extend([0_u8; 31]);
            record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
            Ok(record)
        })
        .collect::<Result<Vec<_>, WriteError>>()?;
    count_records(trims.len())?;
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
    count_records(loops.len())?;
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
    count_records(faces.len())?;
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
        record_count,
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

fn brep_c3_curve(edge: &WritableEdge<'_>) -> Result<([u8; 16], Vec<u8>), WriteError> {
    Ok(match &edge.curve {
        WritableEdgeCurve::Singular => {
            return Err(WriteError::Omit(format!(
                "singular edge {} has no 3D curve",
                edge.source.id
            )))
        }
        WritableEdgeCurve::Line(line) => {
            let origin = line.origin().get();
            let direction = *line.direction().as_raw();
            let from = origin.translated(direction, edge.domain[0].get());
            let to = origin.translated(direction, edge.domain[1].get());
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
        WritableEdgeCurve::Circle(circle) => {
            let center = circle.center().get();
            let axis = circle.frame().axis().as_raw();
            let ref_direction = circle.frame().reference().as_raw();
            let radius = circle.radius().get();
            let domain = FiniteReal::raw_array(edge.domain);
            (
                ARC_CLASS,
                circle_payload(center, *axis, *ref_direction, radius, domain, domain, 3)?,
            )
        }
        WritableEdgeCurve::Nurbs(nurbs) => (NURBS_CURVE_CLASS, nurbs_curve_payload(nurbs)?),
        WritableEdgeCurve::NurbsOwned(nurbs) => (NURBS_CURVE_CLASS, nurbs_curve_payload(nurbs)?),
    })
}

fn generated_projected_brep_c2_curve(
    vertices: &[WritableVertex<'_>],
    edge: &WritableEdge<'_>,
    sense: cadmpeg_ir::topology::Sense,
    origin: cadmpeg_ir::math::Point3,
    u_axis: cadmpeg_ir::math::Vector3,
    v_axis: cadmpeg_ir::math::Vector3,
) -> Result<([u8; 16], Vec<u8>), WriteError> {
    use cadmpeg_ir::topology::Sense;

    let projected_nurbs = |nurbs: &cadmpeg_ir::geometry::nurbs::NurbsCurve| {
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
            .map_err(|error| WriteError::Omit(error.to_string()))?;
        if sense == Sense::Reversed {
            let sum = projected.knots()[usize::try_from(projected.degree()).map_err(|_| {
                WriteError::Omit(format!(
                    "edge {} NURBS degree exceeds address space",
                    edge.source.id
                ))
            })?] + projected.knots()[projected.pole_count()];
            projected.reverse_parameterization();
            projected
                .edit_knots(|knots| {
                    for knot in knots {
                        *knot += sum;
                    }
                })
                .map_err(|error| WriteError::Omit(error.to_string()))?;
            canonicalize_native_curve_knots(&mut projected, edge.curve_id)?;
        }
        Ok::<_, WriteError>((
            NURBS_CURVE_CLASS,
            nurbs_curve_payload_dimension(&projected, 2)?,
        ))
    };
    Ok(match &edge.curve {
        WritableEdgeCurve::Singular => {
            return Err(WriteError::Omit(format!(
                "singular edge {} requires a trim curve",
                edge.source.id
            )))
        }
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
        WritableEdgeCurve::Circle(circle) => {
            // Project an in-plane circle to a 2D arc. A circle outside its
            // face plane fails the boundary agreement check, which samples
            // the 3D carrier against the plane tolerance.
            let center = circle.center().get();
            let axis = circle.frame().axis().as_raw();
            let x = circle.frame().reference().as_raw();
            let y = axis.cross(*x);
            let cuv = plane_uv(center, origin, u_axis, v_axis);
            let at = |point: cadmpeg_ir::math::Point3| {
                let uv = plane_uv(point, origin, u_axis, v_axis);
                [uv[0] - cuv[0], uv[1] - cuv[1]]
            };
            let xuv = at(cadmpeg_ir::math::Point3::new(
                center.x + x.x,
                center.y + x.y,
                center.z + x.z,
            ));
            let yuv = at(cadmpeg_ir::math::Point3::new(
                center.x + y.x,
                center.y + y.y,
                center.z + y.z,
            ));
            let axis_2d = if xuv[0] * yuv[1] - xuv[1] * yuv[0] >= 0.0 {
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
            } else {
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0)
            };
            let domain = FiniteReal::raw_array(edge.domain);
            (
                ARC_CLASS,
                circle_payload(
                    cadmpeg_ir::math::Point3::new(cuv[0], cuv[1], 0.0),
                    axis_2d,
                    cadmpeg_ir::math::Vector3::new(xuv[0], xuv[1], 0.0),
                    circle.radius().get(),
                    domain,
                    domain,
                    2,
                )?,
            )
        }
        WritableEdgeCurve::Nurbs(nurbs) => projected_nurbs(nurbs)?,
        WritableEdgeCurve::NurbsOwned(nurbs) => projected_nurbs(nurbs)?,
    })
}

fn canonicalize_native_curve_knots(
    curve: &mut cadmpeg_ir::geometry::nurbs::NurbsCurve,
    id: &str,
) -> Result<(), WriteError> {
    let order = usize::try_from(curve.degree())
        .map_err(|_| WriteError::Omit(format!("curve {id} NURBS degree exceeds address space")))?
        + 1;
    let count = curve.control_points().len();
    let stored = curve.knots()[1..curve.knots().len() - 1].to_vec();
    let reconstructed = crate::surfaces::reconstruct_knots(&stored, order, count)
        .map_err(|error| WriteError::Omit(format!("curve {id}: {error}")))?;
    curve
        .edit_knots(|knots| knots.copy_from_slice(&reconstructed))
        .map_err(|error| WriteError::Omit(format!("curve {id}: {error}")))?;
    Ok(())
}

fn admit_pcurve<'a>(
    edge: &WritableEdge<'_>,
    pcurve: &'a cadmpeg_ir::geometry::pcurve::Pcurve,
) -> Result<WritablePcurve<'a>, WriteError> {
    if pcurve.wrapper_reversed() == Some(true) || pcurve.native_tail_flags().is_some() {
        return Err(WriteError::Omit(format!(
            "pcurve {} has unsupported wrapper or native tail state",
            pcurve.id.as_str()
        )));
    }
    let trim_domain = pcurve.parameter_range().map_or(
        edge.domain,
        cadmpeg_ir::units::FiniteVector::finite_components,
    );
    let domain = FiniteReal::raw_array(trim_domain);
    if domain[0] >= domain[1] {
        return Err(WriteError::Omit(format!(
            "pcurve {} has an empty trim interval",
            pcurve.id
        )));
    }
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
                    WriteError::Omit(format!("pcurve {}: {error}", pcurve.id.as_str()))
                })?;
            check_nurbs_curve(pcurve.id.as_str(), &curve)?;
            let count = curve.pole_count();
            if curve.periodic()
                || [
                    curve.knots()[usize::try_from(curve.degree()).map_err(|_| {
                        WriteError::Omit(format!(
                            "pcurve {} NURBS degree exceeds address space",
                            pcurve.id
                        ))
                    })?],
                    curve.knots()[count],
                ] != domain
            {
                return Err(WriteError::Omit(format!(
                    "pcurve {} is not a nonperiodic full-domain NURBS curve",
                    pcurve.id.as_str()
                )));
            }
            (NURBS_CURVE_CLASS, nurbs_curve_payload_dimension(&curve, 2)?)
        }
        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Circle(circle) => {
            // A partial circle pcurve decodes to NURBS; the 2D arc payload
            // still holds the exact arc over the trim range.
            let [lo, hi] = domain;
            if hi.partial_cmp(&lo) != Some(std::cmp::Ordering::Greater)
                || hi - lo > std::f64::consts::TAU + MAX_ARC_SPAN_EXCESS
            {
                return Err(WriteError::Omit(format!(
                    "pcurve {} is not a writable arc",
                    pcurve.id.as_str()
                )));
            }
            let center = circle.center().get();
            let x = circle.x_axis().get();
            let y = circle.y_axis().get();
            let x_norm = (x.u * x.u + x.v * x.v).sqrt();
            let y_norm = (y.u * y.u + y.v * y.v).sqrt();
            if (x_norm - 1.0).abs() > EPS_WRITE_DEGENERATE
                || (y_norm - 1.0).abs() > EPS_WRITE_DEGENERATE
                || (x.u * y.u + x.v * y.v).abs() > EPS_WRITE_DEGENERATE
            {
                return Err(WriteError::Omit(format!(
                    "pcurve {} circle axes are not orthonormal to Rhino's tighter bound {EPS_WRITE_DEGENERATE}",
                    pcurve.id.as_str()
                )));
            }
            let axis = if x.u * y.v - x.v * y.u >= 0.0 {
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
            } else {
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0)
            };
            (
                ARC_CLASS,
                circle_payload(
                    cadmpeg_ir::math::Point3::new(center.u, center.v, 0.0),
                    axis,
                    cadmpeg_ir::math::Vector3::new(x.u, x.v, 0.0),
                    circle.radius().get(),
                    [lo, hi],
                    [lo, hi],
                    2,
                )?,
            )
        }
        _ => {
            return Err(WriteError::Omit(format!(
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
) -> Result<(), WriteError> {
    // A reversed coedge's explicit trim follows either the traversal
    // convention Rhino authors or the edge convention other kernels write;
    // see `trims::validate_plane_trim`. Either agreement admits the trim.
    use cadmpeg_ir::topology::Sense;

    let reflected = sense == Sense::Reversed;
    match check_nurbs_trim(surface, face_tolerance, edge, reflected, explicit) {
        Ok(()) => Ok(()),
        Err(WriteError::Omit(first)) if reflected => {
            match check_nurbs_trim(surface, face_tolerance, edge, false, explicit) {
                Err(WriteError::Omit(_)) => Err(WriteError::Omit(first)),
                result => result,
            }
        }
        Err(error) => Err(error),
    }
}

/// The agreement half of [`validate_nurbs_trim`]: the explicit trim tracks
/// the edge curve with (`reflect`) or without the traversal reflection.
fn check_nurbs_trim(
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    face_tolerance: f64,
    edge: &WritableEdge<'_>,
    reflect: bool,
    explicit: &WritablePcurve<'_>,
) -> Result<(), WriteError> {
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let u_domain = [
        surface.u_knots()[usize::try_from(surface.u_degree()).map_err(|_| {
            WriteError::Omit(format!(
                "pcurve {} uses a surface whose NURBS degree exceeds address space",
                explicit.source.id
            ))
        })?],
        surface.u_knots()[u_count],
    ];
    let v_domain = [
        surface.v_knots()[usize::try_from(surface.v_degree()).map_err(|_| {
            WriteError::Omit(format!(
                "pcurve {} uses a surface whose NURBS degree exceeds address space",
                explicit.source.id
            ))
        })?],
        surface.v_knots()[v_count],
    ];
    let pcurve = explicit.source;
    let domain = explicit.domain(edge);
    let edge_domain = edge.domain;
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
                .filter(|value| *value > edge_domain[0] && *value < edge_domain[1])
                .map(|value| {
                    if reflect {
                        cadmpeg_ir::math::reflect_parameter(value, edge_domain[0], edge_domain[1])
                            .ok_or_else(|| {
                                WriteError::Omit("reversed edge knot is non-finite".into())
                            })
                    } else {
                        Ok(value)
                    }
                    .and_then(|value| {
                        trims::map_parameter(value, edge_domain, domain).map(FiniteReal::get)
                    })
                })
                .collect::<Result<Vec<_>, WriteError>>()?,
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
                WriteError::Omit(format!(
                    "pcurve {} cannot be evaluated over its trim domain",
                    pcurve.id.as_str()
                ))
            })?,
        };
        if !inside_domain(uv.u, uv.v) {
            return Err(WriteError::Omit(format!(
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
                WriteError::Omit(format!(
                    "pcurve {} cannot be evaluated through its NURBS surface",
                    pcurve.id.as_str()
                ))
            })?,
        };
        let mapped_parameter = trims::map_parameter(sample, domain, edge_domain)?;
        let curve_parameter = if reflect {
            cadmpeg_ir::math::reflect_parameter(mapped_parameter, edge_domain[0], edge_domain[1])
                .ok_or_else(|| WriteError::Omit("reversed edge parameter is non-finite".into()))?
        } else {
            mapped_parameter
        };
        // A non-finite edge curve point is measured as a finite one is.
        let edge_point = match edge.curve.point(curve_parameter) {
            Ok(point) => point.get(),
            Err(failure) => failure.non_finite()?.ok_or_else(|| {
                WriteError::Omit(format!(
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
                return Err(WriteError::Omit(reason));
            }
            // (C) Endpoint images disagree, or a held carrier is non-finite.
            return Err(WriteError::Omit(reason));
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
) -> Result<Vec<u8>, WriteError> {
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
fn computed(value: f64, field: &str) -> Result<FiniteReal, WriteError> {
    FiniteReal::new(value).ok_or_else(|| {
        WriteError::Omit(format!(
            "Rhino writer computed the non-finite {field} {value}, which 3DM cannot state"
        ))
    })
}

/// Whether two parameter intervals state the same range: serialization dust
/// below one part in a trillion of the endpoint magnitude is not an
/// independent parameterization. A coedge use range and an edge domain that
/// agree this closely select the same trim, and the writer always encodes
/// the edge domain.
pub(super) fn parameter_intervals_agree(first: [f64; 2], second: [f64; 2]) -> bool {
    first.into_iter().zip(second).all(|(left, right)| {
        let scale = 1.0_f64.max(left.abs()).max(right.abs());
        (left - right).abs() <= PARAMETER_DUST_RELATIVE * scale
    })
}

/// How far an arc carrier may span past one turn: the decoder admits an
/// angle interval only within its own degenerate bound, so a wider carrier
/// has no exact arc payload.
pub(super) const MAX_ARC_SPAN_EXCESS: f64 = 1.0e-10;

/// Relative dust below which two interval endpoints state the same bound.
/// See [`parameter_intervals_agree`].
pub(super) const PARAMETER_DUST_RELATIVE: f64 = 1.0e-12;

/// Absolute slack for containing a used V range in the torus profile
/// circle's `[0, 2π]` domain: serialization dust, not a wider domain.
pub(super) const TORUS_PROFILE_DOMAIN_SLACK: f64 = 1.0e-10;

/// A count the native field cannot store.
fn native_count_limit(limit: u64, count: u64) -> CodecError {
    CodecError::InvalidInput(format!(
        "Rhino native count {count} exceeds the format limit {limit}"
    ))
}

fn polymorphic_array<'a>(
    children: impl ExactSizeIterator<Item = &'a ([u8; 16], Vec<u8>)>,
) -> Result<Vec<u8>, CodecError> {
    polymorphic_records(children.map(|(class, body)| (*class, body.as_slice(), None)))
}

fn polymorphic_records<'a>(
    children: impl ExactSizeIterator<Item = ([u8; 16], &'a [u8], Option<&'a [u8]>)>,
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
    for (class, payload, checksum) in children {
        body.extend(1_i32.to_le_bytes());
        direct.extend(1_i32.to_le_bytes());
        body.extend(class_wrapper_with_direct(
            class,
            payload,
            checksum.unwrap_or(payload),
        )?);
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
        if let Some(colors) = encoded_face_colors(faces)? {
            body.push(1);
            for color in colors {
                body.extend(color);
            }
        } else {
            body.push(0);
        }
    }
    crc_chunk(0x4000_8000, &body)
}

fn encoded_face_colors(
    faces: &[&cadmpeg_ir::topology::Face],
) -> Result<Option<Vec<[u8; 4]>>, CodecError> {
    if faces.is_empty() {
        return Ok(None);
    }
    let mut colors = Vec::with_capacity(faces.len());
    let mut any_color = false;
    for face in faces {
        let color = match face.color {
            Some(color) => [
                unit_color_channel(color.r())?,
                unit_color_channel(color.g())?,
                unit_color_channel(color.b())?,
                unit_color_channel(1.0 - color.a())?,
            ],
            None => [u8::MAX; 4],
        };
        any_color |= color != [u8::MAX; 4];
        colors.push(color);
    }
    Ok(any_color.then_some(colors))
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

fn archive_body_len(value: u64) -> Result<i64, cadmpeg_ir::codec::write::EncodeError> {
    let field = &target::CAPACITY_FIELDS[0];
    if value > field.limit {
        return Err(cadmpeg_ir::codec::write::EncodeError::TargetCapacity {
            field,
            limit: field.limit,
            value,
        });
    }
    Ok(value.cast_signed())
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

fn check_mesh(mesh: &cadmpeg_ir::tessellation::Tessellation) -> Result<(), WriteError> {
    let vertex_count = mesh.vertices().len();
    if mesh.triangles().is_empty() {
        return Err(WriteError::Omit(format!(
            "mesh {} has no faces for a native Rhino mesh",
            mesh.id
        )));
    }
    if vertex_count == 0
        || i32::try_from(vertex_count).is_err()
        || i32::try_from(mesh.triangles().len()).is_err()
    {
        return Err(WriteError::Omit(format!(
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
        return Err(WriteError::Omit(format!(
            "mesh {} values exceed Rhino's native finite range",
            mesh.id
        )));
    }
    Ok(())
}

fn check_frame(
    id: &str,
    frame: cadmpeg_ir::units::OrthonormalFrame3,
    family: &str,
) -> Result<(), WriteError> {
    let normal = frame.axis().as_raw();
    let x = frame.reference().as_raw();
    let dot = normal.x * x.x + normal.y * x.y + normal.z * x.z;
    if (normal.norm() - 1.0).abs() > EPS_WRITE_DEGENERATE
        || (x.norm() - 1.0).abs() > EPS_WRITE_DEGENERATE
        || dot.abs() > EPS_WRITE_DEGENERATE
    {
        return Err(WriteError::Omit(format!(
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
) -> Result<i32, WriteError> {
    let u_order = usize::try_from(surface.u_degree()).map_err(|_| {
        WriteError::Omit(format!("surface {id} NURBS degree exceeds address space"))
    })? + 1;
    let v_order = usize::try_from(surface.v_degree()).map_err(|_| {
        WriteError::Omit(format!("surface {id} NURBS degree exceeds address space"))
    })? + 1;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let count_error = || format!("surface {id} cannot be represented by Rhino NURBS counts");
    let pole_count = native_i32_count(surface.poles().len(), count_error())?;
    if u_order < 2 || v_order < 2 {
        return Err(WriteError::Omit(count_error()));
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
) -> Result<(), WriteError> {
    let order = usize::try_from(curve.degree())
        .map_err(|_| WriteError::Omit(format!("curve {id} NURBS degree exceeds address space")))?
        + 1;
    let count = curve.control_points().len();
    let count_error = || format!("curve {id} cannot be represented by Rhino NURBS counts");
    if order < 2 {
        return Err(WriteError::Omit(count_error()));
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
) -> Result<(), WriteError> {
    let stored = &full[1..full.len() - 1];
    if stored[order - 2] >= stored[count - 1] {
        return Err(WriteError::Omit(format!(
            "{direction} {id} has a non-increasing native NURBS domain"
        )));
    }
    let reconstructed =
        crate::surfaces::reconstruct_knots(stored, order, count).map_err(|error| match error {
            crate::curves::GeometryError::Codec(CodecError::ResourceLimit(limit)) => {
                WriteError::Encode(limit.into())
            }
            error => WriteError::Omit(format!("{direction} {id}: {error}")),
        })?;
    let periodic = crate::surfaces::periodic_knots(stored, order, count);
    if reconstructed != full || periodic != declared_periodic {
        return Err(WriteError::Omit(format!(
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

fn point_cloud_payload(
    points: &[cadmpeg_ir::math::Point3],
    colors: Option<&[cadmpeg_ir::topology::Color]>,
) -> Result<Vec<u8>, CodecError> {
    if points.is_empty() {
        return Err(CodecError::InvalidInput(
            "Rhino point cloud requires at least one point".into(),
        ));
    }
    if colors.is_some_and(|colors| colors.len() != points.len()) {
        return Err(CodecError::InvalidInput(
            "point-cloud colors must match the point count".into(),
        ));
    }
    let mut payload = vec![if colors.is_some() { 0x11 } else { 0x10 }];
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
    if let Some(colors) = colors {
        payload.extend(0_i32.to_le_bytes());
        payload.extend(wire_index(colors.len())?.to_le_bytes());
        for color in colors {
            payload.extend([
                unit_color_channel(color.r())?,
                unit_color_channel(color.g())?,
                unit_color_channel(color.b())?,
                unit_color_channel(1.0 - color.a())?,
            ]);
        }
    }
    Ok(payload)
}

fn circle_payload(
    center: cadmpeg_ir::math::Point3,
    axis: cadmpeg_ir::math::Vector3,
    x: cadmpeg_ir::math::Vector3,
    radius: f64,
    angle: [f64; 2],
    domain: [f64; 2],
    dimension: i32,
) -> Result<Vec<u8>, WriteError> {
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
        angle[0],
        angle[1],
        domain[0],
        domain[1],
    ] {
        payload.extend(value.to_le_bytes());
    }
    payload.extend(dimension.to_le_bytes());
    Ok(payload)
}

fn nurbs_curve_payload(
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<Vec<u8>, WriteError> {
    nurbs_curve_payload_dimension(curve, 3)
}

fn nurbs_curve_payload_dimension(
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    dimension: i32,
) -> Result<Vec<u8>, WriteError> {
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
    domain: [[f64; 2]; 2],
) -> Result<Vec<u8>, WriteError> {
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
    .chain(domain[0])
    .chain(domain[1])
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
) -> Result<Vec<u8>, WriteError> {
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

const REV_SURFACE_CLASS: [u8; 16] = [
    0xd3, 0x20, 0x62, 0xa1, 0x3b, 0x16, 0xd4, 0x11, 0x80, 0x00, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];

/// A native revolution surface holding one analytic carrier over the trim
/// range that uses it: the axis endpoints, the used angle and parameter
/// intervals, a bounding box, and the profile curve the decoder converts to
/// the solved NURBS carrier (spec §13.5).
struct RevolutionSurface {
    axis: [cadmpeg_ir::math::Point3; 2],
    angle: [f64; 2],
    parameter: [f64; 2],
    bounds: [f64; 6],
    profile: ClassPayload,
    transposed: bool,
    /// The written carrier's parameter domain: the angle interval by the
    /// profile domain, which the trim agreement checks run against.
    domain: [[f64; 2]; 2],
}

fn revolution_surface_payload(
    revolution: &RevolutionSurface,
) -> Result<SurfacePayload, WriteError> {
    let mut payload = vec![0x20];
    for point in revolution.axis {
        for value in [point.x, point.y, point.z] {
            payload.extend(
                computed(value, "revolution axis endpoint")?
                    .get()
                    .to_le_bytes(),
            );
        }
    }
    for value in revolution.angle.into_iter().chain(revolution.parameter) {
        payload.extend(
            computed(value, "revolution interval bound")?
                .get()
                .to_le_bytes(),
        );
    }
    for value in revolution.bounds {
        payload.extend(
            computed(value, "revolution bounding box")?
                .get()
                .to_le_bytes(),
        );
    }
    payload.extend(i32::from(revolution.transposed).to_le_bytes());
    payload.push(1);
    let direct = payload.clone();
    let (class, profile) = &revolution.profile;
    payload.extend(class_wrapper(*class, profile)?);
    Ok(SurfacePayload {
        class: REV_SURFACE_CLASS,
        body: payload,
        direct: Some(direct),
        record_count: 2,
    })
}

/// The used parameter range of an analytic surface: the trim ultraviolet
/// box over every face that names it. The revolution profile spans the V
/// range and the angle interval spans the U range, so the solved carrier
/// the decoder rebuilds covers every trim.
fn analytic_usage(surface_id: &str, ir: &CadIr) -> Result<[[f64; 2]; 2], WriteError> {
    let mut used: Option<[[f64; 2]; 2]> = None;
    let mut include = |u: f64, v: f64| {
        used = Some(match used {
            None => [[u, u], [v, v]],
            Some([[u0, u1], [v0, v1]]) => [[u0.min(u), u1.max(u)], [v0.min(v), v1.max(v)]],
        });
    };
    let mut faces = 0;
    for face in ir
        .model
        .faces
        .iter()
        .filter(|face| face.surface.as_str() == surface_id)
    {
        faces += 1;
        for loop_id in &face.loops {
            let Some(loop_) = ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id.as_str() == loop_id.as_str())
            else {
                continue;
            };
            for coedge_id in loop_.coedges() {
                let Some(coedge) = ir
                    .model
                    .coedges
                    .iter()
                    .find(|coedge| coedge.id.as_str() == coedge_id.as_str())
                else {
                    continue;
                };
                let Some(edge) = ir.model.edges.iter().find(|edge| edge.id == coedge.edge) else {
                    continue;
                };
                let domain = edge
                    .param_range()
                    .ok_or_else(|| {
                        WriteError::Omit(format!(
                            "surface {surface_id} trim edge {} has no finite domain to bound",
                            edge.id.as_str()
                        ))
                    })?
                    .finite_components();
                for use_ in &coedge.pcurves {
                    let Some(pcurve) = ir
                        .model
                        .pcurves
                        .iter()
                        .find(|pcurve| pcurve.id == use_.pcurve)
                    else {
                        continue;
                    };
                    match &pcurve.geometry {
                        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(line) => {
                            let origin = line.origin().get();
                            let direction = line.direction().as_raw();
                            for t in [domain[0], domain[1]] {
                                include(
                                    origin.u + t.get() * direction.u,
                                    origin.v + t.get() * direction.v,
                                );
                            }
                        }
                        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Circle(circle) => {
                            let center = circle.center().get();
                            let radius = circle.radius().get();
                            for (u, v) in [
                                (center.u - radius, center.v - radius),
                                (center.u + radius, center.v + radius),
                            ] {
                                include(u, v);
                            }
                        }
                        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } => {
                            for point in nurbs.control_points() {
                                let point = point.get();
                                include(point.u, point.v);
                            }
                        }
                        _ => {
                            return Err(WriteError::Omit(format!(
                                "surface {surface_id} trim carrier {} cannot bound analytic usage",
                                pcurve.id.as_str()
                            )));
                        }
                    }
                }
            }
        }
    }
    if faces == 0 {
        return Err(WriteError::Omit(format!(
            "surface {surface_id} analytic carrier has no trim usage to bound its patch",
        )));
    }
    used.ok_or_else(|| {
        WriteError::Omit(format!(
            "surface {surface_id} analytic carrier has no bounded trim usage",
        ))
    })
}

/// Evaluate an analytic patch on a grid of interval ends and interior
/// quarter-turn criticals. Every coordinate is a tensor product of
/// trigonometric and linear factors, so its extrema over the rectangle sit
/// at a critical or an end in each parameter.
fn patch_bounds(
    point_at: impl Fn(f64, f64) -> [f64; 3],
    urange: [f64; 2],
    vrange: [f64; 2],
) -> [f64; 6] {
    fn samples([lo, hi]: [f64; 2]) -> Vec<f64> {
        let mut values = vec![lo, hi];
        for k in -4..=4 {
            let critical = f64::from(k) * std::f64::consts::FRAC_PI_2;
            if critical > lo && critical < hi {
                values.push(critical);
            }
        }
        values
    }
    let mut bounds = [f64::INFINITY, f64::INFINITY, f64::INFINITY];
    let mut upper = [f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for u in samples(urange) {
        for v in samples(vrange) {
            let point = point_at(u, v);
            for (index, value) in point.into_iter().enumerate() {
                bounds[index] = bounds[index].min(value);
                upper[index] = upper[index].max(value);
            }
        }
    }
    [
        bounds[0], bounds[1], bounds[2], upper[0], upper[1], upper[2],
    ]
}

fn line_profile(
    from: cadmpeg_ir::math::Point3,
    to: cadmpeg_ir::math::Point3,
    domain: [f64; 2],
) -> Result<ClassPayload, WriteError> {
    use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3};
    let poles = NurbsPoles3::from_lanes(
        vec![
            cadmpeg_ir::features::FinitePoint3::new(from).ok_or_else(|| {
                WriteError::Omit("revolution profile endpoint is not finite".into())
            })?,
            cadmpeg_ir::features::FinitePoint3::new(to).ok_or_else(|| {
                WriteError::Omit("revolution profile endpoint is not finite".into())
            })?,
        ],
        None,
    )
    .map_err(|error| WriteError::Omit(format!("revolution profile poles: {error}")))?;
    let curve = NurbsCurve::new(
        1,
        vec![domain[0], domain[0], domain[1], domain[1]],
        poles,
        false,
    )
    .map_err(|error| WriteError::Omit(format!("revolution profile curve: {error}")))?;
    Ok((NURBS_CURVE_CLASS, nurbs_curve_payload(&curve)?))
}

/// Rewrite one analytic surface as a native revolution surface over its
/// used parameter range, with a loss naming the converted record: 3DM holds
/// one surface carrier per face (spec §15.4), not the analytic wrapper.
fn analytic_revolution(
    surface: &cadmpeg_ir::geometry::Surface,
    analytic: &cadmpeg_ir::geometry::SolvedSurfaceGeometry,
    ir: &CadIr,
) -> Result<(RevolutionSurface, LossNote), WriteError> {
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
    let id = surface.id.as_str();
    let usage = analytic_usage(id, ir)?;
    let [[u0, u1], [v0, v1]] = usage;
    if u1.partial_cmp(&u0) != Some(std::cmp::Ordering::Greater)
        || v1.partial_cmp(&v0) != Some(std::cmp::Ordering::Greater)
    {
        return Err(WriteError::Omit(format!(
            "surface {id} analytic usage is degenerate",
        )));
    }
    if u1 - u0 > std::f64::consts::TAU + MAX_ARC_SPAN_EXCESS {
        return Err(WriteError::Omit(format!(
            "surface {id} analytic usage spans more than one turn",
        )));
    }
    let kind = match analytic {
        SolvedSurfaceGeometry::Cylinder(_) => "cylinder",
        SolvedSurfaceGeometry::Cone(_) => "cone",
        SolvedSurfaceGeometry::Sphere(_) => "sphere",
        SolvedSurfaceGeometry::Torus(_) => "torus",
        _ => {
            return Err(WriteError::Omit(format!(
                "surface {id} is not a revolution-analytic patch",
            )));
        }
    };
    let origin;
    let axis;
    let reference;
    let radius;
    let ratio;
    let slope;
    let center;
    match analytic {
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            origin = cylinder.origin().get();
            axis = *cylinder.frame().axis().as_raw();
            reference = *cylinder.frame().reference().as_raw();
            radius = cylinder.radius().get();
            ratio = 1.0;
            slope = 0.0;
            center = origin;
        }
        SolvedSurfaceGeometry::Cone(cone) => {
            origin = cone.origin().get();
            axis = *cone.frame().axis().as_raw();
            reference = *cone.frame().reference().as_raw();
            radius = cone.radius().get();
            ratio = cone.ratio().get();
            slope = cone.half_angle().get().tan();
            center = origin;
        }
        SolvedSurfaceGeometry::Sphere(sphere) => {
            center = sphere.center().get();
            axis = *sphere.frame().axis().as_raw();
            reference = *sphere.frame().reference().as_raw();
            radius = sphere.radius().get();
            ratio = 1.0;
            slope = 0.0;
            origin = center;
        }
        SolvedSurfaceGeometry::Torus(torus) => {
            center = torus.center().get();
            axis = *torus.frame().axis().as_raw();
            reference = *torus.frame().reference().as_raw();
            radius = torus.minor_radius().get();
            ratio = 1.0;
            slope = 0.0;
            origin = center;
        }
        _ => {
            return Err(WriteError::Omit(format!(
                "surface {id} is not a revolution-analytic patch",
            )));
        }
    }
    if !(radius.is_finite() && radius > 0.0) {
        return Err(WriteError::Omit(format!(
            "surface {id} analytic radius is not positive",
        )));
    }
    if !(ratio.is_finite() && ratio > 0.0 && slope.is_finite()) {
        return Err(WriteError::Omit(format!(
            "surface {id} analytic parameters are not finite",
        )));
    }
    let transverse = axis.cross(reference);
    let major = match analytic {
        SolvedSurfaceGeometry::Torus(torus) => torus.major_radius().get(),
        _ => 0.0,
    };
    if matches!(analytic, SolvedSurfaceGeometry::Torus(_)) && !(major.is_finite() && major > 0.0) {
        return Err(WriteError::Omit(format!(
            "surface {id} torus major radius is not positive",
        )));
    }
    let local_radius = |v: f64| radius + v * slope;
    if local_radius(v0) < 0.0 || local_radius(v1) < 0.0 {
        return Err(WriteError::Omit(format!(
            "surface {id} analytic radius is negative over its usage",
        )));
    }
    let profile_point = |v: f64| {
        let radial = local_radius(v);
        [
            origin.x + v * axis.x + radial * reference.x,
            origin.y + v * axis.y + radial * reference.y,
            origin.z + v * axis.z + radial * reference.z,
        ]
    };
    let (axis_from, axis_to, profile, point_at): (
        cadmpeg_ir::math::Point3,
        cadmpeg_ir::math::Point3,
        ClassPayload,
        SurfacePointAt,
    ) = match analytic {
        SolvedSurfaceGeometry::Cylinder(_) | SolvedSurfaceGeometry::Cone(_) => {
            let from = profile_point(v0);
            let to = profile_point(v1);
            if from == to {
                return Err(WriteError::Omit(format!(
                    "surface {id} analytic profile is degenerate",
                )));
            }
            let profile = line_profile(
                cadmpeg_ir::math::Point3::new(from[0], from[1], from[2]),
                cadmpeg_ir::math::Point3::new(to[0], to[1], to[2]),
                [v0, v1],
            )?;
            let axis_from = cadmpeg_ir::math::Point3::new(
                origin.x + v0 * axis.x,
                origin.y + v0 * axis.y,
                origin.z + v0 * axis.z,
            );
            let axis_to = cadmpeg_ir::math::Point3::new(
                origin.x + v1 * axis.x,
                origin.y + v1 * axis.y,
                origin.z + v1 * axis.z,
            );
            let origin_c = origin;
            let axis_c = axis;
            let reference_c = reference;
            let ratio_c = ratio;
            let slope_c = slope;
            let radius_c = radius;
            (
                axis_from,
                axis_to,
                profile,
                Box::new(move |u, v| {
                    let radial = radius_c + v * slope_c;
                    let (sin_u, cos_u) = u.sin_cos();
                    [
                        origin_c.x
                            + v * axis_c.x
                            + radial * (cos_u * reference_c.x + ratio_c * sin_u * transverse.x),
                        origin_c.y
                            + v * axis_c.y
                            + radial * (cos_u * reference_c.y + ratio_c * sin_u * transverse.y),
                        origin_c.z
                            + v * axis_c.z
                            + radial * (cos_u * reference_c.z + ratio_c * sin_u * transverse.z),
                    ]
                }),
            )
        }
        SolvedSurfaceGeometry::Sphere(_) => {
            let axis_from = center;
            let axis_to = cadmpeg_ir::math::Point3::new(
                center.x + axis.x,
                center.y + axis.y,
                center.z + axis.z,
            );
            let profile = circle_payload(
                center,
                reference.cross(axis),
                reference,
                radius,
                [v0, v1],
                [v0, v1],
                3,
            )?;
            let center_c = center;
            let axis_c = axis;
            let reference_c = reference;
            let transverse_c = transverse;
            let radius_c = radius;
            (
                axis_from,
                axis_to,
                (ARC_CLASS, profile),
                Box::new(move |u, v| {
                    let (sin_u, cos_u) = u.sin_cos();
                    let (sin_v, cos_v) = v.sin_cos();
                    [
                        center_c.x
                            + radius_c * cos_v * (cos_u * reference_c.x + sin_u * transverse_c.x)
                            + radius_c * sin_v * axis_c.x,
                        center_c.y
                            + radius_c * cos_v * (cos_u * reference_c.y + sin_u * transverse_c.y)
                            + radius_c * sin_v * axis_c.y,
                        center_c.z
                            + radius_c * cos_v * (cos_u * reference_c.z + sin_u * transverse_c.z)
                            + radius_c * sin_v * axis_c.z,
                    ]
                }),
            )
        }
        SolvedSurfaceGeometry::Torus(_) => {
            if v0 < -TORUS_PROFILE_DOMAIN_SLACK
                || v1 > std::f64::consts::TAU + TORUS_PROFILE_DOMAIN_SLACK
            {
                return Err(WriteError::Omit(format!(
                    "surface {id} torus usage leaves the profile circle domain",
                )));
            }
            let axis_from = center;
            let axis_to = cadmpeg_ir::math::Point3::new(
                center.x + axis.x,
                center.y + axis.y,
                center.z + axis.z,
            );
            let profile_center = cadmpeg_ir::math::Point3::new(
                center.x + major * reference.x,
                center.y + major * reference.y,
                center.z + major * reference.z,
            );
            let profile = circle_payload(
                profile_center,
                reference.cross(axis),
                reference,
                radius,
                [0.0, std::f64::consts::TAU],
                [0.0, std::f64::consts::TAU],
                3,
            )?;
            let center_c = center;
            let axis_c = axis;
            let reference_c = reference;
            let transverse_c = transverse;
            let radius_c = radius;
            let major_c = major;
            (
                axis_from,
                axis_to,
                (ARC_CLASS, profile),
                Box::new(move |u, v| {
                    let (sin_u, cos_u) = u.sin_cos();
                    let (sin_v, cos_v) = v.sin_cos();
                    let ring = major_c + radius_c * cos_v;
                    [
                        center_c.x
                            + ring * (cos_u * reference_c.x + sin_u * transverse_c.x)
                            + radius_c * sin_v * axis_c.x,
                        center_c.y
                            + ring * (cos_u * reference_c.y + sin_u * transverse_c.y)
                            + radius_c * sin_v * axis_c.y,
                        center_c.z
                            + ring * (cos_u * reference_c.z + sin_u * transverse_c.z)
                            + radius_c * sin_v * axis_c.z,
                    ]
                }),
            )
        }
        _ => {
            return Err(WriteError::Omit(format!(
                "surface {id} is not a revolution-analytic patch",
            )));
        }
    };
    let bounds = patch_bounds(point_at, [u0, u1], [v0, v1]);
    let loss = crate::loss::RhinoLossCode::WriterCarrierResolved.note(format!(
        "surface {id} analytic {kind} carrier was rewritten as a native revolution surface over the used parameter range",
    ));
    let v_domain = match analytic {
        SolvedSurfaceGeometry::Torus(_) => [0.0, std::f64::consts::TAU],
        _ => [v0, v1],
    };
    Ok((
        RevolutionSurface {
            axis: [axis_from, axis_to],
            angle: [u0, u1],
            parameter: [u0, u1],
            bounds,
            profile,
            transposed: false,
            domain: [[u0, u1], v_domain],
        },
        loss,
    ))
}

struct MeshPayload {
    body: Vec<u8>,
    direct: Vec<u8>,
}

fn mesh_payload(
    mesh: &cadmpeg_ir::tessellation::Tessellation,
    archive_version: RhinoArchiveVersion,
) -> Result<MeshPayload, WriteError> {
    let writes_double_vertices = archive_version.stores_mesh_vertices_as_f64();
    let vertex_width = if writes_double_vertices { 24 } else { 12 };
    let len = mesh
        .vertex_count()
        .checked_mul(vertex_width)
        .ok_or_else(|| WriteError::Omit("mesh buffer length exceeds address space".into()))?;
    mesh_buffer_len(len)?;
    for kind in [
        CHANNEL_UV,
        CHANNEL_CURVATURE,
        CHANNEL_COLOR,
        CHANNEL_SURFACE_PARAMETERS,
    ] {
        mesh_buffer_len(mesh_channel(mesh, kind).len())?;
    }
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
                    WriteError::Omit("mesh face index exceeds chosen width".into())
                })?),
                FaceIndexWidth::Two => payload.extend(
                    (u16::try_from(index).map_err(|_| {
                        WriteError::Omit("mesh face index exceeds chosen width".into())
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
                .ok_or_else(|| WriteError::Omit("mesh vertex exceeds f32 range".into()))?;
            float_vertices.extend(value.to_le_bytes());
        }
    }
    let mut normals = Vec::new();
    for normal in mesh.vertex_normals() {
        for value in [normal.x, normal.y, normal.z] {
            let value = cadmpeg_core::convert::f32_from_f64(value)
                .ok_or_else(|| WriteError::Omit("mesh normal exceeds f32 range".into()))?;
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
        .find(|channel| {
            channel.kind() == kind && mesh_channel_supported(channel, mesh.vertex_count())
        })
        .map_or(&[], |channel| channel.data())
}

fn mesh_channel_supported(
    channel: &cadmpeg_ir::tessellation::TessellationChannel,
    vertex_count: usize,
) -> bool {
    let expected = match channel.kind() {
        CHANNEL_UV => 8,
        CHANNEL_COLOR => 4,
        CHANNEL_SURFACE_PARAMETERS | CHANNEL_CURVATURE => 16,
        _ => return false,
    };
    matches!(
        channel.addressing(),
        cadmpeg_ir::tessellation::ChannelAddressing::Vertex {}
    ) && channel.flags() == 0
        && channel.item_size() == expected
        && usize::try_from(channel.count()).ok() == Some(vertex_count)
}

fn mesh_buffer_len(len: usize) -> Result<u32, WriteError> {
    u32::try_from(len).map_err(|_| WriteError::Omit("mesh buffer exceeds its u32 length".into()))
}

fn mesh_buffer(data: &[u8]) -> Result<Vec<u8>, WriteError> {
    let mut result = mesh_buffer_len(data.len())?.to_le_bytes().to_vec();
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
    materials: &MaterialPlan,
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
            materials,
        )?),
    )
}

fn surface_object_record(
    ir: &CadIr,
    payload: &SurfacePayload,
    identity: &str,
    display: presentation::Display<'_>,
    materials: &MaterialPlan,
) -> Result<Vec<u8>, CodecError> {
    framed_object_record(
        if payload.class == crate::extrusion::ON_EXTRUSION.to_wire() {
            0x10
        } else {
            8
        },
        payload.class,
        &payload.body,
        payload.direct.as_deref(),
        Some(object_attributes_payload(
            ir,
            identity,
            display.name,
            display.color,
            display.visible,
            materials,
        )?),
    )
}

fn curve_object_record(
    ir: &CadIr,
    payload: &compounds::CurvePayload,
    identity: &str,
    display: presentation::Display<'_>,
    materials: &MaterialPlan,
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
            materials,
        )?),
    )
}

fn mesh_object_record(
    ir: &CadIr,
    payload: &MeshPayload,
    identity: &str,
    display: presentation::Display<'_>,
    materials: &MaterialPlan,
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
            materials,
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
    materials: &MaterialPlan,
) -> Result<Vec<u8>, CodecError> {
    framed_object_record(
        0x10,
        BREP_CLASS,
        &payload.body,
        Some(&payload.direct),
        Some(object_attributes_payload(
            ir, identity, name, color, visible, materials,
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
    materials: &MaterialPlan,
) -> Result<Vec<u8>, CodecError> {
    let uuid = tables::products::occurrence_uuid(ir, identity)
        .unwrap_or_else(|| neutral_object_uuid(identity));
    let material = materials.material_index(ir, identity);
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
    let color = color.filter(|value| !tables::inherits_layer_color(ir, identity, *value));
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
        .or_else(|| {
            ir.model
                .view_presentations
                .iter()
                .find(|view| view.object.as_deref() == Some(identity))
                .map(|view| view.visible)
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
    } else if material.is_some()
        || (ir.model.presentation_layers.is_empty()
            && ir
                .model
                .bodies
                .iter()
                .any(|body| body.id.as_str() == identity))
    {
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
