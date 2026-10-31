// SPDX-License-Identifier: Apache-2.0
//! The sketches of a composed layer (spec §18.2).
//!
//! A sketch's placement is its prim's composed transform. Each curve and
//! each point that no curve names becomes an entity; a point a curve names
//! is that curve's start, end, or center. A clockwise arc is stored
//! counterclockwise from its end to its start. Profiles are the chains of
//! non-construction curves whose ends meet.

use std::collections::BTreeMap;

use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId, ParameterValue};
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length, PositiveLength};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
    SketchCoordinateAxis, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchLocus, SketchPlacement, SketchProfiles,
    SketchSameCoordinate,
};
use cadmpeg_ir::units::FinitePoint2;

use crate::schema::sketch::{ConstraintData, CurveKind};
use crate::schema::xform::Affine;
use crate::stage::Prim;

/// The ends of a profile curve in its stored direction, in millimetres.
struct Ends {
    entity: SketchEntityId,
    start: [f64; 2],
    end: [f64; 2],
}

use crate::ids::sketch::{
    constraint as constraint_id, entity as entity_id, parameter as parameter_id,
    sketch as sketch_id,
};

impl super::Builder<'_, '_, '_, '_> {
    fn sketch_point(&self, position: [f64; 2]) -> Option<FinitePoint2> {
        FinitePoint2::new(Point2::new(
            position[0] * self.scale,
            position[1] * self.scale,
        ))
    }

