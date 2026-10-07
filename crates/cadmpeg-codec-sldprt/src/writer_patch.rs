// SPDX-License-Identifier: Apache-2.0
//! In-place edits to a retained native partition with a stable entity graph.

use std::collections::HashMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::Annotations;

use crate::SourceRecord;

pub(crate) fn patch_partition(
    ir: &CadIr,
    annotations: &Annotations,
    retained_records: &[SourceRecord<'_>],
    scale: f64,
) -> Result<Option<(String, Vec<u8>)>, CodecError> {
    let requires_native_carrier_patch = ir.model.surfaces.iter().any(|surface| {
        matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
        )
    }) || ir.model.curves.iter().any(|curve| {
        matches!(
            curve.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
        )
    });
    if !requires_native_carrier_patch {
        return Ok(None);
    }
    let Some(source) = retained_records
        .iter()
        .find(|record| record.id.as_str() == "sldprt:file:source-image#0")
        .and_then(|record| record.data)
    else {
        return Ok(None);
    };
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(source, &arena, &DecodePolicy::desktop())?;
    let scan = crate::container::scan(&ctx, root)?;
    let Some(selected) = crate::container::select_active_parasolid_site(&ctx, &scan)? else {
        return Ok(None);
    };
    let crate::container::Section::Block(block) = selected.section else {
        return Ok(None);
    };
    let header = selected.header;
    if block
        .ps_streams
        .first()
        .map(|stream| stream.payload.as_slice())
        != Some(block.payload.as_slice())
    {
        return Ok(None);
    }
    let site = site_key(block);
    let mut streams = Vec::new();
    for candidate in ctx
        .admit_iter(&scan.blocks, "scan SLDPRT patch stream blocks")?
        .filter(|candidate| site_key(candidate) == site)
    {
        for stream in ctx.admit_iter(&candidate.ps_streams, "scan SLDPRT patch body streams")? {
            if stream.header.is_body_stream() {
                ctx.reserve_vec(&mut streams, 1, "index SLDPRT patch streams")?;
                streams.push((candidate, &stream.payload, &stream.header));
            }
        }
    }
    let mut ordered = Vec::new();
    for (candidate, payload, header) in streams.drain(..) {
        let section = candidate.section.name().unwrap_or("");
        let work = section
            .len()
            .checked_add(header.description.len())
            .ok_or_else(|| {
                ctx.refuse_codec_limit("order SLDPRT patch streams", u64::MAX, u64::MAX)
            })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(work),
            "order SLDPRT patch streams",
        )?;
        let partition = section
            .as_bytes()
            .windows(9)
            .any(|part| part.eq_ignore_ascii_case(b"partition"));
        let body_partition = header
            .description
            .as_bytes()
            .windows(9)
            .any(|part| part.eq_ignore_ascii_case(b"partition"));
        ctx.push_vec(
            &mut ordered,
            ((!partition, !body_partition), (candidate, payload, header)),
            "order SLDPRT patch streams",
        )?;
    }
    ctx.stable_sort_by(
        &mut ordered,
        |value| &value.0,
        Ord::cmp,
        "sort SLDPRT patch streams",
    )?;
    let bodies = ctx.collect_vec(
        ordered
            .iter()
            .map(|(_, (_, payload, header))| (payload.as_slice(), *header)),
        "index SLDPRT patch bodies",
    )?;
    let native = crate::brep::graph::decode_bodies(
        &ctx,
        &bodies,
        &cadmpeg_ir::stream_name!("native-patch-baseline"),
    )?;
    if !same_graph(ir, &native) {
        return Ok(None);
    }
    validate_changed_annotations(
        ir,
        annotations,
        &native,
        block.section.source_stream().as_str(),
    )?;

    let mut payload = ctx.copy_retained(&block.payload, "copy SLDPRT patch partition")?;
    let Some(body) = payload.get_mut(header.body_offset..) else {
        return Ok(None);
    };
    if patch_points(&ctx, ir, annotations, &native, body, scale)?.is_none() {
        return Ok(None);
    }
    if patch_surfaces(&ctx, ir, annotations, &native, body, scale)?.is_none() {
        return Ok(None);
    }
    if patch_curves(&ctx, ir, annotations, &native, body, scale)?.is_none() {
        return Ok(None);
    }
    Ok(Some((
        block.section.source_stream().as_str().to_owned(),
        payload,
    )))
}

