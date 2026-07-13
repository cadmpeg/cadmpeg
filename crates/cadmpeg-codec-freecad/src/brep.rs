// SPDX-License-Identifier: Apache-2.0
//! Bounded framing for text and binary exact-shape side entries.

use std::collections::BTreeMap;

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::geometry::{NurbsCurve, NurbsSurface};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use serde::{Deserialize, Serialize};

use crate::native::{EntryRecord, PropertyFamily, PropertyRecord};

/// Exact-shape side-entry form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapePayloadForm {
    /// Compact text shape-set grammar.
    Text,
    /// Binary shape-set grammar.
    Binary,
}

/// One exact-shape property bound to its side entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapePayloadRecord {
    /// Stable payload identity.
    pub id: String,
    /// Owning property identity.
    pub property: String,
    /// Side-entry identity.
    pub entry: String,
    /// Carrier form.
    pub form: ShapePayloadForm,
    /// Text shape-set facts, when applicable.
    pub text: Option<TextFacts>,
    /// Decoded binary shape-set prefix, when applicable.
    pub binary: Option<BinaryFacts>,
}

/// Versioned prefix tables decoded from a binary shape set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BinaryFacts {
    /// Binary topology grammar version.
    pub topology_version: u8,
    /// Ordered location table with resolved transforms.
    pub locations: Vec<TextLocation>,
    /// Ordered parameter-space curve table.
    pub curve2ds: Vec<TextCurve2d>,
    /// Ordered 3D curve table.
    pub curves: Vec<TextCurve>,
    /// Ordered standalone 3D polygons.
    pub polygons3d: Vec<TextPolygon3d>,
    /// Ordered polygons indexing triangulation nodes.
    pub polygons_on_triangulations: Vec<TextPolygonOnTriangulation>,
    /// Byte offset immediately after the `Surfaces` section header.
    pub surfaces_offset: usize,
}

/// Framing facts from a text shape set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextFacts {
    /// Topology grammar version.
    pub topology_version: u8,
    /// Declared table counts by section name.
    pub section_counts: BTreeMap<String, usize>,
    /// Shape-type token census.
    pub shape_types: BTreeMap<String, usize>,
    /// Ordered location table with resolved transforms.
    pub locations: Vec<TextLocation>,
    /// Ordered parameter-space curve table.
    pub curve2ds: Vec<TextCurve2d>,
    /// Ordered 3D curve table.
    pub curves: Vec<TextCurve>,
    /// Ordered surface table.
    pub surfaces: Vec<TextSurface>,
    /// Ordered standalone 3D polygons.
    pub polygons3d: Vec<TextPolygon3d>,
    /// Ordered polygons indexing triangulation nodes.
    pub polygons_on_triangulations: Vec<TextPolygonOnTriangulation>,
    /// Ordered display triangulations.
    pub triangulations: Vec<TextTriangulation>,
    /// Ordered subshape-first topology records.
    pub tshapes: Vec<TextTShape>,
    /// Oriented root shape uses following the topology table.
    pub roots: Vec<TextShapeUse>,
}

/// Topological shape family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextShapeKind {
    Vertex,
    Edge,
    Wire,
    Face,
    Shell,
    Solid,
    CompSolid,
    Compound,
}

/// Orientation of one shape use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextOrientation {
    Forward,
    Reversed,
    Internal,
    External,
}

/// One oriented, located use of a topology record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextShapeUse {
    /// One-based `tshapes` index.
    pub shape: usize,
    /// Use orientation.
    pub orientation: TextOrientation,
    /// One-based location index, or zero for identity.
    pub location: usize,
}

/// One vertex point representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextPointRepresentation {
    /// First curve/surface parameter.
    pub parameter: f64,
    /// Optional second surface parameter.
    pub second_parameter: Option<f64>,
    /// Representation family code 1 through 3.
    pub kind: u8,
    /// Referenced 3D or 2D curve index.
    pub curve: Option<usize>,
    /// Referenced surface index.
    pub surface: Option<usize>,
    /// Location index, or zero for identity.
    pub location: usize,
}

/// One edge representation record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextEdgeRepresentation {
    /// Representation code 1 through 7.
    pub kind: u8,
    /// Primary curve or polygon index.
    pub primary: usize,
    /// Optional secondary curve or polygon index.
    pub secondary: Option<usize>,
    /// Optional first surface index.
    pub surface: Option<usize>,
    /// Optional second surface index for regularity records.
    pub second_surface: Option<usize>,
    /// Primary location index, or zero for identity.
    pub location: usize,
    /// Optional second location index.
    pub second_location: Option<usize>,
    /// Optional parameter range.
    pub parameter_range: Option<[f64; 2]>,
    /// Optional continuity token.
    pub continuity: Option<String>,
    /// Optional V2 cached UV endpoints.
    pub uv_endpoints: Option<[Point2; 2]>,
}

/// Geometry and flags specific to a topology record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextTShapeGeometry {
    Vertex {
        tolerance: f64,
        point: Point3,
        representations: Vec<TextPointRepresentation>,
    },
    Edge {
        tolerance: f64,
        same_parameter: bool,
        same_range: bool,
        degenerated: bool,
        representations: Vec<TextEdgeRepresentation>,
    },
    Face {
        natural_restriction: bool,
        tolerance: f64,
        surface: usize,
        location: usize,
        triangulation: Option<usize>,
    },
    Empty,
}

/// One subshape-first topology record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextTShape {
    /// One-based table index.
    pub index: usize,
    /// Shape family.
    pub kind: TextShapeKind,
    /// Family-specific geometry.
    pub geometry: TextTShapeGeometry,
    /// Free, modified, checked, orientable, closed, infinite, convex flags.
    pub flags: [bool; 7],
    /// Ordered child uses.
    pub children: Vec<TextShapeUse>,
}

/// One standalone 3D polygon carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextPolygon3d {
    /// Chordal deflection.
    pub deflection: f64,
    /// Ordered model-space nodes.
    pub nodes: Vec<Point3>,
    /// Optional per-node curve parameters.
    pub parameters: Option<Vec<f64>>,
}

/// One polygon whose indices address a triangulation node table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextPolygonOnTriangulation {
    /// One-based source node indices.
    pub nodes: Vec<u32>,
    /// Chordal deflection.
    pub deflection: f64,
    /// Optional per-node curve parameters.
    pub parameters: Option<Vec<f64>>,
}

/// One indexed display triangulation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextTriangulation {
    /// Chordal deflection.
    pub deflection: f64,
    /// Ordered model-space vertices.
    pub nodes: Vec<Point3>,
    /// Optional UV coordinates parallel to `nodes`.
    pub uv_nodes: Option<Vec<Point2>>,
    /// One-based source triangle indices.
    pub triangles: Vec<[u32; 3]>,
    /// Optional normals parallel to `nodes`.
    pub normals: Option<Vec<Vector3>>,
}

/// A rational or non-rational 2D B-spline curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NurbsCurve2d {
    /// Curve degree.
    pub degree: u32,
    /// Full knot vector.
    pub knots: Vec<f64>,
    /// Ordered parameter-space poles.
    pub control_points: Vec<Point2>,
    /// Optional rational weights.
    pub weights: Option<Vec<f64>>,
    /// Periodicity flag.
    pub periodic: bool,
}

/// One exact parameter-space curve record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextCurve2d {
    /// Infinite line.
    Line { origin: Point2, direction: Point2 },
    /// Full circle with its oriented parameter frame.
    Circle {
        center: Point2,
        x_axis: Point2,
        y_axis: Point2,
        radius: f64,
    },
    /// Full ellipse.
    Ellipse {
        center: Point2,
        x_axis: Point2,
        y_axis: Point2,
        major_radius: f64,
        minor_radius: f64,
    },
    /// Parabola.
    Parabola {
        vertex: Point2,
        x_axis: Point2,
        y_axis: Point2,
        focal_distance: f64,
    },
    /// Hyperbola.
    Hyperbola {
        center: Point2,
        x_axis: Point2,
        y_axis: Point2,
        major_radius: f64,
        minor_radius: f64,
    },
    /// Rational or non-rational B-spline.
    Nurbs(NurbsCurve2d),
    /// Parameter restriction of an inline basis curve.
    Trimmed {
        parameter_range: [f64; 2],
        basis: Box<TextCurve2d>,
    },
    /// Signed planar offset of an inline basis curve.
    Offset {
        distance: f64,
        basis: Box<TextCurve2d>,
    },
}

/// One factor in a compound location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationFactor {
    /// One-based index of an earlier location.
    pub location: usize,
    /// Signed composition power.
    pub power: i64,
}

/// One text B-rep location record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextLocation {
    /// Ordered source factors; empty for an elementary transform.
    pub factors: Vec<LocationFactor>,
    /// Fully composed affine transform.
    pub transform: Transform,
}

/// Supported byte-exact 3D curve records from the text carrier table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextCurve {
    /// Infinite line.
    Line { origin: Point3, direction: Vector3 },
    /// Full circle.
    Circle {
        center: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
    },
    /// Full ellipse.
    Ellipse {
        center: Point3,
        axis: Vector3,
        major_direction: Vector3,
        major_radius: f64,
        minor_radius: f64,
    },
    /// Parabola.
    Parabola {
        vertex: Point3,
        axis: Vector3,
        major_direction: Vector3,
        focal_distance: f64,
    },
    /// Hyperbola.
    Hyperbola {
        center: Point3,
        axis: Vector3,
        major_direction: Vector3,
        major_radius: f64,
        minor_radius: f64,
    },
    /// Rational or non-rational B-spline curve.
    Nurbs(NurbsCurve),
    /// A parameter sub-range of an inline basis curve.
    Trimmed {
        parameter_range: [f64; 2],
        basis: Box<TextCurve>,
    },
    /// A signed offset from an inline basis curve in a fixed direction.
    Offset {
        distance: f64,
        direction: Vector3,
        basis: Box<TextCurve>,
    },
}

/// Supported byte-exact surface records from the text carrier table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextSurface {
    /// Infinite plane.
    Plane {
        origin: Point3,
        axis: Vector3,
        u_axis: Vector3,
    },
    /// Circular cylinder.
    Cylinder {
        origin: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
    },
    /// Circular cone.
    Cone {
        origin: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
        half_angle: f64,
    },
    /// Sphere.
    Sphere {
        center: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
    },
    /// Torus.
    Torus {
        center: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        major_radius: f64,
        minor_radius: f64,
    },
    /// Rational or non-rational tensor-product B-spline surface.
    Nurbs(NurbsSurface),
    /// Translation of an inline directrix curve.
    Extrusion {
        direction: Vector3,
        directrix: Box<TextCurve>,
    },
    /// Revolution of an inline directrix around an axis.
    Revolution {
        axis_origin: Point3,
        axis_direction: Vector3,
        directrix: Box<TextCurve>,
    },
    /// Rectangular parameter sub-range of an inline basis surface.
    Trimmed {
        parameter_ranges: [[f64; 2]; 2],
        basis: Box<TextSurface>,
    },
    /// Signed normal offset from an inline basis surface.
    Offset {
        distance: f64,
        basis: Box<TextSurface>,
    },
}

