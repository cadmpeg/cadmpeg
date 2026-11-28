// SPDX-License-Identifier: Apache-2.0
//! The constraints of a Part Studio sketch (§6.1).
//!
//! A constraint the reader decoded is a design constraint under its own id.
//! An operand at the start, end, or centre of a curve is a point entity with
//! that role of the curve (§6.1, Points are entities). Its id is the curve's
//! id and `.start`, `.end`, or `.center`, and a start or end point keeps the
//! point id Onshape stores on the curve as its native name. Every dimension
//! binds the parameter the reader built for it.

use std::collections::BTreeMap;

use cadmpeg_core::serde_erase::json;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::ops::sketch::{
    AngleOperands, DefiningRole, EntityDefinition, EntityRole, Operand, Planar, PlanarCurve,
    RadialMeasure, SketchConstraint, SketchEntity, SketchRelation,
};
use cadmpeg_ir::design::selection::PrincipalEntity;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchConstraintId, SketchEntityId, SketchId};
use cadmpeg_ir::units::FinitePoint2;
use serde_json::Value;

/// The design constraints of one sketch, with the point entities they mint.
#[derive(Default)]
pub(super) struct Projected {
    pub(super) relations: Vec<SketchConstraint<Planar>>,
    pub(super) points: Vec<SketchEntity<Planar>>,
    /// The constraints stated `Native`, with the reason.
    pub(super) native: Vec<(SketchConstraintId, String)>,
}

/// The design constraints of the model constraints `constraints` of
/// `sketch`, whose design entities are `entities`.
pub(super) fn project(
    sketch: &SketchId,
    entities: &[SketchEntity<Planar>],
    constraints: &[&cadmpeg_ir::sketches::SketchConstraint],
    ends: &BTreeMap<&str, &[String]>,
) -> Projected {
    let mut points = Points {
        sketch,
        ends,
        curves: entities
            .iter()
            .map(|entity| (entity.id.as_str(), entity))
            .collect(),
        minted: BTreeMap::new(),
    };
    let mut out = Projected::default();
    for constraint in constraints {
        let wire = json::to_value(&constraint.definition).unwrap_or(Value::Null);
        let relation = relation(&wire, &mut points).unwrap_or_else(|reason| {
            out.native.push((constraint.id.clone(), reason.to_owned()));
            SketchRelation::Native {
                kind: NonBlankString::new(format!(
                    "onshape:{}",
                    wire.get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("constraint")
                ))
                .unwrap_or_else(|| cadmpeg_core::nonblank_const!("onshape:constraint")),
                operands: Vec::new(),
                values: Vec::new(),
            }
        });
        out.relations.push(SketchConstraint {
            id: constraint.id.clone(),
            sketch: sketch.clone(),
            relation,
            name: constraint.name.clone(),
            active: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
            native_ref: constraint.native_ref.clone(),
        });
    }
    out.points = points.minted.into_values().collect();
    out
}

struct Points<'e> {
    sketch: &'e SketchId,
    /// The point ids Onshape stores at the start and end of each curve.
    ends: &'e BTreeMap<&'e str, &'e [String]>,
    curves: BTreeMap<&'e str, &'e SketchEntity<Planar>>,
    minted: BTreeMap<String, SketchEntity<Planar>>,
}

