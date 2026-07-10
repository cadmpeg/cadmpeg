// SPDX-License-Identifier: Apache-2.0
//! In-place edits to a retained native partition with a stable entity graph.

use std::collections::HashMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

pub fn patch_partition(ir: &CadIr, scale: f64) -> Option<(String, Vec<u8>)> {
    if !ir
        .surfaces
        .iter()
        .any(|surface| matches!(surface.geometry, SurfaceGeometry::Unknown { .. }))
    {
        return None;
    }
    let source = ir
        .unknowns
        .iter()
        .find(|record| record.id.0 == "sldprt:source-image")?
        .data
        .as_deref()?;
    let scan = crate::container::scan_bytes(source);
    let (block, header) = crate::container::select_active_parasolid(&scan)?;
    if block.ps_stream.as_deref() != Some(block.payload.as_slice()) {
        return None;
    }
    let site = site_key(block);
    let mut streams = scan
        .blocks
        .iter()
        .filter(|candidate| site_key(candidate) == site)
        .flat_map(|candidate| {
            candidate.ps_streams.iter().filter_map(move |payload| {
                let header = crate::parasolid::stream_header(payload)?;
                crate::parasolid::is_body_stream(&header).then_some((candidate, payload, header))
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
        .map(|(_, payload, header)| (payload.as_slice(), header))
        .collect::<Vec<_>>();
    let native = crate::brep::decode_bodies(&bodies, "native-patch-baseline");
    if !same_graph(ir, &native) {
        return None;
    }

    let mut payload = block.payload.clone();
    patch_points(ir, &native, &mut payload, header.body_offset, scale)?;
    patch_surfaces(ir, &native, &mut payload, header.body_offset, scale)?;
    patch_curves(ir, &native, &mut payload, header.body_offset, scale)?;
    Some((
        block
            .section
            .clone()
            .unwrap_or_else(|| format!("block@{}", block.offset)),
        payload,
    ))
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
    ir.bodies
        .iter()
        .map(|v| (&v.id, v.kind, &v.lumps))
        .eq(native.bodies.iter().map(|v| (&v.id, v.kind, &v.lumps)))
        && ir
            .lumps
            .iter()
            .map(|v| (&v.id, &v.body, &v.shells))
            .eq(native.lumps.iter().map(|v| (&v.id, &v.body, &v.shells)))
        && ir
            .shells
            .iter()
            .map(|v| (&v.id, &v.lump, &v.faces))
            .eq(native.shells.iter().map(|v| (&v.id, &v.lump, &v.faces)))
        && ir
            .faces
            .iter()
            .map(|v| (&v.id, &v.shell, &v.surface, v.sense, &v.loops))
            .eq(native
                .faces
                .iter()
                .map(|v| (&v.id, &v.shell, &v.surface, v.sense, &v.loops)))
        && ir
            .loops
            .iter()
            .map(|v| (&v.id, &v.face, &v.coedges))
            .eq(native.loops.iter().map(|v| (&v.id, &v.face, &v.coedges)))
        && ir
            .coedges
            .iter()
            .map(|v| {
                (
                    &v.id,
                    &v.owner_loop,
                    &v.edge,
                    &v.next,
                    &v.previous,
                    &v.partner,
                    v.sense,
                    &v.pcurve,
                )
            })
            .eq(native.coedges.iter().map(|v| {
                (
                    &v.id,
                    &v.owner_loop,
                    &v.edge,
                    &v.next,
                    &v.previous,
                    &v.partner,
                    v.sense,
                    &v.pcurve,
                )
            }))
        && ir
            .edges
            .iter()
            .map(|v| (&v.id, &v.curve, &v.start, &v.end, v.param_range))
            .eq(native
                .edges
                .iter()
                .map(|v| (&v.id, &v.curve, &v.start, &v.end, v.param_range)))
        && ir
            .vertices
            .iter()
            .map(|v| (&v.id, &v.point))
            .eq(native.vertices.iter().map(|v| (&v.id, &v.point)))
        && ir
            .points
            .iter()
            .map(|v| &v.id)
            .eq(native.points.iter().map(|v| &v.id))
        && ir
            .surfaces
            .iter()
            .map(|v| (&v.id, surface_class(&v.geometry)))
            .eq(native
                .surfaces
                .iter()
                .map(|v| (&v.id, surface_class(&v.geometry))))
        && ir
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
        SurfaceGeometry::Plane { .. } => 0,
        SurfaceGeometry::Cylinder { .. } => 1,
        SurfaceGeometry::Cone { .. } => 2,
        SurfaceGeometry::Sphere { .. } => 3,
        SurfaceGeometry::Torus { .. } => 4,
        SurfaceGeometry::Nurbs(_) => 5,
        SurfaceGeometry::Unknown { .. } => 6,
    }
}

fn curve_class(value: &CurveGeometry) -> u8 {
    match value {
        CurveGeometry::Line { .. } => 0,
        CurveGeometry::Circle { .. } => 1,
        CurveGeometry::Ellipse { .. } => 2,
        CurveGeometry::Nurbs(_) => 3,
        CurveGeometry::Parabola { .. } => 4,
        CurveGeometry::Hyperbola { .. } => 5,
    }
}

fn patch_points(
    ir: &CadIr,
    native: &crate::brep::Brep,
    payload: &mut [u8],
    body_start: usize,
    scale: f64,
) -> Option<()> {
    let current = ir
        .points
        .iter()
        .map(|v| (&v.id, v))
        .collect::<HashMap<_, _>>();
    for old in &native.points {
        let new = current[&old.id];
        if new.position == old.position {
            continue;
        }
        let old_bytes = point_bytes(old.position, 0.001);
        let new_bytes = point_bytes(new.position, scale);
        let start = body_start.checked_add(old.meta.provenance.offset as usize)?;
        if payload.get(start..start + 2) != Some(&[0, 0x1d]) {
            return None;
        }
        let record = start + 2 + usize::from(payload.get(start + 2) == Some(&0xff));
        let mut values = record + 14;
        let mut cursor = record + 6;
        let mut tripled = false;
        while payload.get(cursor + 2) == Some(&1) && cursor < record + 54 {
            tripled = true;
            cursor += 3;
        }
        if tripled {
            values = cursor;
        }
        if payload.get(values..values + 24) != Some(old_bytes.as_slice()) {
            return None;
        }
        payload
            .get_mut(values..values + 24)?
            .copy_from_slice(&new_bytes);
    }
    Some(())
}

fn point_bytes(point: cadmpeg_ir::math::Point3, scale: f64) -> Vec<u8> {
    [point.x, point.y, point.z]
        .into_iter()
        .flat_map(|value| (value * scale).to_be_bytes())
        .collect()
}

fn patch_surfaces(
    ir: &CadIr,
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
    for surface in &ir.surfaces {
        let baseline = old[&surface.id];
        match (&surface.geometry, &baseline.geometry) {
            (SurfaceGeometry::Unknown { .. }, SurfaceGeometry::Unknown { .. }) => continue,
            (SurfaceGeometry::Nurbs(new), SurfaceGeometry::Nurbs(old)) if new == old => continue,
            (SurfaceGeometry::Nurbs(_), SurfaceGeometry::Nurbs(_)) => return None,
            _ if surface.geometry == baseline.geometry => continue,
            _ => {}
        }
        let reference = ir
            .surface_parameterizations
            .iter()
            .find(|frame| frame.surface == surface.id)
            .map_or_else(
                || super::writer::surface_reference(&surface.geometry),
                |frame| frame.u_reference,
            );
        let (_, values) =
            super::writer::surface_values(&surface.geometry, reference, scale).ok()?;
        patch_compact(
            payload,
            body_start,
            baseline.meta.provenance.offset,
            &values,
        )?;
    }
    Some(())
}

fn patch_curves(
    ir: &CadIr,
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
    for curve in &ir.curves {
        let baseline = old[&curve.id];
        match (&curve.geometry, &baseline.geometry) {
            (CurveGeometry::Nurbs(new), CurveGeometry::Nurbs(old)) if new == old => continue,
            (CurveGeometry::Nurbs(_), CurveGeometry::Nurbs(_)) => return None,
            _ if curve.geometry == baseline.geometry => continue,
            _ => {}
        }
        let (_, values) = super::writer::curve_values(&curve.geometry, scale).ok()?;
        patch_compact(
            payload,
            body_start,
            baseline.meta.provenance.offset,
            &values,
        )?;
    }
    Some(())
}

fn patch_compact(payload: &mut [u8], body_start: usize, offset: u64, values: &[f64]) -> Option<()> {
    let carrier = crate::brep::parse_carrier(payload.get(body_start..)?, offset as usize)?;
    let start = body_start.checked_add(carrier.end.checked_sub(values.len() * 8)?)?;
    for (index, value) in values.iter().enumerate() {
        payload
            .get_mut(start + index * 8..start + (index + 1) * 8)?
            .copy_from_slice(&value.to_be_bytes());
    }
    Some(())
}