    /// Adds the sketch at `prim`, whose composed transform is `world`.
    pub(super) fn sketch(
        &mut self,
        prim: &Prim<'_>,
        world: &Affine,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let data = crate::schema::sketch::read(self.stage, prim, &mut self.issues);
        self.tallies
            .rejected_sketch_elements
            .extend(data.rejected.iter().cloned());
        let id = sketch_id(&prim.path);
        let column = |axis: usize| [world[0][axis], world[1][axis], world[2][axis]];
        let (u_axis, v_axis) = (column(0), column(1));
        // The normal is u × v, so that a reflected placement keeps each
        // sketch point where its transform puts it.
        let normal = Vector3::new(
            u_axis[1] * v_axis[2] - u_axis[2] * v_axis[1],
            u_axis[2] * v_axis[0] - u_axis[0] * v_axis[2],
            u_axis[0] * v_axis[1] - u_axis[1] * v_axis[0],
        );
        let placement = SketchPlacement::try_resolved(
            Point3::new(
                world[0][3] * self.scale,
                world[1][3] * self.scale,
                world[2][3] * self.scale,
            ),
            normal,
            Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
        )
        .unwrap_or(SketchPlacement::Unresolved {});
        let positions: BTreeMap<&str, [f64; 2]> = data
            .points
            .iter()
            .map(|point| (point.path.as_str(), point.position))
            .collect();
        let position = |path: &str| positions.get(path).copied();
        let mut loci: BTreeMap<String, SketchLocus> = BTreeMap::new();
        let mut entities: BTreeMap<String, SketchEntityId> = BTreeMap::new();
        let mut ends: Vec<Ends> = Vec::new();
        let tolerance = self.ir.tolerances.linear.get();
        for curve in &data.curves {
            let entity = entity_id(&curve.path);
            let mut locate = |path: &str, locus: SketchLocus| {
                loci.entry(path.to_owned()).or_insert(locus);
            };
            let geometry = match &curve.kind {
                CurveKind::Line { start, end } => {
                    let (Some(a), Some(b)) = (position(start), position(end)) else {
                        self.tallies
                            .rejected_sketch_elements
                            .push(curve.path.clone());
                        continue;
                    };
                    locate(start, SketchLocus::Start(entity.clone()));
                    locate(end, SketchLocus::End(entity.clone()));
                    if !curve.construction {
                        ends.push(Ends {
                            entity: entity.clone(),
                            start: a.map(|value| value * self.scale),
                            end: b.map(|value| value * self.scale),
                        });
                    }
                    self.sketch_point(a)
                        .zip(self.sketch_point(b))
                        .and_then(|(start, end)| {
                            SketchGeometry::from_parts(SketchGeometryDefinition::Line {
                                start,
                                end,
                            })
                            .ok()
                        })
                }
                CurveKind::Arc {
                    start,
                    end,
                    center,
                    radius,
                    clockwise,
                } => {
                    let (Some(a), Some(b), Some(c)) =
                        (position(start), position(end), position(center))
                    else {
                        self.tallies
                            .rejected_sketch_elements
                            .push(curve.path.clone());
                        continue;
                    };
                    let angle = |point: [f64; 2]| (point[1] - c[1]).atan2(point[0] - c[0]);
                    let (first, second, first_point, second_point) = if *clockwise {
                        (angle(b), angle(a), b, a)
                    } else {
                        (angle(a), angle(b), a, b)
                    };
                    let mut end_angle = second;
                    while end_angle <= first {
                        end_angle += std::f64::consts::TAU;
                    }
                    let (first_path, second_path) = if *clockwise {
                        (end, start)
                    } else {
                        (start, end)
                    };
                    locate(first_path, SketchLocus::Start(entity.clone()));
                    locate(second_path, SketchLocus::End(entity.clone()));
                    locate(center, SketchLocus::Center(entity.clone()));
                    if !curve.construction {
                        ends.push(Ends {
                            entity: entity.clone(),
                            start: first_point.map(|value| value * self.scale),
                            end: second_point.map(|value| value * self.scale),
                        });
                    }
                    self.sketch_point(c).and_then(|center| {
                        SketchGeometry::from_parts(SketchGeometryDefinition::Arc {
                            center,
                            radius: PositiveLength::new(radius * self.scale)?,
                            start_angle: Angle::new(first)?,
                            end_angle: Angle::new(end_angle)?,
                        })
                        .ok()
                    })
                }
                CurveKind::Circle { center, radius } => {
                    let Some(c) = position(center) else {
                        self.tallies
                            .rejected_sketch_elements
                            .push(curve.path.clone());
                        continue;
                    };
                    locate(center, SketchLocus::Center(entity.clone()));
                    self.sketch_point(c).and_then(|center| {
                        SketchGeometry::from_parts(SketchGeometryDefinition::Circle {
                            center,
                            radius: PositiveLength::new(radius * self.scale)?,
                        })
                        .ok()
                    })
                }
                CurveKind::Spline {
                    degree,
                    knots,
                    poles,
                    closed,
                    fit_rule,
                    owned,
                } => {
                    for point in owned {
                        locate(point, SketchLocus::Entity(entity.clone()));
                    }
                    if fit_rule.is_some() {
                        self.tallies
                            .sketch_properties
                            .push(format!("{}: sketch:spline:fitRule", curve.path));
                    }
                    let scaled: Vec<Point2> = poles
                        .iter()
                        .map(|pole| Point2::new(pole[0] * self.scale, pole[1] * self.scale))
                        .collect();
                    if let (Some(first), Some(last)) = (scaled.first(), scaled.last()) {
                        if !curve.construction && !*closed {
                            ends.push(Ends {
                                entity: entity.clone(),
                                start: [first.u, first.v],
                                end: [last.u, last.v],
                            });
                        }
                    }
                    PcurveNurbs::from_lanes(*degree, knots.clone(), scaled, None, *closed)
                        .ok()
                        .map(SketchGeometry::nurbs)
                }
            };
            let Some(geometry) = geometry else {
                self.tallies
                    .rejected_sketch_elements
                    .push(curve.path.clone());
                continue;
            };
            if curve.role.is_some() {
                self.tallies
                    .sketch_properties
                    .push(format!("{}: sketch:construction:role", curve.path));
            }
            self.ir.model.sketch_entities.push(
                SketchEntity::new(entity.clone(), id.clone(), geometry)
                    .with_construction(curve.construction)
                    .with_native_ref(None),
            );
            entities.insert(curve.path.clone(), entity);
            self.admit("usd_sketch_entity")?;
        }
        for point in &data.points {
            if loci.contains_key(&point.path) {
                continue;
            }
            let entity = entity_id(&point.path);
            let Some(position) = self.sketch_point(point.position) else {
                continue;
            };
            let Ok(geometry) =
                SketchGeometry::from_parts(SketchGeometryDefinition::Point { position })
            else {
                continue;
            };
            self.ir.model.sketch_entities.push(
                SketchEntity::new(entity.clone(), id.clone(), geometry)
                    .with_construction(point.construction)
                    .with_native_ref(None),
            );
            loci.insert(point.path.clone(), SketchLocus::Entity(entity.clone()));
            entities.insert(point.path.clone(), entity);
            self.admit("usd_sketch_entity")?;
        }
        for (ordinal, constraint) in data.constraints.iter().enumerate() {
            self.sketch_constraint(&id, constraint, ordinal, &loci, &entities, &positions)?;
        }
        if data.tolerance.is_some() {
            self.tallies
                .sketch_properties
                .push(format!("{}: sketch:tolerance", prim.path));
        }
        let profiles = SketchProfiles::try_from(chains(ends, tolerance)).unwrap_or_default();
        self.ir.model.sketches.push(Sketch {
            id,
            name: Some(prim.name.clone()),
            configuration: None,
            visible: None,
            placement,
            profiles,
            native_ref: None,
        });
        self.transferred = true;
        self.admit("usd_sketch")
    }