/// Bind every exact-shape property to and frame its payload.
pub fn parse_payloads(
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
) -> Result<Vec<ShapePayloadRecord>, CodecError> {
    let entries = entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    properties
        .iter()
        .filter(|property| property.family == PropertyFamily::Geometry)
        .flat_map(|property| {
            property
                .side_entries
                .iter()
                .map(move |name| (property, name))
        })
        .map(|(property, name)| {
            let entry = entries.get(name.as_str()).ok_or_else(|| {
                CodecError::Malformed(format!("missing exact-shape entry {name}"))
            })?;
            let form = if name.to_ascii_lowercase().ends_with(".bin") {
                ShapePayloadForm::Binary
            } else {
                ShapePayloadForm::Text
            };
            let (text, binary) = match form {
                ShapePayloadForm::Text => (Some(parse_text(&entry.data)?), None),
                ShapePayloadForm::Binary => (None, Some(parse_binary_prefix(&entry.data)?)),
            };
            Ok(ShapePayloadRecord {
                id: format!("{}:shape-payload", property.id),
                property: property.id.clone(),
                entry: entry.id.clone(),
                form,
                text,
                binary,
            })
        })
        .collect()
}

fn parse_text(bytes: &[u8]) -> Result<TextFacts, CodecError> {
    const MAX_TEXT_BREP_BYTES: usize = 256 * 1024 * 1024;
    if bytes.len() > MAX_TEXT_BREP_BYTES {
        return Err(CodecError::Malformed(
            "text B-rep size limit exceeded".into(),
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("text B-rep is not UTF-8".into()))?;
    let topology_version = if text.contains("CASCADE Topology V1, (c) Matra-Datavision") {
        1
    } else if text.contains("CASCADE Topology V2, (c) Matra-Datavision") {
        2
    } else if text.contains("CASCADE Topology V3, (c) Open Cascade") {
        3
    } else {
        return Err(CodecError::Malformed(
            "text B-rep has no supported topology header".into(),
        ));
    };
    let tokens = text.split_ascii_whitespace().collect::<Vec<_>>();
    let mut section_counts = BTreeMap::new();
    for section in [
        "Locations",
        "Curve2ds",
        "Curves",
        "Polygon3D",
        "PolygonOnTriangulations",
        "Surfaces",
        "Triangulations",
        "TShapes",
    ] {
        let index = tokens
            .iter()
            .position(|token| *token == section)
            .ok_or_else(|| CodecError::Malformed(format!("text B-rep has no {section} table")))?;
        let count = tokens
            .get(index + 1)
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| CodecError::Malformed(format!("invalid {section} count")))?;
        section_counts.insert(section.to_owned(), count);
    }
    let tshapes = tokens
        .iter()
        .position(|token| *token == "TShapes")
        .expect("TShapes was validated");
    let mut shape_types = BTreeMap::new();
    for token in &tokens[tshapes + 2..] {
        let name = match *token {
            "Ve" => "vertex",
            "Ed" => "edge",
            "Wi" => "wire",
            "Fa" => "face",
            "Sh" => "shell",
            "So" => "solid",
            "CS" => "compsolid",
            "Co" => "compound",
            _ => continue,
        };
        *shape_types.entry(name.to_owned()).or_insert(0) += 1;
    }
    let declared_shapes = section_counts.get("TShapes").copied().unwrap_or(0);
    if shape_types.values().sum::<usize>() != declared_shapes {
        return Err(CodecError::Malformed(format!(
            "TShapes declares {declared_shapes} records but the shape-type census found {}",
            shape_types.values().sum::<usize>()
        )));
    }
    let locations = parse_locations(&tokens, &section_counts)?;
    let curve2ds = parse_curve2ds(&tokens, &section_counts)?;
    let curves = parse_curves(&tokens, &section_counts)?;
    let surfaces = parse_surfaces(&tokens, &section_counts)?;
    let polygons3d = parse_polygons3d(&tokens, &section_counts)?;
    let polygons_on_triangulations = parse_polygons_on_triangulations(&tokens, &section_counts)?;
    let triangulations = parse_triangulations(&tokens, &section_counts, topology_version)?;
    let (tshapes, roots) = parse_tshapes(&tokens, &section_counts, topology_version)?;
    Ok(TextFacts {
        topology_version,
        section_counts,
        shape_types,
        locations,
        curve2ds,
        curves,
        surfaces,
        polygons3d,
        polygons_on_triangulations,
        triangulations,
        tshapes,
        roots,
    })
}

fn parse_binary_prefix(bytes: &[u8]) -> Result<BinaryFacts, CodecError> {
    const MAX_BINARY_BREP_BYTES: usize = 256 * 1024 * 1024;
    if bytes.len() > MAX_BINARY_BREP_BYTES {
        return Err(CodecError::Malformed(
            "binary B-rep exceeds the 256 MiB parser limit".into(),
        ));
    }
    let mut cursor = BinaryCursor::new(bytes);
    let version = loop {
        let line = cursor.line("binary B-rep version")?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let version = line
            .strip_prefix("Open CASCADE Topology V")
            .and_then(|tail| tail.as_bytes().first())
            .and_then(|byte| byte.checked_sub(b'0'))
            .filter(|version| (1..=4).contains(version))
            .ok_or_else(|| CodecError::Malformed("unsupported binary B-rep header".into()))?;
        break version;
    };
    let location_count = cursor.section_count("Locations")?;
    let mut locations: Vec<TextLocation> = Vec::with_capacity(location_count);
    for index in 0..location_count {
        let kind = cursor.u8("binary location kind")?;
        let location = match kind {
            1 => {
                let mut transform = Transform::identity();
                for row in 0..3 {
                    for column in 0..4 {
                        transform.rows[row][column] = cursor.f64("binary location transform")?;
                    }
                }
                invert_affine(transform)?;
                TextLocation {
                    factors: Vec::new(),
                    transform,
                }
            }
            2 => {
                let mut factors = Vec::new();
                let mut transform = Transform::identity();
                loop {
                    let referenced = cursor.i32("binary location factor")?;
                    if referenced == 0 {
                        break;
                    }
                    let referenced = usize::try_from(referenced).map_err(|_| {
                        CodecError::Malformed("negative binary location factor".into())
                    })?;
                    if referenced == 0 || referenced > locations.len() {
                        return Err(CodecError::Malformed(format!(
                            "binary location {} references unavailable location {referenced}",
                            index + 1
                        )));
                    }
                    let power = cursor.i32("binary location power")?;
                    let powered =
                        transform_power(locations[referenced - 1].transform, i64::from(power))?;
                    transform = multiply_transform(powered, transform);
                    factors.push(LocationFactor {
                        location: referenced,
                        power: i64::from(power),
                    });
                }
                TextLocation { factors, transform }
            }
            other => {
                return Err(CodecError::Malformed(format!(
                    "invalid binary location type {other}"
                )))
            }
        };
        locations.push(location);
    }
    let curve_count = cursor.section_count("Curve2ds")?;
    let mut curve2ds = Vec::with_capacity(curve_count);
    for _ in 0..curve_count {
        curve2ds.push(parse_binary_curve2d(&mut cursor, 0)?);
    }
    let curve_count = cursor.section_count("Curves")?;
    let mut curves = Vec::with_capacity(curve_count);
    for _ in 0..curve_count {
        curves.push(parse_binary_curve(&mut cursor, 0)?);
    }
    let polygon_count = cursor.section_count("Polygon3D")?;
    let mut polygons3d = Vec::with_capacity(polygon_count);
    for _ in 0..polygon_count {
        let node_count = cursor.count("binary 3D polygon node count")?;
        let has_parameters = cursor.bool("binary 3D polygon parameter flag")?;
        let deflection = cursor.f64("binary 3D polygon deflection")?;
        let nodes = (0..node_count)
            .map(|_| cursor.point3("binary 3D polygon node"))
            .collect::<Result<Vec<_>, _>>()?;
        let parameters = has_parameters
            .then(|| {
                (0..node_count)
                    .map(|_| cursor.f64("binary 3D polygon parameter"))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        polygons3d.push(TextPolygon3d {
            deflection,
            nodes,
            parameters,
        });
    }
    let indexed_polygon_count = cursor.section_count("PolygonOnTriangulations")?;
    let mut polygons_on_triangulations = Vec::with_capacity(indexed_polygon_count);
    for _ in 0..indexed_polygon_count {
        let node_count = cursor.count("binary indexed polygon node count")?;
        let nodes = (0..node_count)
            .map(|_| {
                let node = cursor.i32("binary indexed polygon node")?;
                u32::try_from(node).map_err(|_| {
                    CodecError::Malformed("non-positive binary indexed polygon node".into())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if nodes.contains(&0) {
            return Err(CodecError::Malformed(
                "binary indexed polygon node indices are one-based".into(),
            ));
        }
        let deflection = cursor.f64("binary indexed polygon deflection")?;
        let has_parameters = cursor.bool("binary indexed polygon parameter flag")?;
        let parameters = has_parameters
            .then(|| {
                (0..node_count)
                    .map(|_| cursor.f64("binary indexed polygon parameter"))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        polygons_on_triangulations.push(TextPolygonOnTriangulation {
            nodes,
            deflection,
            parameters,
        });
    }
    cursor.expect_section_name("Surfaces")?;
    Ok(BinaryFacts {
        topology_version: version,
        locations,
        curve2ds,
        curves,
        polygons3d,
        polygons_on_triangulations,
        surfaces_offset: cursor.offset,
    })
}

fn parse_binary_curve(
    cursor: &mut BinaryCursor<'_>,
    depth: usize,
) -> Result<TextCurve, CodecError> {
    if depth > 256 {
        return Err(CodecError::Malformed(
            "binary 3D curve nesting exceeds 256".into(),
        ));
    }
    Ok(match cursor.u8("binary 3D curve kind")? {
        1 => TextCurve::Line {
            origin: cursor.point3("binary line origin")?,
            direction: cursor.vector3("binary line direction")?,
        },
        2 => {
            let center = cursor.point3("binary circle center")?;
            let axis = cursor.vector3("binary circle axis")?;
            let ref_direction = cursor.vector3("binary circle reference direction")?;
            cursor.vector3("binary circle y axis")?;
            TextCurve::Circle {
                center,
                axis,
                ref_direction,
                radius: cursor.f64("binary circle radius")?,
            }
        }
        3 => {
            let center = cursor.point3("binary ellipse center")?;
            let axis = cursor.vector3("binary ellipse axis")?;
            let major_direction = cursor.vector3("binary ellipse major direction")?;
            cursor.vector3("binary ellipse minor direction")?;
            TextCurve::Ellipse {
                center,
                axis,
                major_direction,
                major_radius: cursor.f64("binary ellipse major radius")?,
                minor_radius: cursor.f64("binary ellipse minor radius")?,
            }
        }
        4 => {
            let vertex = cursor.point3("binary parabola vertex")?;
            let axis = cursor.vector3("binary parabola axis")?;
            let major_direction = cursor.vector3("binary parabola major direction")?;
            cursor.vector3("binary parabola minor direction")?;
            TextCurve::Parabola {
                vertex,
                axis,
                major_direction,
                focal_distance: cursor.f64("binary parabola focal distance")?,
            }
        }
        5 => {
            let center = cursor.point3("binary hyperbola center")?;
            let axis = cursor.vector3("binary hyperbola axis")?;
            let major_direction = cursor.vector3("binary hyperbola major direction")?;
            cursor.vector3("binary hyperbola minor direction")?;
            TextCurve::Hyperbola {
                center,
                axis,
                major_direction,
                major_radius: cursor.f64("binary hyperbola major radius")?,
                minor_radius: cursor.f64("binary hyperbola minor radius")?,
            }
        }
        6 => {
            let rational = cursor.bool("binary Bezier rational flag")?;
            let degree = usize::from(cursor.u16("binary Bezier degree")?);
            let pole_count = degree
                .checked_add(1)
                .ok_or_else(|| CodecError::Malformed("binary Bezier pole count overflow".into()))?;
            let mut control_points = Vec::with_capacity(pole_count);
            let mut weights = rational.then(|| Vec::with_capacity(pole_count));
            for _ in 0..pole_count {
                control_points.push(cursor.point3("binary Bezier pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.f64("binary Bezier weight")?);
                }
            }
            TextCurve::Nurbs(NurbsCurve {
                degree: u32::try_from(degree).map_err(|_| {
                    CodecError::Malformed("binary Bezier degree exceeds u32".into())
                })?,
                knots: clamped_bezier_knots(degree),
                control_points,
                weights,
                periodic: false,
            })
        }
        7 => {
            let rational = cursor.bool("binary B-spline rational flag")?;
            let periodic = cursor.bool("binary B-spline periodic flag")?;
            let degree = u32::from(cursor.u16("binary B-spline degree")?);
            let pole_count = cursor.count("binary B-spline pole count")?;
            let knot_count = cursor.count("binary B-spline knot count")?;
            let mut control_points = Vec::with_capacity(pole_count);
            let mut weights = rational.then(|| Vec::with_capacity(pole_count));
            for _ in 0..pole_count {
                control_points.push(cursor.point3("binary B-spline pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.f64("binary B-spline weight")?);
                }
            }
            let knots = cursor.expanded_knots(knot_count, "binary B-spline")?;
            TextCurve::Nurbs(NurbsCurve {
                degree,
                knots,
                control_points,
                weights,
                periodic,
            })
        }
        8 => TextCurve::Trimmed {
            parameter_range: [
                cursor.f64("binary trim start")?,
                cursor.f64("binary trim end")?,
            ],
            basis: Box::new(parse_binary_curve(cursor, depth + 1)?),
        },
        9 => TextCurve::Offset {
            distance: cursor.f64("binary offset distance")?,
            direction: cursor.vector3("binary offset direction")?,
            basis: Box::new(parse_binary_curve(cursor, depth + 1)?),
        },
        other => {
            return Err(CodecError::Malformed(format!(
                "invalid binary 3D curve kind {other}"
            )))
        }
    })
}

fn parse_binary_curve2d(
    cursor: &mut BinaryCursor<'_>,
    depth: usize,
) -> Result<TextCurve2d, CodecError> {
    if depth > 256 {
        return Err(CodecError::Malformed(
            "binary parameter-curve nesting exceeds 256".into(),
        ));
    }
    let point = |cursor: &mut BinaryCursor<'_>, label| -> Result<Point2, CodecError> {
        Ok(Point2::new(cursor.f64(label)?, cursor.f64(label)?))
    };
    Ok(match cursor.u8("binary parameter-curve kind")? {
        1 => TextCurve2d::Line {
            origin: point(cursor, "binary line origin")?,
            direction: point(cursor, "binary line direction")?,
        },
        2 => TextCurve2d::Circle {
            center: point(cursor, "binary circle center")?,
            x_axis: point(cursor, "binary circle x axis")?,
            y_axis: point(cursor, "binary circle y axis")?,
            radius: cursor.f64("binary circle radius")?,
        },
        3 => TextCurve2d::Ellipse {
            center: point(cursor, "binary ellipse center")?,
            x_axis: point(cursor, "binary ellipse x axis")?,
            y_axis: point(cursor, "binary ellipse y axis")?,
            major_radius: cursor.f64("binary ellipse major radius")?,
            minor_radius: cursor.f64("binary ellipse minor radius")?,
        },
        4 => TextCurve2d::Parabola {
            vertex: point(cursor, "binary parabola vertex")?,
            x_axis: point(cursor, "binary parabola x axis")?,
            y_axis: point(cursor, "binary parabola y axis")?,
            focal_distance: cursor.f64("binary parabola focal distance")?,
        },
        5 => TextCurve2d::Hyperbola {
            center: point(cursor, "binary hyperbola center")?,
            x_axis: point(cursor, "binary hyperbola x axis")?,
            y_axis: point(cursor, "binary hyperbola y axis")?,
            major_radius: cursor.f64("binary hyperbola major radius")?,
            minor_radius: cursor.f64("binary hyperbola minor radius")?,
        },
        6 => {
            let rational = cursor.bool("binary Bezier rational flag")?;
            let degree = usize::from(cursor.u16("binary Bezier degree")?);
            let pole_count = degree
                .checked_add(1)
                .ok_or_else(|| CodecError::Malformed("binary Bezier pole count overflow".into()))?;
            let mut control_points = Vec::with_capacity(pole_count);
            let mut weights = rational.then(|| Vec::with_capacity(pole_count));
            for _ in 0..pole_count {
                control_points.push(point(cursor, "binary Bezier pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.f64("binary Bezier weight")?);
                }
            }
            TextCurve2d::Nurbs(NurbsCurve2d {
                degree: u32::try_from(degree).map_err(|_| {
                    CodecError::Malformed("binary Bezier degree exceeds u32".into())
                })?,
                knots: clamped_bezier_knots(degree),
                control_points,
                weights,
                periodic: false,
            })
        }
        7 => {
            let rational = cursor.bool("binary B-spline rational flag")?;
            let periodic = cursor.bool("binary B-spline periodic flag")?;
            let degree = u32::from(cursor.u16("binary B-spline degree")?);
            let pole_count = cursor.count("binary B-spline pole count")?;
            let knot_count = cursor.count("binary B-spline knot count")?;
            let mut control_points = Vec::with_capacity(pole_count);
            let mut weights = rational.then(|| Vec::with_capacity(pole_count));
            for _ in 0..pole_count {
                control_points.push(point(cursor, "binary B-spline pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.f64("binary B-spline weight")?);
                }
            }
            let mut knots = Vec::new();
            for _ in 0..knot_count {
                let knot = cursor.f64("binary B-spline knot")?;
                let multiplicity = cursor.count("binary B-spline multiplicity")?;
                if knots
                    .len()
                    .checked_add(multiplicity)
                    .is_none_or(|len| len > 1_000_000)
                {
                    return Err(CodecError::Malformed(
                        "binary B-spline expanded knot-count limit exceeded".into(),
                    ));
                }
                knots.extend(std::iter::repeat_n(knot, multiplicity));
            }
            TextCurve2d::Nurbs(NurbsCurve2d {
                degree,
                knots,
                control_points,
                weights,
                periodic,
            })
        }
        8 => TextCurve2d::Trimmed {
            parameter_range: [
                cursor.f64("binary trim start")?,
                cursor.f64("binary trim end")?,
            ],
            basis: Box::new(parse_binary_curve2d(cursor, depth + 1)?),
        },
        9 => TextCurve2d::Offset {
            distance: cursor.f64("binary offset distance")?,
            basis: Box::new(parse_binary_curve2d(cursor, depth + 1)?),
        },
        other => {
            return Err(CodecError::Malformed(format!(
                "invalid binary parameter-curve kind {other}"
            )))
        }
    })
}

struct BinaryCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> BinaryCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize, label: &str) -> Result<&'a [u8], CodecError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(|| CodecError::Malformed(format!("{label} offset overflow")))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| CodecError::Malformed(format!("truncated {label}")))?;
        self.offset = end;
        Ok(bytes)
    }

    fn line(&mut self, label: &str) -> Result<&'a str, CodecError> {
        let tail = self
            .bytes
            .get(self.offset..)
            .ok_or_else(|| CodecError::Malformed(format!("truncated {label}")))?;
        let length = tail
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or_else(|| CodecError::Malformed(format!("unterminated {label}")))?;
        let line = self.take(length + 1, label)?;
        std::str::from_utf8(&line[..length])
            .map_err(|_| CodecError::Malformed(format!("non-UTF-8 {label}")))
    }

    fn section_count(&mut self, name: &str) -> Result<usize, CodecError> {
        let line = self.line(name)?;
        let mut tokens = line.split_ascii_whitespace();
        if tokens.next() != Some(name) || tokens.clone().count() != 1 {
            return Err(CodecError::Malformed(format!(
                "binary B-rep expected {name} section"
            )));
        }
        tokens
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|count| *count <= 1_000_000)
            .ok_or_else(|| CodecError::Malformed(format!("invalid binary {name} count")))
    }

    fn expect_section_name(&mut self, name: &str) -> Result<(), CodecError> {
        let line = self.line(name)?;
        let mut tokens = line.split_ascii_whitespace();
        if tokens.next() != Some(name) || tokens.next().is_none() || tokens.next().is_some() {
            return Err(CodecError::Malformed(format!(
                "binary B-rep expected {name} section"
            )));
        }
        Ok(())
    }

    fn u8(&mut self, label: &str) -> Result<u8, CodecError> {
        Ok(self.take(1, label)?[0])
    }

    fn bool(&mut self, label: &str) -> Result<bool, CodecError> {
        match self.u8(label)? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(CodecError::Malformed(format!(
                "invalid {label} byte {other}"
            ))),
        }
    }

    fn u16(&mut self, label: &str) -> Result<u16, CodecError> {
        Ok(u16::from_le_bytes(
            self.take(2, label)?.try_into().expect("two-byte slice"),
        ))
    }

    fn i32(&mut self, label: &str) -> Result<i32, CodecError> {
        Ok(i32::from_le_bytes(
            self.take(4, label)?.try_into().expect("four-byte slice"),
        ))
    }

    fn count(&mut self, label: &str) -> Result<usize, CodecError> {
        let value = self.i32(label)?;
        usize::try_from(value)
            .ok()
            .filter(|count| *count <= 1_000_000)
            .ok_or_else(|| CodecError::Malformed(format!("invalid {label}")))
    }

    fn f64(&mut self, label: &str) -> Result<f64, CodecError> {
        let value = f64::from_le_bytes(self.take(8, label)?.try_into().expect("eight-byte slice"));
        value
            .is_finite()
            .then_some(value)
            .ok_or_else(|| CodecError::Malformed(format!("non-finite {label}")))
    }

    fn point3(&mut self, label: &str) -> Result<Point3, CodecError> {
        Ok(Point3::new(
            self.f64(label)?,
            self.f64(label)?,
            self.f64(label)?,
        ))
    }

    fn vector3(&mut self, label: &str) -> Result<Vector3, CodecError> {
        Ok(Vector3::new(
            self.f64(label)?,
            self.f64(label)?,
            self.f64(label)?,
        ))
    }

    fn expanded_knots(&mut self, count: usize, label: &str) -> Result<Vec<f64>, CodecError> {
        let mut knots = Vec::new();
        for _ in 0..count {
            let knot = self.f64(label)?;
            let multiplicity = self.count(label)?;
            if knots
                .len()
                .checked_add(multiplicity)
                .is_none_or(|len| len > 1_000_000)
            {
                return Err(CodecError::Malformed(format!(
                    "{label} expanded knot-count limit exceeded"
                )));
            }
            knots.extend(std::iter::repeat_n(knot, multiplicity));
        }
        Ok(knots)
    }
}

fn parse_locations(
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
) -> Result<Vec<TextLocation>, CodecError> {
    let start = tokens
        .iter()
        .position(|token| *token == "Locations")
        .ok_or_else(|| CodecError::Malformed("text B-rep has no Locations table".into()))?
        + 2;
    let end = tokens
        .iter()
        .position(|token| *token == "Curve2ds")
        .ok_or_else(|| CodecError::Malformed("text B-rep has no Curve2ds table".into()))?;
    let count = section_counts.get("Locations").copied().unwrap_or(0);
    let mut cursor = TokenCursor::new(&tokens[start..end]);
    let mut locations: Vec<TextLocation> = Vec::with_capacity(count);
    for index in 0..count {
        let kind = cursor.integer("location type")?;
        let location = match kind {
            1 => {
                let mut transform = Transform::identity();
                for row in 0..3 {
                    for column in 0..4 {
                        transform.rows[row][column] = cursor.real("location transform value")?;
                    }
                }
                invert_affine(transform)?;
                TextLocation {
                    factors: Vec::new(),
                    transform,
                }
            }
            2 => {
                let mut factors = Vec::new();
                let mut transform = Transform::identity();
                loop {
                    let referenced = cursor.integer("location factor index")?;
                    if referenced == 0 {
                        break;
                    }
                    let referenced = usize::try_from(referenced).map_err(|_| {
                        CodecError::Malformed("negative location factor index".into())
                    })?;
                    if referenced == 0 || referenced > locations.len() {
                        return Err(CodecError::Malformed(format!(
                            "location {} references unavailable location {referenced}",
                            index + 1
                        )));
                    }
                    if factors.len() >= 1_000_000 {
                        return Err(CodecError::Malformed(
                            "location factor-count limit exceeded".into(),
                        ));
                    }
                    let power = cursor.integer("location factor power")?;
                    let powered = transform_power(locations[referenced - 1].transform, power)?;
                    transform = multiply_transform(powered, transform);
                    factors.push(LocationFactor {
                        location: referenced,
                        power,
                    });
                }
                TextLocation { factors, transform }
            }
            other => {
                return Err(CodecError::Malformed(format!(
                    "invalid location type {other} at table index {}",
                    index + 1
                )))
            }
        };
        locations.push(location);
    }
    if !cursor.is_empty() {
        return Err(CodecError::Malformed(
            "text B-rep Locations table contains trailing tokens".into(),
        ));
    }
    Ok(locations)
}

fn parse_curve2ds(
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
) -> Result<Vec<TextCurve2d>, CodecError> {
    let start = tokens
        .iter()
        .position(|token| *token == "Curve2ds")
        .ok_or_else(|| CodecError::Malformed("text B-rep has no Curve2ds table".into()))?
        + 2;
    let end = tokens
        .iter()
        .position(|token| *token == "Curves")
        .ok_or_else(|| CodecError::Malformed("text B-rep has no Curves table".into()))?;
    let count = section_counts.get("Curve2ds").copied().unwrap_or(0);
    let mut cursor = TokenCursor::new(&tokens[start..end]);
    let mut curves = Vec::with_capacity(count);
    for index in 0..count {
        curves.push(parse_curve2d(&mut cursor, 0, index + 1)?);
    }
    if !cursor.is_empty() {
        return Err(CodecError::Malformed(
            "text B-rep Curve2ds table contains trailing tokens".into(),
        ));
    }
    Ok(curves)
}

fn parse_curve2d(
    cursor: &mut TokenCursor<'_>,
    depth: usize,
    table_index: usize,
) -> Result<TextCurve2d, CodecError> {
    if depth > 64 {
        return Err(CodecError::Malformed(
            "text B-rep 2D curve recursion limit exceeded".into(),
        ));
    }
    let kind = cursor.integer("2D curve type")?;
    Ok(match kind {
        1 => TextCurve2d::Line {
            origin: cursor.point2("2D line origin")?,
            direction: cursor.point2("2D line direction")?,
        },
        2 => TextCurve2d::Circle {
            center: cursor.point2("2D circle center")?,
            x_axis: cursor.point2("2D circle x axis")?,
            y_axis: cursor.point2("2D circle y axis")?,
            radius: cursor.real("2D circle radius")?,
        },
        3 => TextCurve2d::Ellipse {
            center: cursor.point2("2D ellipse center")?,
            x_axis: cursor.point2("2D ellipse x axis")?,
            y_axis: cursor.point2("2D ellipse y axis")?,
            major_radius: cursor.real("2D ellipse major radius")?,
            minor_radius: cursor.real("2D ellipse minor radius")?,
        },
        4 => TextCurve2d::Parabola {
            vertex: cursor.point2("2D parabola vertex")?,
            x_axis: cursor.point2("2D parabola x axis")?,
            y_axis: cursor.point2("2D parabola y axis")?,
            focal_distance: cursor.real("2D parabola focal distance")?,
        },
        5 => TextCurve2d::Hyperbola {
            center: cursor.point2("2D hyperbola center")?,
            x_axis: cursor.point2("2D hyperbola x axis")?,
            y_axis: cursor.point2("2D hyperbola y axis")?,
            major_radius: cursor.real("2D hyperbola major radius")?,
            minor_radius: cursor.real("2D hyperbola minor radius")?,
        },
        6 => TextCurve2d::Nurbs(parse_bezier_curve2d(cursor)?),
        7 => TextCurve2d::Nurbs(parse_nurbs_curve2d(cursor)?),
        8 => {
            let first = cursor.real("trimmed 2D curve first parameter")?;
            let last = cursor.real("trimmed 2D curve last parameter")?;
            if first > last {
                return Err(CodecError::Malformed(
                    "trimmed 2D curve parameter range is reversed".into(),
                ));
            }
            TextCurve2d::Trimmed {
                parameter_range: [first, last],
                basis: Box::new(parse_curve2d(cursor, depth + 1, table_index)?),
            }
        }
        9 => TextCurve2d::Offset {
            distance: cursor.real("offset 2D curve distance")?,
            basis: Box::new(parse_curve2d(cursor, depth + 1, table_index)?),
        },
        other => {
            return Err(CodecError::NotImplemented(format!(
                "text B-rep 2D curve family {other} at table index {table_index}"
            )))
        }
    })
}

fn parse_bezier_curve2d(cursor: &mut TokenCursor<'_>) -> Result<NurbsCurve2d, CodecError> {
    let rational = cursor.boolean("2D Bezier rational flag")?;
    let degree = cursor.count("2D Bezier degree", 64)?;
    let pole_count = degree + 1;
    let mut control_points = Vec::with_capacity(pole_count);
    let mut weights = rational.then(|| Vec::with_capacity(pole_count));
    for _ in 0..pole_count {
        control_points.push(cursor.point2("2D Bezier pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.real("2D Bezier weight")?);
        }
    }
    Ok(NurbsCurve2d {
        degree: degree as u32,
        knots: clamped_bezier_knots(degree),
        control_points,
        weights,
        periodic: false,
    })
}

fn parse_nurbs_curve2d(cursor: &mut TokenCursor<'_>) -> Result<NurbsCurve2d, CodecError> {
    let rational = cursor.boolean("2D B-spline rational flag")?;
    let periodic = cursor.boolean("2D B-spline periodic flag")?;
    let degree = cursor.count("2D B-spline degree", 64)?;
    let pole_count = cursor.count("2D B-spline pole count", 1_000_000)?;
    let knot_count = cursor.count("2D B-spline knot count", 1_000_000)?;
    let mut control_points = Vec::with_capacity(pole_count);
    let mut weights = rational.then(|| Vec::with_capacity(pole_count));
    for _ in 0..pole_count {
        control_points.push(cursor.point2("2D B-spline pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.real("2D B-spline weight")?);
        }
    }
    Ok(NurbsCurve2d {
        degree: degree as u32,
        knots: parse_knots(cursor, knot_count, degree, "2D B-spline")?,
        control_points,
        weights,
        periodic,
    })
}

fn multiply_transform(left: Transform, right: Transform) -> Transform {
    let mut result = Transform {
        rows: [[0.0; 4]; 4],
    };
    for row in 0..4 {
        for column in 0..4 {
            result.rows[row][column] = (0..4)
                .map(|inner| left.rows[row][inner] * right.rows[inner][column])
                .sum();
        }
    }
    result
}

fn transform_power(transform: Transform, power: i64) -> Result<Transform, CodecError> {
    let mut base = if power < 0 {
        invert_affine(transform)?
    } else {
        transform
    };
    let mut exponent = power.unsigned_abs();
    let mut result = Transform::identity();
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = multiply_transform(result, base);
        }
        exponent >>= 1;
        if exponent > 0 {
            base = multiply_transform(base, base);
        }
    }
    Ok(result)
}

fn invert_affine(transform: Transform) -> Result<Transform, CodecError> {
    if transform.rows[3] != [0.0, 0.0, 0.0, 1.0] {
        return Err(CodecError::Malformed(
            "location transform is not affine".into(),
        ));
    }
    let m = transform.rows;
    let determinant = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if !determinant.is_finite() || determinant == 0.0 {
        return Err(CodecError::Malformed(
            "location transform is not invertible".into(),
        ));
    }
    let inverse_linear = [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) / determinant,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) / determinant,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) / determinant,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) / determinant,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) / determinant,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) / determinant,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) / determinant,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) / determinant,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) / determinant,
        ],
    ];
    let translation = [m[0][3], m[1][3], m[2][3]];
    let mut result = Transform::identity();
    for (row, inverse_row) in inverse_linear.iter().enumerate() {
        result.rows[row][..3].copy_from_slice(inverse_row);
        result.rows[row][3] = -(0..3)
            .map(|column| inverse_row[column] * translation[column])
            .sum::<f64>();
    }
    Ok(result)
}