fn validate_changed_annotations(
    ir: &CadIr,
    annotations: &Annotations,
    native: &crate::brep::graph::Brep,
    section: &str,
) -> Result<(), CodecError> {
    let points = native
        .points
        .iter()
        .map(|value| (&value.id, value))
        .collect::<HashMap<_, _>>();
    let surfaces = native
        .surfaces
        .iter()
        .map(|value| (&value.id, value))
        .collect::<HashMap<_, _>>();
    let curves = native
        .curves
        .iter()
        .map(|value| (&value.id, value))
        .collect::<HashMap<_, _>>();
    for point in &ir.model.points {
        if points
            .get(&point.id)
            .is_some_and(|old| old.position().get() != point.position().get())
        {
            annotation_offset(annotations, &point.id, section)?;
        }
    }
    for surface in &ir.model.surfaces {
        if surfaces
            .get(&surface.id)
            .is_some_and(|old| old.geometry != surface.geometry)
        {
            annotation_offset(annotations, &surface.id, section)?;
        }
    }
    for curve in &ir.model.curves {
        if curves
            .get(&curve.id)
            .is_some_and(|old| old.geometry != curve.geometry)
        {
            annotation_offset(annotations, &curve.id, section)?;
        }
    }
    Ok(())
}

fn annotation_offset(
    annotations: &Annotations,
    id: impl std::fmt::Display,
    section: &str,
) -> Result<usize, CodecError> {
    let id = id.to_string();
    let provenance = annotations.provenance.get(&id).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "SLDPRT mutation requires provenance annotation for {id}"
        ))
    })?;
    let stream = provenance.stream();
    if stream != section {
        return Err(CodecError::malformed(format_args!(
            "SLDPRT mutation provenance for {id} references {stream}, not {section}"
        )));
    }
    raw_annotation_offset(annotations, &id)
}

fn raw_annotation_offset(
    annotations: &Annotations,
    id: impl std::fmt::Display,
) -> Result<usize, CodecError> {
    let id = id.to_string();
    let provenance = annotations.provenance.get(&id).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "SLDPRT mutation requires provenance annotation for {id}"
        ))
    })?;
    usize::try_from(provenance.offset).map_err(|_| {
        CodecError::malformed(format_args!(
            "SLDPRT mutation provenance offset for {id} exceeds address space"
        ))
    })
}

fn site_key(block: &crate::container::Block) -> String {
    let mut key = block.section.source_stream().as_str().to_ascii_lowercase();
    for suffix in ["partition", "deltas"] {
        if let Some(offset) = key.rfind(suffix) {
            key.truncate(offset);
            break;
        }
    }
    key.trim_end_matches(['-', '/', '_']).to_string()
}