    fn sketch_constraint(
        &mut self,
        sketch: &SketchId,
        constraint: &ConstraintData,
        ordinal: usize,
        loci: &BTreeMap<String, SketchLocus>,
        entities: &BTreeMap<String, SketchEntityId>,
        positions: &BTreeMap<&str, [f64; 2]>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        use SketchConstraintDefinitionInput as Input;
        let locus = |path: &str| {
            loci.get(path)
                .cloned()
                .or_else(|| entities.get(path).cloned().map(SketchLocus::Entity))
        };
        let entity = |path: &str| entities.get(path).cloned();
        let constrained = &constraint.constrained;
        let reference = &constraint.reference;
        let mut parameter = |value: ParameterValue| -> Option<ParameterId> {
            let id = parameter_id(&constraint.path);
            self.ir.model.parameters.push(DesignParameter {
                product_definition: None,
                id: id.clone(),
                owner: None,
                ordinal: u32::try_from(ordinal).ok()?,
                name: constraint
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or("Dimension")
                    .to_owned(),
                expression: constraint
                    .value
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                display: None,
                value: Some(value),
                dependencies: DistinctMembers::default(),
                properties: BTreeMap::new(),
                pmi: None,
                native_ref: None,
            });
            Some(id)
        };
        let scale = self.scale;
        let length = |value: f64| Length::new(value * scale).map(ParameterValue::Length);
        let definition = match (
            constraint.kind.as_str(),
            constrained.as_slice(),
            reference.as_slice(),
        ) {
            ("Coincident", [a, b], []) => locus(a)
                .zip(locus(b))
                .map(|(a, b)| Input::CoincidentLoci { loci: vec![a, b] }),
            ("Horizontal", [line], []) => entity(line).map(|entity| Input::Horizontal { entity }),
            ("Vertical", [line], []) => entity(line).map(|entity| Input::Vertical { entity }),
            // Two points that share the v coordinate, or the u coordinate.
            ("Horizontal" | "Vertical", [a, b], []) => {
                let axis = if constraint.kind == "Horizontal" {
                    SketchCoordinateAxis::V
                } else {
                    SketchCoordinateAxis::U
                };
                locus(a)
                    .zip(locus(b))
                    .and_then(|(first, second)| {
                        SketchSameCoordinate::try_new(first, second, axis).ok()
                    })
                    .map(|relation| Input::SameCoordinate { relation })
            }
            ("Parallel", [a, b], []) => entity(a)
                .zip(entity(b))
                .map(|(first, second)| Input::Parallel { first, second }),
            ("Perpendicular", [a, b], []) => entity(a)
                .zip(entity(b))
                .map(|(first, second)| Input::Perpendicular { first, second }),
            ("Equal", [a, b], []) => entity(a)
                .zip(entity(b))
                .map(|(first, second)| Input::Equal { first, second }),
            ("Tangent", [a, b], []) => entity(a)
                .zip(entity(b))
                .map(|(first, second)| Input::Tangent { first, second }),
            ("Fixed", [target], []) => match (entity(target), locus(target)) {
                (Some(entity), _) => Some(Input::Fixed { entity }),
                // A fixed curve point keeps its coordinates.
                (None, Some(point)) => positions
                    .get(target.as_str())
                    .and_then(|[u, v]| Some([Length::new(u * scale)?, Length::new(v * scale)?]))
                    .map(|values| Input::PointCoordinateValues { point, values }),
                (None, None) => None,
            },
            ("PointOnCurve", [point], [curve]) => locus(point)
                .zip(entity(curve))
                .map(|(point, entity)| Input::PointOnObject { point, entity }),
            ("Symmetric", [a, b], [axis]) => {
                match (locus(a), locus(b), entity(axis), locus(axis)) {
                    (Some(first), Some(second), Some(axis), _)
                        if !loci.contains_key(axis.as_str()) =>
                    {
                        Some(Input::Symmetric {
                            first,
                            second,
                            axis,
                        })
                    }
                    (Some(first), Some(second), _, Some(center)) => Some(Input::PointSymmetric {
                        first,
                        second,
                        center,
                    }),
                    _ => None,
                }
            }
            ("Radius", [curve], []) => match (entity(curve), constraint.value.and_then(length)) {
                (Some(entity), Some(value)) => {
                    parameter(value).map(|parameter| Input::Radius { entity, parameter })
                }
                _ => None,
            },
            ("Distance", [a, b], []) | ("Distance", [a], [b]) => {
                match (locus(a), locus(b), constraint.value.and_then(length)) {
                    (Some(first), Some(second), Some(value)) => {
                        parameter(value).map(|parameter| Input::DistanceLoci {
                            first,
                            second,
                            parameter,
                        })
                    }
                    _ => None,
                }
            }
            ("Angle", [line], [base]) => {
                let value = constraint
                    .value
                    .and_then(|degrees| Angle::new(degrees.to_radians()))
                    .map(ParameterValue::Angle);
                match (entity(base), entity(line), value) {
                    (Some(first), Some(second), Some(value)) => {
                        parameter(value).map(|parameter| Input::Angle {
                            first,
                            second,
                            parameter,
                        })
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        let definition =
            definition.and_then(|definition| SketchConstraintDefinition::try_from(definition).ok());
        let Some(definition) = definition else {
            self.tallies
                .rejected_sketch_elements
                .push(constraint.path.clone());
            return Ok(());
        };
        self.ir.model.sketch_constraints.push(SketchConstraint {
            id: constraint_id(&constraint.path),
            sketch: sketch.clone(),
            definition,
            name: constraint.path.rsplit('/').next().map(str::to_owned),
            driving: (!constraint.driving).then_some(false),
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        });
        self.admit("usd_sketch_constraint")
    }
}

/// Chains of profile curves whose ends meet within `tolerance`, each in
/// traversal order with its curves' directions.
fn chains(mut remaining: Vec<Ends>, tolerance: f64) -> Vec<Vec<SketchEntityUse>> {
    let meets = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= tolerance;
    let mut out = Vec::new();
    while !remaining.is_empty() {
        let first = remaining.remove(0);
        let (start, mut end) = (first.start, first.end);
        let mut chain = vec![SketchEntityUse {
            entity: first.entity,
            reversed: false,
        }];
        while !meets(start, end) || chain.len() == 1 {
            let Some(position) = remaining
                .iter()
                .position(|candidate| meets(candidate.start, end) || meets(candidate.end, end))
            else {
                break;
            };
            let next = remaining.remove(position);
            let reversed = !meets(next.start, end);
            end = if reversed { next.start } else { next.end };
            chain.push(SketchEntityUse {
                entity: next.entity,
                reversed,
            });
        }
        out.push(chain);
    }
    out
}
