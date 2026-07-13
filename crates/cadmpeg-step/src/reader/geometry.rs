// SPDX-License-Identifier: Apache-2.0
//! STEP representation units, placements, and geometry carriers.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    derive_reference_direction, Curve, CurveGeometry, NurbsCurve, NurbsSurface, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, PointId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Point;

use crate::parse::{Exchange, RawRecord, Value};

pub(super) struct GeometryResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
}

pub(super) fn decode(exchange: &Exchange, ir: &mut CadIr) -> GeometryResult {
    let scale = length_scale(exchange).unwrap_or(1.0);
    let mut typed = BTreeSet::new();
    let mut warnings = Vec::new();
    let mut points = BTreeMap::new();
    let mut directions = BTreeMap::new();
    let mut vectors = BTreeMap::new();
    let mut placements = BTreeMap::new();

    for (&id, record) in &exchange.records {
        match record.simple_name() {
            Some("CARTESIAN_POINT") => match coordinates(record, 1, scale) {
                Some(position) => {
                    points.insert(id, position);
                    ir.model.points.push(Point {
                        id: PointId(format!("step:data:point#{id}")),
                        position,
                    });
                    typed.insert(id);
                }
                None => warnings.push(format!("CARTESIAN_POINT #{id} has invalid coordinates")),
            },
            Some("DIRECTION") => match vector3(record.parameter(1), 1.0).and_then(normalize) {
                Some(direction) => {
                    directions.insert(id, direction);
                    typed.insert(id);
                }
                None => warnings.push(format!("DIRECTION #{id} is invalid or zero")),
            },
            _ => {}
        }
    }
    for (&id, record) in &exchange.records {
        if record.simple_name() == Some("VECTOR") {
            let value = record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|direction| directions.get(&direction).copied())
                .zip(record.parameter(2).and_then(Value::number))
                .map(|(direction, magnitude)| scale_vector(direction, magnitude * scale));
            if let Some(value) = value {
                vectors.insert(id, value);
                typed.insert(id);
            } else {
                warnings.push(format!(
                    "VECTOR #{id} has an invalid direction or magnitude"
                ));
            }
        }
    }
    for (&id, record) in &exchange.records {
        if record.simple_name() == Some("AXIS2_PLACEMENT_3D") {
            let placement = record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|point| points.get(&point).copied())
                .map(|origin| {
                    let axis = optional_direction(record.parameter(2), &directions)
                        .unwrap_or(Vector3::new(0.0, 0.0, 1.0));
                    let reference = optional_direction(record.parameter(3), &directions)
                        .unwrap_or_else(|| derive_reference_direction(axis));
                    (origin, axis, reference)
                });
            if let Some(placement) = placement {
                placements.insert(id, placement);
                typed.insert(id);
            } else {
                warnings.push(format!("AXIS2_PLACEMENT_3D #{id} has an invalid location"));
            }
        }
    }
    for (&id, record) in &exchange.records {
        let geometry = match record.simple_name() {
            Some("LINE") => record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|point| points.get(&point).copied())
                .zip(
                    record
                        .parameter(2)
                        .and_then(Value::reference)
                        .and_then(|vector| vectors.get(&vector).copied())
                        .and_then(normalize),
                )
                .map(|(origin, direction)| CurveGeometry::Line { origin, direction }),
            Some("CIRCLE") => record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(record.parameter(2).and_then(Value::number))
                .filter(|(_, radius)| radius.is_finite() && *radius > 0.0)
                .map(
                    |((center, axis, ref_direction), radius)| CurveGeometry::Circle {
                        center,
                        axis,
                        ref_direction,
                        radius: radius * scale,
                    },
                ),
            Some("ELLIPSE") => record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(record.parameter(2).and_then(Value::number))
                .zip(record.parameter(3).and_then(Value::number))
                .filter(|((_, major), minor)| {
                    major.is_finite() && minor.is_finite() && *major > 0.0 && *minor > 0.0
                })
                .map(
                    |(((center, axis, major_direction), major_radius), minor_radius)| {
                        CurveGeometry::Ellipse {
                            center,
                            axis,
                            major_direction,
                            major_radius: major_radius * scale,
                            minor_radius: minor_radius * scale,
                        }
                    },
                ),
            Some("B_SPLINE_CURVE_WITH_KNOTS") => {
                nurbs_curve(record, &points).map(CurveGeometry::Nurbs)
            }
            _ => continue,
        };
        if let Some(geometry) = geometry {
            ir.model.curves.push(Curve {
                id: CurveId(format!("step:data:curve#{id}")),
                geometry,
                source_object: None,
            });
            typed.insert(id);
        } else {
            warnings.push(format!(
                "{} #{id} has invalid geometry",
                record.simple_name().unwrap()
            ));
        }
    }
    for (&id, record) in &exchange.records {
        if record.partial("B_SPLINE_CURVE_WITH_KNOTS").is_none()
            || record.simple_name() == Some("B_SPLINE_CURVE_WITH_KNOTS")
        {
            continue;
        }
        if let Some(nurbs) = nurbs_curve(record, &points) {
            ir.model.curves.push(Curve {
                id: CurveId(format!("step:data:curve#{id}")),
                geometry: CurveGeometry::Nurbs(nurbs),
                source_object: None,
            });
            typed.insert(id);
        } else {
            warnings.push(format!(
                "B_SPLINE_CURVE_WITH_KNOTS #{id} has invalid geometry"
            ));
        }
    }

    for (&id, record) in &exchange.records {
        let placement = record
            .parameter(1)
            .and_then(Value::reference)
            .and_then(|placement| placements.get(&placement).copied());
        let geometry = match record.simple_name() {
            Some("PLANE") => placement.map(|(origin, normal, u_axis)| SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            }),
            Some("CYLINDRICAL_SURFACE") => placement.zip(positive(record.parameter(2))).map(
                |((origin, axis, ref_direction), radius)| SurfaceGeometry::Cylinder {
                    origin,
                    axis,
                    ref_direction,
                    radius: radius * scale,
                },
            ),
            Some("CONICAL_SURFACE") => placement
                .zip(positive(record.parameter(2)))
                .zip(record.parameter(3).and_then(Value::number))
                .filter(|(_, angle)| angle.is_finite() && *angle > 0.0)
                .map(|(((origin, axis, ref_direction), radius), half_angle)| {
                    SurfaceGeometry::Cone {
                        origin,
                        axis,
                        ref_direction,
                        radius: radius * scale,
                        ratio: 1.0,
                        half_angle,
                    }
                }),
            Some("SPHERICAL_SURFACE") => placement.zip(positive(record.parameter(2))).map(
                |((center, axis, ref_direction), radius)| SurfaceGeometry::Sphere {
                    center,
                    axis,
                    ref_direction,
                    radius: radius * scale,
                },
            ),
            Some("TOROIDAL_SURFACE") => placement
                .zip(positive(record.parameter(2)))
                .zip(positive(record.parameter(3)))
                .map(
                    |(((center, axis, ref_direction), major_radius), minor_radius)| {
                        SurfaceGeometry::Torus {
                            center,
                            axis,
                            ref_direction,
                            major_radius: major_radius * scale,
                            minor_radius: minor_radius * scale,
                        }
                    },
                ),
            Some("B_SPLINE_SURFACE_WITH_KNOTS") => {
                nurbs_surface(record, &points).map(SurfaceGeometry::Nurbs)
            }
            _ => continue,
        };
        if let Some(geometry) = geometry {
            ir.model.surfaces.push(Surface {
                id: SurfaceId(format!("step:data:surface#{id}")),
                geometry,
                source_object: None,
            });
            typed.insert(id);
        } else {
            warnings.push(format!(
                "{} #{id} has invalid geometry",
                record.simple_name().unwrap()
            ));
        }
    }
    for (&id, record) in &exchange.records {
        if record.partial("B_SPLINE_SURFACE_WITH_KNOTS").is_none()
            || record.simple_name() == Some("B_SPLINE_SURFACE_WITH_KNOTS")
        {
            continue;
        }
        if let Some(nurbs) = nurbs_surface(record, &points) {
            ir.model.surfaces.push(Surface {
                id: SurfaceId(format!("step:data:surface#{id}")),
                geometry: SurfaceGeometry::Nurbs(nurbs),
                source_object: None,
            });
            typed.insert(id);
        } else {
            warnings.push(format!(
                "B_SPLINE_SURFACE_WITH_KNOTS #{id} has invalid geometry"
            ));
        }
    }

    for (&id, record) in &exchange.records {
        if record.partials.iter().any(|partial| {
            matches!(
                partial.name.as_str(),
                "LENGTH_UNIT"
                    | "NAMED_UNIT"
                    | "SI_UNIT"
                    | "GEOMETRIC_REPRESENTATION_CONTEXT"
                    | "GLOBAL_UNIT_ASSIGNED_CONTEXT"
                    | "REPRESENTATION_CONTEXT"
            )
        }) || record.simple_name() == Some("SHAPE_REPRESENTATION")
        {
            typed.insert(id);
        }
    }
    GeometryResult {
        typed_records: typed,
        warnings,
    }
}

