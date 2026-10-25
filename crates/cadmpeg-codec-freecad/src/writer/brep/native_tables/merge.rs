// SPDX-License-Identifier: Apache-2.0
//! Merge canonical rows into generated tables while neutral roots remain authoritative.

use super::*;
use crate::brep::source::remap::Indices;
use crate::brep::{
    TextCurve2d, TextEdgeRepresentation, TextOrientation, TextPointRepresentation, TextTShape,
    TextTShapeGeometry,
};

mod native;
#[cfg(test)]
mod partial;
mod polygon_order;

const NAMES: [&str; 8] = [
    "Locations",
    "Curve2ds",
    "Curves",
    "Polygon3D",
    "PolygonOnTriangulations",
    "Surfaces",
    "Triangulations",
    "TShapes",
];

pub(super) fn merge(
    ir: &CadIr,
    payload: &ShapePayloadRecord,
    rows: &[BrepRow],
    bytes: &mut Vec<u8>,
    polygon_homes: &HashMap<String, Vec<usize>>,
    native_only: bool,
    has_geometry: &mut bool,
    dispositions: &mut NativeDispositions,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<std::collections::HashSet<String>, CodecError> {
    let mut emitted = std::collections::HashSet::new();
    if native_only && payload.has_unread_tables() {
        bytes.clear();
        losses.push(crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
            "B-rep payload {} has unread tables or root uses and has no complete exact encoding", payload.id
        )));
        super::super::super::dispositions::record(
            dispositions,
            "shape_payloads",
            &payload.id,
            NativeDisposition::Dropped,
        );
        return Ok(emitted);
    }
    let Some(facts) = crate::brep::Tables::from_payload(payload) else {
        bytes.clear();
        super::super::super::dispositions::record(
            dispositions,
            "shape_payloads",
            &payload.id,
            if native_only {
                NativeDisposition::Written
            } else {
                NativeDisposition::Regenerated
            },
        );
        return Ok(emitted);
    };
    // The aggregate is the canonical nonpolygon record. W2 replaces it as a
    // whole; no old table slot can override or resurrect current neutral data.
    let mut generated = if native_only {
        Generated::empty()
    } else {
        Generated::read(bytes)?
    };
    if native_only && facts.tshapes.iter().any(|shape|matches!(&shape.geometry,TextTShapeGeometry::Edge{representations,..} if representations.iter().any(|rep|matches!(rep,TextEdgeRepresentation::Pcurve{uv_endpoints:Some(_),..}|TextEdgeRepresentation::PcurvePair{uv_endpoints:Some(_),..})))) {generated.version=2;}
    if native_only && generated.version==2 && (facts.triangulations.iter().any(|value|value.normals().is_some()) || facts.tshapes.iter().any(|shape|matches!(&shape.geometry,TextTShapeGeometry::Edge{representations,..} if representations.iter().any(|rep|matches!(rep,TextEdgeRepresentation::Pcurve{uv_endpoints:None,..}|TextEdgeRepresentation::PcurvePair{uv_endpoints:None,..}))))) {
        bytes.clear();
        losses.push(crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!("B-rep payload {} has UV endpoint and normal lanes without one exact text encoding and is omitted",payload.id)));
        super::super::super::dispositions::record(dispositions,"shape_payloads",&payload.id,NativeDisposition::Dropped);
        return Ok(emitted);
    }
    generated.polygon_sources = polygon_homes.clone();
    let mut maps = Indices::default();
    if native_only {
        for (offset, value) in facts.locations.iter().enumerate() {
            let text = if value.factors.is_empty() {
                format!(
                    "1\n{}",
                    value
                        .transform
                        .affine_rows()
                        .iter()
                        .map(|row| row
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(" "))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            } else {
                let mut text = String::from("2");
                for factor in &value.factors {
                    let index = maps.get(0, factor.location).ok_or_else(|| {
                        CodecError::malformed("native location factor lost its prior row")
                    })?;
                    text.push_str(&format!(" {index} {}", factor.power));
                }
                text.push_str(" 0");
                text
            };
            generated.tables[0].push(text);
            maps.bind(0, offset + 1, generated.tables[0].len());
        }
        for (table, values) in [
            (1, facts.curve2ds.iter().map(curve2d).collect::<Vec<_>>()),
            (2, facts.curves.iter().map(native::curve).collect()),
            (5, facts.surfaces.iter().map(native::surface).collect()),
        ] {
            for (offset, value) in values.into_iter().enumerate() {
                if let Some(value) = value {
                    generated.tables[table].push(value);
                    maps.bind(
                        u8::try_from(table)
                            .map_err(|_| CodecError::malformed("B-rep table index exceeds u8"))?,
                        offset + 1,
                        generated.tables[table].len(),
                    );
                } else {
                    losses.push(
                        crate::loss::FreecadLossCode::ExportGeometryNotTransferred.note(format!(
                            "B-rep payload {} {} entry {} has no text encoding and is omitted",
                            payload.id,
                            NAMES[table],
                            offset + 1
                        )),
                    );
                    bytes.clear();
                    super::super::super::dispositions::record(
                        dispositions,
                        "shape_payloads",
                        &payload.id,
                        NativeDisposition::Dropped,
                    );
                    return Ok(emitted);
                }
            }
        }
    }
    // Polygon records have their own canonical homes. Their ordered payload
    // references remain authoritative independently of aggregate regeneration.
    for (table, count) in [
        (3, facts.polygons3d.len()),
        (4, facts.polygons_on_triangulations.len()),
    ] {
        for dense in 1..=count {
            let Some(id) = payload.row_identity(table, dense) else {
                continue;
            };
            let Some(row) = rows.iter().find(|row| row.id == id) else {
                continue;
            };
            let currents = if table == 3 && !native_only {
                ir.model
                    .curves
                    .iter()
                    .filter(|curve| curve.native_ref.as_deref() == Some(id))
                    .filter_map(super::super::facets::polyline)
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let texts = if currents.is_empty() {
                vec![match &row.value {
                    BrepValue::Polygon3d(value) => polygon3d(value),
                    BrepValue::PolygonOnTriangulation(value) => indexed_polygon(value),
                    _ => {
                        return Err(CodecError::malformed(
                            "polygon reference names a nonpolygon value",
                        ))
                    }
                }]
            } else {
                currents
            };
            let changed = !native_only
                && ir
                    .model
                    .curves
                    .iter()
                    .any(|curve| curve.native_ref.as_deref() == Some(id));
            for text in texts {
                let target = generated.push_polygon(usize::from(table), id, text)?;
                if maps.get(table, dense).is_none() {
                    maps.bind(table, dense, target);
                }
            }
            super::super::super::dispositions::record(
                dispositions,
                if table == 3 {
                    "polygons3d"
                } else {
                    "polygons_on_triangulations"
                },
                id,
                if changed {
                    NativeDisposition::Regenerated
                } else {
                    NativeDisposition::Written
                },
            );
        }
    }
    if native_only {
        for (offset, value) in facts.triangulations.iter().enumerate() {
            generated.tables[6].push(native::triangulation(value, generated.version)?);
            maps.bind(6, offset + 1, generated.tables[6].len());
        }
        let mut native_shapes = Vec::new();
        for (offset, shape) in facts.tshapes.iter().enumerate() {
            let Some(BrepValue::TShape(shapes)) =
                maps.resolve(BrepValue::TShape(vec![shape.clone()].into()))
            else {
                continue;
            };
            let Some(mut shape) = shapes.first().cloned() else {
                continue;
            };
            for child in &mut shape.children {
                child.shape = maps.get(7, child.shape).ok_or_else(|| {
                    CodecError::malformed("native child lost its admitted prefix")
                })?;
            }
            maps.bind(7, offset + 1, native_shapes.len() + 1);
            native_shapes.push(shape);
        }
        generated.prepend_shapes(&native_shapes);
        let total = native_shapes.len();
        let mut roots = String::new();
        for root in facts.roots {
            let Some(index) = maps.get(7, root.shape) else {
                continue;
            };
            let location = if usize::from(root.location) == 0 {
                0
            } else {
                let Some(index) = maps.get(0, usize::from(root.location)) else {
                    continue;
                };
                index
            };
            let sign = match root.orientation {
                TextOrientation::Forward => '+',
                TextOrientation::Reversed => '-',
                TextOrientation::Internal => 'i',
                TextOrientation::External => 'e',
            };
            roots.push_str(&format!("{sign}{} {location} ", total - index + 1));
        }
        roots.push('*');
        generated.roots = roots;
        emitted.extend(
            ir.model
                .tessellations
                .iter()
                .filter(|mesh| mesh.native_ref.as_deref() == Some(payload.id.as_str()))
                .map(|mesh| mesh.id.to_string()),
        );
    }
    generated.order_polygons()?;
    *has_geometry |= [1, 2, 3, 5, 6]
        .iter()
        .any(|table| !generated.tables[*table].is_empty())
        || generated.shapes.iter().any(|shape| {
            shape
                .split_ascii_whitespace()
                .next()
                .is_some_and(|kind| matches!(kind, "Ve" | "Ed" | "Fa"))
        });
    let mut normalized_counts = Vec::new();
    if native_only {
        if let Some(counts) = payload.declared_text_counts() {
            for (table, name) in NAMES.iter().enumerate() {
                let Some(declared) = counts.get(*name) else {
                    continue;
                };
                let actual = if table == 7 {
                    generated.shapes.len()
                } else {
                    generated.tables[table].len()
                };
                if *declared != actual {
                    normalized_counts.push(format!("section_counts.{name} {declared} -> {actual}"));
                }
            }
        }
    }
    if !normalized_counts.is_empty() {
        losses.push(cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
            "B-rep payload {} retains its readable rows; original count fields cannot be re-emitted exactly: {}",
            payload.id, normalized_counts.join(", "),
        )));
    }
    generated.write(bytes);
    super::super::super::dispositions::record(
        dispositions,
        "shape_payloads",
        &payload.id,
        if native_only && !normalized_counts.is_empty() {
            NativeDisposition::Dropped
        } else if native_only {
            NativeDisposition::Written
        } else {
            NativeDisposition::Regenerated
        },
    );
    Ok(emitted)
}