fn same_graph(ir: &CadIr, native: &crate::brep::graph::Brep) -> bool {
    ir.model
        .bodies
        .iter()
        .map(|v| (&v.id, v.kind, &v.regions))
        .eq(native.bodies.iter().map(|v| (&v.id, v.kind, &v.regions)))
        && ir
            .model
            .regions
            .iter()
            .map(|v| (&v.id, &v.body, &v.shells))
            .eq(native.regions.iter().map(|v| (&v.id, &v.body, &v.shells)))
        && ir
            .model
            .shells
            .iter()
            .map(|v| (&v.id, &v.region, v.faces()))
            .eq(native.shells.iter().map(|v| (&v.id, &v.region, v.faces())))
        && ir
            .model
            .faces
            .iter()
            .map(|v| (&v.id, &v.shell, &v.surface, v.sense, &v.loops))
            .eq(native
                .faces
                .iter()
                .map(|v| (&v.id, &v.shell, &v.surface, v.sense, &v.loops)))
        && ir
            .model
            .loops
            .iter()
            .map(|v| (&v.id, &v.face, v.coedges()))
            .eq(native.loops.iter().map(|v| (&v.id, &v.face, v.coedges())))
        && ir
            .model
            .coedges
            .iter()
            .map(|v| {
                (
                    &v.id,
                    &v.owner_loop,
                    &v.edge,
                    &v.radial_next,
                    v.sense,
                    &v.pcurves,
                )
            })
            .eq(native.coedges.iter().map(|v| {
                (
                    &v.id,
                    &v.owner_loop,
                    &v.edge,
                    &v.radial_next,
                    v.sense,
                    &v.pcurves,
                )
            }))
        && ir
            .model
            .edges
            .iter()
            .map(|v| (&v.id, v.curve(), &v.start, &v.end, v.param_range()))
            .eq(native
                .edges
                .iter()
                .map(|v| (&v.id, v.curve(), &v.start, &v.end, v.param_range())))
        && ir
            .model
            .vertices
            .iter()
            .map(|v| (&v.id, &v.point))
            .eq(native.vertices.iter().map(|v| (&v.id, &v.point)))
        && ir
            .model
            .points
            .iter()
            .map(|v| &v.id)
            .eq(native.points.iter().map(|v| &v.id))
        && ir
            .model
            .surfaces
            .iter()
            .map(|v| (&v.id, surface_class(&v.geometry)))
            .eq(native
                .surfaces
                .iter()
                .map(|v| (&v.id, surface_class(&v.geometry))))
        && ir
            .model
            .curves
            .iter()
            .map(|v| (&v.id, curve_class(&v.geometry)))
            .eq(native
                .curves
                .iter()
                .map(|v| (&v.id, curve_class(&v.geometry))))
}

fn surface_class(value: &SurfaceGeometry) -> u8 {
    match value {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => 0,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => 1,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => 2,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => 3,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_)) => 4,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_)) => 5,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => 6,
        SurfaceGeometry::Procedural { .. } => 7,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(_)) => 7,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Polygonal(_)) => 8,
    }
}

fn curve_class(value: &CurveGeometry) -> u8 {
    match value {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_)) => 0,
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(_)) => 1,
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(_)) => 2,
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_)) => 3,
        CurveGeometry::Solved(SolvedCurveGeometry::Parabola(_)) => 4,
        CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(_)) => 5,
        CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(_)) => 6,
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. }) => 7,
        CurveGeometry::Procedural { .. } => 8,
        CurveGeometry::Solved(SolvedCurveGeometry::Transformed(_)) => 8,
        CurveGeometry::Solved(SolvedCurveGeometry::Polyline(_)) => 9,
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. }) => 10,
    }
}

fn patch_points(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    annotations: &Annotations,
    native: &crate::brep::graph::Brep,
    payload: &mut [u8],
    scale: f64,
) -> Result<Option<()>, CodecError> {
    let mut current = HashMap::new();
    for point in &ir.model.points {
        ctx.insert_hash_map(&mut current, &point.id, point, "index SLDPRT patch points")?;
    }
    for old in &native.points {
        let Some(new) = current.get(&old.id) else {
            return Ok(None);
        };
        if new.position().get() == old.position().get() {
            continue;
        }
        let offset = raw_annotation_offset(annotations, &old.id)?;
        let tables = crate::brep::topology::scan(ctx, payload)?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(tables.points().len()),
            "find SLDPRT patch point",
        )?;
        let Some(point) = tables
            .points()
            .values()
            .find(|point| point.offset == offset)
        else {
            return Ok(None);
        };
        let values = point.xyz_offset;
        let old_xyz_m = [
            old.position().get().x * 0.001,
            old.position().get().y * 0.001,
            old.position().get().z * 0.001,
        ];
        let new_xyz_m = [
            new.position().get().x * scale,
            new.position().get().y * scale,
            new.position().get().z * scale,
        ];
        if !crate::brep::topology::patch_point_values(payload, values, old_xyz_m, new_xyz_m) {
            return Ok(None);
        }
    }
    Ok(Some(()))
}