fn parse_polygons3d(
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
) -> Result<Vec<TextPolygon3d>, CodecError> {
    let mut cursor = section_cursor(tokens, "Polygon3D", "PolygonOnTriangulations")?;
    let count = section_counts.get("Polygon3D").copied().unwrap_or(0);
    let mut polygons = Vec::with_capacity(count);
    for _ in 0..count {
        let node_count = cursor.count("3D polygon node count", 10_000_000)?;
        let has_parameters = cursor.boolean("3D polygon parameter flag")?;
        let deflection = cursor.real("3D polygon deflection")?;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            nodes.push(cursor.point("3D polygon node")?);
        }
        let parameters = if has_parameters {
            let mut parameters = Vec::with_capacity(node_count);
            for _ in 0..node_count {
                parameters.push(cursor.real("3D polygon parameter")?);
            }
            Some(parameters)
        } else {
            None
        };
        polygons.push(TextPolygon3d {
            deflection,
            nodes,
            parameters,
        });
    }
    ensure_section_consumed(&cursor, "Polygon3D")?;
    Ok(polygons)
}

fn parse_polygons_on_triangulations(
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
) -> Result<Vec<TextPolygonOnTriangulation>, CodecError> {
    let mut cursor = section_cursor(tokens, "PolygonOnTriangulations", "Surfaces")?;
    let count = section_counts
        .get("PolygonOnTriangulations")
        .copied()
        .unwrap_or(0);
    let mut polygons = Vec::with_capacity(count);
    for _ in 0..count {
        let node_count = cursor.count("polygon-on-triangulation node count", 10_000_000)?;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            let node = cursor.count("polygon-on-triangulation node index", u32::MAX as usize)?;
            if node == 0 {
                return Err(CodecError::Malformed(
                    "polygon-on-triangulation node index is zero".into(),
                ));
            }
            nodes.push(node as u32);
        }
        if cursor.next("polygon-on-triangulation parameter marker")? != "p" {
            return Err(CodecError::Malformed(
                "polygon-on-triangulation has no parameter marker".into(),
            ));
        }
        let deflection = cursor.real("polygon-on-triangulation deflection")?;
        let has_parameters = cursor.boolean("polygon-on-triangulation parameter flag")?;
        let parameters = if has_parameters {
            let mut parameters = Vec::with_capacity(node_count);
            for _ in 0..node_count {
                parameters.push(cursor.real("polygon-on-triangulation parameter")?);
            }
            Some(parameters)
        } else {
            None
        };
        polygons.push(TextPolygonOnTriangulation {
            nodes,
            deflection,
            parameters,
        });
    }
    ensure_section_consumed(&cursor, "PolygonOnTriangulations")?;
    Ok(polygons)
}

