// SPDX-License-Identifier: Apache-2.0
//! Bounded framing for text and binary exact-shape side entries.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{bounded_len, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, ProceduralCurve, ProceduralCurveDefinition,
    ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::SourceObjectAssociation;
use serde::{Deserialize, Serialize};

use crate::native::{self, EntryRecord, PropertyRecord};

/// Exact-shape side-entry form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapePayloadForm {
    /// Explicit zero-byte null shape.
    Empty,
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
    /// Ordered exact surface table.
    pub surfaces: Vec<TextSurface>,
    /// Ordered display triangulation table.
    pub triangulations: Vec<TextTriangulation>,
    /// Ordered subshape-first topology records.
    pub tshapes: Vec<TextTShape>,
    /// Root shape use stored after the shape set.
    pub roots: Vec<TextShapeUse>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
        v_reversed: bool,
    },
    /// Circular cylinder.
    Cylinder {
        origin: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
        u_reversed: bool,
    },
    /// Circular cone.
    Cone {
        origin: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
        half_angle: f64,
        u_reversed: bool,
    },
    /// Sphere.
    Sphere {
        center: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
        u_reversed: bool,
    },
    /// Torus.
    Torus {
        center: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        major_radius: f64,
        minor_radius: f64,
        u_reversed: bool,
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SurfaceParameterAffine {
    pub(crate) u_scale: f64,
    pub(crate) u_offset: f64,
    pub(crate) v_scale: f64,
    pub(crate) v_offset: f64,
}

pub(crate) fn surface_parameter_affine(surface: &TextSurface) -> SurfaceParameterAffine {
    let identity = SurfaceParameterAffine {
        u_scale: 1.0,
        u_offset: 0.0,
        v_scale: 1.0,
        v_offset: 0.0,
    };
    match surface {
        TextSurface::Plane {
            v_reversed: true, ..
        } => SurfaceParameterAffine {
            v_scale: -1.0,
            ..identity
        },
        TextSurface::Cylinder {
            u_reversed: true, ..
        }
        | TextSurface::Sphere {
            u_reversed: true, ..
        }
        | TextSurface::Torus {
            u_reversed: true, ..
        } => SurfaceParameterAffine {
            u_scale: -1.0,
            ..identity
        },
        TextSurface::Cone {
            half_angle,
            u_reversed,
            ..
        } => SurfaceParameterAffine {
            u_scale: if *u_reversed { -1.0 } else { 1.0 },
            v_scale: half_angle.cos(),
            ..identity
        },
        TextSurface::Trimmed {
            parameter_ranges,
            basis,
        } => {
            let basis = surface_parameter_affine(basis);
            let u_scale = basis.u_scale.abs();
            let v_scale = basis.v_scale.abs();
            SurfaceParameterAffine {
                u_scale,
                u_offset: -parameter_ranges[0][0] * u_scale,
                v_scale,
                v_offset: -parameter_ranges[1][0] * v_scale,
            }
        }
        TextSurface::Offset { basis, .. } => surface_parameter_affine(basis),
        _ => identity,
    }
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
    let mut payloads = Vec::new();
    for property in properties
        .iter()
        .filter(|property| property.type_name == "Part::PropertyPartShape")
    {
        let Some(name) = direct_shape_entry(property)? else {
            continue;
        };
        let entry = entries.get(name.as_str()).ok_or_else(|| {
            CodecError::malformed(format_args!("missing exact-shape entry {name}"))
        })?;
        let form = if entry.data.is_empty() {
            ShapePayloadForm::Empty
        } else if name.to_ascii_lowercase().ends_with(".bin") {
            ShapePayloadForm::Binary
        } else {
            ShapePayloadForm::Text
        };
        let (text, binary) = match form {
            ShapePayloadForm::Empty => (None, None),
            ShapePayloadForm::Text => (Some(parse_text(&entry.data)?), None),
            ShapePayloadForm::Binary => (None, Some(parse_binary_prefix(&entry.data)?)),
        };
        payloads.push(ShapePayloadRecord {
            id: crate::native::native_child_id("shape-payload", &property.id, &name),
            property: property.id.clone(),
            entry: entry.id.clone(),
            form,
            text,
            binary,
        });
    }
    Ok(payloads)
}

fn direct_shape_entry(property: &PropertyRecord) -> Result<Option<String>, CodecError> {
    let document = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
        CodecError::malformed(format_args!(
            "invalid exact-shape property XML {}: {error}",
            property.id
        ))
    })?;
    let root = document.root_element();
    if !matches!(root.tag_name().name(), "Property" | "_Property") {
        return Err(CodecError::malformed(format_args!(
            "exact-shape property {} has no property record root",
            property.id
        )));
    }
    let parts = root
        .children()
        .filter(|node| node.has_tag_name("Part"))
        .collect::<Vec<_>>();
    let part = match parts.as_slice() {
        [] => return Ok(None),
        [part] => *part,
        _ => {
            return Err(CodecError::malformed(format_args!(
                "exact-shape property {} has multiple direct Part carriers",
                property.id
            )));
        }
    };
    Ok(part
        .attribute("file")
        .filter(|file| !file.is_empty())
        .map(str::to_owned))
}

/// Derive an exhaustive family census from successfully parsed exact-shape payloads.
pub fn carrier_census(payloads: &[ShapePayloadRecord]) -> Vec<crate::native::CarrierCensusRecord> {
    payloads
        .iter()
        .filter_map(|payload| {
            let (version, curve2ds, curves, surfaces, polygons3d, indexed, triangulations, tshapes) =
                if let Some(facts) = &payload.text {
                    (
                        facts.topology_version,
                        &facts.curve2ds,
                        &facts.curves,
                        &facts.surfaces,
                        facts.polygons3d.len(),
                        facts.polygons_on_triangulations.len(),
                        facts.triangulations.len(),
                        &facts.tshapes,
                    )
                } else if let Some(facts) = &payload.binary {
                    (
                        facts.topology_version,
                        &facts.curve2ds,
                        &facts.curves,
                        &facts.surfaces,
                        facts.polygons3d.len(),
                        facts.polygons_on_triangulations.len(),
                        facts.triangulations.len(),
                        &facts.tshapes,
                    )
                } else {
                    return None;
                };
            let mut record = crate::native::CarrierCensusRecord {
                id: crate::native::native_child_id("carrier-census", &payload.id, "families"),
                payload: payload.id.clone(),
                form: match payload.form {
                    ShapePayloadForm::Empty => "empty".into(),
                    ShapePayloadForm::Text => "text".into(),
                    ShapePayloadForm::Binary => "binary".into(),
                },
                topology_version: version,
                curves_2d: BTreeMap::new(),
                curves_3d: BTreeMap::new(),
                surfaces: BTreeMap::new(),
                topology: BTreeMap::new(),
                polygons_3d: polygons3d as u64,
                polygons_on_triangulations: indexed as u64,
                triangulations: triangulations as u64,
            };
            for curve in curve2ds {
                census_curve2d(curve, &mut record.curves_2d);
            }
            for curve in curves {
                census_curve(curve, &mut record.curves_3d);
            }
            for surface in surfaces {
                census_surface(surface, &mut record.surfaces, &mut record.curves_3d);
            }
            for shape in tshapes {
                increment(
                    &mut record.topology,
                    match shape.kind {
                        TextShapeKind::Vertex => "vertex",
                        TextShapeKind::Edge => "edge",
                        TextShapeKind::Wire => "wire",
                        TextShapeKind::Face => "face",
                        TextShapeKind::Shell => "shell",
                        TextShapeKind::Solid => "solid",
                        TextShapeKind::CompSolid => "compsolid",
                        TextShapeKind::Compound => "compound",
                    },
                );
            }
            Some(record)
        })
        .collect()
}

fn increment(counts: &mut BTreeMap<String, u64>, family: &str) {
    *counts.entry(family.into()).or_default() += 1;
}

fn census_curve2d(curve: &TextCurve2d, counts: &mut BTreeMap<String, u64>) {
    let family = match curve {
        TextCurve2d::Line { .. } => "line",
        TextCurve2d::Circle { .. } => "circle",
        TextCurve2d::Ellipse { .. } => "ellipse",
        TextCurve2d::Parabola { .. } => "parabola",
        TextCurve2d::Hyperbola { .. } => "hyperbola",
        TextCurve2d::Nurbs(_) => "nurbs",
        TextCurve2d::Trimmed { basis, .. } => {
            increment(counts, "trimmed");
            census_curve2d(basis, counts);
            return;
        }
        TextCurve2d::Offset { basis, .. } => {
            increment(counts, "offset");
            census_curve2d(basis, counts);
            return;
        }
    };
    increment(counts, family);
}

fn census_curve(curve: &TextCurve, counts: &mut BTreeMap<String, u64>) {
    let family = match curve {
        TextCurve::Line { .. } => "line",
        TextCurve::Circle { .. } => "circle",
        TextCurve::Ellipse { .. } => "ellipse",
        TextCurve::Parabola { .. } => "parabola",
        TextCurve::Hyperbola { .. } => "hyperbola",
        TextCurve::Nurbs(_) => "nurbs",
        TextCurve::Trimmed { basis, .. } => {
            increment(counts, "trimmed");
            census_curve(basis, counts);
            return;
        }
        TextCurve::Offset { basis, .. } => {
            increment(counts, "offset");
            census_curve(basis, counts);
            return;
        }
    };
    increment(counts, family);
}

fn census_surface(
    surface: &TextSurface,
    counts: &mut BTreeMap<String, u64>,
    curves: &mut BTreeMap<String, u64>,
) {
    let family = match surface {
        TextSurface::Plane { .. } => "plane",
        TextSurface::Cylinder { .. } => "cylinder",
        TextSurface::Cone { .. } => "cone",
        TextSurface::Sphere { .. } => "sphere",
        TextSurface::Torus { .. } => "torus",
        TextSurface::Nurbs(_) => "nurbs",
        TextSurface::Extrusion { directrix, .. } => {
            increment(counts, "extrusion");
            census_curve(directrix, curves);
            return;
        }
        TextSurface::Revolution { directrix, .. } => {
            increment(counts, "revolution");
            census_curve(directrix, curves);
            return;
        }
        TextSurface::Trimmed { basis, .. } => {
            increment(counts, "trimmed");
            census_surface(basis, counts, curves);
            return;
        }
        TextSurface::Offset { basis, .. } => {
            increment(counts, "offset");
            census_surface(basis, counts, curves);
            return;
        }
    };
    increment(counts, family);
}