impl Points<'_> {
    /// The operand of a model locus: a whole entity, or the point entity at
    /// the start, end, or centre of a curve.
    fn operand(&mut self, locus: &Value) -> Result<Operand<Planar>, &'static str> {
        self.entity(locus).map(Operand::Entity)
    }

    fn entity(&mut self, locus: &Value) -> Result<SketchEntityId, &'static str> {
        let curve = locus
            .get("value")
            .and_then(Value::as_str)
            .ok_or("a constraint locus names no entity")?;
        let role = match locus.get("kind").and_then(Value::as_str) {
            Some("entity") => {
                return self
                    .curves
                    .get(curve)
                    .map(|entity| entity.id.clone())
                    .ok_or("a constraint entity is not stated in the design");
            }
            Some("start") => DefiningRole::Start,
            Some("end") => DefiningRole::End,
            Some("center") => DefiningRole::Center,
            _ => return Err("a constraint locus kind has no design form"),
        };
        let parent = *self
            .curves
            .get(curve)
            .ok_or("a constraint entity is not stated in the design")?;
        let suffix = match role {
            DefiningRole::Start => "start",
            DefiningRole::End => "end",
            _ => "center",
        };
        let key = format!("{curve}.{suffix}");
        if self.curves.contains_key(key.as_str()) {
            return Err("a curve's end point id repeats a sketch entity id");
        }
        if let Some(point) = self.minted.get(&key) {
            return Ok(point.id.clone());
        }
        let position = position(parent, &role).ok_or("the curve has no such point")?;
        let id = SketchEntityId::mint(key.clone()).map_err(|_| "the point id is not an id")?;
        let stored = match (&role, self.ends.get(curve)) {
            (DefiningRole::Start, Some([start, _])) => Some(start),
            (DefiningRole::End, Some([_, end])) => Some(end),
            _ => None,
        };
        let native = stored
            .map(|stored| -> Result<_, &'static str> {
                Ok(cadmpeg_ir::design::names::NativeName {
                    codec: cadmpeg_ir::codec_id!(crate::dialect::registry_ids::FORMAT),
                    value: cadmpeg_ir::design::names::NativeNameValue::Text(
                        NonBlankString::new(stored.as_str()).ok_or("a stored point id is blank")?,
                    ),
                })
            })
            .transpose()?;
        self.minted.insert(
            key,
            SketchEntity {
                id: id.clone(),
                sketch: self.sketch.clone(),
                definition: EntityDefinition::Free(PlanarCurve::Point(position)),
                roles: vec![EntityRole {
                    of: parent.id.clone(),
                    role,
                }],
                construction: parent.construction,
                centerline: None,
                native,
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
                native_ref: None,
            },
        );
        Ok(id)
    }

    fn curve(&self, value: &Value) -> Result<SketchEntityId, &'static str> {
        value
            .as_str()
            .and_then(|curve| self.curves.get(curve))
            .map(|entity| entity.id.clone())
            .ok_or("a constraint entity is not stated in the design")
    }

    fn is_line(&self, id: &SketchEntityId) -> bool {
        self.curves.get(id.as_str()).is_some_and(|entity| {
            matches!(
                entity.definition,
                EntityDefinition::Free(PlanarCurve::Line { .. })
            )
        })
    }
}

/// The point at `role` of a free curve.
fn position(curve: &SketchEntity<Planar>, role: &DefiningRole) -> Option<FinitePoint2> {
    let EntityDefinition::Free(curve) = &curve.definition else {
        return None;
    };
    let on_circle = |center: &FinitePoint2, radius: f64, angle: f64| {
        let (sin, cos) = angle.portable_sin_cos();
        let center = center.get();
        FinitePoint2::new(Point2 {
            u: center.u + radius * cos,
            v: center.v + radius * sin,
        })
    };
    match (curve, role) {
        (PlanarCurve::Line { start, .. }, DefiningRole::Start) => Some(*start),
        (PlanarCurve::Line { end, .. }, DefiningRole::End) => Some(*end),
        (
            PlanarCurve::Arc { center, .. }
            | PlanarCurve::Circle { center, .. }
            | PlanarCurve::Ellipse { center, .. },
            DefiningRole::Center,
        ) => Some(*center),
        (
            PlanarCurve::Arc {
                center,
                radius,
                start,
                ..
            },
            DefiningRole::Start,
        ) => on_circle(center, radius.get(), start.get()),
        (
            PlanarCurve::Arc {
                center,
                radius,
                end,
                ..
            },
            DefiningRole::End,
        ) => on_circle(center, radius.get(), end.get()),
        _ => None,
    }
}

fn parameter(wire: &Value) -> Result<ParameterId, &'static str> {
    wire.get("parameter")
        .and_then(Value::as_str)
        .and_then(|id| ParameterId::mint(id).ok())
        .ok_or("the dimension names no parameter")
}

fn pair(wire: &Value, first: &str, second: &str) -> Result<[Value; 2], &'static str> {
    Ok([
        wire.get(first)
            .cloned()
            .ok_or("a constraint operand is missing")?,
        wire.get(second)
            .cloned()
            .ok_or("a constraint operand is missing")?,
    ])
}

