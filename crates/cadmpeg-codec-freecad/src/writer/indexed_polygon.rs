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

pub(super) fn representation<'a>(
    ctx: &DecodeContext<'_>,
    ir: &'a CadIr,
    edge: &Edge,
    sources: &PolygonSources,
    face_rows: &HashMap<String, usize>,
    covered: &HashSet<String>,
    rows: &mut Vec<String>,
    source_rows: &HashMap<(String, usize), usize>,
) -> Result<Option<(String, Option<&'a str>)>, CodecError> {
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
    let (polygon, secondary, triangulation) =
        required!(representations.iter().find_map(|record| match record {
            TextEdgeRepresentation::PolygonOnTriangulation {
                polygon,
                triangulation,
                ..
            } => Some((*polygon, None, *triangulation)),
            TextEdgeRepresentation::PolygonPair {
                polygons,
                triangulation,
                ..
            } => Some((polygons[0], Some(polygons[1]), *triangulation)),
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
    for coedge in ir.model.coedges.iter().filter(|use_| use_.edge == edge.id) {
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
        let Some(nodes) = super::polygon_nodes::remap(ctx, &original.nodes, &points, &vertices)?
        else {
            continue;
        };
        let secondary = if let Some(secondary) = secondary {
            let Some(original_secondary) = secondary
                .checked_sub(1)
                .and_then(|index| tables.polygons_on_triangulations.get(index))
            else {
                continue;
            };
            let Some(source_mesh) = triangulation
                .checked_sub(1)
                .and_then(|index| tables.triangulations.get(index))
            else {
                continue;
            };
            let source_row = source_rows.get(&(payload.id.clone(), secondary)).copied();
            let Some(secondary) = super::polygon_pair::secondary(
                ctx,
                ir,
                curve,
                original_secondary,
                source_mesh.nodes(),
                &vertices,
                rows,
                source_row,
            )?
            else {
                continue;
            };
            Some(secondary)
        } else {
            None
        };
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
        return Ok(Some(match secondary {
            Some((secondary, curve)) => (format!("7 {polygon} {secondary} {row} 0"), Some(curve)),
            None => (format!("6 {polygon} {row} 0"), None),
        }));
    }
    Ok(None)
}