fn parse_triangulations(
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
    topology_version: u8,
) -> Result<Vec<TextTriangulation>, CodecError> {
    let mut cursor = section_cursor(tokens, "Triangulations", "TShapes")?;
    let count = section_counts.get("Triangulations").copied().unwrap_or(0);
    let mut triangulations = Vec::with_capacity(count);
    for _ in 0..count {
        let node_count = cursor.count("triangulation node count", 10_000_000)?;
        let triangle_count = cursor.count("triangulation triangle count", 10_000_000)?;
        let has_uv = cursor.boolean("triangulation UV flag")?;
        let has_normals = topology_version >= 3 && cursor.boolean("triangulation normal flag")?;
        let deflection = cursor.real("triangulation deflection")?;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            nodes.push(cursor.point("triangulation node")?);
        }
        let uv_nodes = if has_uv {
            let mut uv_nodes = Vec::with_capacity(node_count);
            for _ in 0..node_count {
                uv_nodes.push(cursor.point2("triangulation UV node")?);
            }
            Some(uv_nodes)
        } else {
            None
        };
        let mut triangles = Vec::with_capacity(triangle_count);
        for _ in 0..triangle_count {
            let mut triangle = [0_u32; 3];
            for node in &mut triangle {
                let index = cursor.count("triangulation node index", node_count)?;
                if index == 0 {
                    return Err(CodecError::Malformed(
                        "triangulation node index is zero".into(),
                    ));
                }
                *node = index as u32;
            }
            triangles.push(triangle);
        }
        let normals = if has_normals {
            let mut normals = Vec::with_capacity(node_count);
            for _ in 0..node_count {
                normals.push(cursor.vector("triangulation normal")?);
            }
            Some(normals)
        } else {
            None
        };
        triangulations.push(TextTriangulation {
            deflection,
            nodes,
            uv_nodes,
            triangles,
            normals,
        });
    }
    ensure_section_consumed(&cursor, "Triangulations")?;
    Ok(triangulations)
}

