// SPDX-License-Identifier: Apache-2.0
//! Native extrusions authored from held procedural carriers and cap frames.

use super::WriteError;
use cadmpeg_ir::geometry::{
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::CadIr;

pub(super) struct Candidate {
    pub(super) payload: Vec<u8>,
    pub(super) body_id: String,
    pub(super) profile_count: usize,
    pub(super) userdata: Vec<Vec<u8>>,
}

pub(super) fn payload(
    ir: &CadIr,
    version: crate::RhinoArchiveVersion,
    only: Option<&cadmpeg_ir::ids::ProceduralSurfaceId>,
) -> Result<Option<Candidate>, WriteError> {
    let [body] = ir.model.bodies.as_slice() else {
        return Ok(None);
    };
    if body
        .transform
        .is_some_and(|value| value != cadmpeg_ir::transform::Transform::default())
    {
        return Ok(None);
    }
    let mut profiles = Vec::new();
    let mut path = None;
    for procedure in &ir.model.procedural_surfaces {
        if only.is_some_and(|id| id != &procedure.id) {
            continue;
        }
        let ProceduralSurfaceDefinition::Extrusion(extrusion) = procedure.definition() else {
            return Ok(None);
        };
        if extrusion.parameter_interval().is_some()
            || extrusion.native_position().is_some()
            || extrusion.revision_form().is_some()
            || extrusion.legacy_cache().is_some()
        {
            return Ok(None);
        }
        let Some([Some(_u0), Some(v0), Some(_u1), Some(v1)]) = procedure
            .record_bounds()
            .map(cadmpeg_ir::geometry::RecordBounds::get)
        else {
            return Ok(None);
        };
        if v0 >= v1 {
            return Ok(None);
        }
        let direction = *extrusion.direction().as_raw();
        if let Some(previous) = path {
            if previous != (direction, v0, v1) {
                return Ok(None);
            }
        } else {
            path = Some((direction, v0, v1));
        }
        let Some(curve) = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *extrusion.directrix())
        else {
            return Ok(None);
        };
        let Some(SolvedCurveGeometry::Nurbs(source)) = curve.geometry.solved() else {
            return Ok(None);
        };
        profiles.push((curve, source));
    }
    let Some((direction, v0, v1)) = path else {
        return Ok(None);
    };
    let Some(tangent) = direction.unit() else {
        return Ok(None);
    };
    let mut caps = [false; 2];
    let mut selected = None;
    for face in &ir.model.faces {
        let Some(surface) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
        else {
            return Ok(None);
        };
        let Some(SolvedSurfaceGeometry::Plane(plane)) = surface.geometry.solved() else {
            return Ok(None);
        };
        let cap = match face.sense {
            Sense::Reversed => 0,
            Sense::Forward => 1,
        };
        if caps[cap] || (only.is_none() && face.loops.len() != profiles.len()) {
            return Ok(None);
        }
        caps[cap] = true;
        let axis = *plane.frame().axis().as_raw();
        let xaxis = *plane.frame().reference().as_raw();
        if axis != tangent {
            return Ok(None);
        }
        let origin = plane.origin().get();
        let start = if cap == 0 {
            origin
        } else {
            origin.translated(direction, v0 - v1)
        };
        if let Some((earlier, x)) = selected {
            if earlier != start || x != xaxis {
                return Ok(None);
            }
        } else {
            selected = Some((start, xaxis));
        }
    }
    let Some((start, xaxis)) = selected else {
        return Ok(None);
    };
    if only.is_some() {
        caps = [false, false];
    }
    let up = tangent.cross(xaxis);
    let mut children = Vec::with_capacity(profiles.len());
    for (curve, source) in &profiles {
        let mut profile = (**source).clone();
        profile.try_map_control_points(|_, pole| {
            let point = pole.get().translated(direction, v0);
            let delta = point.vector_from(start);
            if delta.dot(tangent) != 0.0 {
                return Err(WriteError::Omit("extrusion profile is not planar".into()));
            }
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                delta.dot(xaxis),
                delta.dot(up),
                0.0,
            ))
            .ok_or_else(|| WriteError::Omit("extrusion profile has a non-finite local pole".into()))
        })?;
        super::check_nurbs_curve(curve.id.as_str(), &profile)?;
        let body = super::nurbs_curve_payload_dimension(&profile, 2)?;
        children.push(super::class_wrapper(super::NURBS_CURVE_CLASS, &body)?);
    }
    let profile_count = profiles.len();
    let count =
        super::writable_i32_count(profile_count, "extrusion profile count exceeds i32".into())?;
    let profile_wrapper = if profile_count == 1 {
        children.remove(0)
    } else {
        let mut polycurve = vec![0x10];
        polycurve.extend(count.to_le_bytes());
        polycurve.extend(0_i32.to_le_bytes());
        polycurve.extend(0_i32.to_le_bytes());
        polycurve.extend([0_u8; 48]);
        let parameter_count = count.checked_add(1).ok_or_else(|| {
            WriteError::Omit("extrusion profile parameter count exceeds i32".into())
        })?;
        polycurve.extend(parameter_count.to_le_bytes());
        for index in 0..parameter_count {
            polycurve.extend(f64::from(index).to_le_bytes());
        }
        let direct = polycurve.clone();
        for child in children {
            polycurve.extend(child);
        }
        super::class_wrapper_with_direct(crate::curves::POLYCURVE.to_wire(), &polycurve, &direct)?
    };
    let mut direct = Vec::new();
    direct.extend(1_i32.to_le_bytes());
    let minor = if version == crate::RhinoArchiveVersion::V5 {
        2_i32
    } else {
        3_i32
    };
    direct.extend(minor.to_le_bytes());
    let end = start.translated(direction, v1 - v0);
    for value in [
        start.x, start.y, start.z, end.x, end.y, end.z, 0.0, 1.0, up.x, up.y, up.z,
    ] {
        direct.extend(value.to_le_bytes());
    }
    direct.extend([0_u8, 0_u8]);
    for value in [0.0_f64; 6] {
        direct.extend(value.to_le_bytes());
    }
    for value in [v0, v1] {
        direct.extend(value.to_le_bytes());
    }
    direct.push(0);
    direct.extend(count.to_le_bytes());
    direct.extend([u8::from(caps[0]), u8::from(caps[1])]);
    let mut payload_body = direct[..8].to_vec();
    payload_body.extend(profile_wrapper);
    payload_body.extend(&direct[8..]);
    let mut userdata = Vec::new();
    if only.is_none() {
        let mut meshes = Vec::new();
        for mesh in &ir.model.tessellations {
            let belongs_to_body = mesh.body.as_ref() == Some(&body.id);
            let shares_source = mesh.source_object.as_ref().is_some_and(|source| {
                ir.model.surfaces.iter().any(|surface| {
                    surface.source_object.as_ref() == Some(source)
                        && surface.geometry.procedural_construction().is_some()
                })
            });
            if !belongs_to_body && !shares_source {
                continue;
            }
            let mesh = super::mesh_payload(mesh, version)?;
            meshes.push(super::class_wrapper_with_direct(
                super::MESH_CLASS,
                &mesh.body,
                &mesh.direct,
            )?);
        }
        if minor == 3 {
            let mut cache = 1_i32.to_le_bytes().to_vec();
            cache.extend(0_i32.to_le_bytes());
            let mut cache_direct = cache.clone();
            for wrapper in meshes {
                let mut item = 1_i32.to_le_bytes().to_vec();
                item.extend(0_i32.to_le_bytes());
                item.extend([0_u8; 16]);
                let item_direct = item.clone();
                item.extend(wrapper);
                cache.push(1);
                cache_direct.push(1);
                cache.extend(super::crc_chunk_with_direct(
                    0x4000_8000,
                    &item,
                    &item_direct,
                )?);
            }
            cache.push(0);
            cache_direct.push(0);
            payload_body.extend(super::crc_chunk_with_direct(
                0x4000_8000,
                &cache,
                &cache_direct,
            )?);
        } else if !meshes.is_empty() {
            let null_uuid = super::crc_chunk(0x0002_fffb, &[0; 16])?;
            let null_mesh = super::long_chunk(0x0002_7ffa, &null_uuid)?;
            let mut slots = Vec::with_capacity(3);
            for index in 0..3 {
                if let Some(mesh) = meshes.get(index) {
                    slots.push(mesh.clone());
                } else {
                    slots.push(null_mesh.clone());
                }
            }
            let cache_payload = slots.concat();
            let application_uuid = crate::wire::Uuid::from_canonical([
                0xc8, 0xcd, 0xa5, 0x97, 0xd9, 0x57, 0x46, 0x25, 0xa4, 0xb3, 0xa0, 0xb5, 0x10, 0xfc,
                0x30, 0xd4,
            ])
            .to_wire();
            userdata.push(v5_mesh_cache_userdata(
                crate::extrusion::ON_V5_EXTRUSION_DISPLAY_MESH_CACHE.to_wire(),
                application_uuid,
                i32::try_from(version.value()).unwrap_or(50),
                &cache_payload,
            )?);
        }
    }
    let payload = super::crc_chunk_with_direct(0x4000_8000, &payload_body, &direct)?;
    Ok(Some(Candidate {
        payload,
        body_id: body.id.to_string(),
        profile_count,
        userdata,
    }))
}