struct Generated {
    version: u8,
    polygon_sources: HashMap<String, Vec<usize>>,
    polygon_ids: BTreeMap<String, (usize, usize)>,
    polygon_order: [Vec<usize>; 2],
    tables: [Vec<String>; 7],
    shapes: Vec<String>,
    roots: String,
}
impl Generated {
    fn empty() -> Self {
        Self {
            version: 3,
            polygon_sources: HashMap::new(),
            polygon_ids: BTreeMap::new(),
            polygon_order: Default::default(),
            tables: Default::default(),
            shapes: Vec::new(),
            roots: String::from("*"),
        }
    }
    fn read(bytes: &[u8]) -> Result<Self, CodecError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| CodecError::malformed("generated B-rep is not text"))?;
        let mut tables: [Vec<String>; 7] = Default::default();
        let mut bodies = Vec::new();
        for name in NAMES {
            let marker = format!("{name} ");
            let start = text
                .match_indices(&marker)
                .find(|(offset, _)| *offset == 0 || text.as_bytes().get(offset - 1) == Some(&b'\n'))
                .map(|(offset, _)| offset)
                .ok_or_else(|| CodecError::malformed("generated B-rep table is absent"))?;
            let count_end = text[start..].find('\n').ok_or_else(|| {
                CodecError::malformed("generated B-rep table header is unterminated")
            })? + start;
            let count = text[start + marker.len()..count_end]
                .parse::<usize>()
                .map_err(|_| CodecError::malformed("generated B-rep count is invalid"))?;
            bodies.push((count, count_end + 1, start));
        }
        for table in 0..7 {
            let end = bodies[table + 1].2;
            let lines = text[bodies[table].1..end]
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect::<Vec<_>>();
            if table == 0 {
                for row in lines.chunks(4) {
                    if row.len() != 4 {
                        return Err(CodecError::malformed(
                            "generated location row is incomplete",
                        ));
                    }
                    tables[table].push(row.join("\n"));
                }
            } else {
                tables[table] = lines.into_iter().map(str::to_owned).collect();
            }
            if tables[table].len() != bodies[table].0 {
                return Err(CodecError::malformed(
                    "generated B-rep table length differs from its count",
                ));
            }
        }
        let mut tail = &text[bodies[7].1..];
        let mut shapes = Vec::new();
        for _ in 0..bodies[7].0 {
            let end = tail
                .find('*')
                .ok_or_else(|| CodecError::malformed("generated TShape lacks terminator"))?
                + 1;
            shapes.push(tail[..end].trim().to_owned());
            tail = &tail[end..];
        }
        Ok(Self {
            version: 3,
            polygon_sources: HashMap::new(),
            polygon_ids: BTreeMap::new(),
            polygon_order: Default::default(),
            tables,
            shapes,
            roots: tail.trim().to_owned(),
        })
    }
    fn push(&mut self, table: usize, row: String) -> usize {
        if let Some(index) = self.tables[table].iter().position(|value| *value == row) {
            index + 1
        } else {
            self.tables[table].push(row);
            self.tables[table].len()
        }
    }
    fn prepend_shapes(&mut self, shapes: &[TextTShape]) {
        let total = shapes.len() + self.shapes.len();
        let mut prefix = shapes
            .iter()
            .map(|shape| tshape(shape, total, self.version))
            .collect::<Vec<_>>();
        prefix.append(&mut self.shapes);
        self.shapes = prefix;
    }
    fn write(self, bytes: &mut Vec<u8>) {
        let mut text = if self.version == 2 {
            String::from("CASCADE Topology V2, (c) Matra-Datavision\n")
        } else {
            String::from("CASCADE Topology V3, (c) Open Cascade\n")
        };
        for (name, rows) in NAMES[..7].iter().zip(self.tables) {
            text.push_str(&format!("{name} {}\n", rows.len()));
            for row in rows {
                text.push_str(&row);
                text.push('\n');
            }
        }
        text.push_str(&format!("TShapes {}\n", self.shapes.len()));
        for shape in self.shapes {
            text.push_str(&shape);
            text.push('\n');
        }
        text.push_str(&self.roots);
        text.push('\n');
        *bytes = text.into_bytes();
    }
}

