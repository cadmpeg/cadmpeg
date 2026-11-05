// SPDX-License-Identifier: Apache-2.0
//! Native Breps for bounded wire edges.

use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::CadIr;

use super::{BrepPayload, WriteError};

pub(super) fn single_curve(
    ir: &CadIr,
    version: crate::RhinoArchiveVersion,
) -> Result<
    Option<(
        BrepPayload,
        cadmpeg_ir::ids::CurveId,
        Option<cadmpeg_ir::report::loss::LossNote>,
    )>,
    WriteError,
> {
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
    let Some(geometry) = curve.geometry.solved() else {
        return Ok(None);
    };
    if !matches!(
        geometry,
        SolvedCurveGeometry::Line(_)
            | SolvedCurveGeometry::Circle(_)
            | SolvedCurveGeometry::Nurbs(_)
    ) {
        return Ok(None);
    }
    let Some(range) = edge.param_range() else {
        return Ok(None);
    };
    let [lo, hi] = range.get();
    if lo >= hi {
        return Ok(None);
    }
    let mut vertices = Vec::new();
    let mut positions = Vec::new();
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
    let (carrier, conversion) = match super::loose::curve_payload(ir, edge) {
        Ok(value) => value,
        Err(WriteError::Omit(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
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
    let mut minimum = [positions[0].x, positions[0].y, positions[0].z];
    let mut maximum = minimum;
    let mut include = |point: [f64; 3]| {
        if point.iter().any(|value| !value.is_finite()) {
            return false;
        }
        for i in 0..3 {
            minimum[i] = minimum[i].min(point[i]);
            maximum[i] = maximum[i].max(point[i]);
        }
        true
    };
    if !include([positions[1].x, positions[1].y, positions[1].z]) {
        return Ok(None);
    }
    match geometry {
        SolvedCurveGeometry::Line(line) => {
            let origin = line.origin().get();
            let direction = *line.direction().as_raw();
            let start = origin.translated(direction, lo);
            let end = origin.translated(direction, hi);
            if !include([start.x, start.y, start.z]) || !include([end.x, end.y, end.z]) {
                return Ok(None);
            }
        }
        SolvedCurveGeometry::Circle(circle) => {
            let center = circle.center().get();
            let radius = circle.radius().get();
            if !include([center.x - radius, center.y - radius, center.z - radius])
                || !include([center.x + radius, center.y + radius, center.z + radius])
            {
                return Ok(None);
            }
        }
        SolvedCurveGeometry::Nurbs(nurbs) => {
            if let Some(weights) = nurbs.weights() {
                if let Some(first) = weights.first() {
                    if weights.iter().any(|weight| {
                        weight.get().is_sign_negative() != first.get().is_sign_negative()
                    }) {
                        return Ok(None);
                    }
                }
            }
            for control in nurbs.control_points() {
                let point = control.get();
                if !include([point.x, point.y, point.z]) {
                    return Ok(None);
                }
            }
        }
        _ => return Ok(None),
    }
    for value in minimum.into_iter().chain(maximum) {
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
        conversion,
    )))
}
pub(super) fn vertex_body(
    ir: &CadIr,
    version: crate::RhinoArchiveVersion,
) -> Result<Option<(BrepPayload, Vec<String>)>, WriteError> {
    let mut vertices = Vec::new();
    let mut carried = Vec::new();
    let mut bounds: Option<([f64; 3], [f64; 3])> = None;
    for vertex in &ir.model.vertices {
        let Some(point) = ir
            .model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
        else {
            continue;
        };
        let position = point.position().get();
        let coordinates = [position.x, position.y, position.z];
        let (minimum, maximum) = bounds.get_or_insert((coordinates, coordinates));
        for i in 0..3 {
            minimum[i] = minimum[i].min(coordinates[i]);
            maximum[i] = maximum[i].max(coordinates[i]);
        }
        let mut record = super::wire_index(vertices.len())?.to_le_bytes().to_vec();
        for value in coordinates {
            record.extend(value.to_le_bytes());
        }
        record.extend(super::indexes(&[])?);
        record.extend(
            vertex
                .tolerance
                .map_or(0.0, |value| value.get())
                .to_le_bytes(),
        );
        vertices.push(record);
        carried.push(vertex.id.to_string());
    }
    let Some((minimum, maximum)) = bounds else {
        return Ok(None);
    };
    let version_byte = if version.uses_extended_brep_layout() {
        0x33
    } else {
        0x32
    };
    let mut payload = vec![version_byte];
    let mut direct = vec![version_byte];
    payload.extend(super::polymorphic_array(std::iter::empty())?);
    payload.extend(super::polymorphic_array(std::iter::empty())?);
    payload.extend(super::polymorphic_array(std::iter::empty())?);
    payload.extend(super::raw_array(&vertices)?);
    payload.extend(super::raw_array(&[])?);
    payload.extend(super::raw_array(&[])?);
    payload.extend(super::raw_array(&[])?);
    payload.extend(super::face_array(&[], &[], version)?);
    for value in minimum.into_iter().chain(maximum) {
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
            record_count: 1 + cadmpeg_core::decode::u64_from_index(vertices.len()),
        },
        carried,
    )))
}