fn v5_mesh_cache_userdata(
    class_uuid: [u8; 16],
    application_uuid: [u8; 16],
    archive_version: i32,
    userdata_body: &[u8],
) -> Result<Vec<u8>, WriteError> {
    let mut transform = Vec::with_capacity(16 * 8);
    for index in 0..16 {
        let value: f64 = if index % 5 == 0 { 1.0 } else { 0.0 };
        transform.extend(value.to_le_bytes());
    }
    let header_body = [
        class_uuid.to_vec(),
        class_uuid.to_vec(),
        0_i32.to_le_bytes().to_vec(),
        transform,
        application_uuid.to_vec(),
        vec![0],
        archive_version.to_le_bytes().to_vec(),
        0_u32.to_le_bytes().to_vec(),
    ]
    .concat();
    let header = super::crc_chunk(0x0002_fff9, &header_body)?;
    let mut body = vec![0x22];
    let header_range_start = body.len();
    body.extend(header);
    let header_range_end = body.len();
    let payload_range_start = body.len();
    body.extend(super::crc_chunk(0x4000_8000, userdata_body)?);
    let payload_range_end = body.len();
    let children = [
        header_range_start..header_range_end,
        payload_range_start..payload_range_end,
    ];
    let direct = crate::chunks::direct_checksum_ranges(&(0..body.len()), &children)
        .map_err(|error| WriteError::Omit(error.to_string()))?;
    let mut hasher = crc32fast::Hasher::new();
    for range in &direct {
        hasher.update(&body[range]);
    }
    let mut payload = body;
    payload.extend(hasher.finalize().to_le_bytes());
    super::long_chunk(0x0002_7ffd, &payload)
}