fn length_scale(exchange: &Exchange) -> Option<f64> {
    exchange.records.values().find_map(|record| {
        let unit = record.partial("SI_UNIT")?;
        if unit.parameters.get(1)?.enumeration()? != "METRE" {
            return None;
        }
        let prefix = match unit.parameters.first()? {
            Value::Omitted => 1.0,
            Value::Enumeration(prefix) => match prefix.as_str() {
                "EXA" => 1e18,
                "PETA" => 1e15,
                "TERA" => 1e12,
                "GIGA" => 1e9,
                "MEGA" => 1e6,
                "KILO" => 1e3,
                "HECTO" => 1e2,
                "DECA" => 1e1,
                "DECI" => 1e-1,
                "CENTI" => 1e-2,
                "MILLI" => 1e-3,
                "MICRO" => 1e-6,
                "NANO" => 1e-9,
                "PICO" => 1e-12,
                "FEMTO" => 1e-15,
                "ATTO" => 1e-18,
                _ => return None,
            },
            _ => return None,
        };
        Some(prefix * 1000.0)
    })
}

fn coordinates(record: &RawRecord, index: usize, scale: f64) -> Option<Point3> {
    let values = record.parameter(index)?.list()?;
    if values.len() != 3 {
        return None;
    }
    Some(Point3::new(
        values[0].number()? * scale,
        values[1].number()? * scale,
        values[2].number()? * scale,
    ))
}