fn point2(value: cadmpeg_ir::units::FinitePoint2) -> String {
    let value = value.get();
    format!("{} {}", value.u, value.v)
}
fn curve2d(value: &TextCurve2d) -> Option<String> {
    Some(match value {
        TextCurve2d::Line { origin, direction } => {
            format!("1 {} {}", point2(*origin), point2(*direction))
        }
        TextCurve2d::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => format!(
            "2 {} {} {} {}",
            point2(*center),
            point2(*x_axis),
            point2(*y_axis),
            radius.get()
        ),
        TextCurve2d::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => format!(
            "3 {} {} {} {} {}",
            point2(*center),
            point2(*x_axis),
            point2(*y_axis),
            major_radius.get(),
            minor_radius.get()
        ),
        TextCurve2d::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } => format!(
            "4 {} {} {} {}",
            point2(*vertex),
            point2(*x_axis),
            point2(*y_axis),
            focal_distance.get()
        ),
        TextCurve2d::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => format!(
            "5 {} {} {} {} {}",
            point2(*center),
            point2(*x_axis),
            point2(*y_axis),
            major_radius.get(),
            minor_radius.get()
        ),
        TextCurve2d::Trimmed {
            parameter_range,
            basis,
        } => format!(
            "8 {} {} {}",
            parameter_range[0].get(),
            parameter_range[1].get(),
            curve2d(basis.curve())?
        ),
        TextCurve2d::Offset { distance, basis } => {
            format!("9 {} {}", distance.get(), curve2d(basis.curve())?)
        }
        TextCurve2d::Nurbs(value) => {
            let knots =
                cadmpeg_ir::geometry::nurbs::KnotVector::from_finite_lanes(value.knots.clone())
                    .ok()?;
            let axis = super::super::super::geometry::nurbs::axis(
                &knots,
                value.degree,
                value.control_points.len(),
                value.periodic,
            )?;
            if !super::super::super::geometry::nurbs::cyclic_tail(&value.control_points, &axis)
                || value.weights.as_ref().is_some_and(|weights| {
                    !super::super::super::geometry::nurbs::cyclic_tail(weights, &axis)
                })
            {
                return None;
            }
            let mut text = format!(
                "7 {} {} {} {} {}",
                u8::from(value.weights.is_some()),
                u8::from(value.periodic),
                value.degree,
                axis.count,
                axis.rows.len()
            );
            for (index, point) in value.control_points.iter().take(axis.count).enumerate() {
                text.push_str(&format!(" {}", point2(*point)));
                if let Some(weights) = &value.weights {
                    text.push_str(&format!(" {}", weights[index].get()));
                }
            }
            for (knot, count) in axis.rows {
                text.push_str(&format!(" {knot} {count}"));
            }
            text
        }
    })
}