fn geometry_agrees(input: &CadIr, output: &CadIr) -> bool {
    use serde_json::Value;
    use std::collections::{BTreeMap, BTreeSet};
    // Tessellations are render-mesh caches; extrusion mesh-cache round-trip
    // may not produce identical tessellation records in a minimal validation
    // decode, while the geometric/procedural content remains faithful.
    const ARENAS: &[&str] = &[
        "bodies",
        "regions",
        "shells",
        "faces",
        "loops",
        "coedges",
        "edges",
        "vertices",
        "points",
        "surfaces",
        "curves",
        "pcurves",
        "procedural_curves",
        "procedural_surfaces",
    ];
    fn normalize(value: &mut Value, ids: &BTreeMap<String, String>, literal: bool) {
        match value {
            Value::Object(fields) => {
                fields.remove("native_ref");
                fields.remove("source_object");
                for (key, value) in fields.iter_mut() {
                    normalize(value, ids, matches!(key.as_str(), "name" | "description"));
                }
            }
            Value::Array(values) => {
                for value in values {
                    normalize(value, ids, literal);
                }
            }
            Value::String(value) if !literal => {
                if let Some(replacement) = ids.get(value) {
                    *value = replacement.clone();
                }
            }
            _ => {}
        }
    }
    let (Ok(mut left), Ok(mut right)) = (
        serde_json::to_value(&input.model),
        serde_json::to_value(&output.model),
    ) else {
        return false;
    };
    let mut ids = BTreeMap::new();
    let mut targets = BTreeSet::new();
    for arena in ARENAS {
        let (Some(a), Some(b)) = (
            left.get(*arena).and_then(Value::as_array),
            right.get(*arena).and_then(Value::as_array),
        ) else {
            return false;
        };
        if a.len() != b.len() {
            return false;
        }
        for (a, b) in a.iter().zip(b) {
            let (Some(a), Some(b)) = (
                a.get("id").and_then(Value::as_str),
                b.get("id").and_then(Value::as_str),
            ) else {
                return false;
            };
            if ids.insert(a.to_owned(), b.to_owned()).is_some() || !targets.insert(b.to_owned()) {
                return false;
            }
        }
    }
    for arena in ARENAS {
        let Some(a) = left.get_mut(*arena) else {
            return false;
        };
        let Some(b) = right.get_mut(*arena) else {
            return false;
        };
        normalize(a, &ids, false);
        normalize(b, &BTreeMap::new(), false);
        if a != b {
            return false;
        }
    }
    true
}