fn patch_surfaces(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    annotations: &Annotations,
    native: &crate::brep::graph::Brep,
    payload: &mut [u8],
    scale: f64,
) -> Result<Option<()>, CodecError> {
    let old = ctx.collect_hash_map(
        native.surfaces.iter().map(|v| (&v.id, v)),
        "index SLDPRT patch surfaces",
    )?;
    for surface in &ir.model.surfaces {
        let Some(baseline) = old.get(&surface.id) else {
            return Ok(None);
        };
        match (&surface.geometry, &baseline.geometry) {
            (
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }),
            ) => continue,
            (
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(new)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(old)),
            ) if new == old => continue,
            (
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(new)),
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(old)),
            ) => {
                if crate::brep::spline::patch_nurbs_surface(
                    ctx,
                    payload,
                    raw_annotation_offset(annotations, &surface.id)?,
                    old,
                    new,
                    scale,
                )?
                .is_none()
                {
                    return Ok(None);
                }
                continue;
            }
            _ if surface.geometry == baseline.geometry => continue,
            _ => {}
        }
        let Some(solved) = surface.geometry.solved() else {
            return Ok(None);
        };
        let reference = super::writer::surface_reference(solved);
        let (_, values) = super::writer::surface_values(&surface.geometry, reference, scale)?;
        if patch_compact(
            payload,
            raw_annotation_offset(annotations, &surface.id)?,
            &values,
        )
        .is_none()
        {
            return Ok(None);
        }
    }
    Ok(Some(()))
}

fn patch_curves(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    annotations: &Annotations,
    native: &crate::brep::graph::Brep,
    payload: &mut [u8],
    scale: f64,
) -> Result<Option<()>, CodecError> {
    let old = ctx.collect_hash_map(
        native.curves.iter().map(|v| (&v.id, v)),
        "index SLDPRT patch curves",
    )?;
    for curve in &ir.model.curves {
        let Some(baseline) = old.get(&curve.id) else {
            return Ok(None);
        };
        match (&curve.geometry, &baseline.geometry) {
            (
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. }),
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. }),
            ) => continue,
            (
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(new)),
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(old)),
            ) if new == old => continue,
            (
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(new)),
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(old)),
            ) => {
                if crate::brep::spline::patch_nurbs_curve(
                    ctx,
                    payload,
                    raw_annotation_offset(annotations, &curve.id)?,
                    old,
                    new,
                    scale,
                )?
                .is_none()
                {
                    return Ok(None);
                }
                continue;
            }
            _ if curve.geometry == baseline.geometry => continue,
            _ => {}
        }
        let (_, values) = super::writer::curve_values(&curve.geometry, scale)?;
        if patch_compact(
            payload,
            raw_annotation_offset(annotations, &curve.id)?,
            &values,
        )
        .is_none()
        {
            return Ok(None);
        }
    }
    Ok(Some(()))
}

/// Overwrite the trailing scalar run of the compact carrier at `offset`.
fn patch_compact(payload: &mut [u8], offset: usize, values: &[f64]) -> Option<()> {
    let end = match crate::brep::parse_carrier(payload, offset)? {
        crate::brep::Carrier::Curve(carrier) => carrier.end,
        crate::brep::Carrier::Surface(carrier) => carrier.end,
    };
    let start = end.checked_sub(values.len() * 8)?;
    for (index, value) in values.iter().enumerate() {
        payload
            .get_mut(start + index * 8..start + (index + 1) * 8)?
            .copy_from_slice(&value.to_be_bytes());
    }
    Some(())
}

#[cfg(test)]
mod tests;
