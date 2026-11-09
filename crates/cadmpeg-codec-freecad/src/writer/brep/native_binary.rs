// SPDX-License-Identifier: Apache-2.0
//! A typed binary shape payload written back in its own OCCT grammar (W1).
//!
//! The binary grammar holds the tables of the text grammar with typed
//! fields: section headers stay text lines, flags are bytes, degrees are
//! 16-bit, counts and table references are 32-bit, and reals are 64-bit,
//! all little-endian. Geometry rows are restated from their text form.

use super::native_text;
use crate::brep::{
    Polygon3dRecord, PolygonOnTriangulationRecord, ShapePayload, ShapePayloadRecord, Tables,
    TextEdgeRepresentation, TextOrientation, TextPointRepresentation, TextShapeKind, TextShapeUse,
    TextTShapeGeometry,
};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::units::FinitePoint2;

/// The OCCT binary bytes of one typed payload, or `None` for a text or
/// empty carrier or a table the grammar cannot restate exactly.
pub(super) fn bytes(
    payload: &ShapePayloadRecord,
    polygons3d: &[Polygon3dRecord],
    polygons_on_triangulations: &[PolygonOnTriangulationRecord],
    root: Option<cadmpeg_ir::transform::Transform>,
) -> Option<Vec<u8>> {
    let ShapePayload::Binary { .. } = &payload.payload else {
        return None;
    };
    let version = payload.payload.topology_version()?;
    let tables = Tables::from_payload(payload, polygons3d, polygons_on_triangulations)
        .ok()
        .flatten()?;
    let mut out = Out(Vec::new());
    out.line(&format!("\nOpen CASCADE Topology V{version} (c)"));
    out.line(&format!(
        "Locations {}",
        tables.locations.len() + usize::from(root.is_some())
    ));
    for location in tables.locations {
        if location.factors.is_empty() {
            out.u8(1);
            out.rows(location.transform);
        } else {
            out.u8(2);
            for factor in &location.factors {
                out.index(factor.location())?;
                out.i32(i32::try_from(factor.power()).ok()?);
            }
            out.i32(0);
        }
    }
    if let Some(transform) = root {
        out.u8(1);
        out.rows(transform);
    }
    out.line(&format!("Curve2ds {}", tables.curve2ds.len()));
    for curve in tables.curve2ds {
        out.geometry(&native_text::curve2d(curve)?, Row::Curve2d)?;
    }
    out.line(&format!("Curves {}", tables.curves.len()));
    for curve in tables.curves {
        out.geometry(&native_text::curve(curve)?, Row::Curve)?;
    }
    out.line(&format!("Polygon3D {}", tables.polygons3d.len()));
    for polygon in &tables.polygons3d {
        out.index(polygon.nodes.len())?;
        out.flag(polygon.parameters.is_some());
        out.f64(polygon.deflection.get());
        for node in &polygon.nodes {
            out.point3(*node);
        }
        for parameter in polygon.parameters.iter().flatten() {
            out.f64(parameter.get());
        }
    }
    out.line(&format!(
        "PolygonOnTriangulations {}",
        tables.polygons_on_triangulations.len()
    ));
    for polygon in &tables.polygons_on_triangulations {
        out.index(polygon.nodes.len())?;
        for node in &polygon.nodes {
            out.index(usize::try_from(*node).ok()?)?;
        }
        out.f64(polygon.deflection.get());
        out.flag(polygon.parameters.is_some());
        for parameter in polygon.parameters.iter().flatten() {
            out.f64(parameter.get());
        }
    }
    out.line(&format!("Surfaces {}", tables.surfaces.len()));
    for surface in tables.surfaces {
        out.geometry(&native_text::surface(surface)?, Row::Surface)?;
    }
    out.line(&format!("Triangulations {}", tables.triangulations.len()));
    for triangulation in tables.triangulations {
        let nodes = triangulation.nodes();
        let uv = triangulation.uv_nodes();
        let normals = triangulation.normals();
        out.index(nodes.len())?;
        out.index(triangulation.triangles().len())?;
        out.flag(uv.is_some());
        if version >= 4 {
            out.flag(normals.is_some());
        } else if normals.is_some() {
            return None;
        }
        out.f64(triangulation.deflection.get());
        for node in nodes {
            out.point3(*node);
        }
        for node in uv.into_iter().flatten() {
            out.point2(*node);
        }
        // Triangles hold zero-based node positions; the grammar is one-based.
        for triangle in triangulation.triangles() {
            for node in triangle {
                out.i32(i32::try_from(*node).ok()?.checked_add(1)?);
            }
        }
        for normal in normals.into_iter().flatten() {
            let normal = normal.get();
            for value in [normal.x, normal.y, normal.z] {
                let value = cadmpeg_core::convert::f32_from_f64(value)?;
                out.0.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    let count = tables.tshapes.len();
    out.line(&format!("TShapes {count}"));
    for shape in tables.tshapes.iter() {
        out.u8(kind(shape.kind()));
        tshape_geometry(
            &mut out,
            &shape.geometry,
            version,
            tables.triangulations.is_empty(),
        )?;
        for flag in shape.flags {
            out.flag(flag);
        }
        for child in &shape.children {
            out.u8(orientation(child.orientation));
            out.index(count.checked_sub(child.shape)?.checked_add(1)?)?;
            out.index(usize::from(child.location))?;
        }
        out.u8(b'*');
    }
    match tables.roots {
        [] => {}
        [use_] => {
            let location = root.map_or(usize::from(use_.location), |_| tables.locations.len() + 1);
            root_use(&mut out, use_, location)?;
        }
        _ => return None,
    }
    Some(out.0)
}

/// A root names its shape by table position, then its location and
/// orientation.
fn root_use(out: &mut Out, value: &TextShapeUse, location: usize) -> Option<()> {
    out.index(value.shape)?;
    out.index(location)?;
    out.i32(i32::from(orientation(value.orientation)));
    Some(())
}

const fn kind(kind: TextShapeKind) -> u8 {
    match kind {
        TextShapeKind::Compound => 0,
        TextShapeKind::CompSolid => 1,
        TextShapeKind::Solid => 2,
        TextShapeKind::Shell => 3,
        TextShapeKind::Face => 4,
        TextShapeKind::Wire => 5,
        TextShapeKind::Edge => 6,
        TextShapeKind::Vertex => 7,
    }
}

const fn orientation(orientation: TextOrientation) -> u8 {
    match orientation {
        TextOrientation::Forward => 0,
        TextOrientation::Reversed => 1,
        TextOrientation::Internal => 2,
        TextOrientation::External => 3,
    }
}

fn tshape_geometry(
    out: &mut Out,
    geometry: &TextTShapeGeometry,
    version: u8,
    untriangulated: bool,
) -> Option<()> {
    match geometry {
        TextTShapeGeometry::Vertex {
            tolerance,
            point,
            representations,
        } => {
            out.f64(tolerance.get());
            out.point3(*point);
            for representation in representations {
                match representation {
                    TextPointRepresentation::Curve3d {
                        parameter,
                        curve,
                        location,
                    } => {
                        out.u8(1);
                        out.f64(parameter.get());
                        out.index(*curve)?;
                        out.index(*location)?;
                    }
                    TextPointRepresentation::Pcurve {
                        parameter,
                        curve,
                        surface,
                        location,
                    } => {
                        out.u8(2);
                        out.f64(parameter.get());
                        out.index(*curve)?;
                        out.index(*surface)?;
                        out.index(*location)?;
                    }
                    TextPointRepresentation::Surface {
                        parameter,
                        second_parameter,
                        surface,
                        location,
                    } => {
                        out.u8(3);
                        out.f64(parameter.get());
                        out.f64(second_parameter.get());
                        out.index(*surface)?;
                        out.index(*location)?;
                    }
                }
            }
            out.u8(0);
        }
        TextTShapeGeometry::Edge {
            tolerance,
            same_parameter,
            same_range,
            degenerated,
            representations,
        } => {
            out.f64(tolerance.get());
            out.flag(*same_parameter);
            out.flag(*same_range);
            out.flag(*degenerated);
            for representation in representations {
                edge_representation(out, representation, version)?;
            }
            out.u8(0);
        }
        TextTShapeGeometry::Face {
            natural_restriction,
            tolerance,
            surface,
            location,
            triangulation,
        } => {
            out.flag(*natural_restriction);
            out.f64(tolerance.get());
            out.index(surface.map_or(0, crate::brep::TableRef::index))?;
            out.index(usize::from(*location))?;
            // Marker 2 precedes a triangulation reference; 1 states that the
            // face has none and 0 that the set holds no triangulations.
            match triangulation {
                Some(triangulation) => {
                    out.u8(2);
                    out.index(triangulation.index())?;
                }
                None => out.u8(u8::from(!untriangulated)),
            }
        }
        TextTShapeGeometry::Wire
        | TextTShapeGeometry::Shell
        | TextTShapeGeometry::Solid
        | TextTShapeGeometry::CompSolid
        | TextTShapeGeometry::Compound => {}
    }
    Some(())
}

fn edge_representation(
    out: &mut Out,
    representation: &TextEdgeRepresentation,
    version: u8,
) -> Option<()> {
    let uv = |out: &mut Out, endpoints: &Option<[FinitePoint2; 2]>| -> Option<()> {
        match (matches!(version, 2 | 3), endpoints) {
            (true, Some([first, last])) => {
                out.point2(*first);
                out.point2(*last);
                Some(())
            }
            (false, None) => Some(()),
            _ => None,
        }
    };
    let continuity = |text: &str| text.parse::<u8>().ok();
    match representation {
        TextEdgeRepresentation::Curve3d {
            curve,
            location,
            parameter_range,
        } => {
            out.u8(1);
            out.index(*curve)?;
            out.index(*location)?;
            out.f64(parameter_range[0].get());
            out.f64(parameter_range[1].get());
        }
        TextEdgeRepresentation::Pcurve {
            curve,
            surface,
            location,
            parameter_range,
            uv_endpoints,
        } => {
            out.u8(2);
            out.index(*curve)?;
            out.index(*surface)?;
            out.index(*location)?;
            out.f64(parameter_range[0].get());
            out.f64(parameter_range[1].get());
            uv(out, uv_endpoints)?;
        }
        TextEdgeRepresentation::PcurvePair {
            curves,
            continuity: order,
            surface,
            location,
            parameter_range,
            uv_endpoints,
        } => {
            out.u8(3);
            out.index(curves[0])?;
            out.index(curves[1])?;
            out.u8(continuity(order)?);
            out.index(*surface)?;
            out.index(*location)?;
            out.f64(parameter_range[0].get());
            out.f64(parameter_range[1].get());
            uv(out, uv_endpoints)?;
        }
        TextEdgeRepresentation::Regularity {
            continuity: order,
            surfaces,
            locations,
        } => {
            out.u8(4);
            out.u8(continuity(order)?);
            out.index(surfaces[0])?;
            out.index(locations[0])?;
            out.index(surfaces[1])?;
            out.index(locations[1])?;
        }
        TextEdgeRepresentation::Polygon3d { polygon, location } => {
            out.u8(5);
            out.index(*polygon)?;
            out.index(*location)?;
        }
        TextEdgeRepresentation::PolygonOnTriangulation {
            polygon,
            triangulation,
            location,
        } => {
            out.u8(6);
            out.index(*polygon)?;
            out.index(*triangulation)?;
            out.index(*location)?;
        }
        TextEdgeRepresentation::PolygonPair {
            polygons,
            triangulation,
            location,
        } => {
            out.u8(7);
            out.index(polygons[0])?;
            out.index(polygons[1])?;
            out.index(*triangulation)?;
            out.index(*location)?;
        }
    }
    Some(())
}

/// The table a geometry row belongs to.
#[derive(Clone, Copy)]
enum Row {
    Curve2d,
    Curve,
    Surface,
}

impl Row {
    /// The coordinates of one pole.
    const fn dimension(self) -> usize {
        match self {
            Self::Curve2d => 2,
            Self::Curve | Self::Surface => 3,
        }
    }
}

/// Output bytes.
struct Out(Vec<u8>);

impl Out {
    fn line(&mut self, text: &str) {
        self.0.extend_from_slice(text.as_bytes());
        self.0.push(b'\n');
    }

    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn flag(&mut self, value: bool) {
        self.0.push(u8::from(value));
    }

    fn u16(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn index(&mut self, value: usize) -> Option<()> {
        self.i32(i32::try_from(value).ok()?);
        Some(())
    }

    fn f64(&mut self, value: f64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn point2(&mut self, point: FinitePoint2) {
        let point = point.get();
        self.f64(point.u);
        self.f64(point.v);
    }

    fn point3(&mut self, point: FinitePoint3) {
        let point = point.get();
        self.f64(point.x);
        self.f64(point.y);
        self.f64(point.z);
    }

    fn rows(&mut self, transform: cadmpeg_ir::transform::Transform) {
        for row in transform.affine_rows() {
            for value in row {
                self.f64(value);
            }
        }
    }

    /// Restates one text geometry row, whose tokens hold the binary fields
    /// in order.
    fn geometry(&mut self, text: &str, row: Row) -> Option<()> {
        let mut tokens = text.split_ascii_whitespace();
        self.record(&mut tokens, row)?;
        tokens.next().is_none().then_some(())
    }

    fn record<'a>(&mut self, tokens: &mut impl Iterator<Item = &'a str>, row: Row) -> Option<()> {
        let code = tokens.next()?.parse::<u8>().ok()?;
        self.u8(code);
        let reals = |out: &mut Self, tokens: &mut dyn Iterator<Item = &'a str>, count: usize| {
            for _ in 0..count {
                out.f64(tokens.next()?.parse().ok()?);
            }
            Some(())
        };
        match (row, code) {
            (Row::Curve2d, 1) => reals(self, tokens, 4),
            (Row::Curve2d, 2 | 4) => reals(self, tokens, 7),
            (Row::Curve2d, 3 | 5) => reals(self, tokens, 8),
            (Row::Curve, 1) => reals(self, tokens, 6),
            (Row::Curve | Row::Surface, 2 | 4) => reals(self, tokens, 13),
            (Row::Curve | Row::Surface, 3 | 5) => reals(self, tokens, 14),
            (Row::Surface, 1) => reals(self, tokens, 12),
            (Row::Curve2d | Row::Curve, 6) => self.bezier(tokens, row.dimension()),
            (Row::Curve2d | Row::Curve, 7) => self.bspline(tokens, row.dimension()),
            (Row::Curve2d | Row::Curve, 8) => {
                reals(self, tokens, 2)?;
                self.record(tokens, row)
            }
            (Row::Curve2d, 9) => {
                reals(self, tokens, 1)?;
                self.record(tokens, row)
            }
            (Row::Curve, 9) => {
                reals(self, tokens, 4)?;
                self.record(tokens, row)
            }
            (Row::Surface, 6) => {
                reals(self, tokens, 3)?;
                self.record(tokens, Row::Curve)
            }
            (Row::Surface, 7) => {
                reals(self, tokens, 6)?;
                self.record(tokens, Row::Curve)
            }
            (Row::Surface, 8) => self.bezier_surface(tokens),
            (Row::Surface, 9) => self.bspline_surface(tokens),
            (Row::Surface, 10) => {
                reals(self, tokens, 4)?;
                self.record(tokens, row)
            }
            (Row::Surface, 11) => {
                reals(self, tokens, 1)?;
                self.record(tokens, row)
            }
            _ => None,
        }
    }

    fn token_flag<'a>(&mut self, tokens: &mut impl Iterator<Item = &'a str>) -> Option<bool> {
        let flag = match tokens.next()? {
            "0" => false,
            "1" => true,
            _ => return None,
        };
        self.flag(flag);
        Some(flag)
    }

    fn token_degree<'a>(&mut self, tokens: &mut impl Iterator<Item = &'a str>) -> Option<usize> {
        let degree = tokens.next()?.parse::<u16>().ok()?;
        self.u16(degree);
        Some(usize::from(degree))
    }

    fn token_count<'a>(&mut self, tokens: &mut impl Iterator<Item = &'a str>) -> Option<usize> {
        let count = tokens.next()?.parse::<usize>().ok()?;
        self.index(count)?;
        Some(count)
    }

    fn token_reals<'a>(
        &mut self,
        tokens: &mut impl Iterator<Item = &'a str>,
        count: usize,
    ) -> Option<()> {
        for _ in 0..count {
            self.f64(tokens.next()?.parse().ok()?);
        }
        Some(())
    }

    /// Knot rows: a real knot and its multiplicity.
    fn token_knots<'a>(
        &mut self,
        tokens: &mut impl Iterator<Item = &'a str>,
        count: usize,
    ) -> Option<()> {
        for _ in 0..count {
            self.f64(tokens.next()?.parse().ok()?);
            self.token_count(tokens)?;
        }
        Some(())
    }

    fn bezier<'a>(
        &mut self,
        tokens: &mut impl Iterator<Item = &'a str>,
        dimension: usize,
    ) -> Option<()> {
        let rational = self.token_flag(tokens)?;
        let poles = self.token_degree(tokens)?.checked_add(1)?;
        self.token_reals(tokens, poles * (dimension + usize::from(rational)))
    }

    fn bspline<'a>(
        &mut self,
        tokens: &mut impl Iterator<Item = &'a str>,
        dimension: usize,
    ) -> Option<()> {
        let rational = self.token_flag(tokens)?;
        self.token_flag(tokens)?;
        self.token_degree(tokens)?;
        let poles = self.token_count(tokens)?;
        let knots = self.token_count(tokens)?;
        self.token_reals(
            tokens,
            poles.checked_mul(dimension + usize::from(rational))?,
        )?;
        self.token_knots(tokens, knots)
    }

    fn bezier_surface<'a>(&mut self, tokens: &mut impl Iterator<Item = &'a str>) -> Option<()> {
        let u_rational = self.token_flag(tokens)?;
        let v_rational = self.token_flag(tokens)?;
        let u_poles = self.token_degree(tokens)?.checked_add(1)?;
        let v_poles = self.token_degree(tokens)?.checked_add(1)?;
        let weight = usize::from(u_rational || v_rational);
        self.token_reals(
            tokens,
            u_poles.checked_mul(v_poles)?.checked_mul(3 + weight)?,
        )
    }

    fn bspline_surface<'a>(&mut self, tokens: &mut impl Iterator<Item = &'a str>) -> Option<()> {
        let u_rational = self.token_flag(tokens)?;
        let v_rational = self.token_flag(tokens)?;
        self.token_flag(tokens)?;
        self.token_flag(tokens)?;
        self.token_degree(tokens)?;
        self.token_degree(tokens)?;
        let u_poles = self.token_count(tokens)?;
        let v_poles = self.token_count(tokens)?;
        let u_knots = self.token_count(tokens)?;
        let v_knots = self.token_count(tokens)?;
        let weight = usize::from(u_rational || v_rational);
        self.token_reals(
            tokens,
            u_poles.checked_mul(v_poles)?.checked_mul(3 + weight)?,
        )?;
        self.token_knots(tokens, u_knots)?;
        self.token_knots(tokens, v_knots)
    }
}