pub(super) fn admit(
    session: &cadmpeg_core::encode::EncodeSession,
    ir: &CadIr,
    version: crate::RhinoArchiveVersion,
    materials: &super::MaterialPlan,
) -> Result<Option<(Candidate, Vec<u8>)>, WriteError> {
    if !ir.model.appearance_bindings.is_empty()
        || !ir.model.presentation_layers.is_empty()
        || !ir.model.view_presentations.is_empty()
        || !ir.model.attributes.is_empty()
    {
        return Ok(None);
    }
    let candidate = match payload(ir, version, None) {
        Ok(Some(candidate)) => candidate,
        Ok(None) | Err(WriteError::Omit(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let body = &ir.model.bodies[0];
    let attributes = super::object_attributes_payload(
        ir,
        &candidate.body_id,
        body.name.as_deref(),
        body.color,
        body.visible,
        materials,
    )?;
    let class = crate::extrusion::ON_EXTRUSION.to_wire();
    let record = super::framed_object_record_with_userdata(
        0x10,
        class,
        &candidate.payload,
        Some(&[]),
        Some(attributes),
        None,
        &candidate.userdata,
    )?;
    let Some(decoded) = decode_record(session, ir, version, &record)? else {
        return Ok(None);
    };
    if !geometry_agrees(ir, decoded.ir()) {
        return Ok(None);
    }
    Ok(Some((candidate, record)))
}

fn decode_record(
    _session: &cadmpeg_core::encode::EncodeSession,
    ir: &CadIr,
    version: crate::RhinoArchiveVersion,
    record: &[u8],
) -> Result<Option<cadmpeg_ir::codec::DecodeResult>, WriteError> {
    use cadmpeg_ir::codec::Codec;
    let mut table = record.to_vec();
    table.extend(super::short_chunk(super::TCODE_ENDOFTABLE, 0));
    let mut bytes = super::header(version);
    bytes.extend(super::long_chunk(1, b"cadmpeg")?);
    let mut settings = super::units_record(
        ir.tolerances.linear_or_default().get(),
        ir.tolerances.angular.get(),
    )?;
    settings.extend(super::short_chunk(super::TCODE_ENDOFTABLE, 0));
    bytes.extend(super::long_chunk(super::TCODE_SETTINGS_TABLE, &settings)?);
    bytes.extend(super::long_chunk(super::TCODE_OBJECT_TABLE, &table)?);
    let end = super::archive_final_size(cadmpeg_core::decode::u64_from_index(bytes.len()));
    bytes.extend(super::long_chunk(
        crate::chunks::TCODE_ENDOFFILE,
        &end.to_le_bytes(),
    )?);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .map_err(|error| match error {
            cadmpeg_core::CodecError::ResourceLimit(limit) => WriteError::Encode(limit.into()),
            other => WriteError::Fail(other),
        })?;
    match crate::RhinoCodec.decode_with_context(&ctx, root) {
        Ok(decoded) => Ok(Some(decoded)),
        Err(cadmpeg_ir::codec::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(
            limit,
        ))) => Err(WriteError::Encode(limit.into())),
        Err(_) => Ok(None),
    }
}

pub(super) fn separate(
    session: &cadmpeg_core::encode::EncodeSession,
    ir: &CadIr,
    surface: &cadmpeg_ir::geometry::Surface,
    version: crate::RhinoArchiveVersion,
) -> Result<Option<(cadmpeg_ir::ids::CurveId, super::SurfacePayload)>, WriteError> {
    let Some(id) = surface.geometry.procedural_construction() else {
        return Ok(None);
    };
    let Some(procedure) = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|row| row.id == *id)
    else {
        return Ok(None);
    };
    let ProceduralSurfaceDefinition::Extrusion(definition) = procedure.definition() else {
        return Ok(None);
    };
    let candidate = match payload(ir, version, Some(id)) {
        Ok(Some(value)) => value,
        Ok(None) | Err(WriteError::Omit(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let record = super::framed_object_record(
        0x10,
        crate::extrusion::ON_EXTRUSION.to_wire(),
        &candidate.payload,
        Some(&[]),
        None,
    )?;
    let Some(decoded) = decode_record(session, ir, version, &record)? else {
        return Ok(None);
    };
    let output = decoded.ir();
    let [out_surface] = output.model.surfaces.as_slice() else {
        return Ok(None);
    };
    let [out_procedure] = output.model.procedural_surfaces.as_slice() else {
        return Ok(None);
    };
    let ProceduralSurfaceDefinition::Extrusion(out_definition) = out_procedure.definition() else {
        return Ok(None);
    };
    let Some(source_curve) = ir
        .model
        .curves
        .iter()
        .find(|row| row.id == *definition.directrix())
    else {
        return Ok(None);
    };
    let Some(out_curve) = output
        .model
        .curves
        .iter()
        .find(|row| row.id == *out_definition.directrix())
    else {
        return Ok(None);
    };
    let (Ok(mut left), Ok(right)) = (
        serde_json::to_value(definition),
        serde_json::to_value(out_definition),
    ) else {
        return Ok(None);
    };
    if let Some(object) = left.as_object_mut() {
        object.insert(
            "directrix".to_owned(),
            serde_json::Value::String(out_definition.directrix().to_string()),
        );
    }
    if left != right
        || procedure.record_bounds() != out_procedure.record_bounds()
        || surface.geometry.solved() != out_surface.geometry.solved()
        || source_curve.parameter_range != out_curve.parameter_range
        || source_curve.geometry.solved() != out_curve.geometry.solved()
    {
        return Ok(None);
    }
    Ok(Some((
        definition.directrix().clone(),
        super::SurfacePayload {
            class: crate::extrusion::ON_EXTRUSION.to_wire(),
            body: candidate.payload,
            direct: Some(Vec::new()),
            record_count: 2,
        },
    )))
}

pub(super) fn sum(
    session: &cadmpeg_core::encode::EncodeSession,
    ir: &CadIr,
    surface: &cadmpeg_ir::geometry::Surface,
    version: crate::RhinoArchiveVersion,
) -> Result<Option<(cadmpeg_ir::ids::CurveId, super::SurfacePayload)>, WriteError> {
    let Some(id) = surface.geometry.procedural_construction() else {
        return Ok(None);
    };
    let Some(procedure) = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|row| row.id == *id)
    else {
        return Ok(None);
    };
    let ProceduralSurfaceDefinition::Extrusion(definition) = procedure.definition() else {
        return Ok(None);
    };
    if definition.parameter_interval().is_some()
        || definition.native_position().is_some()
        || definition.revision_form().is_some()
        || definition.legacy_cache().is_some()
    {
        return Ok(None);
    }
    let Some([Some(_u0), Some(v0), Some(_u1), Some(v1)]) = procedure
        .record_bounds()
        .map(cadmpeg_ir::geometry::RecordBounds::get)
    else {
        return Ok(None);
    };
    if v0 >= v1 {
        return Ok(None);
    }
    let Some(source_curve) = ir
        .model
        .curves
        .iter()
        .find(|row| row.id == *definition.directrix())
    else {
        return Ok(None);
    };
    if source_curve.parameter_range.is_some()
        || source_curve.geometry.procedural_construction().is_some()
    {
        return Ok(None);
    }
    let Some(SolvedCurveGeometry::Nurbs(curve)) = source_curve.geometry.solved() else {
        return Ok(None);
    };
    let Some(SolvedSurfaceGeometry::Nurbs(solved)) = surface.geometry.solved() else {
        return Ok(None);
    };
    if solved
        .weights()
        .is_some_and(|weights| weights.iter().flatten().any(|weight| weight.get() <= 0.0))
    {
        return Ok(None);
    }
    match super::check_nurbs_curve(source_curve.id.as_str(), curve) {
        Ok(()) => {}
        Err(WriteError::Omit(_)) => return Ok(None),
        Err(error) => return Err(error),
    }
    let direction = *definition.direction().as_raw();
    let from = direction.scale(v0);
    let to = direction.scale(v1);
    let first = super::nurbs_curve_payload_dimension(curve, 3)?;
    let second =
        super::bounded_line_payload([from.x, from.y, from.z], [to.x, to.y, to.z], [v0, v1], 3)?;
    let mut direct = vec![0x10];
    for _ in 0..3 {
        direct.extend(0.0_f64.to_le_bytes());
    }
    let poles = solved.poles();
    if poles.is_empty() {
        return Ok(None);
    }
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for point in poles {
        let point = point.get();
        for (index, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            min[index] = min[index].min(value);
            max[index] = max[index].max(value);
        }
    }
    for value in min.into_iter().chain(max) {
        direct.extend(value.to_le_bytes());
    }
    let mut body = direct.clone();
    body.extend(super::class_wrapper(super::NURBS_CURVE_CLASS, &first)?);
    body.extend(super::class_wrapper(super::LINE_CLASS, &second)?);
    let class = crate::surfaces::SUM_SURFACE.to_wire();
    let record = super::framed_object_record(8, class, &body, Some(&direct), None)?;
    let Some(decoded) = decode_record(session, ir, version, &record)? else {
        return Ok(None);
    };
    let output = decoded.ir();
    let [out_surface] = output.model.surfaces.as_slice() else {
        return Ok(None);
    };
    let [out_procedure] = output.model.procedural_surfaces.as_slice() else {
        return Ok(None);
    };
    let ProceduralSurfaceDefinition::Sum(sum) = out_procedure.definition() else {
        return Ok(None);
    };
    let Some(out_curve) = output
        .model
        .curves
        .iter()
        .find(|row| row.id == *sum.first())
    else {
        return Ok(None);
    };
    if surface.geometry.solved() != out_surface.geometry.solved()
        || source_curve.geometry.solved() != out_curve.geometry.solved()
    {
        return Ok(None);
    }
    Ok(Some((
        definition.directrix().clone(),
        super::SurfacePayload {
            class,
            body,
            direct: Some(direct),
            record_count: 3,
        },
    )))
}