fn vector3(value: Option<&Value>, scale: f64) -> Option<Vector3> {
    let values = value?.list()?;
    if values.len() != 3 {
        return None;
    }
    Some(Vector3::new(
        values[0].number()? * scale,
        values[1].number()? * scale,
        values[2].number()? * scale,
    ))
}

fn positive(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::number)
        .filter(|value| value.is_finite() && *value > 0.0)
}

fn nurbs_curve(record: &RawRecord, points: &BTreeMap<u64, Point3>) -> Option<NurbsCurve> {
    let complex = record.partials.len() > 1;
    let base = if complex {
        record.partial("B_SPLINE_CURVE")?
    } else {
        record.partial("B_SPLINE_CURVE_WITH_KNOTS")?
    };
    let offset = usize::from(!complex);
    let degree = u32::try_from(base.parameters.get(offset)?.integer()?).ok()?;
    let control_points = references(base.parameters.get(offset + 1)?)?
        .into_iter()
        .map(|id| points.get(&id).copied())
        .collect::<Option<Vec<_>>>()?;
    let periodic = base.parameters.get(offset + 3)?.logical()?;
    let knot_leaf = record.partial("B_SPLINE_CURVE_WITH_KNOTS")?;
    let tail = knot_leaf.parameters.len().checked_sub(3)?;
    let knots = expand_knots(
        knot_leaf.parameters.get(tail)?,
        knot_leaf.parameters.get(tail + 1)?,
    )?;
    if knots.len() != control_points.len() + degree as usize + 1 {
        return None;
    }
    let weights = if let Some(leaf) = record.partial("RATIONAL_B_SPLINE_CURVE") {
        let values = numbers(leaf.parameters.first()?)?;
        (values.len() == control_points.len())
            .then_some(values)
            .map(Some)?
    } else {
        None
    };
    Some(NurbsCurve {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    })
}

