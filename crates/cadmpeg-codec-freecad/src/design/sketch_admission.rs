// SPDX-License-Identifier: Apache-2.0
//! Typed sketch admission facts precede record construction.

use super::*;

pub(super) struct GeometryFact {
    pub(super) ordinal: usize,
    pub(super) order: usize,
    pub(super) property: String,
    pub(super) geometry: SketchGeometry,
}
pub(super) struct SketchFacts {
    pub(super) geometry: Vec<GeometryFact>,
    pub(super) constraints: Option<PropertyRecord>,
    pub(super) frame: (Point3, Vector3, Vector3),
}

pub(super) fn facts(
    reader: &crate::property_reader::PropertyReader<'_, '_>,
    properties: &[&PropertyRecord],
) -> Result<SketchFacts, CodecError> {
    let mut geometry = Vec::new();
    if let Some(property) = property(properties, "Geometry") {
        if property.type_name != "Part::PropertyGeometryList" {
            return Err(CodecError::malformed(
                "sketch Geometry runtime is not Part::PropertyGeometryList",
            ));
        }
        let current = source_property(reader, property, false)?;
        validate_count(reader, &current, "GeometryList", "Geometry")?;
        let bytes = reader.property_xml(&current)?;
        let xml = reader.parse_xml(bytes.as_ref(), "FreeCAD sketch admission geometry")?;
        let records = counted_records(reader, &current, &xml, "GeometryList", "Geometry")?;
        let held = current
            .values()
            .iter()
            .filter(|value| value.tag == "Geometry")
            .collect::<Vec<_>>();
        for (index, node) in records.into_iter().enumerate() {
            let carrier = sketch_carrier(node);
            if let (Some(kind), Some(carrier)) = (node.attribute("type"), carrier.as_ref()) {
                validate_sketch_carrier(kind, carrier, index + 1)?;
            }
            let kind = node
                .attribute("type")
                .or_else(|| carrier.map(|child| child.tag_name().name()))
                .unwrap_or("unknown");
            let value = match carrier
                .map(|carrier| sketch_nurbs(kind, carrier))
                .transpose()?
                .flatten()
            {
                Some(value) => value,
                None => sketch_geometry(kind, &sketch_attributes(carrier))?,
            };
            let row = held.get(index).ok_or_else(|| {
                CodecError::malformed("sketch geometry has no admitted source slot")
            })?;
            geometry.push(GeometryFact {
                ordinal: reader.parameter_ordinal(property, row.order)?,
                order: row.order,
                property: property.id.clone(),
                geometry: value,
            });
        }
    }
    let constraints = property(properties, "Constraints")
        .map(|property| -> Result<_, CodecError> {
            if property.type_name != "Sketcher::PropertyConstraintList" {
                return Err(CodecError::malformed(
                    "sketch Constraints runtime is not Sketcher::PropertyConstraintList",
                ));
            }
            let current = source_property(reader, property, true)?;
            validate_count(reader, &current, "ConstraintList", "Constrain")?;
            let bytes = reader.property_xml(&current)?;
            let xml = reader.parse_xml(bytes.as_ref(), "FreeCAD sketch admission constraints")?;
            for node in counted_records(reader, &current, &xml, "ConstraintList", "Constrain")? {
                constraint_operands(node)?;
                if matches!(int_attr(node, "Type"), Some(6..=9 | 11 | 16 | 18 | 19)) {
                    if let Some(value) = node
                        .attribute("Value")
                        .and_then(|value| value.parse::<f64>().ok())
                    {
                        if !value.is_finite() {
                            return Err(CodecError::malformed(
                                "constraint dimension must be finite",
                            ));
                        }
                    }
                }
            }
            Ok(current)
        })
        .transpose()?;
    let placement = property(properties, "Placement")
        .or_else(|| property(properties, "AttachmentOffset"))
        .map(|property| source_property(reader, property, false))
        .transpose()?;
    let mut frame_properties = properties.to_vec();
    if let Some(placement) = &placement {
        frame_properties.retain(|property| property.id != placement.id);
        frame_properties.push(placement);
    }
    let frame = sketch_frame(reader, &frame_properties)?;
    Ok(SketchFacts {
        geometry,
        constraints,
        frame,
    })
}