pub(crate) fn parse_text(bytes: &[u8]) -> Result<TextFacts, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("text B-rep is not UTF-8".into()))?;
    let headers = [
        ("CASCADE Topology V1, (c) Matra-Datavision", 1),
        ("CASCADE Topology V2, (c) Matra-Datavision", 2),
        ("CASCADE Topology V3, (c) Open Cascade", 3),
    ];
    let mut topology_version = None;
    for (header, version) in headers {
        let count = text.matches(header).count();
        if count > 1 {
            return Err(CodecError::Malformed(
                "text B-rep has duplicate topology headers".into(),
            ));
        }
        if count == 1 && topology_version.replace(version).is_some() {
            return Err(CodecError::Malformed(
                "text B-rep has multiple topology headers".into(),
            ));
        }
    }
    let topology_version = topology_version.ok_or_else(|| {
        CodecError::Malformed("text B-rep has no supported topology header".into())
    })?;
    let tokens = text.split_ascii_whitespace().collect::<Vec<_>>();
    let mut section_counts = BTreeMap::new();
    let mut previous_section = None;
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
        let mut section_tokens = tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| **token == section);
        let Some((index, _)) = section_tokens.next() else {
            return Err(CodecError::malformed(format_args!(
                "text B-rep has no {section} table"
            )));
        };
        if section_tokens.next().is_some() {
            return Err(CodecError::Malformed(
                "text B-rep has duplicate section markers".into(),
            ));
        }
        let count = tokens
            .get(index + 1)
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| CodecError::malformed(format_args!("invalid {section} count")))?;
        if count > 1_000_000 {
            return Err(CodecError::malformed(format_args!(
                "{section} count limit exceeded"
            )));
        }
        if previous_section.is_some_and(|previous| index <= previous) {
            return Err(CodecError::malformed(format_args!(
                "text B-rep {section} table is out of order"
            )));
        }
        previous_section = Some(index);
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
        return Err(CodecError::malformed(format_args!(
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

pub(crate) fn parse_binary_prefix(bytes: &[u8]) -> Result<BinaryFacts, CodecError> {
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
    // Each location consumes at least its 1-byte kind discriminant.
    let mut locations: Vec<TextLocation> =
        Vec::with_capacity(cursor.bounded(location_count, 1, "binary Locations")?);
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
                    if factors.len() >= 1_000_000 {
                        return Err(CodecError::Malformed(
                            "binary location factor-count limit exceeded".into(),
                        ));
                    }
                    let referenced = cursor.i32("binary location factor")?;
                    if referenced == 0 {
                        break;
                    }
                    let referenced = usize::try_from(referenced).map_err(|_| {
                        CodecError::Malformed("negative binary location factor".into())
                    })?;
                    if referenced == 0 || referenced > locations.len() {
                        return Err(CodecError::malformed(format_args!(
                            "binary location {} references unavailable location {referenced}",
                            index + 1
                        )));
                    }
                    let power = cursor.i32("binary location power")?;
                    let powered =
                        transform_power(locations[referenced - 1].transform, i64::from(power))?;
                    transform = powered.compose(transform);
                    factors.push(LocationFactor {
                        location: referenced,
                        power: i64::from(power),
                    });
                }
                TextLocation { factors, transform }
            }
            other => {
                return Err(CodecError::malformed(format_args!(
                    "invalid binary location type {other}"
                )))
            }
        };
        locations.push(location);
    }
    let curve_count = cursor.section_count("Curve2ds")?;
    // Each parameter curve consumes at least its 1-byte kind discriminant.
    let mut curve2ds = Vec::with_capacity(cursor.bounded(curve_count, 1, "binary Curve2ds")?);
    for _ in 0..curve_count {
        curve2ds.push(parse_binary_curve2d(&mut cursor, 0)?);
    }
    let curve_count = cursor.section_count("Curves")?;
    // Each 3D curve consumes at least its 1-byte kind discriminant.
    let mut curves = Vec::with_capacity(cursor.bounded(curve_count, 1, "binary Curves")?);
    for _ in 0..curve_count {
        curves.push(parse_binary_curve(&mut cursor, 0)?);
    }
    let polygon_count = cursor.section_count("Polygon3D")?;
    // Each 3D polygon consumes at least a 4-byte node count, a 1-byte flag, and an 8-byte deflection.
    let mut polygons3d =
        Vec::with_capacity(cursor.bounded(polygon_count, 13, "binary Polygon3D")?);
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
    // Each indexed polygon consumes at least a 4-byte node count, an 8-byte deflection, and a 1-byte flag.
    let mut polygons_on_triangulations = Vec::with_capacity(cursor.bounded(
        indexed_polygon_count,
        13,
        "binary PolygonOnTriangulations",
    )?);
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
    let surface_count = cursor.section_count("Surfaces")?;
    // Each surface consumes at least its 1-byte kind discriminant.
    let mut surfaces = Vec::with_capacity(cursor.bounded(surface_count, 1, "binary Surfaces")?);
    for _ in 0..surface_count {
        surfaces.push(parse_binary_surface(&mut cursor, 0)?);
    }
    let triangulation_count = cursor.section_count("Triangulations")?;
    // Each triangulation consumes at least two 4-byte counts, a 1-byte flag, and an 8-byte deflection.
    let mut triangulations =
        Vec::with_capacity(cursor.bounded(triangulation_count, 17, "binary Triangulations")?);
    for _ in 0..triangulation_count {
        let node_count = cursor.count("binary triangulation node count")?;
        let triangle_count = cursor.count("binary triangulation triangle count")?;
        let has_uv = cursor.bool("binary triangulation UV flag")?;
        let has_normals = version >= 4 && cursor.bool("binary triangulation normal flag")?;
        let deflection = cursor.f64("binary triangulation deflection")?;
        let nodes = (0..node_count)
            .map(|_| cursor.point3("binary triangulation node"))
            .collect::<Result<Vec<_>, _>>()?;
        let uv_nodes = has_uv
            .then(|| {
                (0..node_count)
                    .map(|_| cursor.point2("binary triangulation UV node"))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let triangles = (0..triangle_count)
            .map(|_| {
                let mut triangle = [0_u32; 3];
                for node in &mut triangle {
                    let value = cursor.i32("binary triangulation triangle node")?;
                    *node = u32::try_from(value).map_err(|_| {
                        CodecError::Malformed("negative binary triangle node".into())
                    })?;
                    if *node == 0 || usize::try_from(*node).is_ok_and(|node| node > node_count) {
                        return Err(CodecError::Malformed(
                            "binary triangle node index is out of bounds".into(),
                        ));
                    }
                }
                Ok(triangle)
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        let normals = has_normals
            .then(|| {
                (0..node_count)
                    .map(|_| cursor.vector3_f32("binary triangulation normal"))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        triangulations.push(TextTriangulation {
            deflection,
            nodes,
            uv_nodes,
            triangles,
            normals,
        });
    }
    let tshape_count = cursor.section_count("TShapes")?;
    // Each TShape consumes at least its 1-byte kind discriminant.
    let mut tshapes = Vec::with_capacity(cursor.bounded(tshape_count, 1, "binary TShapes")?);
    for index in 0..tshape_count {
        tshapes.push(parse_binary_tshape(
            &mut cursor,
            version,
            index + 1,
            tshape_count,
            curve_count,
            curve2ds.len(),
            surfaces.len(),
            locations.len(),
            polygons3d.len(),
            polygons_on_triangulations.len(),
            triangulations.len(),
        )?);
    }
    let roots = if cursor.remaining() == 0 {
        Vec::new()
    } else {
        if cursor.remaining() != 12 {
            return Err(CodecError::Malformed(
                "binary B-rep root record has invalid length".into(),
            ));
        }
        let shape = cursor.i32("binary root shape")?;
        let location = cursor.i32("binary root location")?;
        let orientation = cursor.i32("binary root orientation")?;
        if shape == -1 && location == -1 && orientation == -1 {
            Vec::new()
        } else {
            vec![TextShapeUse {
                shape: checked_binary_reference(shape, tshape_count, false, "root shape")?,
                location: checked_binary_reference(
                    location,
                    locations.len(),
                    true,
                    "root location",
                )?,
                orientation: binary_orientation(orientation)?,
            }]
        }
    };
    Ok(BinaryFacts {
        topology_version: version,
        locations,
        curve2ds,
        curves,
        polygons3d,
        polygons_on_triangulations,
        surfaces,
        triangulations,
        tshapes,
        roots,
    })
}

#[allow(clippy::too_many_arguments)]
fn parse_binary_tshape(
    cursor: &mut BinaryCursor<'_>,
    version: u8,
    index: usize,
    tshape_count: usize,
    curve_count: usize,
    curve2d_count: usize,
    surface_count: usize,
    location_count: usize,
    polygon3d_count: usize,
    indexed_polygon_count: usize,
    triangulation_count: usize,
) -> Result<TextTShape, CodecError> {
    let kind = match cursor.u8("binary TShape kind")? {
        0 => TextShapeKind::Compound,
        1 => TextShapeKind::CompSolid,
        2 => TextShapeKind::Solid,
        3 => TextShapeKind::Shell,
        4 => TextShapeKind::Face,
        5 => TextShapeKind::Wire,
        6 => TextShapeKind::Edge,
        7 => TextShapeKind::Vertex,
        other => {
            return Err(CodecError::malformed(format_args!(
                "invalid binary TShape kind {other}"
            )))
        }
    };
    let geometry = match kind {
        TextShapeKind::Vertex => {
            let tolerance = cursor.f64("binary vertex tolerance")?;
            let point = cursor.point3("binary vertex point")?;
            let mut representations = Vec::new();
            loop {
                let representation_kind = cursor.u8("binary vertex representation kind")?;
                if representation_kind == 0 {
                    break;
                }
                if representations.len() >= 1_000_000 {
                    return Err(CodecError::Malformed(
                        "binary vertex representation-count limit exceeded".into(),
                    ));
                }
                let parameter = cursor.f64("binary vertex parameter")?;
                let (second_parameter, curve, surface) = match representation_kind {
                    1 => (
                        None,
                        Some(checked_binary_reference(
                            cursor.i32("binary vertex curve")?,
                            curve_count,
                            false,
                            "vertex curve",
                        )?),
                        None,
                    ),
                    2 => (
                        None,
                        Some(checked_binary_reference(
                            cursor.i32("binary vertex pcurve")?,
                            curve2d_count,
                            false,
                            "vertex pcurve",
                        )?),
                        Some(checked_binary_reference(
                            cursor.i32("binary vertex surface")?,
                            surface_count,
                            false,
                            "vertex surface",
                        )?),
                    ),
                    3 => (
                        Some(cursor.f64("binary vertex second surface parameter")?),
                        None,
                        Some(checked_binary_reference(
                            cursor.i32("binary vertex surface")?,
                            surface_count,
                            false,
                            "vertex surface",
                        )?),
                    ),
                    other => {
                        return Err(CodecError::malformed(format_args!(
                            "invalid binary vertex representation kind {other}"
                        )))
                    }
                };
                representations.push(TextPointRepresentation {
                    parameter,
                    second_parameter,
                    kind: representation_kind,
                    curve,
                    surface,
                    location: checked_binary_reference(
                        cursor.i32("binary vertex location")?,
                        location_count,
                        true,
                        "vertex location",
                    )?,
                });
            }
            TextTShapeGeometry::Vertex {
                tolerance,
                point,
                representations,
            }
        }
        TextShapeKind::Edge => {
            let tolerance = cursor.f64("binary edge tolerance")?;
            let same_parameter = cursor.bool("binary edge same-parameter flag")?;
            let same_range = cursor.bool("binary edge same-range flag")?;
            let degenerated = cursor.bool("binary edge degenerated flag")?;
            let mut representations = Vec::new();
            loop {
                let representation_kind = cursor.u8("binary edge representation kind")?;
                if representation_kind == 0 {
                    break;
                }
                if representations.len() >= 1_000_000 {
                    return Err(CodecError::Malformed(
                        "binary edge representation-count limit exceeded".into(),
                    ));
                }
                representations.push(parse_binary_edge_representation(
                    cursor,
                    version,
                    representation_kind,
                    curve_count,
                    curve2d_count,
                    surface_count,
                    location_count,
                    polygon3d_count,
                    indexed_polygon_count,
                    triangulation_count,
                )?);
            }
            TextTShapeGeometry::Edge {
                tolerance,
                same_parameter,
                same_range,
                degenerated,
                representations,
            }
        }
        TextShapeKind::Face => {
            let natural_restriction = cursor.bool("binary face natural-restriction flag")?;
            let tolerance = cursor.f64("binary face tolerance")?;
            let surface = checked_binary_reference(
                cursor.i32("binary face surface")?,
                surface_count,
                true,
                "face surface",
            )?;
            let location = checked_binary_reference(
                cursor.i32("binary face location")?,
                location_count,
                true,
                "face location",
            )?;
            let triangulation = match cursor.u8("binary face triangulation marker")? {
                0 | 1 => None,
                2 => Some(checked_binary_reference(
                    cursor.i32("binary face triangulation")?,
                    triangulation_count,
                    false,
                    "face triangulation",
                )?),
                other => {
                    return Err(CodecError::malformed(format_args!(
                        "invalid binary face triangulation marker {other}"
                    )))
                }
            };
            TextTShapeGeometry::Face {
                natural_restriction,
                tolerance,
                surface,
                location,
                triangulation,
            }
        }
        TextShapeKind::Wire
        | TextShapeKind::Shell
        | TextShapeKind::Solid
        | TextShapeKind::CompSolid
        | TextShapeKind::Compound => TextTShapeGeometry::Empty,
    };
    let mut flags = [false; 7];
    for flag in &mut flags {
        *flag = cursor.bool("binary TShape flag")?;
    }
    let mut children = Vec::new();
    loop {
        let orientation = cursor.u8("binary child orientation")?;
        if orientation == b'*' {
            break;
        }
        let reverse_index = checked_binary_reference(
            cursor.i32("binary child reverse index")?,
            tshape_count,
            false,
            "child reverse index",
        )?;
        let shape = tshape_count - reverse_index + 1;
        if shape >= index {
            return Err(CodecError::malformed(format_args!(
                "binary TShape {index} references non-prior child {shape}"
            )));
        }
        children.push(TextShapeUse {
            shape,
            orientation: binary_orientation(i32::from(orientation))?,
            location: checked_binary_reference(
                cursor.i32("binary child location")?,
                location_count,
                true,
                "child location",
            )?,
        });
    }
    Ok(TextTShape {
        index,
        kind,
        geometry,
        flags,
        children,
    })
}

#[allow(clippy::too_many_arguments)]
fn parse_binary_edge_representation(
    cursor: &mut BinaryCursor<'_>,
    version: u8,
    kind: u8,
    curve_count: usize,
    curve2d_count: usize,
    surface_count: usize,
    location_count: usize,
    polygon3d_count: usize,
    indexed_polygon_count: usize,
    triangulation_count: usize,
) -> Result<TextEdgeRepresentation, CodecError> {
    let mut record = TextEdgeRepresentation {
        kind,
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
            record.primary = checked_binary_reference(
                cursor.i32("binary edge curve")?,
                curve_count,
                false,
                "edge curve",
            )?;
            record.location = checked_binary_reference(
                cursor.i32("binary edge curve location")?,
                location_count,
                true,
                "edge curve location",
            )?;
            record.parameter_range = Some([
                cursor.f64("binary edge curve start")?,
                cursor.f64("binary edge curve end")?,
            ]);
        }
        2 | 3 => {
            record.primary = checked_binary_reference(
                cursor.i32("binary edge pcurve")?,
                curve2d_count,
                false,
                "edge pcurve",
            )?;
            if kind == 3 {
                record.secondary = Some(checked_binary_reference(
                    cursor.i32("binary edge secondary pcurve")?,
                    curve2d_count,
                    false,
                    "edge secondary pcurve",
                )?);
                record.continuity = Some(cursor.u8("binary edge continuity")?.to_string());
            }
            record.surface = Some(checked_binary_reference(
                cursor.i32("binary edge surface")?,
                surface_count,
                false,
                "edge surface",
            )?);
            record.location = checked_binary_reference(
                cursor.i32("binary edge surface location")?,
                location_count,
                true,
                "edge surface location",
            )?;
            record.parameter_range = Some([
                cursor.f64("binary edge pcurve start")?,
                cursor.f64("binary edge pcurve end")?,
            ]);
            if matches!(version, 2 | 3) {
                record.uv_endpoints = Some([
                    cursor.point2("binary edge first UV endpoint")?,
                    cursor.point2("binary edge last UV endpoint")?,
                ]);
            }
        }
        4 => {
            record.continuity = Some(cursor.u8("binary edge continuity")?.to_string());
            record.surface = Some(checked_binary_reference(
                cursor.i32("binary edge regularity surface")?,
                surface_count,
                false,
                "edge regularity surface",
            )?);
            record.location = checked_binary_reference(
                cursor.i32("binary edge regularity location")?,
                location_count,
                true,
                "edge regularity location",
            )?;
            record.second_surface = Some(checked_binary_reference(
                cursor.i32("binary edge second regularity surface")?,
                surface_count,
                false,
                "edge second regularity surface",
            )?);
            record.second_location = Some(checked_binary_reference(
                cursor.i32("binary edge second regularity location")?,
                location_count,
                true,
                "edge second regularity location",
            )?);
        }
        5 => {
            record.primary = checked_binary_reference(
                cursor.i32("binary edge 3D polygon")?,
                polygon3d_count,
                false,
                "edge 3D polygon",
            )?;
            record.location = checked_binary_reference(
                cursor.i32("binary edge polygon location")?,
                location_count,
                true,
                "edge polygon location",
            )?;
        }
        6 | 7 => {
            record.primary = checked_binary_reference(
                cursor.i32("binary edge indexed polygon")?,
                indexed_polygon_count,
                false,
                "edge indexed polygon",
            )?;
            if kind == 7 {
                record.secondary = Some(checked_binary_reference(
                    cursor.i32("binary edge secondary indexed polygon")?,
                    indexed_polygon_count,
                    false,
                    "edge secondary indexed polygon",
                )?);
            }
            record.surface = Some(checked_binary_reference(
                cursor.i32("binary edge triangulation")?,
                triangulation_count,
                false,
                "edge triangulation",
            )?);
            record.location = checked_binary_reference(
                cursor.i32("binary edge triangulation location")?,
                location_count,
                true,
                "edge triangulation location",
            )?;
        }
        other => {
            return Err(CodecError::malformed(format_args!(
                "invalid binary edge representation kind {other}"
            )))
        }
    }
    Ok(record)
}

fn checked_binary_reference(
    value: i32,
    count: usize,
    allow_zero: bool,
    label: &str,
) -> Result<usize, CodecError> {
    let value = usize::try_from(value)
        .map_err(|_| CodecError::malformed(format_args!("negative binary {label}")))?;
    if value > count || (!allow_zero && value == 0) {
        return Err(CodecError::malformed(format_args!(
            "binary {label} index {value} exceeds table count {count}"
        )));
    }
    Ok(value)
}

fn binary_orientation(value: i32) -> Result<TextOrientation, CodecError> {
    match value {
        0 => Ok(TextOrientation::Forward),
        1 => Ok(TextOrientation::Reversed),
        2 => Ok(TextOrientation::Internal),
        3 => Ok(TextOrientation::External),
        other => Err(CodecError::malformed(format_args!(
            "invalid binary orientation {other}"
        ))),
    }
}

fn parse_binary_surface(
    cursor: &mut BinaryCursor<'_>,
    depth: usize,
) -> Result<TextSurface, CodecError> {
    if depth > 256 {
        return Err(CodecError::Malformed(
            "binary surface nesting exceeds 256".into(),
        ));
    }
    Ok(match cursor.u8("binary surface kind")? {
        1 => {
            let origin = cursor.point3("binary plane origin")?;
            let axis = cursor.vector3("binary plane axis")?;
            let u_axis = cursor.vector3("binary plane u axis")?;
            let v_axis = cursor.vector3("binary plane v axis")?;
            TextSurface::Plane {
                origin,
                axis,
                u_axis,
                v_reversed: frame_v_reversed(axis, u_axis, v_axis),
            }
        }
        2 => {
            let origin = cursor.point3("binary cylinder origin")?;
            let axis = cursor.vector3("binary cylinder axis")?;
            let ref_direction = cursor.vector3("binary cylinder reference direction")?;
            let y_direction = cursor.vector3("binary cylinder v direction")?;
            TextSurface::Cylinder {
                origin,
                axis,
                ref_direction,
                radius: cursor.f64("binary cylinder radius")?,
                u_reversed: frame_v_reversed(axis, ref_direction, y_direction),
            }
        }
        3 => {
            let origin = cursor.point3("binary cone origin")?;
            let axis = cursor.vector3("binary cone axis")?;
            let ref_direction = cursor.vector3("binary cone reference direction")?;
            let y_direction = cursor.vector3("binary cone v direction")?;
            TextSurface::Cone {
                origin,
                axis,
                ref_direction,
                radius: cursor.f64("binary cone reference radius")?,
                half_angle: cursor.f64("binary cone half angle")?,
                u_reversed: frame_v_reversed(axis, ref_direction, y_direction),
            }
        }
        4 => {
            let center = cursor.point3("binary sphere center")?;
            let axis = cursor.vector3("binary sphere axis")?;
            let ref_direction = cursor.vector3("binary sphere reference direction")?;
            let y_direction = cursor.vector3("binary sphere v direction")?;
            TextSurface::Sphere {
                center,
                axis,
                ref_direction,
                radius: cursor.f64("binary sphere radius")?,
                u_reversed: frame_v_reversed(axis, ref_direction, y_direction),
            }
        }
        5 => {
            let center = cursor.point3("binary torus center")?;
            let axis = cursor.vector3("binary torus axis")?;
            let ref_direction = cursor.vector3("binary torus reference direction")?;
            let y_direction = cursor.vector3("binary torus v direction")?;
            TextSurface::Torus {
                center,
                axis,
                ref_direction,
                major_radius: cursor.f64("binary torus major radius")?,
                minor_radius: cursor.f64("binary torus minor radius")?,
                u_reversed: frame_v_reversed(axis, ref_direction, y_direction),
            }
        }
        6 => TextSurface::Extrusion {
            direction: cursor.vector3("binary extrusion direction")?,
            directrix: Box::new(parse_binary_curve(cursor, depth + 1)?),
        },
        7 => TextSurface::Revolution {
            axis_origin: cursor.point3("binary revolution axis origin")?,
            axis_direction: cursor.vector3("binary revolution axis direction")?,
            directrix: Box::new(parse_binary_curve(cursor, depth + 1)?),
        },
        8 => {
            let u_rational = cursor.bool("binary Bezier u-rational flag")?;
            let v_rational = cursor.bool("binary Bezier v-rational flag")?;
            let u_degree = usize::from(cursor.u16("binary Bezier u degree")?);
            let v_degree = usize::from(cursor.u16("binary Bezier v degree")?);
            let u_count = u_degree.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("binary Bezier u pole count overflow".into())
            })?;
            let v_count = v_degree.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("binary Bezier v pole count overflow".into())
            })?;
            let pole_count = checked_grid_count(u_count, v_count, "binary Bezier")?;
            let rational = u_rational || v_rational;
            // Each pole consumes at least a 24-byte point3.
            let capacity = cursor.bounded(pole_count, 24, "binary Bezier surface pole")?;
            let mut control_points = Vec::with_capacity(capacity);
            let mut weights = rational.then(|| Vec::with_capacity(capacity));
            for _ in 0..pole_count {
                control_points.push(cursor.point3("binary Bezier surface pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.f64("binary Bezier surface weight")?);
                }
            }
            TextSurface::Nurbs(NurbsSurface {
                u_degree: u32::try_from(u_degree).map_err(|_| {
                    CodecError::Malformed("binary Bezier u degree exceeds u32".into())
                })?,
                v_degree: u32::try_from(v_degree).map_err(|_| {
                    CodecError::Malformed("binary Bezier v degree exceeds u32".into())
                })?,
                u_knots: clamped_bezier_knots(u_degree),
                v_knots: clamped_bezier_knots(v_degree),
                u_count: u32::try_from(u_count).map_err(|_| {
                    CodecError::Malformed("binary Bezier u count exceeds u32".into())
                })?,
                v_count: u32::try_from(v_count).map_err(|_| {
                    CodecError::Malformed("binary Bezier v count exceeds u32".into())
                })?,
                control_points,
                weights,
                normal_reversed: false,
                u_periodic: false,
                v_periodic: false,
            })
        }
        9 => {
            let u_rational = cursor.bool("binary B-spline u-rational flag")?;
            let v_rational = cursor.bool("binary B-spline v-rational flag")?;
            let u_periodic = cursor.bool("binary B-spline u-periodic flag")?;
            let v_periodic = cursor.bool("binary B-spline v-periodic flag")?;
            let u_degree = u32::from(cursor.u16("binary B-spline u degree")?);
            let v_degree = u32::from(cursor.u16("binary B-spline v degree")?);
            let u_count = cursor.count("binary B-spline u pole count")?;
            let v_count = cursor.count("binary B-spline v pole count")?;
            let u_knot_count = cursor.count("binary B-spline u knot count")?;
            let v_knot_count = cursor.count("binary B-spline v knot count")?;
            let pole_count = checked_grid_count(u_count, v_count, "binary B-spline")?;
            let rational = u_rational || v_rational;
            // Each pole consumes at least a 24-byte point3.
            let capacity = cursor.bounded(pole_count, 24, "binary B-spline surface pole")?;
            let mut control_points = Vec::with_capacity(capacity);
            let mut weights = rational.then(|| Vec::with_capacity(capacity));
            for _ in 0..pole_count {
                control_points.push(cursor.point3("binary B-spline surface pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.f64("binary B-spline surface weight")?);
                }
            }
            TextSurface::Nurbs(normalize_periodic_surface(NurbsSurface {
                u_degree,
                v_degree,
                u_knots: cursor.expanded_knots(u_knot_count, "binary B-spline u knots")?,
                v_knots: cursor.expanded_knots(v_knot_count, "binary B-spline v knots")?,
                u_count: u32::try_from(u_count).map_err(|_| {
                    CodecError::Malformed("binary B-spline u count exceeds u32".into())
                })?,
                v_count: u32::try_from(v_count).map_err(|_| {
                    CodecError::Malformed("binary B-spline v count exceeds u32".into())
                })?,
                control_points,
                weights,
                normal_reversed: false,
                u_periodic,
                v_periodic,
            })?)
        }
        10 => TextSurface::Trimmed {
            parameter_ranges: [
                [
                    cursor.f64("binary surface u trim start")?,
                    cursor.f64("binary surface u trim end")?,
                ],
                [
                    cursor.f64("binary surface v trim start")?,
                    cursor.f64("binary surface v trim end")?,
                ],
            ],
            basis: Box::new(parse_binary_surface(cursor, depth + 1)?),
        },
        11 => TextSurface::Offset {
            distance: cursor.f64("binary surface offset")?,
            basis: Box::new(parse_binary_surface(cursor, depth + 1)?),
        },
        other => {
            return Err(CodecError::malformed(format_args!(
                "invalid binary surface kind {other}"
            )))
        }
    })
}

fn checked_grid_count(u_count: usize, v_count: usize, label: &str) -> Result<usize, CodecError> {
    u_count
        .checked_mul(v_count)
        .filter(|count| *count <= 1_000_000)
        .ok_or_else(|| CodecError::malformed(format_args!("{label} pole-count limit exceeded")))
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
            // Each pole consumes at least a 24-byte point3.
            let capacity = cursor.bounded(pole_count, 24, "binary Bezier pole")?;
            let mut control_points = Vec::with_capacity(capacity);
            let mut weights = rational.then(|| Vec::with_capacity(capacity));
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
            // Each pole consumes at least a 24-byte point3.
            let capacity = cursor.bounded(pole_count, 24, "binary B-spline pole")?;
            let mut control_points = Vec::with_capacity(capacity);
            let mut weights = rational.then(|| Vec::with_capacity(capacity));
            for _ in 0..pole_count {
                control_points.push(cursor.point3("binary B-spline pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.f64("binary B-spline weight")?);
                }
            }
            let knots = cursor.expanded_knots(knot_count, "binary B-spline")?;
            let (knots, padding) = normalize_periodic_knots(knots, degree, periodic)?;
            append_periodic_curve_poles(&mut control_points, weights.as_mut(), padding)?;
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
            return Err(CodecError::malformed(format_args!(
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
            // Each pole consumes at least a 16-byte point2.
            let capacity = cursor.bounded(pole_count, 16, "binary Bezier parameter pole")?;
            let mut control_points = Vec::with_capacity(capacity);
            let mut weights = rational.then(|| Vec::with_capacity(capacity));
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
            // Each pole consumes at least a 16-byte point2.
            let capacity = cursor.bounded(pole_count, 16, "binary B-spline parameter pole")?;
            let mut control_points = Vec::with_capacity(capacity);
            let mut weights = rational.then(|| Vec::with_capacity(capacity));
            for _ in 0..pole_count {
                control_points.push(point(cursor, "binary B-spline pole")?);
                if let Some(weights) = &mut weights {
                    weights.push(cursor.f64("binary B-spline weight")?);
                }
            }
            let knots = cursor.expanded_knots(knot_count, "binary B-spline")?;
            let (knots, padding) = normalize_periodic_knots(knots, degree, periodic)?;
            append_periodic_curve_poles(&mut control_points, weights.as_mut(), padding)?;
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
            return Err(CodecError::malformed(format_args!(
                "invalid binary parameter-curve kind {other}"
            )))
        }
    })
}

struct BinaryCursor<'a> {
    view: View<'a>,
}

impl<'a> BinaryCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            view: View::over_retained(bytes),
        }
    }

    fn remaining(&self) -> usize {
        self.view.remaining()
    }

    fn unread(&self) -> &'a [u8] {
        let rel = self.view.position().saturating_sub(self.view.start());
        self.view.window().get(rel..).unwrap_or_default()
    }

    fn truncated(label: &str) -> CodecError {
        CodecError::malformed(format_args!("truncated {label}"))
    }

    /// Clamps a declared element count to what the unread bytes can hold.
    ///
    /// `element_size` is the minimum encoded bytes of one element; a count that
    /// could not physically fit in the remaining input is rejected.
    fn bounded(&self, count: usize, element_size: usize, label: &str) -> Result<usize, CodecError> {
        bounded_len(count as u64, element_size, self.remaining()).ok_or_else(|| {
            CodecError::malformed(format_args!("{label} count exceeds remaining input"))
        })
    }

    fn take(&mut self, count: usize, label: &str) -> Result<&'a [u8], CodecError> {
        self.view.take(count).ok_or_else(|| Self::truncated(label))
    }

    fn line(&mut self, label: &str) -> Result<&'a str, CodecError> {
        let tail = self.unread();
        let length = tail
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or_else(|| CodecError::malformed(format_args!("unterminated {label}")))?;
        let line = self.take(length + 1, label)?;
        std::str::from_utf8(&line[..length])
            .map_err(|_| CodecError::malformed(format_args!("non-UTF-8 {label}")))
    }

    fn section_count(&mut self, name: &str) -> Result<usize, CodecError> {
        let line = loop {
            let line = self.line(name)?;
            if !line.trim().is_empty() {
                break line;
            }
        };
        let mut tokens = line.split_ascii_whitespace();
        if tokens.next() != Some(name) || tokens.clone().count() != 1 {
            return Err(CodecError::malformed(format_args!(
                "binary B-rep expected {name} section, found {line:?}"
            )));
        }
        tokens
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|count| *count <= 1_000_000)
            .ok_or_else(|| CodecError::malformed(format_args!("invalid binary {name} count")))
    }

    fn u8(&mut self, label: &str) -> Result<u8, CodecError> {
        self.view.u8().ok_or_else(|| Self::truncated(label))
    }

    fn bool(&mut self, label: &str) -> Result<bool, CodecError> {
        match self.u8(label)? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(CodecError::malformed(format_args!(
                "invalid {label} byte {other}"
            ))),
        }
    }

    fn u16(&mut self, label: &str) -> Result<u16, CodecError> {
        self.view.u16_le().ok_or_else(|| Self::truncated(label))
    }

    fn i32(&mut self, label: &str) -> Result<i32, CodecError> {
        self.view.i32_le().ok_or_else(|| Self::truncated(label))
    }

    fn count(&mut self, label: &str) -> Result<usize, CodecError> {
        let value = self.i32(label)?;
        usize::try_from(value)
            .ok()
            .filter(|count| *count <= 1_000_000)
            .ok_or_else(|| CodecError::malformed(format_args!("invalid {label}")))
    }

    fn f64(&mut self, label: &str) -> Result<f64, CodecError> {
        let value = self.view.f64_le().ok_or_else(|| Self::truncated(label))?;
        value
            .is_finite()
            .then_some(value)
            .ok_or_else(|| CodecError::malformed(format_args!("non-finite {label}")))
    }

    fn f32(&mut self, label: &str) -> Result<f32, CodecError> {
        let value = self.view.f32_le().ok_or_else(|| Self::truncated(label))?;
        value
            .is_finite()
            .then_some(value)
            .ok_or_else(|| CodecError::malformed(format_args!("non-finite {label}")))
    }

    fn point2(&mut self, label: &str) -> Result<Point2, CodecError> {
        Ok(Point2::new(self.f64(label)?, self.f64(label)?))
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

    fn vector3_f32(&mut self, label: &str) -> Result<Vector3, CodecError> {
        Ok(Vector3::new(
            f64::from(self.f32(label)?),
            f64::from(self.f32(label)?),
            f64::from(self.f32(label)?),
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
                return Err(CodecError::malformed(format_args!(
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
    // Each location consumes at least its one type token.
    let mut locations: Vec<TextLocation> =
        Vec::with_capacity(cursor.bounded(count, 1, "text Locations")?);
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
                        return Err(CodecError::malformed(format_args!(
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
                    transform = powered.compose(transform);
                    factors.push(LocationFactor {
                        location: referenced,
                        power,
                    });
                }
                TextLocation { factors, transform }
            }
            other => {
                return Err(CodecError::malformed(format_args!(
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
    // Each parameter curve consumes at least its one type token.
    let mut curves = Vec::with_capacity(cursor.bounded(count, 1, "text Curve2ds")?);
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
    // Each pole consumes at least its two point2 tokens.
    let capacity = cursor.bounded(pole_count, 2, "2D B-spline pole")?;
    let mut control_points = Vec::with_capacity(capacity);
    let mut weights = rational.then(|| Vec::with_capacity(capacity));
    for _ in 0..pole_count {
        control_points.push(cursor.point2("2D B-spline pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.real("2D B-spline weight")?);
        }
    }
    let knots = parse_knots(cursor, knot_count, degree, "2D B-spline")?;
    let (knots, padding) = normalize_periodic_knots(knots, degree as u32, periodic)?;
    append_periodic_curve_poles(&mut control_points, weights.as_mut(), padding)?;
    Ok(NurbsCurve2d {
        degree: degree as u32,
        knots,
        control_points,
        weights,
        periodic,
    })
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
            result = result.compose(base);
        }
        exponent >>= 1;
        if exponent > 0 {
            base = base.compose(base);
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
    // Each polygon consumes at least a node-count, flag, and deflection token.
    let mut polygons = Vec::with_capacity(cursor.bounded(count, 3, "text Polygon3D")?);
    for _ in 0..count {
        let node_count = cursor.count("3D polygon node count", 1_000_000)?;
        let has_parameters = cursor.boolean("3D polygon parameter flag")?;
        let deflection = cursor.real("3D polygon deflection")?;
        // Each node consumes its three point tokens.
        let mut nodes = Vec::with_capacity(cursor.bounded(node_count, 3, "3D polygon node")?);
        for _ in 0..node_count {
            nodes.push(cursor.point("3D polygon node")?);
        }
        let parameters = if has_parameters {
            // Each parameter consumes its one token.
            let mut parameters =
                Vec::with_capacity(cursor.bounded(node_count, 1, "3D polygon parameter")?);
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
    // Each polygon consumes at least a node-count, marker, deflection, and flag token.
    let mut polygons =
        Vec::with_capacity(cursor.bounded(count, 4, "text PolygonOnTriangulations")?);
    for _ in 0..count {
        let node_count = cursor.count("polygon-on-triangulation node count", 1_000_000)?;
        // Each node index consumes its one token.
        let mut nodes =
            Vec::with_capacity(cursor.bounded(node_count, 1, "polygon-on-triangulation node")?);
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
            // Each parameter consumes its one token.
            let mut parameters = Vec::with_capacity(cursor.bounded(
                node_count,
                1,
                "polygon-on-triangulation parameter",
            )?);
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
    // Each triangulation consumes at least two counts, a flag, and a deflection token.
    let mut triangulations = Vec::with_capacity(cursor.bounded(count, 4, "text Triangulations")?);
    for _ in 0..count {
        let node_count = cursor.count("triangulation node count", 1_000_000)?;
        let triangle_count = cursor.count("triangulation triangle count", 1_000_000)?;
        let has_uv = cursor.boolean("triangulation UV flag")?;
        let has_normals = topology_version >= 3 && cursor.boolean("triangulation normal flag")?;
        let deflection = cursor.real("triangulation deflection")?;
        // Each node consumes its three point tokens.
        let mut nodes = Vec::with_capacity(cursor.bounded(node_count, 3, "triangulation node")?);
        for _ in 0..node_count {
            nodes.push(cursor.point("triangulation node")?);
        }
        let uv_nodes = if has_uv {
            // Each UV node consumes its two point2 tokens.
            let mut uv_nodes =
                Vec::with_capacity(cursor.bounded(node_count, 2, "triangulation UV node")?);
            for _ in 0..node_count {
                uv_nodes.push(cursor.point2("triangulation UV node")?);
            }
            Some(uv_nodes)
        } else {
            None
        };
        // Each triangle consumes its three index tokens.
        let mut triangles =
            Vec::with_capacity(cursor.bounded(triangle_count, 3, "triangulation triangle")?);
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
            // Each normal consumes its three vector tokens.
            let mut normals =
                Vec::with_capacity(cursor.bounded(node_count, 3, "triangulation normal")?);
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
        .ok_or_else(|| CodecError::malformed(format_args!("text B-rep has no {section} table")))?
        + 2;
    let end = tokens
        .iter()
        .position(|token| *token == following)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("text B-rep has no {following} table"))
        })?;
    Ok(TokenCursor::new(&tokens[start..end]))
}

fn ensure_section_consumed(cursor: &TokenCursor<'_>, section: &str) -> Result<(), CodecError> {
    if cursor.is_empty() {
        Ok(())
    } else {
        Err(CodecError::malformed(format_args!(
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
    // Each TShape consumes at least its one kind token.
    let mut shapes = Vec::with_capacity(cursor.bounded(count, 1, "text TShapes")?);
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
                return Err(CodecError::malformed(format_args!(
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
        _ => Err(CodecError::malformed(format_args!(
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
                return Err(CodecError::malformed(format_args!(
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
                let (secondary, joined_continuity) = parse_reference_suffix(
                    cursor,
                    "edge secondary parameter curve",
                    counts["Curve2ds"],
                )?;
                record.secondary = Some(secondary);
                record.continuity = Some(
                    joined_continuity
                        .map_or_else(|| cursor.next("edge continuity").map(str::to_owned), Ok)?,
                );
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
            return Err(CodecError::malformed(format_args!(
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
        return Err(CodecError::malformed(format_args!(
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
            return Err(CodecError::malformed(format_args!(
                "invalid shape use {token:?}"
            )))
        }
    };
    let encoded = encoded
        .parse::<usize>()
        .map_err(|_| CodecError::malformed(format_args!("invalid shape use {token:?}")))?;
    if encoded == 0 || encoded > shape_count {
        return Err(CodecError::malformed(format_args!(
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
        return Err(CodecError::malformed(format_args!("{label} index is zero")));
    }
    Ok(value)
}

fn parse_reference_suffix(
    cursor: &mut TokenCursor<'_>,
    label: &str,
    maximum: usize,
) -> Result<(usize, Option<String>), CodecError> {
    let token = cursor.next(label)?;
    let split = token
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(token.len());
    let (reference, suffix) = token.split_at(split);
    let value = reference
        .parse::<usize>()
        .map_err(|_| CodecError::malformed(format_args!("invalid {label}")))?;
    if value == 0 || value > maximum {
        return Err(CodecError::malformed(format_args!(
            "{label} limit exceeded"
        )));
    }
    Ok((value, (!suffix.is_empty()).then(|| suffix.to_owned())))
}

fn parse_range(cursor: &mut TokenCursor<'_>, label: &str) -> Result<[f64; 2], CodecError> {
    let range = [
        cursor.real(&format!("{label} first parameter"))?,
        cursor.real(&format!("{label} last parameter"))?,
    ];
    if range[0] > range[1] {
        return Err(CodecError::malformed(format_args!(
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
    // Each surface consumes at least its one type token.
    let mut surfaces = Vec::with_capacity(cursor.bounded(count, 1, "text Surfaces")?);
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
    let y_direction = cursor.vector("surface y direction")?;
    Ok(match kind {
        1 => TextSurface::Plane {
            origin,
            axis,
            u_axis: ref_direction,
            v_reversed: frame_v_reversed(axis, ref_direction, y_direction),
        },
        2 => TextSurface::Cylinder {
            origin,
            axis,
            ref_direction,
            radius: cursor.real("cylinder radius")?,
            u_reversed: frame_v_reversed(axis, ref_direction, y_direction),
        },
        3 => TextSurface::Cone {
            origin,
            axis,
            ref_direction,
            radius: cursor.real("cone radius")?,
            half_angle: cursor.real("cone half angle")?,
            u_reversed: frame_v_reversed(axis, ref_direction, y_direction),
        },
        4 => TextSurface::Sphere {
            center: origin,
            axis,
            ref_direction,
            radius: cursor.real("sphere radius")?,
            u_reversed: frame_v_reversed(axis, ref_direction, y_direction),
        },
        5 => TextSurface::Torus {
            center: origin,
            axis,
            ref_direction,
            major_radius: cursor.real("torus major radius")?,
            minor_radius: cursor.real("torus minor radius")?,
            u_reversed: frame_v_reversed(axis, ref_direction, y_direction),
        },
        _ => unreachable!("analytic surface kind was range checked"),
    })
}

fn frame_v_reversed(axis: Vector3, x_axis: Vector3, y_axis: Vector3) -> bool {
    let expected_y = Vector3::new(
        axis.y.mul_add(x_axis.z, -axis.z * x_axis.y),
        axis.z.mul_add(x_axis.x, -axis.x * x_axis.z),
        axis.x.mul_add(x_axis.y, -axis.y * x_axis.x),
    );
    expected_y.x.mul_add(
        y_axis.x,
        expected_y.y.mul_add(y_axis.y, expected_y.z * y_axis.z),
    ) < 0.0
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
    // Each pole consumes its three point tokens.
    let capacity = cursor.bounded(pole_count, 3, "B-spline surface pole")?;
    let mut control_points = Vec::with_capacity(capacity);
    let mut weights = rational.then(|| Vec::with_capacity(capacity));
    for _ in 0..pole_count {
        control_points.push(cursor.point("B-spline surface pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.real("B-spline surface weight")?);
        }
    }
    let u_knots = parse_knots(cursor, u_knot_count, u_degree, "B-spline u")?;
    let v_knots = parse_knots(cursor, v_knot_count, v_degree, "B-spline v")?;
    normalize_periodic_surface(NurbsSurface {
        u_degree: u_degree as u32,
        v_degree: v_degree as u32,
        u_knots,
        v_knots,
        u_count: u_count as u32,
        v_count: v_count as u32,
        control_points,
        weights,
        normal_reversed: false,
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
        normal_reversed: false,
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
                CodecError::malformed(format_args!("expanded {label} knot limit exceeded"))
            })?;
        knots.resize(expanded, knot);
    }
    Ok(knots)
}

fn normalize_periodic_knots(
    knots: Vec<f64>,
    degree: u32,
    periodic: bool,
) -> Result<(Vec<f64>, usize), CodecError> {
    if !periodic {
        return Ok((knots, 0));
    }
    let degree = usize::try_from(degree)
        .map_err(|_| CodecError::Malformed("periodic B-spline degree exceeds usize".into()))?;
    let Some(&first) = knots.first() else {
        return Err(CodecError::Malformed(
            "periodic B-spline has no knots".into(),
        ));
    };
    let last = *knots.last().expect("nonempty periodic knot vector");
    let first_multiplicity = knots.iter().take_while(|knot| **knot == first).count();
    let last_multiplicity = knots.iter().rev().take_while(|knot| **knot == last).count();
    if first_multiplicity == 0
        || first_multiplicity > degree
        || last_multiplicity != first_multiplicity
        || !first.is_finite()
        || !last.is_finite()
        || first >= last
    {
        return Err(CodecError::Malformed(
            "periodic B-spline endpoint knots are invalid".into(),
        ));
    }
    let padding = degree + 1 - first_multiplicity;
    let before_last = knots.len().checked_sub(last_multiplicity).ok_or_else(|| {
        CodecError::Malformed("periodic B-spline knot cardinality underflow".into())
    })?;
    if before_last < padding || knots.len() - first_multiplicity < padding {
        return Err(CodecError::Malformed(
            "periodic B-spline has insufficient interior knots".into(),
        ));
    }
    let period = last - first;
    let mut normalized =
        Vec::with_capacity(knots.len().checked_add(2 * padding).ok_or_else(|| {
            CodecError::Malformed("periodic B-spline knot limit exceeded".into())
        })?);
    normalized.extend(
        knots[before_last - padding..before_last]
            .iter()
            .map(|knot| knot - period),
    );
    normalized.extend_from_slice(&knots);
    normalized.extend(
        knots[first_multiplicity..first_multiplicity + padding]
            .iter()
            .map(|knot| knot + period),
    );
    Ok((normalized, padding))
}

fn append_periodic_curve_poles<T: Clone>(
    control_points: &mut Vec<T>,
    weights: Option<&mut Vec<f64>>,
    padding: usize,
) -> Result<(), CodecError> {
    if padding == 0 {
        return Ok(());
    }
    if control_points.len() < padding {
        return Err(CodecError::Malformed(
            "periodic B-spline has insufficient poles".into(),
        ));
    }
    control_points.extend_from_within(..padding);
    if let Some(weights) = weights {
        if weights.len() < padding {
            return Err(CodecError::Malformed(
                "periodic B-spline has insufficient weights".into(),
            ));
        }
        weights.extend_from_within(..padding);
    }
    Ok(())
}

fn normalize_periodic_surface(mut surface: NurbsSurface) -> Result<NurbsSurface, CodecError> {
    let (u_knots, u_padding) = normalize_periodic_knots(
        std::mem::take(&mut surface.u_knots),
        surface.u_degree,
        surface.u_periodic,
    )?;
    let (v_knots, v_padding) = normalize_periodic_knots(
        std::mem::take(&mut surface.v_knots),
        surface.v_degree,
        surface.v_periodic,
    )?;
    let old_u = usize::try_from(surface.u_count)
        .map_err(|_| CodecError::Malformed("B-spline u pole count exceeds usize".into()))?;
    let old_v = usize::try_from(surface.v_count)
        .map_err(|_| CodecError::Malformed("B-spline v pole count exceeds usize".into()))?;
    if old_u == 0 || old_v == 0 {
        return Err(CodecError::Malformed(
            "periodic B-spline pole grid is empty".into(),
        ));
    }
    let new_u = old_u
        .checked_add(u_padding)
        .ok_or_else(|| CodecError::Malformed("periodic B-spline u pole limit exceeded".into()))?;
    let new_v = old_v
        .checked_add(v_padding)
        .ok_or_else(|| CodecError::Malformed("periodic B-spline v pole limit exceeded".into()))?;
    let new_count = new_u
        .checked_mul(new_v)
        .filter(|count| *count <= 2_000_000)
        .ok_or_else(|| CodecError::Malformed("periodic B-spline pole limit exceeded".into()))?;
    if surface.control_points.len() != old_u.saturating_mul(old_v)
        || surface
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != surface.control_points.len())
    {
        return Err(CodecError::Malformed(
            "periodic B-spline pole grid is invalid".into(),
        ));
    }
    if u_padding != 0 || v_padding != 0 {
        let old_points = std::mem::take(&mut surface.control_points);
        let old_weights = surface.weights.take();
        let mut points = Vec::with_capacity(new_count);
        let mut weights = old_weights.as_ref().map(|_| Vec::with_capacity(new_count));
        for u in 0..new_u {
            for v in 0..new_v {
                let source = (u % old_u) * old_v + v % old_v;
                points.push(old_points[source]);
                if let (Some(source_weights), Some(target_weights)) = (&old_weights, &mut weights) {
                    target_weights.push(source_weights[source]);
                }
            }
        }
        surface.control_points = points;
        surface.weights = weights;
    }
    surface.u_knots = u_knots;
    surface.v_knots = v_knots;
    surface.u_count = u32::try_from(new_u)
        .map_err(|_| CodecError::Malformed("periodic B-spline u pole count exceeds u32".into()))?;
    surface.v_count = u32::try_from(new_v)
        .map_err(|_| CodecError::Malformed("periodic B-spline v pole count exceeds u32".into()))?;
    Ok(surface)
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
    // Each 3D curve consumes at least its one type token.
    let mut curves = Vec::with_capacity(cursor.bounded(count, 1, "text Curves")?);
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
    // Each pole consumes its three point tokens.
    let capacity = cursor.bounded(pole_count, 3, "B-spline pole")?;
    let mut control_points = Vec::with_capacity(capacity);
    let mut weights = rational.then(|| Vec::with_capacity(capacity));
    for _ in 0..pole_count {
        control_points.push(cursor.point("B-spline pole")?);
        if let Some(weights) = &mut weights {
            weights.push(cursor.real("B-spline weight")?);
        }
    }
    let knots = parse_knots(cursor, knot_count, degree, "B-spline")?;
    let (knots, padding) = normalize_periodic_knots(knots, degree as u32, periodic)?;
    append_periodic_curve_poles(&mut control_points, weights.as_mut(), padding)?;
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

    fn remaining(&self) -> usize {
        self.tokens.len().saturating_sub(self.index)
    }

    /// Clamps a declared element count to the unread token count.
    ///
    /// `element_size` is the minimum tokens one element consumes; a count that
    /// could not fit in the remaining tokens is rejected.
    fn bounded(&self, count: usize, element_size: usize, label: &str) -> Result<usize, CodecError> {
        bounded_len(count as u64, element_size, self.remaining()).ok_or_else(|| {
            CodecError::malformed(format_args!("{label} count exceeds available tokens"))
        })
    }

    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.index).copied()
    }

    fn integer(&mut self, label: &str) -> Result<i64, CodecError> {
        self.next(label)?.parse().map_err(|_| {
            CodecError::malformed(format_args!("invalid {label} in text B-rep Curves table"))
        })
    }

    fn count(&mut self, label: &str, maximum: usize) -> Result<usize, CodecError> {
        let value = self.integer(label)?;
        let value = usize::try_from(value)
            .map_err(|_| CodecError::malformed(format_args!("negative {label}")))?;
        if value > maximum {
            return Err(CodecError::malformed(format_args!(
                "{label} limit exceeded"
            )));
        }
        Ok(value)
    }

    fn boolean(&mut self, label: &str) -> Result<bool, CodecError> {
        match self.integer(label)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CodecError::malformed(format_args!("invalid {label}"))),
        }
    }

    fn real(&mut self, label: &str) -> Result<f64, CodecError> {
        let value = self.next(label)?.parse::<f64>().map_err(|_| {
            CodecError::malformed(format_args!("invalid {label} in text B-rep Curves table"))
        })?;
        if !value.is_finite() {
            return Err(CodecError::malformed(format_args!(
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
            CodecError::malformed(format_args!("truncated {label} in text B-rep Curves table"))
        })?;
        self.index += 1;
        Ok(token)
    }
}

#[derive(Default)]
pub(crate) struct CurveTransfer {
    pub(crate) curves: Vec<Curve>,
    pub(crate) procedural: Vec<ProceduralCurve>,
}

pub(crate) fn transfer_text_curves(
    payloads: &[ShapePayloadRecord],
    properties: &[PropertyRecord],
) -> CurveTransfer {
    let mut transfer = CurveTransfer::default();
    for payload in payloads {
        let curves = if let Some(text) = &payload.text {
            &text.curves
        } else if let Some(binary) = &payload.binary {
            &binary.curves
        } else {
            continue;
        };
        let object_id = properties
            .iter()
            .find(|property| property.id == payload.property)
            .map_or_else(
                || payload.property.clone(),
                |property| property.owner.clone(),
            );
        let association = SourceObjectAssociation {
            format: "fcstd".into(),
            object_id,
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        };
        for (index, curve) in curves.iter().enumerate() {
            let id = CurveId(native::model_id(
                "curve",
                &payload.id,
                (index + 1).to_string(),
            ));
            append_text_curve(curve, id, &association, &mut transfer);
        }
    }
    transfer
}

pub(crate) fn append_text_curve(
    curve: &TextCurve,
    id: CurveId,
    association: &SourceObjectAssociation,
    transfer: &mut CurveTransfer,
) -> CurveGeometry {
    let geometry = match curve {
        TextCurve::Line { origin, direction } => CurveGeometry::Line {
            origin: *origin,
            direction: *direction,
        },
        TextCurve::Circle {
            center,
            axis: _,
            ref_direction: _,
            radius,
        } if *radius == 0.0 => CurveGeometry::Degenerate { point: *center },
        TextCurve::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => CurveGeometry::Circle {
            center: *center,
            axis: *axis,
            ref_direction: *ref_direction,
            radius: *radius,
        },
        TextCurve::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => CurveGeometry::Ellipse {
            center: *center,
            axis: *axis,
            major_direction: *major_direction,
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        },
        TextCurve::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => CurveGeometry::Parabola {
            vertex: *vertex,
            axis: *axis,
            major_direction: *major_direction,
            focal_distance: *focal_distance,
        },
        TextCurve::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => CurveGeometry::Hyperbola {
            center: *center,
            axis: *axis,
            major_direction: *major_direction,
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        },
        TextCurve::Nurbs(nurbs) => CurveGeometry::Nurbs(nurbs.clone()),
        TextCurve::Trimmed {
            parameter_range,
            basis,
        } => {
            let basis_id = CurveId(format!("{}:basis", id.0));
            let basis_geometry = append_text_curve(basis, basis_id.clone(), association, transfer);
            let parameter_range = crate::topology_transfer::normalize_occt_curve_range(
                &basis_geometry,
                Some(*parameter_range),
            )
            .unwrap_or(*parameter_range);
            transfer.procedural.push(ProceduralCurve {
                id: ProceduralCurveId(format!("{}:construction", id.0)),
                curve: id.clone(),
                definition: ProceduralCurveDefinition::Subset {
                    source: basis_id,
                    parameter_range,
                    sense: true,
                },
                cache_fit_tolerance: None,
            });
            basis_geometry
        }
        TextCurve::Offset {
            distance,
            direction,
            basis,
        } => {
            let basis_id = CurveId(format!("{}:basis", id.0));
            append_text_curve(basis, basis_id.clone(), association, transfer);
            transfer.procedural.push(ProceduralCurve {
                id: ProceduralCurveId(format!("{}:construction", id.0)),
                curve: id.clone(),
                definition: ProceduralCurveDefinition::Offset {
                    source: basis_id,
                    distance: *distance,
                    direction: Some(*direction),
                    support: None,
                    distance_law: None,
                    normal: None,
                    parameter_range: None,
                },
                cache_fit_tolerance: None,
            });
            CurveGeometry::Unknown { record: None }
        }
    };
    transfer.curves.push(Curve {
        id,
        geometry: geometry.clone(),
        source_object: Some(association.clone()),
    });
    geometry
}

#[derive(Default)]
pub(crate) struct SurfaceTransfer {
    pub(crate) surfaces: Vec<Surface>,
    pub(crate) procedural: Vec<ProceduralSurface>,
}

pub(crate) fn transfer_text_surfaces(
    payloads: &[ShapePayloadRecord],
    properties: &[PropertyRecord],
    curve_transfer: &mut CurveTransfer,
) -> SurfaceTransfer {
    let mut transfer = SurfaceTransfer::default();
    for payload in payloads {
        let surfaces = if let Some(text) = &payload.text {
            &text.surfaces
        } else if let Some(binary) = &payload.binary {
            &binary.surfaces
        } else {
            continue;
        };
        let object_id = properties
            .iter()
            .find(|property| property.id == payload.property)
            .map_or_else(
                || payload.property.clone(),
                |property| property.owner.clone(),
            );
        let association = SourceObjectAssociation {
            format: "fcstd".into(),
            object_id,
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        };
        for (index, surface) in surfaces.iter().enumerate() {
            append_text_surface(
                surface,
                SurfaceId(native::model_id(
                    "surface",
                    &payload.id,
                    (index + 1).to_string(),
                )),
                &association,
                curve_transfer,
                &mut transfer,
            );
        }
    }
    transfer
}

pub(crate) fn append_text_surface(
    surface: &TextSurface,
    id: SurfaceId,
    association: &SourceObjectAssociation,
    curve_transfer: &mut CurveTransfer,
    transfer: &mut SurfaceTransfer,
) -> SurfaceGeometry {
    let geometry = match surface {
        TextSurface::Plane {
            origin,
            axis,
            u_axis,
            ..
        } => SurfaceGeometry::Plane {
            origin: *origin,
            normal: *axis,
            u_axis: *u_axis,
        },
        TextSurface::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
            ..
        } => SurfaceGeometry::Cylinder {
            origin: *origin,
            axis: *axis,
            ref_direction: *ref_direction,
            radius: *radius,
        },
        TextSurface::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            half_angle,
            ..
        } => SurfaceGeometry::Cone {
            origin: *origin,
            axis: *axis,
            ref_direction: *ref_direction,
            radius: *radius,
            ratio: 1.0,
            half_angle: *half_angle,
        },
        TextSurface::Sphere {
            center,
            axis,
            ref_direction,
            radius,
            ..
        } => SurfaceGeometry::Sphere {
            center: *center,
            axis: *axis,
            ref_direction: *ref_direction,
            radius: *radius,
        },
        TextSurface::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
            ..
        } => SurfaceGeometry::Torus {
            center: *center,
            axis: *axis,
            ref_direction: *ref_direction,
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        },
        TextSurface::Nurbs(nurbs) => SurfaceGeometry::Nurbs(nurbs.clone()),
        TextSurface::Extrusion {
            direction,
            directrix,
        } => {
            let directrix_id = CurveId(format!("{}:directrix", id.0));
            append_text_curve(directrix, directrix_id.clone(), association, curve_transfer);
            transfer.procedural.push(ProceduralSurface {
                id: ProceduralSurfaceId(format!("{}:construction", id.0)),
                surface: id.clone(),
                definition: ProceduralSurfaceDefinition::Extrusion {
                    directrix: directrix_id,
                    parameter_interval: None,
                    direction: *direction,
                    native_position: None,
                    revision_form: None,
                },
                record_bounds: None,
                cache_fit_tolerance: None,
            });
            SurfaceGeometry::Unknown { record: None }
        }
        TextSurface::Revolution {
            axis_origin,
            axis_direction,
            directrix,
        } => {
            let directrix_id = CurveId(format!("{}:directrix", id.0));
            append_text_curve(directrix, directrix_id.clone(), association, curve_transfer);
            transfer.procedural.push(ProceduralSurface {
                id: ProceduralSurfaceId(format!("{}:construction", id.0)),
                surface: id.clone(),
                definition: ProceduralSurfaceDefinition::Revolution {
                    directrix: directrix_id,
                    axis_origin: *axis_origin,
                    axis_direction: *axis_direction,
                    angular_interval: [0.0, std::f64::consts::TAU],
                    angular_parameter_interval: None,
                    parameter_interval: None,
                    transposed: true,
                    revision_form: None,
                },
                record_bounds: None,
                cache_fit_tolerance: None,
            });
            SurfaceGeometry::Unknown { record: None }
        }
        TextSurface::Trimmed {
            parameter_ranges,
            basis,
        } => {
            let basis_parameters = surface_parameter_affine(basis);
            let parameter_ranges = [
                parameter_ranges[0].map(|value| {
                    value.mul_add(basis_parameters.u_scale, basis_parameters.u_offset)
                }),
                parameter_ranges[1].map(|value| {
                    value.mul_add(basis_parameters.v_scale, basis_parameters.v_offset)
                }),
            ];
            let basis_id = SurfaceId(format!("{}:basis", id.0));
            let basis_geometry = append_text_surface(
                basis,
                basis_id.clone(),
                association,
                curve_transfer,
                transfer,
            );
            transfer.procedural.push(ProceduralSurface {
                id: ProceduralSurfaceId(format!("{}:construction", id.0)),
                surface: id.clone(),
                definition: ProceduralSurfaceDefinition::Subset {
                    support: basis_id,
                    parameter_ranges,
                    u_sense: None,
                    v_sense: None,
                },
                record_bounds: None,
                cache_fit_tolerance: None,
            });
            basis_geometry
        }
        TextSurface::Offset { distance, basis } => {
            let basis_id = SurfaceId(format!("{}:basis", id.0));
            append_text_surface(
                basis,
                basis_id.clone(),
                association,
                curve_transfer,
                transfer,
            );
            transfer.procedural.push(ProceduralSurface {
                id: ProceduralSurfaceId(format!("{}:construction", id.0)),
                surface: id.clone(),
                definition: ProceduralSurfaceDefinition::Offset {
                    support: basis_id,
                    distance: *distance,
                    u_sense: None,
                    v_sense: None,
                    support_extension: None,
                    extension_flags: Vec::new(),
                    revision_form: None,
                },
                record_bounds: None,
                cache_fit_tolerance: None,
            });
            SurfaceGeometry::Unknown { record: None }
        }
    };
    transfer.surfaces.push(Surface {
        id,
        geometry: geometry.clone(),
        source_object: Some(association.clone()),
    });
    geometry
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_support::*;
    use crate::FcstdCodec;
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

    #[test]
    fn expands_occt_periodic_knots_and_cyclic_surface_poles() {
        let surface = NurbsSurface {
            u_degree: 3,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 6,
            v_count: 2,
            control_points: (0..6)
                .flat_map(|u| {
                    [
                        Point3::new(f64::from(u), 0.0, 0.0),
                        Point3::new(f64::from(u), 1.0, 0.0),
                    ]
                })
                .collect(),
            weights: None,
            normal_reversed: false,
            u_periodic: true,
            v_periodic: false,
        };

        let normalized = normalize_periodic_surface(surface).expect("periodic surface");
        assert_eq!(normalized.u_count, 7);
        assert_eq!(
            normalized.u_knots,
            [-0.5, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.5]
        );
        assert_eq!(normalized.control_points.len(), 14);
        assert_eq!(normalized.control_points[12], normalized.control_points[0]);
        assert_eq!(normalized.control_points[13], normalized.control_points[1]);
        let start = cadmpeg_ir::eval::nurbs_surface_point(&normalized, 0.0, 0.5)
            .expect("periodic start point");
        let end = cadmpeg_ir::eval::nurbs_surface_point(&normalized, 1.0, 0.5)
            .expect("periodic end point");
        assert!((start.x - end.x).abs() <= 1.0e-12);
        assert!((start.y - end.y).abs() <= 1.0e-12);
        assert!((start.z - end.z).abs() <= 1.0e-12);
    }

    fn text_brep(curves: &str, curve_count: usize, surfaces: &str, surface_count: usize) -> String {
        format!(
            "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves {curve_count}\n{curves}\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces {surface_count}\n{surfaces}\nTriangulations 0\nTShapes 0\n*"
        )
    }

    #[test]
    fn parses_joined_seam_pcurve_continuity_token() {
        let tokens = ["1", "2CN", "1", "0", "0", "10"];
        let mut cursor = TokenCursor::new(&tokens);
        let counts = BTreeMap::from([
            ("Curve2ds".to_owned(), 2),
            ("Surfaces".to_owned(), 1),
            ("Locations".to_owned(), 0),
        ]);
        let record = parse_edge_representation(3, &mut cursor, &counts, 1)
            .expect("joined pcurve continuity");
        assert_eq!(record.primary, 1);
        assert_eq!(record.secondary, Some(2));
        assert_eq!(record.continuity.as_deref(), Some("CN"));
        assert!(cursor.is_empty());
    }

    #[test]
    fn retains_indirect_analytic_surface_parameter_frames() {
        let tokens = [
            "0", "0", "0", "0", "0", "1", "1", "0", "0", "0", "-1", "0", "2", "0.5",
        ];
        let mut cursor = TokenCursor::new(&tokens);
        let cone = parse_analytic_surface(3, &mut cursor).expect("indirect cone");
        assert!(matches!(
            cone,
            TextSurface::Cone {
                radius: 2.0,
                half_angle: 0.5,
                u_reversed: true,
                ..
            }
        ));

        let tokens = [
            "0", "0", "0", "0", "0", "1", "1", "0", "0", "0", "-1", "0", "2",
        ];
        let mut cursor = TokenCursor::new(&tokens);
        let sphere = parse_analytic_surface(4, &mut cursor).expect("indirect sphere");
        assert!(matches!(
            sphere,
            TextSurface::Sphere {
                radius: 2.0,
                u_reversed: true,
                ..
            }
        ));

        let tokens = [
            "0", "0", "0", "0", "0", "1", "1", "0", "0", "0", "-1", "0", "4", "1",
        ];
        let mut cursor = TokenCursor::new(&tokens);
        let torus = parse_analytic_surface(5, &mut cursor).expect("indirect torus");
        assert!(matches!(
            torus,
            TextSurface::Torus {
                major_radius: 4.0,
                minor_radius: 1.0,
                u_reversed: true,
                ..
            }
        ));
    }

    #[test]
    fn binds_only_the_direct_shape_entry_as_typed_payload() {
        let property = PropertyRecord {
            id: crate::native::native_id("property", "Shape:SuppressedShape"),
            owner: crate::native::native_id("object", "Shape"),
            name: "SuppressedShape".into(),
            type_name: "Part::PropertyPartShape".into(),
            family: crate::native::PropertyFamily::Geometry,
            status: None,
            transient: false,
            dynamic: None,
            order: 0,
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["empty.brp".into(), "empty-2.brp".into()],
            raw_xml: r#"<Property><Part file="empty.brp"/><Extra file="empty-2.brp"/></Property>"#
                .into(),
            byte_start: 0,
            byte_end: 0,
        };
        let entry = EntryRecord {
            id: crate::native::native_id("entry", "empty.brp"),
            name: "empty.brp".into(),
            role: "brep".into(),
            byte_len: 0,
            sha256: cadmpeg_ir::hash::sha256_hex(b""),
            referenced_by: vec![property.id.clone()],
            data: Vec::new(),
        };
        let second_entry = EntryRecord {
            id: crate::native::native_id("entry", "empty-2.brp"),
            name: "empty-2.brp".into(),
            role: "brep".into(),
            byte_len: 0,
            sha256: cadmpeg_ir::hash::sha256_hex(b""),
            referenced_by: vec![property.id.clone()],
            data: Vec::new(),
        };
        let payloads =
            parse_payloads(&[property], &[entry, second_entry]).expect("empty shape payload");
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].entry, "fcstd:native:entry#empty.brp");
        assert_eq!(payloads[0].form, ShapePayloadForm::Empty);
        assert!(payloads[0].text.is_none());
        assert!(payloads[0].binary.is_none());
    }

    #[test]
    fn ignores_nested_shape_part_carriers() {
        let property = PropertyRecord {
            id: crate::native::native_id("property", "Shape:Nested"),
            owner: crate::native::native_id("object", "Shape"),
            name: "Nested".into(),
            type_name: "Part::PropertyPartShape".into(),
            family: crate::native::PropertyFamily::Geometry,
            status: None,
            transient: false,
            dynamic: None,
            order: 0,
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["nested.brp".into()],
            raw_xml: r#"<Property><Wrapper><Part file="nested.brp"/></Wrapper></Property>"#.into(),
            byte_start: 0,
            byte_end: 0,
        };
        let payloads = parse_payloads(&[property], &[]).expect("nested carrier is ignored");
        assert!(payloads.is_empty());
    }

    #[test]
    fn rejects_duplicate_direct_shape_part_carriers() {
        let property = PropertyRecord {
            id: crate::native::native_id("property", "Shape:Duplicate"),
            owner: crate::native::native_id("object", "Shape"),
            name: "Duplicate".into(),
            type_name: "Part::PropertyPartShape".into(),
            family: crate::native::PropertyFamily::Geometry,
            status: None,
            transient: false,
            dynamic: None,
            order: 0,
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["first.brp".into(), "second.brp".into()],
            raw_xml: r#"<Property><Part file="first.brp"/><Part file="second.brp"/></Property>"#
                .into(),
            byte_start: 0,
            byte_end: 0,
        };
        assert!(parse_payloads(&[property], &[]).is_err());
    }

    #[test]
    fn retains_transient_shape_without_a_carrier() {
        let property = PropertyRecord {
            id: crate::native::native_id("property", "Shape:PreviewShape"),
            owner: crate::native::native_id("object", "Shape"),
            name: "PreviewShape".into(),
            type_name: "Part::PropertyPartShape".into(),
            family: crate::native::PropertyFamily::Geometry,
            status: Some(152),
            transient: true,
            dynamic: None,
            order: 0,
            values: Vec::new(),
            links: Vec::new(),
            side_entries: Vec::new(),
            raw_xml:
                r#"<_Property name="PreviewShape" type="Part::PropertyPartShape" status="152"/>"#
                    .into(),
            byte_start: 0,
            byte_end: 0,
        };
        let payloads = parse_payloads(&[property], &[]).expect("transient shape is retained");
        assert!(payloads.is_empty());
    }

    #[test]
    fn ignores_non_shape_runtime_names_when_framing_payloads() {
        let property = PropertyRecord {
            id: crate::native::native_id("property", "Shape:Custom"),
            owner: crate::native::native_id("object", "Shape"),
            name: "Custom".into(),
            type_name: "Custom::PropertyPartShape".into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            transient: false,
            dynamic: None,
            order: 0,
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["custom.brp".into()],
            raw_xml: String::new(),
            byte_start: 0,
            byte_end: 0,
        };

        let payloads = parse_payloads(&[property], &[]).expect("unknown type is retained");
        assert!(payloads.is_empty());
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
        bytes.extend_from_slice(b"Surfaces 2\n");
        bytes.push(1);
        for value in [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            real(&mut bytes, value);
        }
        bytes.push(11);
        real(&mut bytes, 0.25);
        bytes.push(4);
        for value in [
            0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 2.0,
        ] {
            real(&mut bytes, value);
        }
        bytes.extend_from_slice(b"Triangulations 1\n");
        bytes.extend_from_slice(&3_i32.to_le_bytes());
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.push(1);
        real(&mut bytes, 0.03);
        for value in [
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,
        ] {
            real(&mut bytes, value);
        }
        for node in [1_i32, 2, 3] {
            bytes.extend_from_slice(&node.to_le_bytes());
        }
        bytes.extend_from_slice(b"TShapes 0\n");

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
        assert!(matches!(facts.surfaces[0], TextSurface::Plane { .. }));
        assert!(matches!(facts.surfaces[1], TextSurface::Offset { .. }));
        assert_eq!(facts.triangulations[0].triangles, [[1, 2, 3]]);
        assert!(facts.tshapes.is_empty());
        assert!(facts.roots.is_empty());
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
        assert_eq!(*parameter_range, [0.0, 314.0 / 50.0]);
        assert!(matches!(basis.as_ref(), TextCurve2d::Offset { .. }));
    }

    #[test]
    fn expands_periodic_parameter_curve_knots_and_poles() {
        let input = "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 1\n7 1 1 6 6 2 0 0 1 1 0 1 1 1 1 0 1 1 -1 0 1 -1 -1 1 0 6 6.283185307179586 6\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
        let facts = parse_text(input.as_bytes()).expect("periodic parameter curve");
        let TextCurve2d::Nurbs(nurbs) = &facts.curve2ds[0] else {
            panic!("expected periodic NURBS")
        };

        assert_eq!(nurbs.control_points.len(), 7);
        assert_eq!(nurbs.weights.as_ref().map(Vec::len), Some(7));
        assert_eq!(nurbs.knots.len(), 14);
        assert_eq!(nurbs.control_points.first(), nurbs.control_points.last());
        assert_eq!(nurbs.weights.as_ref().map(|weights| weights[0]), Some(1.0));
        assert_eq!(nurbs.weights.as_ref().map(|weights| weights[6]), Some(1.0));
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

    #[test]
    fn rejects_oversized_and_out_of_order_text_tables() {
        let oversized = b"CASCADE Topology V1, (c) Matra-Datavision\nLocations 1000001\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
        assert!(parse_text(oversized)
            .expect_err("oversized table")
            .to_string()
            .contains("count limit"));

        let out_of_order = b"CASCADE Topology V1, (c) Matra-Datavision\nCurve2ds 0\nLocations 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
        assert!(parse_text(out_of_order)
            .expect_err("out-of-order table")
            .to_string()
            .contains("out of order"));
    }

    #[test]
    fn rejects_duplicate_text_headers_and_section_markers() {
        let duplicate_header = b"CASCADE Topology V1, (c) Matra-Datavision\nCASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
        assert!(matches!(
            parse_text(duplicate_header),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));

        let duplicate_section = b"CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
        assert!(matches!(
            parse_text(duplicate_section),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }

    #[test]
    pub(crate) fn transfers_recursive_exact_parameter_curve_geometry() {
        let source = crate::brep::TextCurve2d::Offset {
            distance: 0.25,
            basis: Box::new(crate::brep::TextCurve2d::Trimmed {
                parameter_range: [0.0, std::f64::consts::PI],
                basis: Box::new(crate::brep::TextCurve2d::Circle {
                    center: cadmpeg_ir::math::Point2::new(1.0, 2.0),
                    x_axis: cadmpeg_ir::math::Point2::new(1.0, 0.0),
                    y_axis: cadmpeg_ir::math::Point2::new(0.0, 1.0),
                    radius: 3.0,
                }),
            }),
        };
        let cadmpeg_ir::geometry::PcurveGeometry::Offset { distance, basis } =
            crate::topology_transfer::pcurve_geometry(&source)
        else {
            panic!("expected offset pcurve");
        };
        assert_eq!(distance, 0.25);
        assert!(matches!(
            basis.as_ref(),
            cadmpeg_ir::geometry::PcurveGeometry::Trimmed { basis, .. }
                if matches!(basis.as_ref(), cadmpeg_ir::geometry::PcurveGeometry::Circle { radius: 3.0, .. })
        ));
    }

    #[test]
    pub(crate) fn transfers_binary_exact_curve_and_surface_carriers() {
        let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.bin"/></Property></Properties></Object></ObjectData>
</Document>"#;
        let mut brep =
            b"\nOpen CASCADE Topology V3 (c)\nLocations 0\nCurve2ds 0\nCurves 1\n".to_vec();
        brep.push(1);
        for value in [0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0] {
            brep.extend_from_slice(&value.to_le_bytes());
        }
        brep.extend_from_slice(b"Polygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n");
        brep.push(1);
        for value in [
            0.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ] {
            brep.extend_from_slice(&value.to_le_bytes());
        }
        brep.extend_from_slice(b"Triangulations 1\n");
        brep.extend_from_slice(&3_i32.to_le_bytes());
        brep.extend_from_slice(&1_i32.to_le_bytes());
        brep.push(0);
        for value in [0.01_f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            brep.extend_from_slice(&value.to_le_bytes());
        }
        for node in [1_i32, 2, 3] {
            brep.extend_from_slice(&node.to_le_bytes());
        }
        brep.extend_from_slice(b"TShapes 7\n");
        let flags = |brep: &mut Vec<u8>| brep.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0]);
        let child = |brep: &mut Vec<u8>, orientation: u8, reverse_index: i32| {
            brep.push(orientation);
            brep.extend_from_slice(&reverse_index.to_le_bytes());
            brep.extend_from_slice(&0_i32.to_le_bytes());
        };
        brep.push(7);
        brep.extend_from_slice(&0.001_f64.to_le_bytes());
        for value in [0.0_f64, 0.0, 0.0] {
            brep.extend_from_slice(&value.to_le_bytes());
        }
        brep.push(0);
        flags(&mut brep);
        brep.push(b'*');
        brep.push(6);
        brep.extend_from_slice(&0.001_f64.to_le_bytes());
        brep.extend_from_slice(&[1, 1, 1, 0]);
        flags(&mut brep);
        child(&mut brep, 0, 7);
        child(&mut brep, 1, 7);
        brep.push(b'*');
        brep.push(5);
        flags(&mut brep);
        child(&mut brep, 0, 6);
        brep.push(b'*');
        brep.push(4);
        brep.push(0);
        brep.extend_from_slice(&0.001_f64.to_le_bytes());
        brep.extend_from_slice(&1_i32.to_le_bytes());
        brep.extend_from_slice(&0_i32.to_le_bytes());
        brep.push(2);
        brep.extend_from_slice(&1_i32.to_le_bytes());
        flags(&mut brep);
        child(&mut brep, 0, 5);
        brep.push(b'*');
        for (kind, reverse_index) in [(3_u8, 4_i32), (2, 3), (0, 2)] {
            brep.push(kind);
            flags(&mut brep);
            child(&mut brep, 0, reverse_index);
            brep.push(b'*');
        }
        brep.extend_from_slice(&7_i32.to_le_bytes());
        brep.extend_from_slice(&0_i32.to_le_bytes());
        brep.extend_from_slice(&0_i32.to_le_bytes());
        let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.bin", &brep)]);
        let result = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("binary curve carrier");
        assert_eq!(result.ir().model.curves.len(), 1);
        assert!(matches!(
            result.ir().model.curves[0].geometry,
            cadmpeg_ir::geometry::CurveGeometry::Line { .. }
        ));
        assert_eq!(result.ir().model.surfaces.len(), 1);
        assert!(matches!(
            result.ir().model.surfaces[0].geometry,
            cadmpeg_ir::geometry::SurfaceGeometry::Plane { .. }
        ));
        assert_eq!(result.ir().model.tessellations.len(), 1);
        assert_eq!(result.ir().model.tessellations[0].triangles, [[0, 1, 2]]);
        assert_eq!(result.ir().model.bodies.len(), 1);
        assert_eq!(result.ir().model.faces.len(), 1);
        assert_eq!(
            result.ir().model.tessellations[0].body.as_ref(),
            Some(&result.ir().model.bodies[0].id)
        );
        assert_eq!(
            result.ir().model.tessellations[0].faces,
            [result.ir().model.faces[0].id.clone()]
        );
        assert_eq!(result.ir().model.coedges.len(), 1);
        assert!(result.report().geometry_transferred);
    }

    #[test]
    fn transfers_zero_radius_brep_circles_as_degenerate_curves() {
        let center = cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0);
        let curve = crate::brep::TextCurve::Circle {
            center,
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            radius: 0.0,
        };
        let association = cadmpeg_ir::SourceObjectAssociation {
            format: "fcstd".into(),
            object_id: "object".into(),
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        };
        let mut transfer = crate::brep::CurveTransfer::default();

        let geometry = crate::brep::append_text_curve(
            &curve,
            cadmpeg_ir::ids::CurveId("curve".into()),
            &association,
            &mut transfer,
        );

        assert_eq!(
            geometry,
            cadmpeg_ir::geometry::CurveGeometry::Degenerate { point: center }
        );
    }

    #[test]
    fn transfers_occt_revolution_surface_parameter_order() {
        let surface = crate::brep::TextSurface::Revolution {
            axis_origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            axis_direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            directrix: Box::new(crate::brep::TextCurve::Circle {
                center: cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
                axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
                ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                radius: 1.0,
            }),
        };
        let association = cadmpeg_ir::SourceObjectAssociation {
            format: "fcstd".into(),
            object_id: "fcstd:native:object#Surface".into(),
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        };
        let mut curves = crate::brep::CurveTransfer::default();
        let mut surfaces = crate::brep::SurfaceTransfer::default();
        crate::brep::append_text_surface(
            &surface,
            cadmpeg_ir::ids::SurfaceId("fcstd:model:surface#revolution".into()),
            &association,
            &mut curves,
            &mut surfaces,
        );
        assert!(matches!(
            surfaces.procedural[0].definition,
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
                transposed: true,
                ..
            }
        ));
    }
}