fn nurbs_surface(record: &RawRecord, points: &BTreeMap<u64, Point3>) -> Option<NurbsSurface> {
    let complex = record.partials.len() > 1;
    let base = if complex {
        record.partial("B_SPLINE_SURFACE")?
    } else {
        record.partial("B_SPLINE_SURFACE_WITH_KNOTS")?
    };
    let offset = usize::from(!complex);
    let u_degree = u32::try_from(base.parameters.get(offset)?.integer()?).ok()?;
    let v_degree = u32::try_from(base.parameters.get(offset + 1)?.integer()?).ok()?;
    let rows = base.parameters.get(offset + 2)?.list()?;
    let u_count = u32::try_from(rows.len()).ok()?;
    let v_count = u32::try_from(rows.first()?.list()?.len()).ok()?;
    if u_count == 0
        || v_count == 0
        || rows.iter().any(|row| {
            row.list()
                .map_or(true, |values| values.len() != v_count as usize)
        })
    {
        return None;
    }
    let control_points = rows
        .iter()
        .flat_map(|row| row.list().unwrap())
        .map(|value| value.reference().and_then(|id| points.get(&id).copied()))
        .collect::<Option<Vec<_>>>()?;
    let u_periodic = base.parameters.get(offset + 4)?.logical()?;
    let v_periodic = base.parameters.get(offset + 5)?.logical()?;
    let knot_leaf = record.partial("B_SPLINE_SURFACE_WITH_KNOTS")?;
    let tail = knot_leaf.parameters.len().checked_sub(5)?;
    let u_knots = expand_knots(&knot_leaf.parameters[tail], &knot_leaf.parameters[tail + 2])?;
    let v_knots = expand_knots(
        &knot_leaf.parameters[tail + 1],
        &knot_leaf.parameters[tail + 3],
    )?;
    if u_knots.len() != u_count as usize + u_degree as usize + 1
        || v_knots.len() != v_count as usize + v_degree as usize + 1
    {
        return None;
    }
    let weights = if let Some(leaf) = record.partial("RATIONAL_B_SPLINE_SURFACE") {
        let rows = leaf.parameters.first()?.list()?;
        let mut values = Vec::new();
        for row in rows {
            values.extend(
                row.list()?
                    .iter()
                    .map(Value::number)
                    .collect::<Option<Vec<_>>>()?,
            );
        }
        (values.len() == control_points.len())
            .then_some(values)
            .map(Some)?
    } else {
        None
    };
    Some(NurbsSurface {
        u_degree,
        v_degree,
        u_knots,
        v_knots,
        u_count,
        v_count,
        control_points,
        weights,
        u_periodic,
        v_periodic,
    })
}

fn expand_knots(multiplicities: &Value, distinct: &Value) -> Option<Vec<f64>> {
    let multiplicities = multiplicities.list()?;
    let distinct = distinct.list()?;
    if multiplicities.len() != distinct.len() {
        return None;
    }
    let mut knots = Vec::new();
    for (multiplicity, knot) in multiplicities.iter().zip(distinct) {
        let count = usize::try_from(multiplicity.integer()?).ok()?;
        let knot = knot.number()?;
        if count == 0 || !knot.is_finite() {
            return None;
        }
        knots.extend(std::iter::repeat_n(knot, count));
    }
    knots
        .windows(2)
        .all(|pair| pair[0] <= pair[1])
        .then_some(knots)
}

fn references(value: &Value) -> Option<Vec<u64>> {
    value.list()?.iter().map(Value::reference).collect()
}

fn numbers(value: &Value) -> Option<Vec<f64>> {
    value.list()?.iter().map(Value::number).collect()
}

fn normalize(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0).then(|| scale_vector(vector, 1.0 / norm))
}

fn scale_vector(vector: Vector3, scale: f64) -> Vector3 {
    Vector3::new(vector.x * scale, vector.y * scale, vector.z * scale)
}

fn optional_direction(
    value: Option<&Value>,
    directions: &BTreeMap<u64, Vector3>,
) -> Option<Vector3> {
    match value? {
        Value::Omitted => None,
        Value::Reference(id) => directions.get(id).copied(),
        _ => None,
    }
}

trait RecordExt {
    fn simple_name(&self) -> Option<&str>;
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord>;
    fn parameter(&self, index: usize) -> Option<&Value>;
}

impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord> {
        self.partials.iter().find(|partial| partial.name == name)
    }
    fn parameter(&self, index: usize) -> Option<&Value> {
        self.partials.first()?.parameters.get(index)
    }
}

trait ValueExt {
    fn number(&self) -> Option<f64>;
    fn reference(&self) -> Option<u64>;
    fn list(&self) -> Option<&[Value]>;
    fn enumeration(&self) -> Option<&str>;
    fn integer(&self) -> Option<i64>;
    fn logical(&self) -> Option<bool>;
}

impl ValueExt for Value {
    fn number(&self) -> Option<f64> {
        match self {
            Value::Real(v) => Some(*v),
            Value::Integer(v) => Some(*v as f64),
            _ => None,
        }
    }
    fn reference(&self) -> Option<u64> {
        match self {
            Value::Reference(id) => Some(*id),
            _ => None,
        }
    }
    fn list(&self) -> Option<&[Value]> {
        match self {
            Value::List(values) => Some(values),
            _ => None,
        }
    }
    fn enumeration(&self) -> Option<&str> {
        match self {
            Value::Enumeration(value) => Some(value),
            _ => None,
        }
    }
    fn integer(&self) -> Option<i64> {
        match self {
            Value::Integer(value) => Some(*value),
            _ => None,
        }
    }
    fn logical(&self) -> Option<bool> {
        match self {
            Value::Enumeration(value) if value == "T" => Some(true),
            Value::Enumeration(value) if value == "F" => Some(false),
            _ => None,
        }
    }
}
