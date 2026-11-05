// SPDX-License-Identifier: Apache-2.0
//! Reconnect current edge polylines to emitted face triangulations.

use super::polygon_sources::PolygonSources;
use crate::brep::{PolygonOnTriangulationRecord, TextEdgeRepresentation, TextTShapeGeometry};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::topology::Edge;
use std::collections::{HashMap, HashSet};

pub(super) fn representation(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    edge: &Edge,
    sources: &PolygonSources,
    face_rows: &HashMap<String, usize>,
    covered: &HashSet<String>,
    rows: &mut Vec<String>,
    source_rows: &HashMap<(String, usize), usize>,
) -> Result<Option<String>, CodecError> {
    macro_rules! required {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let curve = required!(edge.curve());
    let curve = required!(ir.model.curves.iter().find(|record| record.id == *curve));
    let SolvedCurveGeometry::Polyline(polyline) = required!(curve.geometry.solved()) else {
        return Ok(None);
    };
    let parameters = if let Some(parameters) = polyline.parameters() {
        let mut values = Vec::new();
        ctx.reserve_working(
            &mut values,
            polyline.point_count(),
            "FreeCAD polygon parameters",
        )?;
        values.extend(parameters);
        Some(values)
    } else {
        None
    };
    let range = parameters
        .as_ref()
        .and_then(|values| Some([values.first()?.get(), values.last()?.get()]));
    if edge.param_range().map(cadmpeg_ir::units::FiniteVector::get) != range {
        return Ok(None);
    }
    let native = required!(edge.native_ref.as_deref());
    let (payload, source) = required!(sources.shape(ctx, native)?);
    let TextTShapeGeometry::Edge {
        representations, ..
    } = &source.geometry
    else {
        return Ok(None);
    };
    let (polygon, triangulation) =
        required!(representations.iter().find_map(|record| match record {
            TextEdgeRepresentation::PolygonOnTriangulation {
                polygon,
                triangulation,
                ..
            } => Some((*polygon, *triangulation)),
            _ => None,
        }));
    let tables = required!(sources.tables(payload));
    let original = required!(polygon
        .checked_sub(1)
        .and_then(|index| tables.polygons_on_triangulations.get(index)));
    let mut points = Vec::new();
    ctx.reserve_working(
        &mut points,
        polyline.point_count(),
        "FreeCAD polygon points",
    )?;
    points.extend(polyline.points());
    'face: for coedge in ir.model.coedges.iter().filter(|use_| use_.edge == edge.id) {
        let Some(wire) = ir
            .model
            .loops
            .iter()
            .find(|wire| wire.id == coedge.owner_loop)
        else {
            continue;
        };
        let Some(face) = ir.model.faces.iter().find(|face| face.id == wire.face) else {
            continue;
        };
        let Some(row) = face_rows.get(face.id.as_str()) else {
            continue;
        };
        let Some(native_face) = face.native_ref.as_deref() else {
            continue;
        };
        let Some((face_payload, native_face)) = sources.shape(ctx, native_face)? else {
            continue;
        };
        if face_payload.id != payload.id {
            continue;
        }
        let TextTShapeGeometry::Face {
            triangulation: Some(native),
            ..
        } = &native_face.geometry
        else {
            continue;
        };
        if native.index() != triangulation {
            continue;
        }
        let Some(mesh) = ir.model.tessellations.iter().find(|mesh| {
            mesh.faces.contains(&face.id)
                && !mesh.source_object.as_ref().is_some_and(|source| {
                    source.format.as_str() == "fcstd" && covered.contains(source.object_id.as_str())
                })
        }) else {
            continue;
        };
        let vertices = mesh.vertices();
        let unchanged = original.nodes.len() == points.len()
            && original.nodes.iter().zip(&points).all(|(node, point)| {
                usize::try_from(*node)
                    .ok()
                    .and_then(|index| index.checked_sub(1))
                    .and_then(|index| vertices.get(index))
                    .is_some_and(|vertex| vertex == point)
            });
        let mut nodes = Vec::new();
        ctx.reserve_working(&mut nodes, points.len(), "FreeCAD indexed polygon nodes")?;
        if unchanged {
            nodes.extend_from_slice(&original.nodes);
        } else {
            let key = |point: cadmpeg_ir::features::FinitePoint3| {
                let point = point.get();
                [point.x, point.y, point.z].map(
                    |value| {
                        if value == 0.0 {
                            0
                        } else {
                            value.to_bits()
                        }
                    },
                )
            };
            let mut indexes = Vec::new();
            ctx.reserve_working(
                &mut indexes,
                vertices.len(),
                "FreeCAD triangulation point index",
            )?;
            for (index, vertex) in vertices.iter().copied().enumerate() {
                let Some(index) = u32::try_from(index)
                    .ok()
                    .and_then(|index| index.checked_add(1))
                else {
                    return Ok(None);
                };
                indexes.push((key(vertex), index));
            }
            indexes.sort_unstable_by_key(|row| row.0);
            for (position, point) in points.iter().enumerate() {
                let node = original.nodes.get(position).copied();
                let retained = node
                    .and_then(|node| usize::try_from(node).ok())
                    .and_then(|index| index.checked_sub(1))
                    .and_then(|index| vertices.get(index))
                    .is_some_and(|vertex| vertex == point);
                if let Some(node) = node.filter(|_| retained) {
                    nodes.push(node);
                    continue;
                }
                let target = key(*point);
                let position = indexes.partition_point(|row| row.0 < target);
                let Some((_, index)) = indexes.get(position).filter(|row| row.0 == target) else {
                    continue 'face;
                };
                if indexes.get(position + 1).is_some_and(|row| row.0 == target) {
                    continue 'face;
                }
                nodes.push(*index);
            }
        }
        let output = PolygonOnTriangulationRecord {
            id: curve.id.to_string(),
            nodes,
            deflection: polyline.chordal_deflection(),
            parameters,
        };
        let value = crate::brep::polygon_on_triangulation_text(&output);
        let original_row = source_rows
            .get(&(payload.id.clone(), polygon))
            .copied()
            .filter(|index| index.checked_sub(1).and_then(|index| rows.get(index)) == Some(&value));
        let polygon = if let Some(index) = original_row {
            index
        } else {
            ctx.reserve_working(rows, 1, "FreeCAD indexed polygon table")?;
            rows.push(value);
            rows.len()
        };
        return Ok(Some(format!("6 {polygon} {row} 0")));
    }
    Ok(None)
}