/// The relation of a model constraint definition.
fn relation(wire: &Value, points: &mut Points<'_>) -> Result<SketchRelation<Planar>, &'static str> {
    let kind = wire
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("the constraint states no kind")?;
    let entity = |points: &Points<'_>, key: &str| -> Result<Operand<Planar>, &'static str> {
        points
            .curve(wire.get(key).ok_or("a constraint operand is missing")?)
            .map(Operand::Entity)
    };
    Ok(match kind {
        "fixed" => SketchRelation::Fixed {
            of: entity(points, "entity")?,
        },
        "horizontal" => SketchRelation::Horizontal {
            a: entity(points, "entity")?,
            b: None,
        },
        "vertical" => SketchRelation::Vertical {
            a: entity(points, "entity")?,
            b: None,
        },
        "parallel" | "perpendicular" | "tangent" | "equal" | "concentric" => {
            let (a, b) = (entity(points, "first")?, entity(points, "second")?);
            match kind {
                "parallel" => SketchRelation::Parallel { a, b },
                "perpendicular" => SketchRelation::Perpendicular { a, b },
                "tangent" => SketchRelation::Tangent {
                    a,
                    b,
                    contact: None,
                },
                "equal" => SketchRelation::Equal {
                    members: vec![a, b],
                },
                _ => SketchRelation::Concentric { a, b },
            }
        }
        "coincident_loci" => {
            let [first, second] = wire
                .get("loci")
                .and_then(Value::as_array)
                .and_then(|loci| <[Value; 2]>::try_from(loci.clone()).ok())
                .ok_or("a coincidence names two loci")?;
            SketchRelation::Coincident {
                a: points.operand(&first)?,
                b: points.operand(&second)?,
            }
        }
        "point_coordinate_values" => {
            let values = wire
                .get("values")
                .and_then(Value::as_array)
                .ok_or("the point coordinates are missing")?;
            if values.iter().any(|value| value.as_f64() != Some(0.0)) {
                return Err("the point lies off the sketch origin");
            }
            SketchRelation::Coincident {
                a: points.operand(wire.get("point").ok_or("the point is missing")?)?,
                b: Operand::Frame(PrincipalEntity::Origin),
            }
        }
        "midpoint" => SketchRelation::Midpoint {
            point: points.entity(wire.get("point").ok_or("the midpoint is missing")?)?,
            of: entity(points, "entity")?,
        },
        "distance" => {
            let [curve] = wire
                .get("entities")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .ok_or("the length names no entity")?
            else {
                return Err("the length names more than one entity");
            };
            let id = points.curve(curve)?;
            let value = Dimension::Parameter(parameter(wire)?);
            if points.is_line(&id) {
                let locus = |kind: &str| cadmpeg_core::json!({"kind": kind, "value": id.as_str()});
                SketchRelation::Distance {
                    a: points.operand(&locus("start"))?,
                    b: points.operand(&locus("end"))?,
                    along: None,
                    doubled: None,
                    value,
                }
            } else {
                SketchRelation::Length {
                    of: vec![id],
                    value,
                }
            }
        }
        "distance_loci" => {
            let [first, second] = pair(wire, "first", "second")?;
            SketchRelation::Distance {
                a: points.operand(&first)?,
                b: points.operand(&second)?,
                along: None,
                doubled: None,
                value: Dimension::Parameter(parameter(wire)?),
            }
        }
        "angle" => SketchRelation::Angle {
            between: AngleOperands::Between([entity(points, "first")?, entity(points, "second")?]),
            sector: None,
            value: Dimension::Parameter(parameter(wire)?),
        },
        "radius" | "diameter" => {
            let Operand::Entity(of) = entity(points, "entity")? else {
                return Err("a radial dimension names no entity");
            };
            SketchRelation::Radial {
                of,
                measure: if kind == "radius" {
                    RadialMeasure::Radius
                } else {
                    RadialMeasure::Diameter
                },
                value: Dimension::Parameter(parameter(wire)?),
            }
        }
        _ => return Err("the constraint kind has no design relation"),
    })
}
