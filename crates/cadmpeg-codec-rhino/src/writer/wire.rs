// SPDX-License-Identifier: Apache-2.0
//! Native Breps for bounded wire edges.

use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::CadIr;

use super::{BrepPayload, WriteError};

pub(super) fn single_line(
    ir: &CadIr,
    version: crate::RhinoArchiveVersion,
) -> Result<Option<(BrepPayload, cadmpeg_ir::ids::CurveId)>, WriteError> {
    let [body] = ir.model.bodies.as_slice() else {
        return Ok(None);
    };
    let [region] = ir.model.regions.as_slice() else {
        return Ok(None);
    };
    let [shell] = ir.model.shells.as_slice() else {
        return Ok(None);
    };
    if region.body != body.id
        || region.shells.as_slice() != [shell.id.clone()]
        || shell.region != region.id
        || !shell.faces().is_empty()
        || !shell.free_vertices().is_empty()
        || !ir.model.faces.is_empty()
    {
        return Ok(None);
    }
    let [edge_id] = shell.wire_edges() else {
        return Ok(None);
    };
    let [edge] = ir.model.edges.as_slice() else {
        return Ok(None);
    };
    if edge.id != *edge_id {
        return Ok(None);
    }
    if edge.start == edge.end {
        return Ok(None);
    }
    let Some(curve_id) = edge.curve() else {
        return Ok(None);
    };
    let Some(curve) = ir.model.curves.iter().find(|curve| curve.id == *curve_id) else {
        return Ok(None);
    };
    if curve.geometry.procedural_construction().is_some() {
        return Ok(None);
    }
    let Some(SolvedCurveGeometry::Line(line)) = curve.geometry.solved() else {
        return Ok(None);
    };
    let Some(range) = edge.param_range() else {
        return Ok(None);
    };
    let [lo, hi] = range.get();
    if lo >= hi {
        return Ok(None);
    }
    let mut vertices = Vec::new();
    let mut positions = Vec::new();
    let mut tolerances = Vec::new();
    for (index, id) in [&edge.start, &edge.end].into_iter().enumerate() {
        let Some(vertex) = ir.model.vertices.iter().find(|vertex| vertex.id == *id) else {
            return Ok(None);
        };
        let Some(point) = ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
        else {
            return Ok(None);
        };
        let position = point.position().get();
        positions.push(position);
        tolerances.push(
            match super::stated_write_tolerance(
                edge.tolerance.or(vertex.tolerance),
                ir,
                edge.id.as_str(),
            ) {
                Ok(value) => value,
                Err(WriteError::Omit(_)) => return Ok(None),
                Err(error) => return Err(error),
            },
        );
        let mut record = super::wire_index(index)?.to_le_bytes().to_vec();
        for value in [position.x, position.y, position.z] {
            record.extend(value.to_le_bytes());
        }
        record.extend(super::indexes(&[0])?);
        record.extend(
            vertex
                .tolerance
                .map_or(0.0, |value| value.get())
                .to_le_bytes(),
        );
        vertices.push(record);
    }
    let origin = line.origin().get();
    let direction = *line.direction().as_raw();
    let endpoints = [
        origin.translated(direction, lo),
        origin.translated(direction, hi),
    ];
    if !super::close_point(positions[0], endpoints[0], tolerances[0])
        || !super::close_point(positions[1], endpoints[1], tolerances[1])
    {
        return Ok(None);
    }
    let (carrier, conversion) = match super::loose::curve_payload(ir, edge) {
        Ok(value) => value,
        Err(WriteError::Omit(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if conversion.is_some() {
        return Ok(None);
    }
    let mut edge_record = 0_i32.to_le_bytes().to_vec();
    edge_record.extend(0_i32.to_le_bytes());
    edge_record.extend(0_i32.to_le_bytes());
    for value in [lo, hi] {
        edge_record.extend(value.to_le_bytes());
    }
    edge_record.extend(0_i32.to_le_bytes());
    edge_record.extend(1_i32.to_le_bytes());
    edge_record.extend(super::indexes(&[])?);
    edge_record.extend(
        edge.tolerance
            .map_or(0.0, |value| value.get())
            .to_le_bytes(),
    );
    for value in [lo, hi] {
        edge_record.extend(value.to_le_bytes());
    }
    let version_byte = if version.uses_extended_brep_layout() {
        0x33
    } else {
        0x32
    };
    let mut payload = vec![version_byte];
    let mut direct = vec![version_byte];
    payload.extend(super::polymorphic_array(std::iter::empty())?);
    payload.extend(super::polymorphic_array(std::iter::once(&carrier))?);
    payload.extend(super::polymorphic_array(std::iter::empty())?);
    payload.extend(super::raw_array(&vertices)?);
    payload.extend(super::raw_array(&[edge_record])?);
    payload.extend(super::raw_array(&[])?);
    payload.extend(super::raw_array(&[])?);
    payload.extend(super::face_array(&[], &[], version)?);
    let a = positions[0];
    let b = positions[1];
    let [c, d] = endpoints;
    for value in [
        a.x.min(b.x).min(c.x).min(d.x),
        a.y.min(b.y).min(c.y).min(d.y),
        a.z.min(b.z).min(c.z).min(d.z),
        a.x.max(b.x).max(c.x).max(d.x),
        a.y.max(b.y).max(c.y).max(d.y),
        a.z.max(b.z).max(c.z).max(d.z),
    ] {
        payload.extend(value.to_le_bytes());
        direct.extend(value.to_le_bytes());
    }
    payload.extend(super::crc_chunk(0x4000_8000, &[])?);
    payload.extend(super::crc_chunk(0x4000_8000, &[])?);
    payload.extend(0_i32.to_le_bytes());
    direct.extend(0_i32.to_le_bytes());
    if version.uses_extended_brep_layout() {
        payload.extend(super::empty_region_wrapper()?);
    }
    Ok(Some((
        BrepPayload {
            body: payload,
            direct,
            record_count: 5,
        },
        curve_id.clone(),
    )))
}
