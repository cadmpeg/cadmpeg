// SPDX-License-Identifier: Apache-2.0
//! Bounded framing for text and binary exact-shape side entries.

use std::collections::BTreeMap;

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
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
    /// Ordered 3D curve table.
    pub curves: Vec<TextCurve>,
    /// Ordered surface table.
    pub surfaces: Vec<TextSurface>,
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
            let text = match form {
                ShapePayloadForm::Text => Some(parse_text(&entry.data)?),
                ShapePayloadForm::Binary => None,
            };
            Ok(ShapePayloadRecord {
                id: format!("{}:shape-payload", property.id),
                property: property.id.clone(),
                entry: entry.id.clone(),
                form,
                text,
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
    let curves = parse_curves(&tokens, &section_counts)?;
    let surfaces = parse_surfaces(&tokens, &section_counts)?;
    Ok(TextFacts {
        topology_version,
        section_counts,
        shape_types,
        curves,
        surfaces,
    })
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
        let kind = cursor.integer("surface type")?;
        let origin = cursor.point("surface origin")?;
        let axis = cursor.vector("surface axis")?;
        let ref_direction = cursor.vector("surface reference direction")?;
        let _y_direction = cursor.vector("surface y direction")?;
        let surface = match kind {
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
            other => {
                return Err(CodecError::NotImplemented(format!(
                    "text B-rep surface family {other} at table index {}",
                    index + 1
                )))
            }
        };
        surfaces.push(surface);
    }
    if !cursor.is_empty() {
        return Err(CodecError::Malformed(
            "text B-rep Surfaces table contains trailing tokens".into(),
        ));
    }
    Ok(surfaces)
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
        let kind = cursor.integer("curve type")?;
        let curve = match kind {
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
            other => {
                return Err(CodecError::NotImplemented(format!(
                    "text B-rep 3D curve family {other} at table index {}",
                    index + 1
                )))
            }
        };
        curves.push(curve);
    }
    if !cursor.is_empty() {
        return Err(CodecError::Malformed(
            "text B-rep Curves table contains trailing tokens".into(),
        ));
    }
    Ok(curves)
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

    fn integer(&mut self, label: &str) -> Result<i64, CodecError> {
        self.next(label)?.parse().map_err(|_| {
            CodecError::Malformed(format!("invalid {label} in text B-rep Curves table"))
        })
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