fn section_cursor<'a>(
    tokens: &'a [&'a str],
    section: &str,
    following: &str,
) -> Result<TokenCursor<'a>, CodecError> {
    let start = tokens
        .iter()
        .position(|token| *token == section)
        .ok_or_else(|| CodecError::Malformed(format!("text B-rep has no {section} table")))?
        + 2;
    let end = tokens
        .iter()
        .position(|token| *token == following)
        .ok_or_else(|| CodecError::Malformed(format!("text B-rep has no {following} table")))?;
    Ok(TokenCursor::new(&tokens[start..end]))
}

fn ensure_section_consumed(cursor: &TokenCursor<'_>, section: &str) -> Result<(), CodecError> {
    if cursor.is_empty() {
        Ok(())
    } else {
        Err(CodecError::Malformed(format!(
            "text B-rep {section} table contains trailing tokens"
        )))
    }
}

fn parse_tshapes(
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
    topology_version: u8,
) -> Result<(Vec<TextTShape>, Vec<TextShapeUse>), CodecError> {
    let start = tokens
        .iter()
        .position(|token| *token == "TShapes")
        .ok_or_else(|| CodecError::Malformed("text B-rep has no TShapes table".into()))?
        + 2;
    let count = section_counts.get("TShapes").copied().unwrap_or(0);
    let mut cursor = TokenCursor::new(&tokens[start..]);
    let mut shapes = Vec::with_capacity(count);
    for index in 1..=count {
        let kind = parse_shape_kind(cursor.next("TShape kind")?)?;
        let geometry = parse_tshape_geometry(kind, &mut cursor, section_counts, topology_version)?;
        let flags = parse_shape_flags(cursor.next("TShape flags")?, topology_version)?;
        let mut children = Vec::new();
        loop {
            if cursor.peek() == Some("*") {
                cursor.next("TShape child terminator")?;
                break;
            }
            let child = parse_shape_use(&mut cursor, count, section_counts)?;
            if child.shape >= index {
                return Err(CodecError::Malformed(format!(
                    "TShape {index} references non-prior child {}",
                    child.shape
                )));
            }
            children.push(child);
        }
        shapes.push(TextTShape {
            index,
            kind,
            geometry,
            flags,
            children,
        });
    }
    let mut roots = Vec::new();
    while !cursor.is_empty() {
        if cursor.peek() == Some("*") {
            cursor.next("root shape terminator")?;
            break;
        }
        roots.push(parse_shape_use(&mut cursor, count, section_counts)?);
    }
    if !cursor.is_empty() {
        return Err(CodecError::Malformed(
            "text B-rep contains tokens after root shape terminator".into(),
        ));
    }
    Ok((shapes, roots))
}

fn parse_shape_kind(token: &str) -> Result<TextShapeKind, CodecError> {
    match token {
        "Ve" => Ok(TextShapeKind::Vertex),
        "Ed" => Ok(TextShapeKind::Edge),
        "Wi" => Ok(TextShapeKind::Wire),
        "Fa" => Ok(TextShapeKind::Face),
        "Sh" => Ok(TextShapeKind::Shell),
        "So" => Ok(TextShapeKind::Solid),
        "CS" => Ok(TextShapeKind::CompSolid),
        "Co" => Ok(TextShapeKind::Compound),
        _ => Err(CodecError::Malformed(format!(
            "invalid TShape kind {token:?}"
        ))),
    }
}

fn parse_tshape_geometry(
    kind: TextShapeKind,
    cursor: &mut TokenCursor<'_>,
    counts: &BTreeMap<String, usize>,
    topology_version: u8,
) -> Result<TextTShapeGeometry, CodecError> {
    match kind {
        TextShapeKind::Vertex => parse_vertex_geometry(cursor, counts),
        TextShapeKind::Edge => parse_edge_geometry(cursor, counts, topology_version),
        TextShapeKind::Face => parse_face_geometry(cursor, counts),
        TextShapeKind::Wire
        | TextShapeKind::Shell
        | TextShapeKind::Solid
        | TextShapeKind::CompSolid
        | TextShapeKind::Compound => Ok(TextTShapeGeometry::Empty),
    }
}

fn parse_vertex_geometry(
    cursor: &mut TokenCursor<'_>,
    counts: &BTreeMap<String, usize>,
) -> Result<TextTShapeGeometry, CodecError> {
    let tolerance = cursor.real("vertex tolerance")?;
    let point = cursor.point("vertex point")?;
    let mut representations = Vec::new();
    loop {
        let parameter = cursor.real("vertex representation parameter")?;
        let kind = cursor.integer("vertex representation kind")?;
        if kind == 0 {
            break;
        }
        if representations.len() >= 1_000_000 {
            return Err(CodecError::Malformed(
                "vertex representation-count limit exceeded".into(),
            ));
        }
        let (second_parameter, curve, surface) = match kind {
            1 => (
                None,
                Some(parse_reference(
                    cursor,
                    "vertex curve",
                    counts["Curves"],
                    false,
                )?),
                None,
            ),
            2 => (
                None,
                Some(parse_reference(
                    cursor,
                    "vertex parameter curve",
                    counts["Curve2ds"],
                    false,
                )?),
                Some(parse_reference(
                    cursor,
                    "vertex surface",
                    counts["Surfaces"],
                    false,
                )?),
            ),
            3 => (
                Some(cursor.real("vertex second surface parameter")?),
                None,
                Some(parse_reference(
                    cursor,
                    "vertex surface",
                    counts["Surfaces"],
                    false,
                )?),
            ),
            other => {
                return Err(CodecError::Malformed(format!(
                    "invalid vertex representation kind {other}"
                )))
            }
        };
        let location = parse_reference(cursor, "vertex location", counts["Locations"], true)?;
        representations.push(TextPointRepresentation {
            parameter,
            second_parameter,
            kind: kind as u8,
            curve,
            surface,
            location,
        });
    }
    Ok(TextTShapeGeometry::Vertex {
        tolerance,
        point,
        representations,
    })
}

fn parse_edge_geometry(
    cursor: &mut TokenCursor<'_>,
    counts: &BTreeMap<String, usize>,
    topology_version: u8,
) -> Result<TextTShapeGeometry, CodecError> {
    let tolerance = cursor.real("edge tolerance")?;
    let same_parameter = cursor.boolean("edge same-parameter flag")?;
    let same_range = cursor.boolean("edge same-range flag")?;
    let degenerated = cursor.boolean("edge degenerated flag")?;
    let mut representations = Vec::new();
    loop {
        let kind = cursor.integer("edge representation kind")?;
        if kind == 0 {
            break;
        }
        if representations.len() >= 1_000_000 {
            return Err(CodecError::Malformed(
                "edge representation-count limit exceeded".into(),
            ));
        }
        representations.push(parse_edge_representation(
            kind,
            cursor,
            counts,
            topology_version,
        )?);
    }
    Ok(TextTShapeGeometry::Edge {
        tolerance,
        same_parameter,
        same_range,
        degenerated,
        representations,
    })
}