fn validate_count(
    reader: &crate::property_reader::PropertyReader<'_, '_>,
    property: &PropertyRecord,
    root: &str,
    row: &str,
) -> Result<(), CodecError> {
    let Some(root) = property
        .values()
        .iter()
        .find(|value| value.parent.is_none() && value.tag == root)
    else {
        reader.unavailable(property)?;
        return Err(CodecError::malformed("sketch counted root is unavailable"));
    };
    let count = root
        .attributes
        .get("count")
        .and_then(|count| count.parse::<usize>().ok());
    let held = property
        .values()
        .iter()
        .filter(|value| value.parent == Some(root.order) && value.tag == row)
        .count();
    if count != Some(held) {
        reader.unavailable(property)?;
        return Err(CodecError::malformed(
            "sketch source count differs from its held rows",
        ));
    }
    Ok(())
}

fn counted_records<'a, 'input>(
    reader: &crate::property_reader::PropertyReader<'_, '_>,
    property: &PropertyRecord,
    xml: &'a roxmltree::Document<'input>,
    root: &str,
    row: &str,
) -> Result<Vec<roxmltree::Node<'a, 'input>>, CodecError> {
    match direct_counted_records(xml, root, row, &property.id) {
        Err(error @ CodecError::Malformed(_)) if matches!(&property.body, Some(crate::native::PropertyBody::Persisted { values, .. }) if values.has_missing()) =>
        {
            // The actual row helper already owns the failure. Its absence
            // prevents this required counted carrier from forming; requesting
            // its genuine home lets the existing Native fallback recover.
            reader.unavailable(property)?;
            Err(error)
        }
        result => result,
    }
}

