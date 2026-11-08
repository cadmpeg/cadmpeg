// SPDX-License-Identifier: Apache-2.0
//! Native Breps for bounded wire edges.

use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::CadIr;

use super::{BrepPayload, WriteError};

/// One wire edge of a [`wire_brep`] and the carrier it writes.
pub(super) struct WireEdge<'a> {
    pub(super) edge: &'a cadmpeg_ir::topology::Edge,
    pub(super) curve: &'a cadmpeg_ir::geometry::Curve,
    pub(super) conversion: Option<cadmpeg_ir::report::loss::LossNote>,
}

/// A wire body written as a faceless 3DM Brep: every wire edge of its one
/// shell becomes a Brep edge over shared Brep vertices.
pub(super) fn wire_brep(
    ir: &CadIr,
    version: crate::RhinoArchiveVersion,
) -> Result<Option<(BrepPayload, Vec<WireEdge<'_>>)>, WriteError> {
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
        || shell.wire_edges().is_empty()
        || shell.wire_edges().len() != ir.model.edges.len()
    {
        return Ok(None);
    }
    let mut vertex_ids: Vec<&cadmpeg_ir::ids::VertexId> = Vec::new();
    let mut vertex_edges: Vec<Vec<i32>> = Vec::new();
    let mut written = Vec::new();
    let mut carriers = Vec::new();
    let mut edge_records = Vec::new();
    let mut bounds = Bounds::default();
    for (index, edge_id) in shell.wire_edges().iter().enumerate() {
        let Some(edge) = ir.model.edges.iter().find(|edge| edge.id == *edge_id) else {
            return Ok(None);
        };
        let Some(curve) = edge
            .curve()
            .and_then(|id| ir.model.curves.iter().find(|curve| curve.id == *id))
        else {
            return Ok(None);
        };
        if curve.geometry.procedural_construction().is_some() {
            return Ok(None);
        }
        let Some(geometry) = curve.geometry.solved() else {
            return Ok(None);
        };
        let Some(range) = edge.param_range() else {
            return Ok(None);
        };
        let [lo, hi] = range.get();
        if lo >= hi || !bounds.include_curve(geometry, [lo, hi]) {
            return Ok(None);
        }
        let mut ends = [0_usize; 2];
        for (end, id) in ends.iter_mut().zip([&edge.start, &edge.end]) {
            let slot = match vertex_ids.iter().position(|known| *known == id) {
                Some(slot) => slot,
                None => {
                    vertex_ids.push(id);
                    vertex_edges.push(Vec::new());
                    vertex_ids.len() - 1
                }
            };
            vertex_edges[slot].push(super::wire_index(index)?);
            *end = slot;
        }
        let (carrier, conversion) = match super::loose::curve_payload(ir, edge) {
            Ok(value) => value,
            Err(WriteError::Omit(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut record = super::wire_index(index)?.to_le_bytes().to_vec();
        record.extend(super::wire_index(index)?.to_le_bytes());
        record.extend(0_i32.to_le_bytes());
        for value in [lo, hi] {
            record.extend(value.to_le_bytes());
        }
        for end in ends {
            record.extend(super::wire_index(end)?.to_le_bytes());
        }
        record.extend(super::indexes(&[])?);
        record.extend(
            edge.tolerance
                .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get)
                .to_le_bytes(),
        );
        for value in [lo, hi] {
            record.extend(value.to_le_bytes());
        }
        edge_records.push(record);
        carriers.push(carrier);
        written.push(WireEdge {
            edge,
            curve,
            conversion,
        });
    }
    let mut vertices = Vec::new();
    for (index, (id, edges)) in vertex_ids.iter().zip(&vertex_edges).enumerate() {
        let Some(vertex) = ir.model.vertices.iter().find(|vertex| vertex.id == **id) else {
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
        if !bounds.include([position.x, position.y, position.z]) {
            return Ok(None);
        }
        let mut record = super::wire_index(index)?.to_le_bytes().to_vec();
        for value in [position.x, position.y, position.z] {
            record.extend(value.to_le_bytes());
        }
        record.extend(super::indexes(edges)?);
        record.extend(
            vertex
                .tolerance
                .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get)
                .to_le_bytes(),
        );
        vertices.push(record);
    }
    let Some((minimum, maximum)) = bounds.extent else {
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
    payload.extend(super::polymorphic_array(carriers.iter())?);
    payload.extend(super::polymorphic_array(std::iter::empty())?);
    payload.extend(super::raw_array(&vertices)?);
    payload.extend(super::raw_array(&edge_records)?);
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
            record_count: cadmpeg_core::decode::u64_from_index(2 * written.len() + 3),
        },
        written,
    )))
}

/// The bounding box of a wire Brep's vertices and carriers.
#[derive(Default)]
struct Bounds {
    extent: Option<([f64; 3], [f64; 3])>,
}

impl Bounds {
    fn include(&mut self, point: [f64; 3]) -> bool {
        if point.iter().any(|value| !value.is_finite()) {
            return false;
        }
        let (minimum, maximum) = self.extent.get_or_insert((point, point));
        for ((low, high), value) in minimum.iter_mut().zip(maximum.iter_mut()).zip(point) {
            *low = low.min(value);
            *high = high.max(value);
        }
        true
    }

    fn include_curve(&mut self, geometry: &SolvedCurveGeometry, [lo, hi]: [f64; 2]) -> bool {
        match geometry {
            SolvedCurveGeometry::Line(line) => {
                let origin = line.origin().get();
                let direction = *line.direction().as_raw();
                let start = origin.translated(direction, lo);
                let end = origin.translated(direction, hi);
                self.include([start.x, start.y, start.z]) && self.include([end.x, end.y, end.z])
            }
            SolvedCurveGeometry::Circle(circle) => {
                let center = circle.center().get();
                let radius = circle.radius().get();
                self.include([center.x - radius, center.y - radius, center.z - radius])
                    && self.include([center.x + radius, center.y + radius, center.z + radius])
            }
            SolvedCurveGeometry::Nurbs(nurbs) => {
                let mixed = nurbs.weights().is_some_and(|weights| {
                    weights.first().is_some_and(|first| {
                        weights.iter().any(|weight| {
                            weight.get().is_sign_negative() != first.get().is_sign_negative()
                        })
                    })
                });
                !mixed
                    && nurbs.control_points().iter().all(|control| {
                        let point = control.get();
                        self.include([point.x, point.y, point.z])
                    })
            }
            _ => false,
        }
    }
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
                .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get)
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