fn tshape(shape: &TextTShape, total: usize, version: u8) -> String {
    let (kind, geometry) = match &shape.geometry {
        TextTShapeGeometry::Vertex {
            tolerance,
            point,
            representations,
        } => {
            let mut text = format!(
                "{} {}",
                tolerance.get(),
                super::super::super::geometry::point(point.get())
            );
            for value in representations {
                match value {
                    TextPointRepresentation::Curve3d {
                        parameter,
                        curve,
                        location,
                    } => text.push_str(&format!(" {} 1 {curve} {location}", parameter.get())),
                    TextPointRepresentation::Pcurve {
                        parameter,
                        curve,
                        surface,
                        location,
                    } => text.push_str(&format!(
                        " {} 2 {curve} {surface} {location}",
                        parameter.get()
                    )),
                    TextPointRepresentation::Surface {
                        parameter,
                        second_parameter,
                        surface,
                        location,
                    } => text.push_str(&format!(
                        " {} 3 {} {surface} {location}",
                        parameter.get(),
                        second_parameter.get()
                    )),
                }
            }
            text.push_str(" 0 0");
            ("Ve", text)
        }
        TextTShapeGeometry::Edge {
            tolerance,
            same_parameter,
            same_range,
            degenerated,
            representations,
        } => {
            let mut text = format!(
                "{} {} {} {}",
                tolerance.get(),
                u8::from(*same_parameter),
                u8::from(*same_range),
                u8::from(*degenerated)
            );
            for value in representations {
                match value {
                    TextEdgeRepresentation::Curve3d {
                        curve,
                        location,
                        parameter_range,
                    } => text.push_str(&format!(
                        " 1 {curve} {location} {} {}",
                        parameter_range[0].get(),
                        parameter_range[1].get()
                    )),
                    TextEdgeRepresentation::Pcurve {
                        curve,
                        surface,
                        location,
                        parameter_range,
                        uv_endpoints,
                    } => {
                        text.push_str(&format!(
                            " 2 {curve} {surface} {location} {} {}",
                            parameter_range[0].get(),
                            parameter_range[1].get()
                        ));
                        if version == 2 {
                            if let Some(points) = uv_endpoints {
                                text.push_str(&format!(
                                    " {} {}",
                                    point2(points[0]),
                                    point2(points[1])
                                ));
                            }
                        }
                    }
                    TextEdgeRepresentation::PcurvePair {
                        curves,
                        continuity,
                        surface,
                        location,
                        parameter_range,
                        uv_endpoints,
                    } => {
                        text.push_str(&format!(
                            " 3 {} {} {continuity} {surface} {location} {} {}",
                            curves[0],
                            curves[1],
                            parameter_range[0].get(),
                            parameter_range[1].get()
                        ));
                        if version == 2 {
                            if let Some(points) = uv_endpoints {
                                text.push_str(&format!(
                                    " {} {}",
                                    point2(points[0]),
                                    point2(points[1])
                                ));
                            }
                        }
                    }
                    TextEdgeRepresentation::Regularity {
                        continuity,
                        surfaces,
                        locations,
                    } => text.push_str(&format!(
                        " 4 {continuity} {} {} {} {}",
                        surfaces[0], locations[0], surfaces[1], locations[1]
                    )),
                    TextEdgeRepresentation::Polygon3d { polygon, location } => {
                        text.push_str(&format!(" 5 {polygon} {location}"))
                    }
                    TextEdgeRepresentation::PolygonOnTriangulation {
                        polygon,
                        triangulation,
                        location,
                    } => text.push_str(&format!(" 6 {polygon} {triangulation} {location}")),
                    TextEdgeRepresentation::PolygonPair {
                        polygons,
                        triangulation,
                        location,
                    } => text.push_str(&format!(
                        " 7 {} {} {triangulation} {location}",
                        polygons[0], polygons[1]
                    )),
                }
            }
            text.push_str(" 0");
            ("Ed", text)
        }
        TextTShapeGeometry::Face {
            natural_restriction,
            tolerance,
            surface,
            location,
            triangulation,
        } => {
            let mut text = format!(
                "{} {} {} {}",
                u8::from(*natural_restriction),
                tolerance.get(),
                surface.map_or(0, crate::brep::TableRef::index),
                usize::from(*location)
            );
            if let Some(index) = triangulation {
                text.push_str(&format!(" 2 {}", index.index()));
            }
            ("Fa", text)
        }
        TextTShapeGeometry::Wire => ("Wi", String::new()),
        TextTShapeGeometry::Shell => ("Sh", String::new()),
        TextTShapeGeometry::Solid => ("So", String::new()),
        TextTShapeGeometry::CompSolid => ("CS", String::new()),
        TextTShapeGeometry::Compound => ("Co", String::new()),
    };
    let mut text = format!(
        "{kind}\n{geometry}\n{}\n",
        shape
            .flags
            .iter()
            .map(|flag| if *flag { '1' } else { '0' })
            .collect::<String>()
    );
    for child in &shape.children {
        let orientation = match child.orientation {
            TextOrientation::Forward => '+',
            TextOrientation::Reversed => '-',
            TextOrientation::Internal => 'i',
            TextOrientation::External => 'e',
        };
        text.push_str(&format!(
            "{orientation}{} {} ",
            total - child.shape + 1,
            usize::from(child.location)
        ));
    }
    text.push('*');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brep::{ShapePayload, TextTopologyVersion};

    fn canonical_fixture(owner: &str, rows: &[BrepValue]) -> ShapePayloadRecord {
        let mut facts = serde_json::json!({"topology_version":4,"locations":[],"curve2ds":[],"curves":[],"surfaces":[],"polygons3d":[],"polygons_on_triangulations":[],"triangulations":[],"tshapes":[],"roots":[]});
        for row in rows {
            let field = match row {
                BrepValue::Curve2d(_) => "curve2ds",
                BrepValue::Curve(_) => "curves",
                BrepValue::Triangulation(_) => "triangulations",
                BrepValue::TShape(shapes) => {
                    facts["tshapes"].as_array_mut().unwrap().extend(
                        serde_json::to_value(shapes)
                            .unwrap()
                            .as_array()
                            .unwrap()
                            .iter()
                            .cloned(),
                    );
                    continue;
                }
                _ => panic!("fixture table not supported"),
            };
            let value = match row {
                BrepValue::Curve2d(value) => serde_json::to_value(value).unwrap(),
                BrepValue::Curve(value) => serde_json::to_value(value).unwrap(),
                BrepValue::Triangulation(value) => serde_json::to_value(value).unwrap(),
                _ => unreachable!(),
            };
            facts[field].as_array_mut().unwrap().push(value);
        }
        serde_json::from_value(serde_json::json!({"id":owner,"property":"fcstd:native:property#Shape","entry":"fcstd:native:entry#Shape.brp","form":"binary","text":null,"binary":facts})).unwrap()
    }

    fn parsed(bytes: &[u8], original: &ShapePayloadRecord) -> ShapePayloadRecord {
        let (facts, _) = crate::brep::source::parse_text(bytes).unwrap();
        ShapePayloadRecord {
            id: original.id.clone(),
            property: original.property.clone(),
            entry: original.entry.clone(),
            rows: Vec::new(),
            payload: ShapePayload::Text {
                version: TextTopologyVersion::V3,
                facts,
            },
        }
    }

    #[test]
    fn current_neutral_curve_wins_native_flags_and_unused_rows_survive_and_generated_root_stays_authoritative(
    ) {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut ir = CadIr::empty();
        let owner = "fcstd:native:shape-payload#merge";
        let rows = vec![
            BrepValue::Curve2d(TextCurve2d::Line {
                origin: cadmpeg_ir::units::FinitePoint2::ZERO,
                direction: cadmpeg_ir::units::FinitePoint2::from_coordinates(
                    cadmpeg_ir::scalar::FiniteReal::ONE,
                    cadmpeg_ir::scalar::FiniteReal::ZERO,
                ),
            }),
            BrepValue::Curve(crate::brep::TextCurve::Line {
                origin: cadmpeg_ir::features::FinitePoint3::ZERO,
                direction: cadmpeg_ir::features::FiniteVector3::new(
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            }),
            BrepValue::TShape(
                vec![TextTShape {
                    geometry: TextTShapeGeometry::Vertex {
                        tolerance: cadmpeg_ir::scalar::FiniteReal::ZERO,
                        point: cadmpeg_ir::features::FinitePoint3::ZERO,
                        representations: Vec::new(),
                    },
                    flags: [true, false, true, false, true, false, true],
                    children: Vec::new(),
                }]
                .into(),
            ),
        ];
        let payload = canonical_fixture(owner, &rows);
        ir.native
            .namespace_mut("fcstd")
            .set_arena(&ctx, "shape_payloads", &[payload.clone()])
            .unwrap();
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: cadmpeg_ir::ids::CurveId::mint("renamed:model:curve#arbitrary-current").unwrap(),
            native_ref: Some(owner.to_owned()),
            parameter_range: None,
            source_object: None,
            geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(
                cadmpeg_ir::geometry::SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::new(
                        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                            42.0, 0.0, 0.0,
                        ))
                        .unwrap(),
                        cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::new(
                            1.0, 0.0, 0.0,
                        ))
                        .unwrap(),
                    ),
                ),
            ),
        });
        let seed = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 1\nCo\n\n0101101\n*\n+1 0\n";
        let generated = super::super::super::serialize_payload(&ir, &payload)
            .unwrap()
            .unwrap();
        let mut bytes = generated.bytes;
        let mut dispositions = super::super::super::super::dispositions::initial();
        let mut losses = Vec::new();
        let mut has_geometry = false;
        merge(
            &ir,
            &payload,
            &[],
            &mut bytes,
            &generated.polygon_homes,
            false,
            &mut has_geometry,
            &mut dispositions,
            &mut losses,
        )
        .unwrap();
        assert!(losses.is_empty());
        assert!(ir.source.is_none());
        assert!(has_geometry);
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("Curves 1\n1 42 0 0 1 0 0\n"));
        assert!(text.contains("Curve2ds 0\n"));
        assert!(!text.contains("1010101\n*"));
        assert_eq!(
            dispositions.record_disposition("fcstd", "shape_payloads", owner),
            NativeDisposition::Regenerated
        );
        // Whole-payload W2 keeps the supplied generated root; it never appends
        // a vertex or unused curve from the old canonical aggregate.
        bytes = seed.to_vec();
        merge(
            &ir,
            &payload,
            &[],
            &mut bytes,
            &HashMap::new(),
            false,
            &mut has_geometry,
            &mut dispositions,
            &mut losses,
        )
        .unwrap();
        let current = parsed(&bytes, &payload);
        let tables = crate::brep::Tables::from_payload(&current).unwrap();
        assert_eq!(tables.tshapes.len(), 1);
        assert_eq!(tables.roots[0].shape, 1);
        assert!(tables.curves.is_empty());
        // W1 retains the same original native flags and unused parameter curve.
        bytes = seed.to_vec();
        merge(
            &ir,
            &payload,
            &[],
            &mut bytes,
            &HashMap::new(),
            true,
            &mut has_geometry,
            &mut dispositions,
            &mut losses,
        )
        .unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("Curve2ds 1\n1 0 0 1 0\n"));
        assert!(text.contains("1010101\n*"));
        assert!(text.contains("Curves 1\n1 0 0 0 1 0 0\n"));
        assert_eq!(
            dispositions.record_disposition("fcstd", "shape_payloads", owner),
            NativeDisposition::Written
        );
        ir.model.curves.clear();
        assert!(super::super::super::serialize_payload(&ir, &payload)
            .unwrap()
            .is_none());
        assert!(losses.is_empty());
    }

    #[test]
    fn triangulation_uses_the_current_linked_mesh_and_deleting_it_does_not_restore_native_nodes() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut ir = CadIr::empty();
        let owner = "fcstd:native:shape-payload#mesh";
        let row: BrepValue = serde_json::from_value(serde_json::json!({"table":"triangulation","value":{"deflection":0.1,"nodes":[[0.0,0.0,0.0],[1.0,0.0,0.0],[0.0,1.0,0.0]],"uv_nodes":[[0.0,0.0],[1.0,0.0],[0.0,1.0]],"triangles":[[1,2,3]],"normals":null}})).unwrap();
        let payload = canonical_fixture(owner, &[row.clone(), row]);
        ir.native
            .namespace_mut("fcstd")
            .set_arena(&ctx, "shape_payloads", &[payload.clone()])
            .unwrap();
        let point =
            cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(42.0, 0.0, 0.0))
                .unwrap();
        ir.model.tessellations.push(
            cadmpeg_ir::tessellation::Tessellation::from_parts(
                cadmpeg_ir::tessellation::TessellationId::mint("fcstd:model:tessellation#mesh")
                    .unwrap(),
                cadmpeg_ir::tessellation::TessellationMesh::from_checked_list_lanes(
                    vec![
                        point,
                        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                            1.0, 0.0, 0.0,
                        ))
                        .unwrap(),
                        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                            0.0, 1.0, 0.0,
                        ))
                        .unwrap(),
                    ],
                    vec![[0, 1, 2]],
                    None,
                )
                .unwrap(),
                Vec::new(),
            )
            .unwrap()
            .with_native_ref(Some(owner.to_owned()))
            .with_admitted_chordal_deflection(cadmpeg_ir::scalar::NonNegativeReal::new(0.2)),
        );
        let first_id = ir.model.tessellations[0].id.clone();
        let mut second_mesh = ir.model.tessellations[0].clone();
        second_mesh.id = cadmpeg_ir::tessellation::TessellationId::mint(
            "renamed:model:tessellation#arbitrary-second",
        )
        .unwrap();
        second_mesh
            .edit_vertices(|point| {
                point.x *= 2.0;
                Ok(())
            })
            .unwrap();
        ir.model.tessellations.push(second_mesh);
        let mut outcomes = super::super::super::super::dispositions::initial();
        let mut losses = Vec::new();
        let generated = super::super::super::serialize_payload(&ir, &payload)
            .unwrap()
            .unwrap();
        assert_eq!(
            generated.meshes,
            ir.model
                .tessellations
                .iter()
                .map(|mesh| mesh.id.as_str().to_owned())
                .collect()
        );
        let mut bytes = generated.bytes;
        let mut has_geometry = false;
        merge(
            &ir,
            &payload,
            &[],
            &mut bytes,
            &generated.polygon_homes,
            false,
            &mut has_geometry,
            &mut outcomes,
            &mut losses,
        )
        .unwrap();
        assert!(losses.is_empty());
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("Triangulations 2\n3 1 0 0 0.2 42 0 0"));
        assert!(text.contains("3 1 0 0 0.2 84 0 0"));
        let current = parsed(&bytes, &payload);
        assert!(crate::brep::Tables::from_payload(&current)
            .unwrap()
            .triangulations
            .iter()
            .all(|mesh| mesh.uv_nodes().is_none()));
        assert_eq!(
            outcomes.record_disposition("fcstd", "shape_payloads", owner),
            NativeDisposition::Regenerated
        );
        ir.model.tessellations.retain(|mesh| mesh.id != first_id);
        let generated = super::super::super::serialize_payload(&ir, &payload)
            .unwrap()
            .unwrap();
        assert_eq!(
            generated.meshes,
            std::collections::HashSet::from([
                "renamed:model:tessellation#arbitrary-second".to_owned()
            ])
        );
        bytes = generated.bytes;
        merge(
            &ir,
            &payload,
            &[],
            &mut bytes,
            &generated.polygon_homes,
            false,
            &mut has_geometry,
            &mut outcomes,
            &mut losses,
        )
        .unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("Triangulations 1\n3 1 0 0 0.2 84 0 0"));
        assert!(!text.contains("0.2 42 0 0"));
        assert!(losses.is_empty());
        ir.model.tessellations.clear();
        assert!(super::super::super::serialize_payload(&ir, &payload)
            .unwrap()
            .is_none());
        // A source-less W1 write independently retains both original native
        // triangulations. W2 deletion above never invokes this path.
        bytes.clear();
        merge(
            &ir,
            &payload,
            &[],
            &mut bytes,
            &HashMap::new(),
            true,
            &mut has_geometry,
            &mut outcomes,
            &mut losses,
        )
        .unwrap();
        let current = parsed(&bytes, &payload);
        let tables = crate::brep::Tables::from_payload(&current).unwrap();
        assert_eq!(tables.triangulations.len(), 2);
        let original = crate::brep::Tables::from_payload(&payload).unwrap();
        for (current, original) in tables.triangulations.iter().zip(original.triangulations) {
            assert_eq!(current.uv_nodes(), original.uv_nodes());
            assert_eq!(current.uv_nodes().unwrap().len(), 3);
        }
        assert_eq!(
            tables.triangulations[0].nodes()[0],
            cadmpeg_ir::features::FinitePoint3::ZERO
        );
        assert_eq!(
            outcomes.record_disposition("fcstd", "shape_payloads", owner),
            NativeDisposition::Written
        );
        assert!(losses.is_empty());
    }
}