fn parse_edge_representation(
    kind: i64,
    cursor: &mut TokenCursor<'_>,
    counts: &BTreeMap<String, usize>,
    topology_version: u8,
) -> Result<TextEdgeRepresentation, CodecError> {
    let mut record = TextEdgeRepresentation {
        kind: u8::try_from(kind)
            .map_err(|_| CodecError::Malformed("invalid edge representation kind".into()))?,
        primary: 0,
        secondary: None,
        surface: None,
        second_surface: None,
        location: 0,
        second_location: None,
        parameter_range: None,
        continuity: None,
        uv_endpoints: None,
    };
    match kind {
        1 => {
            record.primary = parse_reference(cursor, "edge 3D curve", counts["Curves"], false)?;
            record.location =
                parse_reference(cursor, "edge curve location", counts["Locations"], true)?;
            record.parameter_range = Some(parse_range(cursor, "edge curve")?);
        }
        2 | 3 => {
            record.primary =
                parse_reference(cursor, "edge parameter curve", counts["Curve2ds"], false)?;
            if kind == 3 {
                record.secondary = Some(parse_reference(
                    cursor,
                    "edge secondary parameter curve",
                    counts["Curve2ds"],
                    false,
                )?);
                record.continuity = Some(cursor.next("edge continuity")?.to_owned());
            }
            record.surface = Some(parse_reference(
                cursor,
                "edge surface",
                counts["Surfaces"],
                false,
            )?);
            record.location =
                parse_reference(cursor, "edge surface location", counts["Locations"], true)?;
            record.parameter_range = Some(parse_range(cursor, "edge parameter curve")?);
            if topology_version == 2 {
                record.uv_endpoints = Some([
                    cursor.point2("edge first UV endpoint")?,
                    cursor.point2("edge last UV endpoint")?,
                ]);
            }
        }
        4 => {
            record.continuity = Some(cursor.next("edge continuity")?.to_owned());
            record.surface = Some(parse_reference(
                cursor,
                "edge regularity surface",
                counts["Surfaces"],
                false,
            )?);
            record.location = parse_reference(
                cursor,
                "edge regularity location",
                counts["Locations"],
                true,
            )?;
            record.second_surface = Some(parse_reference(
                cursor,
                "edge second regularity surface",
                counts["Surfaces"],
                false,
            )?);
            record.second_location = Some(parse_reference(
                cursor,
                "edge second regularity location",
                counts["Locations"],
                true,
            )?);
        }
        5 => {
            record.primary =
                parse_reference(cursor, "edge 3D polygon", counts["Polygon3D"], false)?;
            record.location =
                parse_reference(cursor, "edge polygon location", counts["Locations"], true)?;
        }
        6 | 7 => {
            record.primary = parse_reference(
                cursor,
                "edge polygon on triangulation",
                counts["PolygonOnTriangulations"],
                false,
            )?;
            if kind == 7 {
                record.secondary = Some(parse_reference(
                    cursor,
                    "edge second polygon on triangulation",
                    counts["PolygonOnTriangulations"],
                    false,
                )?);
            }
            record.surface = Some(parse_reference(
                cursor,
                "edge triangulation",
                counts["Triangulations"],
                false,
            )?);
            record.location = parse_reference(
                cursor,
                "edge triangulation location",
                counts["Locations"],
                true,
            )?;
        }
        other => {
            return Err(CodecError::Malformed(format!(
                "invalid edge representation kind {other}"
            )))
        }
    }
    Ok(record)
}

fn parse_face_geometry(
    cursor: &mut TokenCursor<'_>,
    counts: &BTreeMap<String, usize>,
) -> Result<TextTShapeGeometry, CodecError> {
    let natural_restriction = cursor.boolean("face natural-restriction flag")?;
    let tolerance = cursor.real("face tolerance")?;
    let surface = parse_reference(cursor, "face surface", counts["Surfaces"], true)?;
    let location = parse_reference(cursor, "face location", counts["Locations"], true)?;
    let triangulation = if cursor.peek() == Some("2") {
        cursor.next("face triangulation marker")?;
        Some(parse_reference(
            cursor,
            "face triangulation",
            counts["Triangulations"],
            false,
        )?)
    } else {
        None
    };
    Ok(TextTShapeGeometry::Face {
        natural_restriction,
        tolerance,
        surface,
        location,
        triangulation,
    })
}

fn parse_shape_flags(token: &str, topology_version: u8) -> Result<[bool; 7], CodecError> {
    if token.len() != 7 || !token.bytes().all(|byte| matches!(byte, b'0' | b'1')) {
        return Err(CodecError::Malformed(format!(
            "invalid TShape flags {token:?}"
        )));
    }
    let mut flags = [false; 7];
    for (index, byte) in token.bytes().enumerate() {
        flags[index] = byte == b'1';
    }
    if topology_version == 1 {
        flags[2] = false;
    }
    Ok(flags)
}

fn parse_shape_use(
    cursor: &mut TokenCursor<'_>,
    shape_count: usize,
    counts: &BTreeMap<String, usize>,
) -> Result<TextShapeUse, CodecError> {
    let token = cursor.next("shape use")?;
    let (orientation, encoded) = match token.as_bytes().first() {
        Some(b'+') => (TextOrientation::Forward, &token[1..]),
        Some(b'-') => (TextOrientation::Reversed, &token[1..]),
        Some(b'i') => (TextOrientation::Internal, &token[1..]),
        Some(b'e') => (TextOrientation::External, &token[1..]),
        _ => {
            return Err(CodecError::Malformed(format!(
                "invalid shape use {token:?}"
            )))
        }
    };
    let encoded = encoded
        .parse::<usize>()
        .map_err(|_| CodecError::Malformed(format!("invalid shape use {token:?}")))?;
    if encoded == 0 || encoded > shape_count {
        return Err(CodecError::Malformed(format!(
            "shape use index {encoded} is out of range"
        )));
    }
    let shape = shape_count - encoded + 1;
    let location = parse_reference(cursor, "shape use location", counts["Locations"], true)?;
    Ok(TextShapeUse {
        shape,
        orientation,
        location,
    })
}

fn parse_reference(
    cursor: &mut TokenCursor<'_>,
    label: &str,
    maximum: usize,
    allow_zero: bool,
) -> Result<usize, CodecError> {
    let value = cursor.count(label, maximum)?;
    if value == 0 && !allow_zero {
        return Err(CodecError::Malformed(format!("{label} index is zero")));
    }
    Ok(value)
}

fn parse_range(cursor: &mut TokenCursor<'_>, label: &str) -> Result<[f64; 2], CodecError> {
    let range = [
        cursor.real(&format!("{label} first parameter"))?,
        cursor.real(&format!("{label} last parameter"))?,
    ];
    if range[0] > range[1] {
        return Err(CodecError::Malformed(format!(
            "{label} parameter range is reversed"
        )));
    }
    Ok(range)
}

fn parse_surfaces(
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
) -> Result<Vec<TextSurface>, CodecError> {
    let start = tokens
        .iter()
        .position(|token| *token == "Surfaces")
        .ok_or_else(|| CodecError::Malformed("text B-rep has no Surfaces table".into()))?
        + 2;
    let end = tokens
        .iter()
        .position(|token| *token == "Triangulations")
        .ok_or_else(|| CodecError::Malformed("text B-rep has no Triangulations table".into()))?;
    let count = section_counts.get("Surfaces").copied().unwrap_or(0);
    let mut cursor = TokenCursor::new(&tokens[start..end]);
    let mut surfaces = Vec::with_capacity(count);
    for index in 0..count {
        surfaces.push(parse_surface(&mut cursor, 0, index + 1)?);
    }
    if !cursor.is_empty() {
        return Err(CodecError::Malformed(
            "text B-rep Surfaces table contains trailing tokens".into(),
        ));
    }
    Ok(surfaces)
}

fn parse_surface(
    cursor: &mut TokenCursor<'_>,
    depth: usize,
    table_index: usize,
) -> Result<TextSurface, CodecError> {
    if depth > 64 {
        return Err(CodecError::Malformed(
            "text B-rep surface recursion limit exceeded".into(),
        ));
    }
    let kind = cursor.integer("surface type")?;
    Ok(match kind {
        1..=5 => parse_analytic_surface(kind, cursor)?,
        6 => {
            let direction = cursor.vector("extrusion direction")?;
            if direction.norm() == 0.0 {
                return Err(CodecError::Malformed("extrusion direction is zero".into()));
            }
            TextSurface::Extrusion {
                direction,
                directrix: Box::new(parse_curve(cursor, 0, table_index)?),
            }
        }
        7 => {
            let axis_origin = cursor.point("revolution axis origin")?;
            let axis_direction = cursor.vector("revolution axis direction")?;
            if axis_direction.norm() == 0.0 {
                return Err(CodecError::Malformed(
                    "revolution axis direction is zero".into(),
                ));
            }
            TextSurface::Revolution {
                axis_origin,
                axis_direction,
                directrix: Box::new(parse_curve(cursor, 0, table_index)?),
            }
        }
        8 => TextSurface::Nurbs(parse_bezier_surface(cursor)?),
        9 => TextSurface::Nurbs(parse_nurbs_surface(cursor)?),
        10 => {
            let u_range = [
                cursor.real("trimmed surface first u parameter")?,
                cursor.real("trimmed surface last u parameter")?,
            ];
            let v_range = [
                cursor.real("trimmed surface first v parameter")?,
                cursor.real("trimmed surface last v parameter")?,
            ];
            if u_range[0] > u_range[1] || v_range[0] > v_range[1] {
                return Err(CodecError::Malformed(
                    "trimmed surface parameter range is reversed".into(),
                ));
            }
            TextSurface::Trimmed {
                parameter_ranges: [u_range, v_range],
                basis: Box::new(parse_surface(cursor, depth + 1, table_index)?),
            }
        }
        11 => TextSurface::Offset {
            distance: cursor.real("offset surface distance")?,
            basis: Box::new(parse_surface(cursor, depth + 1, table_index)?),
        },
        other => {
            return Err(CodecError::NotImplemented(format!(
                "text B-rep surface family {other} at table index {table_index}"
            )))
        }
    })
}

fn parse_analytic_surface(
    kind: i64,
    cursor: &mut TokenCursor<'_>,
) -> Result<TextSurface, CodecError> {
    let origin = cursor.point("surface origin")?;
    let axis = cursor.vector("surface axis")?;
    let ref_direction = cursor.vector("surface reference direction")?;
    let _y_direction = cursor.vector("surface y direction")?;
    Ok(match kind {
        1 => TextSurface::Plane {
            origin,
            axis,
            u_axis: ref_direction,
        },
        2 => TextSurface::Cylinder {
            origin,
            axis,
            ref_direction,
            radius: cursor.real("cylinder radius")?,
        },
        3 => TextSurface::Cone {
            origin,
            axis,
            ref_direction,
            radius: cursor.real("cone radius")?,
            half_angle: cursor.real("cone half angle")?,
        },
        4 => TextSurface::Sphere {
            center: origin,
            axis,
            ref_direction,
            radius: cursor.real("sphere radius")?,
        },
        5 => TextSurface::Torus {
            center: origin,
            axis,
            ref_direction,
            major_radius: cursor.real("torus major radius")?,
            minor_radius: cursor.real("torus minor radius")?,
        },
        _ => unreachable!("analytic surface kind was range checked"),
    })
}

