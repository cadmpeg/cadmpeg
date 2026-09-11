// SPDX-License-Identifier: Apache-2.0
//! In-place edits to a retained native partition with a stable entity graph.

use std::collections::HashMap;

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
    patch_partition_inner(ir, annotations, retained_records, scale).transpose()
}

fn patch_partition_inner(
    ir: &CadIr,
    annotations: &Annotations,
    retained_records: &[SourceRecord<'_>],
    scale: f64,
) -> Option<Result<(String, Vec<u8>), CodecError>> {
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
        return None;
    }
    let source = retained_records
        .iter()
        .find(|record| record.id.as_str() == "sldprt:file:source-image#0")?
        .data?;
    let scan = crate::container::scan_bytes(source);
    let selected = crate::container::select_active_parasolid_site(&scan)?;
    let crate::container::Section::Block(block) = selected.section else {
        return None;
    };
    let header = selected.header;
    if block
        .ps_streams
        .first()
        .map(|stream| stream.payload.as_slice())
        != Some(block.payload.as_slice())
    {
        return None;
    }
    let site = site_key(block);
    let mut streams = scan
        .blocks
        .iter()
        .filter(|candidate| site_key(candidate) == site)
        .flat_map(|candidate| {
            candidate.ps_streams.iter().filter_map(move |stream| {
                crate::parasolid::is_body_stream(&stream.header).then_some((
                    candidate,
                    &stream.payload,
                    &stream.header,
                ))
            })
        })
        .collect::<Vec<_>>();
    streams.sort_by_key(|(candidate, _, header)| {
        let section = candidate
            .section
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        (
            !section.contains("partition"),
            !header
                .description
                .to_ascii_lowercase()
                .contains("partition"),
        )
    });
    let bodies = streams
        .iter()
        .map(|(_, payload, header)| (payload.as_slice(), *header))
        .collect::<Vec<_>>();
    let native = crate::brep::decode_bodies(&bodies, "native-patch-baseline").ok()?;
    if !same_graph(ir, &native) {
        return None;
    }
    let section = block
        .section
        .clone()
        .unwrap_or_else(|| format!("block@{}", block.offset));
    if let Err(error) = validate_changed_annotations(ir, annotations, &native, &section) {
        return Some(Err(error));
    }

    let mut payload = block.payload.clone();
    patch_points(
        ir,
        annotations,
        &native,
        &mut payload,
        header.body_offset,
        scale,
    )?;
    patch_surfaces(
        ir,
        annotations,
        &native,
        &mut payload,
        header.body_offset,
        scale,
    )?;
    patch_curves(
        ir,
        annotations,
        &native,
        &mut payload,
        header.body_offset,
        scale,
    )?;
    Some(Ok((section, payload)))
}

fn validate_changed_annotations(
    ir: &CadIr,
    annotations: &Annotations,
    native: &crate::brep::Brep,
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
            .is_some_and(|old| old.position != point.position)
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
    let mut key = block
        .section
        .clone()
        .unwrap_or_else(|| format!("block@{}", block.offset))
        .to_ascii_lowercase();
    for suffix in ["partition", "deltas"] {
        if let Some(offset) = key.rfind(suffix) {
            key.truncate(offset);
            break;
        }
    }
    key.trim_end_matches(['-', '/', '_']).to_string()
}

fn same_graph(ir: &CadIr, native: &crate::brep::Brep) -> bool {
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
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed { .. }) => 7,
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
        CurveGeometry::Solved(SolvedCurveGeometry::Transformed { .. }) => 8,
        CurveGeometry::Solved(SolvedCurveGeometry::Polyline(_)) => 9,
        CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. }) => 10,
    }
}

fn patch_points(
    ir: &CadIr,
    annotations: &Annotations,
    native: &crate::brep::Brep,
    payload: &mut [u8],
    body_start: usize,
    scale: f64,
) -> Option<()> {
    let current = ir
        .model
        .points
        .iter()
        .map(|v| (&v.id, v))
        .collect::<HashMap<_, _>>();
    for old in &native.points {
        let new = current[&old.id];
        if new.position == old.position {
            continue;
        }
        let offset = raw_annotation_offset(annotations, &old.id).ok()?;
        let tables = crate::brep::topology::scan(payload.get(body_start..)?);
        let point = tables
            .points()
            .values()
            .find(|point| point.offset == offset)?;
        let values = body_start.checked_add(point.xyz_offset)?;
        let old_xyz_m = [
            old.position.x * 0.001,
            old.position.y * 0.001,
            old.position.z * 0.001,
        ];
        let new_xyz_m = [
            new.position.x * scale,
            new.position.y * scale,
            new.position.z * scale,
        ];
        if !crate::brep::topology::patch_point_values(payload, values, old_xyz_m, new_xyz_m) {
            return None;
        }
    }
    Some(())
}

fn patch_surfaces(
    ir: &CadIr,
    annotations: &Annotations,
    native: &crate::brep::Brep,
    payload: &mut [u8],
    body_start: usize,
    scale: f64,
) -> Option<()> {
    let old = native
        .surfaces
        .iter()
        .map(|v| (&v.id, v))
        .collect::<HashMap<_, _>>();
    for surface in &ir.model.surfaces {
        let baseline = old[&surface.id];
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
                crate::brep::patch_nurbs_surface(
                    payload.get_mut(body_start..)?,
                    raw_annotation_offset(annotations, &surface.id).ok()?,
                    old,
                    new,
                    scale,
                )?;
                continue;
            }
            _ if surface.geometry == baseline.geometry => continue,
            _ => {}
        }
        let reference = super::writer::surface_reference(surface.geometry.solved()?);
        let (_, values) =
            super::writer::surface_values(&surface.geometry, reference, scale).ok()?;
        patch_compact(
            payload,
            body_start,
            raw_annotation_offset(annotations, &surface.id).ok()? as u64,
            &values,
        )?;
    }
    Some(())
}

fn patch_curves(
    ir: &CadIr,
    annotations: &Annotations,
    native: &crate::brep::Brep,
    payload: &mut [u8],
    body_start: usize,
    scale: f64,
) -> Option<()> {
    let old = native
        .curves
        .iter()
        .map(|v| (&v.id, v))
        .collect::<HashMap<_, _>>();
    for curve in &ir.model.curves {
        let baseline = old[&curve.id];
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
                crate::brep::patch_nurbs_curve(
                    payload.get_mut(body_start..)?,
                    raw_annotation_offset(annotations, &curve.id).ok()?,
                    old,
                    new,
                    scale,
                )?;
                continue;
            }
            _ if curve.geometry == baseline.geometry => continue,
            _ => {}
        }
        let (_, values) = super::writer::curve_values(&curve.geometry, scale).ok()?;
        patch_compact(
            payload,
            body_start,
            raw_annotation_offset(annotations, &curve.id).ok()? as u64,
            &values,
        )?;
    }
    Some(())
}

fn patch_compact(payload: &mut [u8], body_start: usize, offset: u64, values: &[f64]) -> Option<()> {
    let carrier = crate::brep::parse_carrier(payload.get(body_start..)?, offset as usize)?;
    let end = match carrier {
        crate::brep::Carrier::Curve(carrier) => carrier.end,
        crate::brep::Carrier::Surface(carrier) => carrier.end,
    };
    let start = body_start.checked_add(end.checked_sub(values.len() * 8)?)?;
    for (index, value) in values.iter().enumerate() {
        payload
            .get_mut(start + index * 8..start + (index + 1) * 8)?
            .copy_from_slice(&value.to_be_bytes());
    }
    Some(())
}

#[cfg(test)]
mod tests;