fn source_property(
    reader: &crate::property_reader::PropertyReader<'_, '_>,
    property: &PropertyRecord,
    constraint: bool,
) -> Result<PropertyRecord, CodecError> {
    if property.body.is_none() {
        reader.unavailable(property)?;
        return Err(CodecError::malformed(
            "sketch admission carrier is unavailable",
        ));
    }
    let mut current = property.clone();
    let mut rows = Vec::new();
    for original in property.values() {
        let mut value = original.clone();
        let geometry_kind = property
            .values()
            .iter()
            .find(|row| {
                row.tag == "Geometry"
                    && (Some(row.order) == value.parent || row.order == value.order)
            })
            .and_then(|row| row.attributes.get("type"))
            .map(String::as_str)
            .unwrap_or(value.tag.as_str());
        let mut fields = fields(&value, geometry_kind, constraint).to_vec();
        for (primary, fallback) in [
            ("StartAngle", "FirstParameter"),
            ("EndAngle", "LastParameter"),
        ] {
            if value
                .attributes
                .get(primary)
                .is_some_and(|raw| raw.parse::<f64>().is_ok())
            {
                fields.retain(|field| *field != fallback);
            }
        }
        if matches!(
            geometry_kind,
            "Part::GeomEllipse" | "Part::GeomArcOfEllipse" | "Ellipse" | "ArcOfEllipse"
        ) {
            if value
                .attributes
                .get("MajorAngle")
                .is_some_and(|raw| raw.parse::<f64>().is_ok())
            {
                fields.retain(|field| !matches!(*field, "AngleXU" | "MajorAxisX" | "MajorAxisY"));
            } else if value
                .attributes
                .get("AngleXU")
                .is_some_and(|raw| raw.parse::<f64>().is_ok())
            {
                fields.retain(|field| !matches!(*field, "MajorAxisX" | "MajorAxisY"));
            }
        }
        if matches!(
            geometry_kind,
            "Part::GeomHyperbola"
                | "Part::GeomArcOfHyperbola"
                | "Hyperbola"
                | "ArcOfHyperbola"
                | "Part::GeomParabola"
                | "Part::GeomArcOfParabola"
                | "Parabola"
                | "ArcOfParabola"
        ) && value
            .attributes
            .get("AngleXU")
            .is_some_and(|raw| raw.parse::<f64>().is_ok())
        {
            fields.retain(|field| !matches!(*field, "MajorAngle" | "AxisAngle"));
        }
        if !fields.is_empty() {
            if let Some(source) = reader.feature_fields(property, value.order, &fields)? {
                current.type_name = source.kind;
                current.name = source.name;
                for field in fields {
                    value.attributes.remove(field);
                }
                value.attributes.extend(source.attributes);
            }
        }
        rows.push(value);
    }
    let count = match &property.body {
        Some(crate::native::PropertyBody::Persisted { values, .. }) => values.slot_count(),
        _ => 0,
    };
    current.body = Some(crate::native::PropertyBody::Persisted {
        values: crate::native::ValueSlots::new(rows, count).map_err(CodecError::malformed)?,
        links: property.links().to_vec(),
        side_entries: property.side_entries().to_vec(),
    });
    Ok(current)
}
fn fields<'a>(value: &crate::native::ValueRecord, kind: &str, constraint: bool) -> &'a [&'a str] {
    match value.tag.as_str() {
        "GeometryList" | "ConstraintList" => &["count"],
        "Geometry" => &["type"],
        "Constrain" if constraint => match (
            value.attributes.contains_key("ElementIds")
                || value.attributes.contains_key("ElementPositions"),
            value
                .attributes
                .get("Type")
                .and_then(|raw| raw.parse::<i64>().ok())
                .is_some_and(|kind| matches!(kind, 6..=9 | 11 | 16 | 18 | 19)),
        ) {
            (true, true) => &["Type", "ElementIds", "ElementPositions", "Value"],
            (true, false) => &["Type", "ElementIds", "ElementPositions"],
            (false, true) => &[
                "Type",
                "First",
                "FirstPos",
                "Second",
                "SecondPos",
                "Third",
                "ThirdPos",
                "Value",
            ],
            (false, false) => &[
                "Type",
                "First",
                "FirstPos",
                "Second",
                "SecondPos",
                "Third",
                "ThirdPos",
            ],
        },
        "PropertyPlacement" if value.attributes.contains_key("A") => {
            &["Px", "Py", "Pz", "Ox", "Oy", "Oz", "A"]
        }
        "PropertyPlacement" => &["Px", "Py", "Pz", "Q0", "Q1", "Q2", "Q3"],
        "BSplineCurve" => &["Degree", "IsPeriodic", "PolesCount", "KnotsCount"],
        "Pole" => &["X", "Y", "Z", "Weight"],
        "Knot" => &["Value", "Mult"],
        _ => match kind {
            "Part::GeomLine" | "Part::GeomLineSegment" | "Line" | "LineSegment" => {
                &["StartX", "StartY", "EndX", "EndY"]
            }
            "Part::GeomArcOfCircle" | "ArcOfCircle" => &[
                "CenterX",
                "CenterY",
                "Radius",
                "AngleXU",
                "StartAngle",
                "EndAngle",
                "FirstParameter",
                "LastParameter",
            ],
            "Part::GeomCircle" | "Circle" => &["CenterX", "CenterY", "Radius"],
            "Part::GeomEllipse" | "Ellipse" => &[
                "CenterX",
                "CenterY",
                "MajorAngle",
                "AngleXU",
                "MajorAxisY",
                "MajorAxisX",
                "MajorRadius",
                "MinorRadius",
            ],
            "Part::GeomArcOfEllipse" | "ArcOfEllipse" => &[
                "CenterX",
                "CenterY",
                "MajorAngle",
                "AngleXU",
                "MajorAxisY",
                "MajorAxisX",
                "MajorRadius",
                "MinorRadius",
                "StartAngle",
                "EndAngle",
                "FirstParameter",
                "LastParameter",
            ],
            "Part::GeomHyperbola" | "Hyperbola" => &[
                "CenterX",
                "CenterY",
                "AngleXU",
                "MajorAngle",
                "MajorRadius",
                "MinorRadius",
            ],
            "Part::GeomArcOfHyperbola" | "ArcOfHyperbola" => &[
                "CenterX",
                "CenterY",
                "AngleXU",
                "MajorAngle",
                "MajorRadius",
                "MinorRadius",
                "StartAngle",
                "EndAngle",
                "FirstParameter",
                "LastParameter",
            ],
            "Part::GeomParabola" | "Parabola" => {
                &["CenterX", "CenterY", "AngleXU", "AxisAngle", "Focal"]
            }
            "Part::GeomArcOfParabola" | "ArcOfParabola" => &[
                "CenterX",
                "CenterY",
                "AngleXU",
                "AxisAngle",
                "Focal",
                "StartAngle",
                "EndAngle",
                "FirstParameter",
                "LastParameter",
            ],
            "Part::GeomPoint" | "GeomPoint" => &["X", "Y"],
            _ => &[],
        },
    }
}

pub(super) fn construction(
    reader: &crate::property_reader::PropertyReader<'_, '_>,
    property: &PropertyRecord,
    order: usize,
) -> Result<bool, CodecError> {
    for value in property
        .values()
        .iter()
        .filter(|value| value.tag == "Construction")
    {
        let mut parent = value.parent;
        while let Some(ancestor) = parent {
            if ancestor == order {
                let source = reader.feature_fields(property, value.order, &["value"])?;
                let raw = match &source {
                    Some(source) => source.attributes.get("value"),
                    None => value.attributes.get("value"),
                };
                if raw.is_some_and(|value| value != "0") {
                    return Ok(true);
                }
                break;
            }
            parent = property
                .values()
                .iter()
                .find(|row| row.order == ancestor)
                .and_then(|row| row.parent);
        }
    }
    Ok(false)
}