fn parse_nurbs_surface(cursor: &mut TokenCursor<'_>) -> Result<NurbsSurface, CodecError> {
    let u_rational = cursor.boolean("B-spline u rational flag")?;
    let v_rational = cursor.boolean("B-spline v rational flag")?;
    let rational = u_rational || v_rational;
    let u_periodic = cursor.boolean("B-spline u periodic flag")?;
    let v_periodic = cursor.boolean("B-spline v periodic flag")?;
    let u_degree = cursor.count("B-spline u degree", 64)?;
    let v_degree = cursor.count("B-spline v degree", 64)?;
    let u_count = cursor.count("B-spline u pole count", 1_000_000)?;
    let v_count = cursor.count("B-spline v pole count", 1_000_000)?;
    let u_knot_count = cursor.count("B-spline u knot count", 1_000_000)?;
    let v_knot_count = cursor.count("B-spline v knot count", 1_000_000)?;
    let pole_count = u_count
        .checked_mul(v_count)
        .filter(|count| *count <= 1_000_000)
        .ok_or_else(|| CodecError::Malformed("B-spline surface pole limit exceeded".into()))?;
    let mut control_points = Vec::with_capacity(pole_count);
    let mut weights = rational.then(|| Vec::with_capacity(pole_count));
    for _ in 0..pole_count {
        control_points.push(cursor.point("B-spline surface pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.real("B-spline surface weight")?);
        }
    }
    let u_knots = parse_knots(cursor, u_knot_count, u_degree, "B-spline u")?;
    let v_knots = parse_knots(cursor, v_knot_count, v_degree, "B-spline v")?;
    Ok(NurbsSurface {
        u_degree: u_degree as u32,
        v_degree: v_degree as u32,
        u_knots,
        v_knots,
        u_count: u_count as u32,
        v_count: v_count as u32,
        control_points,
        weights,
        u_periodic,
        v_periodic,
    })
}

fn parse_bezier_surface(cursor: &mut TokenCursor<'_>) -> Result<NurbsSurface, CodecError> {
    let u_rational = cursor.boolean("Bezier u rational flag")?;
    let v_rational = cursor.boolean("Bezier v rational flag")?;
    let rational = u_rational || v_rational;
    let u_degree = cursor.count("Bezier u degree", 64)?;
    let v_degree = cursor.count("Bezier v degree", 64)?;
    let u_count = u_degree + 1;
    let v_count = v_degree + 1;
    let pole_count = u_count
        .checked_mul(v_count)
        .ok_or_else(|| CodecError::Malformed("Bezier surface pole count overflow".into()))?;
    let mut control_points = Vec::with_capacity(pole_count);
    let mut weights = rational.then(|| Vec::with_capacity(pole_count));
    for _ in 0..pole_count {
        control_points.push(cursor.point("Bezier surface pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.real("Bezier surface weight")?);
        }
    }
    Ok(NurbsSurface {
        u_degree: u_degree as u32,
        v_degree: v_degree as u32,
        u_knots: clamped_bezier_knots(u_degree),
        v_knots: clamped_bezier_knots(v_degree),
        u_count: u_count as u32,
        v_count: v_count as u32,
        control_points,
        weights,
        u_periodic: false,
        v_periodic: false,
    })
}

fn parse_knots(
    cursor: &mut TokenCursor<'_>,
    knot_count: usize,
    degree: usize,
    label: &str,
) -> Result<Vec<f64>, CodecError> {
    let mut knots = Vec::new();
    for _ in 0..knot_count {
        let knot = cursor.real(&format!("{label} knot"))?;
        let multiplicity = cursor.count(&format!("{label} knot multiplicity"), degree + 1)?;
        let expanded = knots
            .len()
            .checked_add(multiplicity)
            .filter(|count| *count <= 2_000_000)
            .ok_or_else(|| {
                CodecError::Malformed(format!("expanded {label} knot limit exceeded"))
            })?;
        knots.resize(expanded, knot);
    }
    Ok(knots)
}

fn parse_curves(
    tokens: &[&str],
    section_counts: &BTreeMap<String, usize>,
) -> Result<Vec<TextCurve>, CodecError> {
    let start = tokens
        .iter()
        .position(|token| *token == "Curves")
        .ok_or_else(|| CodecError::Malformed("text B-rep has no Curves table".into()))?
        + 2;
    let end = tokens
        .iter()
        .position(|token| *token == "Polygon3D")
        .ok_or_else(|| CodecError::Malformed("text B-rep has no Polygon3D table".into()))?;
    let count = section_counts.get("Curves").copied().unwrap_or(0);
    let mut cursor = TokenCursor::new(&tokens[start..end]);
    let mut curves = Vec::with_capacity(count);
    for index in 0..count {
        curves.push(parse_curve(&mut cursor, 0, index + 1)?);
    }
    if !cursor.is_empty() {
        return Err(CodecError::Malformed(
            "text B-rep Curves table contains trailing tokens".into(),
        ));
    }
    Ok(curves)
}

fn parse_curve(
    cursor: &mut TokenCursor<'_>,
    depth: usize,
    table_index: usize,
) -> Result<TextCurve, CodecError> {
    if depth > 64 {
        return Err(CodecError::Malformed(
            "text B-rep curve recursion limit exceeded".into(),
        ));
    }
    let kind = cursor.integer("curve type")?;
    Ok(match kind {
        1 => TextCurve::Line {
            origin: cursor.point("line origin")?,
            direction: cursor.vector("line direction")?,
        },
        2 => {
            let center = cursor.point("circle center")?;
            let axis = cursor.vector("circle axis")?;
            let ref_direction = cursor.vector("circle reference direction")?;
            let _y_direction = cursor.vector("circle y direction")?;
            TextCurve::Circle {
                center,
                axis,
                ref_direction,
                radius: cursor.real("circle radius")?,
            }
        }
        3 => {
            let center = cursor.point("ellipse center")?;
            let axis = cursor.vector("ellipse axis")?;
            let major_direction = cursor.vector("ellipse major direction")?;
            let _y_direction = cursor.vector("ellipse y direction")?;
            TextCurve::Ellipse {
                center,
                axis,
                major_direction,
                major_radius: cursor.real("ellipse major radius")?,
                minor_radius: cursor.real("ellipse minor radius")?,
            }
        }
        4 => {
            let vertex = cursor.point("parabola vertex")?;
            let axis = cursor.vector("parabola axis")?;
            let major_direction = cursor.vector("parabola major direction")?;
            let _y_direction = cursor.vector("parabola y direction")?;
            TextCurve::Parabola {
                vertex,
                axis,
                major_direction,
                focal_distance: cursor.real("parabola focal distance")?,
            }
        }
        5 => {
            let center = cursor.point("hyperbola center")?;
            let axis = cursor.vector("hyperbola axis")?;
            let major_direction = cursor.vector("hyperbola major direction")?;
            let _y_direction = cursor.vector("hyperbola y direction")?;
            TextCurve::Hyperbola {
                center,
                axis,
                major_direction,
                major_radius: cursor.real("hyperbola major radius")?,
                minor_radius: cursor.real("hyperbola minor radius")?,
            }
        }
        6 => TextCurve::Nurbs(parse_bezier_curve(cursor)?),
        7 => TextCurve::Nurbs(parse_nurbs_curve(cursor)?),
        8 => {
            let first = cursor.real("trimmed curve first parameter")?;
            let last = cursor.real("trimmed curve last parameter")?;
            if first > last {
                return Err(CodecError::Malformed(
                    "trimmed curve parameter range is reversed".into(),
                ));
            }
            TextCurve::Trimmed {
                parameter_range: [first, last],
                basis: Box::new(parse_curve(cursor, depth + 1, table_index)?),
            }
        }
        9 => {
            let distance = cursor.real("offset curve distance")?;
            let direction = cursor.vector("offset curve direction")?;
            if direction.norm() == 0.0 {
                return Err(CodecError::Malformed(
                    "offset curve direction is zero".into(),
                ));
            }
            TextCurve::Offset {
                distance,
                direction,
                basis: Box::new(parse_curve(cursor, depth + 1, table_index)?),
            }
        }
        other => {
            return Err(CodecError::NotImplemented(format!(
                "text B-rep 3D curve family {other} at table index {table_index}"
            )))
        }
    })
}

fn parse_nurbs_curve(cursor: &mut TokenCursor<'_>) -> Result<NurbsCurve, CodecError> {
    let rational = cursor.boolean("B-spline rational flag")?;
    let periodic = cursor.boolean("B-spline periodic flag")?;
    let degree = cursor.count("B-spline degree", 64)?;
    let pole_count = cursor.count("B-spline pole count", 1_000_000)?;
    let knot_count = cursor.count("B-spline knot count", 1_000_000)?;
    let mut control_points = Vec::with_capacity(pole_count);
    let mut weights = rational.then(|| Vec::with_capacity(pole_count));
    for _ in 0..pole_count {
        control_points.push(cursor.point("B-spline pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.real("B-spline weight")?);
        }
    }
    let knots = parse_knots(cursor, knot_count, degree, "B-spline")?;
    Ok(NurbsCurve {
        degree: degree as u32,
        knots,
        control_points,
        weights,
        periodic,
    })
}

fn parse_bezier_curve(cursor: &mut TokenCursor<'_>) -> Result<NurbsCurve, CodecError> {
    let rational = cursor.boolean("Bezier rational flag")?;
    let degree = cursor.count("Bezier degree", 64)?;
    let pole_count = degree + 1;
    let mut control_points = Vec::with_capacity(pole_count);
    let mut weights = rational.then(|| Vec::with_capacity(pole_count));
    for _ in 0..pole_count {
        control_points.push(cursor.point("Bezier pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.real("Bezier weight")?);
        }
    }
    Ok(NurbsCurve {
        degree: degree as u32,
        knots: clamped_bezier_knots(degree),
        control_points,
        weights,
        periodic: false,
    })
}

fn clamped_bezier_knots(degree: usize) -> Vec<f64> {
    std::iter::repeat_n(0.0, degree + 1)
        .chain(std::iter::repeat_n(1.0, degree + 1))
        .collect()
}

struct TokenCursor<'a> {
    tokens: &'a [&'a str],
    index: usize,
}

impl<'a> TokenCursor<'a> {
    fn new(tokens: &'a [&'a str]) -> Self {
        Self { tokens, index: 0 }
    }

    fn is_empty(&self) -> bool {
        self.index == self.tokens.len()
    }

    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.index).copied()
    }

    fn integer(&mut self, label: &str) -> Result<i64, CodecError> {
        self.next(label)?.parse().map_err(|_| {
            CodecError::Malformed(format!("invalid {label} in text B-rep Curves table"))
        })
    }

    fn count(&mut self, label: &str, maximum: usize) -> Result<usize, CodecError> {
        let value = self.integer(label)?;
        let value = usize::try_from(value)
            .map_err(|_| CodecError::Malformed(format!("negative {label}")))?;
        if value > maximum {
            return Err(CodecError::Malformed(format!("{label} limit exceeded")));
        }
        Ok(value)
    }

    fn boolean(&mut self, label: &str) -> Result<bool, CodecError> {
        match self.integer(label)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CodecError::Malformed(format!("invalid {label}"))),
        }
    }

    fn real(&mut self, label: &str) -> Result<f64, CodecError> {
        let value = self.next(label)?.parse::<f64>().map_err(|_| {
            CodecError::Malformed(format!("invalid {label} in text B-rep Curves table"))
        })?;
        if !value.is_finite() {
            return Err(CodecError::Malformed(format!(
                "non-finite {label} in text B-rep Curves table"
            )));
        }
        Ok(value)
    }

    fn point(&mut self, label: &str) -> Result<Point3, CodecError> {
        Ok(Point3::new(
            self.real(label)?,
            self.real(label)?,
            self.real(label)?,
        ))
    }

    fn point2(&mut self, label: &str) -> Result<Point2, CodecError> {
        Ok(Point2::new(self.real(label)?, self.real(label)?))
    }

    fn vector(&mut self, label: &str) -> Result<Vector3, CodecError> {
        Ok(Vector3::new(
            self.real(label)?,
            self.real(label)?,
            self.real(label)?,
        ))
    }

    fn next(&mut self, label: &str) -> Result<&'a str, CodecError> {
        let token = self.tokens.get(self.index).copied().ok_or_else(|| {
            CodecError::Malformed(format!("truncated {label} in text B-rep Curves table"))
        })?;
        self.index += 1;
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_brep(curves: &str, curve_count: usize, surfaces: &str, surface_count: usize) -> String {
        format!(
            "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves {curve_count}\n{curves}\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces {surface_count}\n{surfaces}\nTriangulations 0\nTShapes 0\n*"
        )
    }

    #[test]
    fn normalizes_rational_bezier_curve_to_nurbs() {
        let input = text_brep("6 1 2 0 0 0 1 5 0 0 2 10 0 0 1", 1, "", 0);
        let facts = parse_text(input.as_bytes()).expect("valid Bezier curve");
        let TextCurve::Nurbs(curve) = &facts.curves[0] else {
            panic!("Bezier curve was not normalized to NURBS")
        };
        assert_eq!(curve.degree, 2);
        assert_eq!(curve.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        assert_eq!(curve.weights.as_deref(), Some(&[1.0, 2.0, 1.0][..]));
    }

    #[test]
    fn normalizes_bezier_surface_to_nurbs() {
        let input = text_brep("", 0, "8 0 0 1 1 0 0 0 0 1 0 1 0 0 1 1 0", 1);
        let facts = parse_text(input.as_bytes()).expect("valid Bezier surface");
        let TextSurface::Nurbs(surface) = &facts.surfaces[0] else {
            panic!("Bezier surface was not normalized to NURBS")
        };
        assert_eq!((surface.u_degree, surface.v_degree), (1, 1));
        assert_eq!((surface.u_count, surface.v_count), (2, 2));
        assert_eq!(surface.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
        assert_eq!(surface.v_knots, vec![0.0, 0.0, 1.0, 1.0]);
        assert!(surface.weights.is_none());
    }

    #[test]
    fn rejects_invalid_recursive_curve_domains() {
        let reversed = text_brep("8 2 1 1 0 0 0 1 0 0", 1, "", 0);
        let error = parse_text(reversed.as_bytes()).expect_err("reversed trim must fail");
        assert!(error.to_string().contains("parameter range is reversed"));

        let zero_normal = text_brep("9 2 0 0 0 1 0 0 0 1 0 0", 1, "", 0);
        let error = parse_text(zero_normal.as_bytes()).expect_err("zero normal must fail");
        assert!(error.to_string().contains("direction is zero"));
    }

    #[test]
    fn parses_recursive_surface_constructions() {
        let input = text_brep(
            "",
            0,
            "6 0 0 2 1 0 0 0 1 0 0\n7 0 0 0 0 0 1 1 0 0 0 1 0 0\n10 0 1 2 3 11 4 1 0 0 0 0 0 1 1 0 0 0 1 0",
            3,
        );
        let facts = parse_text(input.as_bytes()).expect("recursive surfaces");
        let TextSurface::Extrusion {
            direction,
            directrix,
        } = &facts.surfaces[0]
        else {
            panic!("expected extrusion")
        };
        assert_eq!([direction.x, direction.y, direction.z], [0.0, 0.0, 2.0]);
        assert!(matches!(directrix.as_ref(), TextCurve::Line { .. }));

        let TextSurface::Revolution { directrix, .. } = &facts.surfaces[1] else {
            panic!("expected revolution")
        };
        assert!(matches!(directrix.as_ref(), TextCurve::Line { .. }));

        let TextSurface::Trimmed {
            parameter_ranges,
            basis,
        } = &facts.surfaces[2]
        else {
            panic!("expected trimmed surface")
        };
        assert_eq!(*parameter_ranges, [[0.0, 1.0], [2.0, 3.0]]);
        assert!(matches!(basis.as_ref(), TextSurface::Offset { .. }));
    }

    #[test]
    fn resolves_elementary_and_compound_locations_in_source_order() {
        let input = "CASCADE Topology V1, (c) Matra-Datavision\nLocations 3\n1 1 0 0 5 0 1 0 0 0 0 1 0\n2 1 2 0\n2 1 -1 2 1 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
        let facts = parse_text(input.as_bytes()).expect("location table");
        assert_eq!(facts.locations.len(), 3);
        assert_eq!(facts.locations[0].transform.rows[0][3], 5.0);
        assert_eq!(facts.locations[1].transform.rows[0][3], 10.0);
        assert_eq!(facts.locations[2].transform.rows[0][3], 5.0);
        assert_eq!(facts.locations[2].factors[0].power, -1);
    }

    #[test]
    fn parses_binary_locations_and_recursive_parameter_curves() {
        fn real(bytes: &mut Vec<u8>, value: f64) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }

        let mut bytes = b"\nOpen CASCADE Topology V3 (c)\nLocations 1\n".to_vec();
        bytes.push(1);
        for value in [1.0, 0.0, 0.0, 5.0, 0.0, 1.0, 0.0, 6.0, 0.0, 0.0, 1.0, 7.0] {
            real(&mut bytes, value);
        }
        bytes.extend_from_slice(b"Curve2ds 2\n");
        bytes.push(1);
        for value in [0.0, 0.0, 1.0, 0.0] {
            real(&mut bytes, value);
        }
        bytes.push(8);
        real(&mut bytes, 0.0);
        real(&mut bytes, std::f64::consts::PI);
        bytes.push(2);
        for value in [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 2.0] {
            real(&mut bytes, value);
        }
        bytes.extend_from_slice(b"Curves 2\n");
        bytes.push(1);
        for value in [0.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
            real(&mut bytes, value);
        }
        bytes.push(9);
        for value in [0.5, 0.0, 0.0, 1.0] {
            real(&mut bytes, value);
        }
        bytes.push(1);
        for value in [1.0, 2.0, 3.0, 0.0, 1.0, 0.0] {
            real(&mut bytes, value);
        }
        bytes.extend_from_slice(b"Polygon3D 1\n");
        bytes.extend_from_slice(&2_i32.to_le_bytes());
        bytes.push(1);
        real(&mut bytes, 0.01);
        for value in [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] {
            real(&mut bytes, value);
        }
        bytes.extend_from_slice(b"PolygonOnTriangulations 1\n");
        bytes.extend_from_slice(&2_i32.to_le_bytes());
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.extend_from_slice(&2_i32.to_le_bytes());
        real(&mut bytes, 0.02);
        bytes.push(0);
        bytes.extend_from_slice(b"Surfaces 0\n");

        let facts = parse_binary_prefix(&bytes).expect("binary prefix");
        assert_eq!(facts.topology_version, 3);
        assert_eq!(facts.locations[0].transform.rows[0][3], 5.0);
        assert!(matches!(facts.curve2ds[0], TextCurve2d::Line { .. }));
        assert!(matches!(facts.curve2ds[1], TextCurve2d::Trimmed { .. }));
        assert!(matches!(facts.curves[0], TextCurve::Line { .. }));
        assert!(matches!(facts.curves[1], TextCurve::Offset { .. }));
        assert_eq!(facts.polygons3d[0].nodes.len(), 2);
        assert_eq!(
            facts.polygons3d[0].parameters.as_deref(),
            Some(&[0.0, 1.0][..])
        );
        assert_eq!(facts.polygons_on_triangulations[0].nodes, [1, 2]);
        assert_eq!(&bytes[facts.surfaces_offset..], b"");
    }

    #[test]
    fn parses_analytic_spline_and_recursive_parameter_curves() {
        let input = "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 3\n1 0 0 1 0\n6 1 2 0 0 1 5 0 2 10 0 1\n8 0 6.28 9 2 2 0 0 1 0 0 1 3\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
        let facts = parse_text(input.as_bytes()).expect("2D curve table");
        assert!(matches!(facts.curve2ds[0], TextCurve2d::Line { .. }));
        let TextCurve2d::Nurbs(nurbs) = &facts.curve2ds[1] else {
            panic!("expected normalized 2D Bezier")
        };
        assert_eq!(nurbs.degree, 2);
        assert_eq!(nurbs.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        assert_eq!(nurbs.weights.as_deref(), Some(&[1.0, 2.0, 1.0][..]));
        let TextCurve2d::Trimmed {
            parameter_range,
            basis,
        } = &facts.curve2ds[2]
        else {
            panic!("expected trimmed 2D curve")
        };
        assert_eq!(*parameter_range, [0.0, 6.28]);
        assert!(matches!(basis.as_ref(), TextCurve2d::Offset { .. }));
    }

    #[test]
    fn parses_polygonal_carriers_and_version_three_normals() {
        let input = "CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 1\n2 1 0.1 0 0 0 1 0 0 0 1\nPolygonOnTriangulations 1\n2 1 2 p 0.2 1 0 1\nSurfaces 0\nTriangulations 1\n3 1 1 1 0.01 0 0 0 1 0 0 0 1 0 0 0 1 0 0 1 1 2 3 0 0 1 0 0 1 0 0 1\nTShapes 0\n*";
        let facts = parse_text(input.as_bytes()).expect("polygonal carriers");
        assert_eq!(facts.polygons3d[0].nodes.len(), 2);
        assert_eq!(
            facts.polygons3d[0].parameters.as_deref(),
            Some(&[0.0, 1.0][..])
        );
        assert_eq!(facts.polygons_on_triangulations[0].nodes, [1, 2]);
        let triangulation = &facts.triangulations[0];
        assert_eq!(triangulation.nodes.len(), 3);
        assert_eq!(triangulation.triangles, [[1, 2, 3]]);
        assert_eq!(triangulation.uv_nodes.as_ref().map(Vec::len), Some(3));
        assert_eq!(triangulation.normals.as_ref().map(Vec::len), Some(3));
    }

    #[test]
    fn parses_subshape_first_topology_and_reverse_references() {
        let input = "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 1\n1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 8\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\nEd 0.001 1 1 0 1 1 0 0 1 0 1001000 +8 0 +7 0 *\nWi 1001000 +6 0 *\nFa 0 0.001 1 0 1001000 +5 0 *\nSh 1001000 +4 0 *\nSo 1001000 +3 0 *\nCo 1001000 +2 0 *\n+1 0 *";
        let facts = parse_text(input.as_bytes()).expect("topology table");
        assert_eq!(facts.tshapes.len(), 8);
        assert_eq!(facts.tshapes[2].kind, TextShapeKind::Edge);
        assert_eq!(facts.tshapes[2].children[0].shape, 1);
        assert_eq!(facts.tshapes[2].children[1].shape, 2);
        let TextTShapeGeometry::Edge {
            representations, ..
        } = &facts.tshapes[2].geometry
        else {
            panic!("expected edge geometry")
        };
        assert_eq!(representations[0].parameter_range, Some([0.0, 1.0]));
        assert_eq!(facts.roots.len(), 1);
        assert_eq!(facts.roots[0].shape, 8);
    }
}
