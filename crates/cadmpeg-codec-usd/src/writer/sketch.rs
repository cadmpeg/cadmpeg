// SPDX-License-Identifier: Apache-2.0
//! Writes sketches as `CadSketch` prims (spec §18.2).
//!
//! A sketch is a prim of the `Sketches` scope whose transform places the
//! sketch plane. Each entity becomes a curve or point prim; points closer
//! than the model tolerance are one prim, so a shared corner is one point.
//! Arcs are written counterclockwise.

use std::collections::BTreeMap;

use cadmpeg_ir::features::ParameterValue;
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntity,
    SketchEntityId, SketchGeometryDefinition, SketchLocus,
};
use cadmpeg_ir::transform::Transform;

use super::text::{quoted, real, tuple, Names};
use super::{Placement, Writer};

/// Name of the scope that holds the sketches.
pub(super) const SKETCHES: &str = "Sketches";

/// The prims of one sketch's entities.
#[derive(Default)]
struct Prims {
    /// Point prim names and their positions.
    points: Vec<(String, [f64; 2], bool)>,
    /// Lines in written order: the head and the lines of each curve prim.
    curves: Vec<(String, Vec<String>)>,
    /// The prim of each entity, and of its start, end, and center.
    entity: BTreeMap<SketchEntityId, String>,
    start: BTreeMap<SketchEntityId, String>,
    end: BTreeMap<SketchEntityId, String>,
    center: BTreeMap<SketchEntityId, String>,
    /// The center and radius of each arc and circle, for the extent.
    discs: Vec<([f64; 2], f64)>,
}

impl Prims {
    /// The point prim at `position`: an existing one within `tolerance`, or a
    /// new one.
    fn point(
        &mut self,
        names: &mut Names,
        position: [f64; 2],
        construction: bool,
        tolerance: f64,
    ) -> String {
        if let Some((name, _, _)) = self
            .points
            .iter()
            .find(|(_, at, _)| (at[0] - position[0]).hypot(at[1] - position[1]) <= tolerance)
        {
            return name.clone();
        }
        let name = names.unique("p");
        self.points.push((name.clone(), position, construction));
        name
    }

    fn locus(&self, locus: &SketchLocus) -> Option<&String> {
        match locus {
            SketchLocus::Entity(entity) => self.entity.get(entity),
            SketchLocus::Start(entity) => self.start.get(entity),
            SketchLocus::End(entity) => self.end.get(entity),
            SketchLocus::Center(entity) => self.center.get(entity),
        }
    }
}

/// The order key of an entity: its kind and the coordinates that place it.
struct OrderKey {
    kind: u8,
    values: Vec<f64>,
}

impl OrderKey {
    fn total_cmp_key(&self, other: &Self) -> std::cmp::Ordering {
        self.kind.cmp(&other.kind).then_with(|| {
            self.values
                .iter()
                .zip(&other.values)
                .map(|(a, b)| a.total_cmp(b))
                .find(|ordering| ordering.is_ne())
                .unwrap_or_else(|| self.values.len().cmp(&other.values.len()))
        })
    }
}

fn order_key(entity: &SketchEntity) -> OrderKey {
    let point = |point: &cadmpeg_ir::units::FinitePoint2| [point.get().u, point.get().v];
    let (kind, values) = match entity.geometry.definition() {
        SketchGeometryDefinition::Point { position } => (0, point(position).to_vec()),
        SketchGeometryDefinition::Line { start, end } => (1, [point(start), point(end)].concat()),
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            ..
        } => (
            2,
            [point(center).as_slice(), &[radius.get(), start_angle.get()]].concat(),
        ),
        SketchGeometryDefinition::Circle { center, radius } => {
            (3, [point(center).as_slice(), &[radius.get()]].concat())
        }
        SketchGeometryDefinition::Nurbs { curve } => (
            4,
            curve
                .control_points()
                .iter()
                .flat_map(|pole| [pole.get().u, pole.get().v])
                .collect(),
        ),
        _ => (5, Vec::new()),
    };
    OrderKey { kind, values }
}

/// The distinct knots of a knot vector and their multiplicities.
fn compressed(knots: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let mut distinct: Vec<f64> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    for knot in knots {
        match (distinct.last(), counts.last_mut()) {
            (Some(last), Some(count)) if last == knot => *count += 1,
            _ => {
                distinct.push(*knot);
                counts.push(1);
            }
        }
    }
    (distinct, counts)
}

/// The distinct knots, multiplicities, and poles of a closed spline.
type ClosedForm = (Vec<f64>, Vec<usize>, Vec<[f64; 2]>);

/// The closed-form knots, multiplicities, and poles of an unwrapped
/// periodic spline whose last `degree` poles repeat its first ones: the
/// inverse of the reader's unwrapping.
fn closed_form(degree: usize, knots: &[f64], poles: &[[f64; 2]]) -> Option<ClosedForm> {
    let count = poles.len().checked_sub(degree)?;
    if count == 0 || poles[count..] != poles[..degree] {
        return None;
    }
    let anchor = *knots.get(degree)?;
    let first_count = knots
        .get(..=degree)?
        .iter()
        .rev()
        .take_while(|knot| **knot == anchor)
        .count();
    let start = degree + 1 - first_count;
    let flat = knots.get(start..start + count)?;
    let period = knots.get(degree + count)? - anchor;
    let (mut distinct, mut counts) = compressed(flat);
    distinct.push(anchor + period);
    counts.push(first_count);
    Some((distinct, counts, poles[..count].to_vec()))
}

/// A constraint's type, constrained targets, reference targets, and value.
type ConstraintParts = (&'static str, Vec<String>, Vec<String>, Option<f64>);

impl Writer<'_> {
    /// The value of a design parameter: a length in millimetres, or an angle
    /// in degrees.
    fn parameter_value(&self, id: &cadmpeg_ir::features::ParameterId) -> Option<f64> {
        let parameter = self
            .ir
            .model
            .parameters
            .iter()
            .find(|candidate| &candidate.id == id)?;
        match parameter.value.as_ref()? {
            ParameterValue::Length(length) => Some(length.get() / super::MILLIMETRES_PER_UNIT),
            ParameterValue::Angle(angle) => Some(angle.get().to_degrees()),
            _ => None,
        }
    }

    fn sketch_entity(
        entity: &SketchEntity,
        prims: &mut Prims,
        names: &mut Names,
        tolerance: f64,
    ) -> bool {
        let id = entity.id().clone();
        let construction = entity.construction;
        let unit = super::MILLIMETRES_PER_UNIT;
        let point =
            |point: cadmpeg_ir::units::FinitePoint2| [point.get().u / unit, point.get().v / unit];
        let mut flag = Vec::new();
        let curve_head = |kind: &str, flag: &mut Vec<String>| {
            if construction {
                flag.push(format!("uniform bool sketch:{kind}:construction = 1"));
            }
        };
        match entity.geometry.definition() {
            SketchGeometryDefinition::Point { position } => {
                let name = prims.point(names, point(*position), construction, tolerance);
                prims.entity.insert(id, name);
            }
            SketchGeometryDefinition::Line { start, end } => {
                let start = prims.point(names, point(*start), false, tolerance);
                let end = prims.point(names, point(*end), false, tolerance);
                let name = names.unique("line");
                curve_head("line", &mut flag);
                let mut lines = vec![
                    format!("uniform string cad:id = {}", quoted(id.as_str())),
                    format!("rel sketch:line:startPoint = <{{sketch}}/{start}>"),
                    format!("rel sketch:line:endPoint = <{{sketch}}/{end}>"),
                ];
                lines.extend(flag);
                prims
                    .curves
                    .push((format!("def CadSketchLine {}", quoted(&name)), lines));
                prims.start.insert(id.clone(), start);
                prims.end.insert(id.clone(), end);
                prims.entity.insert(id, name);
            }
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let [u, v] = point(*center);
                let r = radius.get() / unit;
                let at = |angle: f64| [u + r * angle.cos(), v + r * angle.sin()];
                let start = prims.point(names, at(start_angle.get()), false, tolerance);
                let end = prims.point(names, at(end_angle.get()), false, tolerance);
                let middle = prims.point(names, [u, v], false, tolerance);
                let name = names.unique("arc");
                curve_head("arc", &mut flag);
                let mut lines = vec![
                    format!("uniform string cad:id = {}", quoted(id.as_str())),
                    format!("rel sketch:arc:startPoint = <{{sketch}}/{start}>"),
                    format!("rel sketch:arc:endPoint = <{{sketch}}/{end}>"),
                    format!("rel sketch:arc:centerPoint = <{{sketch}}/{middle}>"),
                    format!("uniform double sketch:arc:radius = {}", real(r)),
                    "uniform token sketch:arc:sweep = \"ccw\"".to_owned(),
                ];
                lines.extend(flag);
                prims
                    .curves
                    .push((format!("def CadSketchArc {}", quoted(&name)), lines));
                prims.start.insert(id.clone(), start);
                prims.end.insert(id.clone(), end);
                prims.center.insert(id.clone(), middle);
                prims.discs.push(([u, v], r));
                prims.entity.insert(id, name);
            }
            SketchGeometryDefinition::Circle { center, radius } => {
                prims.discs.push((point(*center), radius.get() / unit));
                let middle = prims.point(names, point(*center), false, tolerance);
                let name = names.unique("circle");
                curve_head("circle", &mut flag);
                let mut lines = vec![
                    format!("uniform string cad:id = {}", quoted(id.as_str())),
                    format!("rel sketch:circle:centerPoint = <{{sketch}}/{middle}>"),
                    format!(
                        "uniform double sketch:circle:radius = {}",
                        real(radius.get() / unit)
                    ),
                ];
                lines.extend(flag);
                prims
                    .curves
                    .push((format!("def CadSketchCircle {}", quoted(&name)), lines));
                prims.center.insert(id.clone(), middle);
                prims.entity.insert(id, name);
            }
            SketchGeometryDefinition::Nurbs { curve } => {
                if curve.weights().is_some() {
                    return false;
                }
                let Ok(degree) = usize::try_from(curve.degree()) else {
                    return false;
                };
                let poles: Vec<[f64; 2]> = curve.control_points().into_iter().map(point).collect();
                let knots = curve.knots().as_slice();
                let (form, distinct, counts, poles) = if curve.periodic() {
                    match closed_form(degree, knots, &poles) {
                        Some((distinct, counts, poles)) => ("closed", distinct, counts, poles),
                        None => return false,
                    }
                } else {
                    let (distinct, counts) = compressed(knots);
                    ("open", distinct, counts, poles)
                };
                let controls: Vec<String> = poles
                    .iter()
                    .map(|pole| prims.point(names, *pole, true, 0.0))
                    .collect();
                let name = names.unique("spline");
                curve_head("spline", &mut flag);
                let mut lines = vec![
                    format!("uniform string cad:id = {}", quoted(id.as_str())),
                    format!("uniform token sketch:spline:form = {}", quoted(form)),
                    format!("uniform int sketch:spline:degree = {degree}"),
                    format!(
                        "uniform double[] sketch:spline:knots = [{}]",
                        distinct
                            .iter()
                            .map(|knot| real(*knot))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    format!(
                        "uniform int[] sketch:spline:multiplicities = [{}]",
                        counts
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    format!(
                        "uniform double2[] sketch:spline:poles = [{}]",
                        poles
                            .iter()
                            .map(|pole| tuple(pole))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    "uniform token sketch:spline:fitRule = \"none\"".to_owned(),
                    format!(
                        "rel sketch:spline:controlPoints = [{}]",
                        controls
                            .iter()
                            .map(|control| format!("<{{sketch}}/{control}>"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ];
                lines.extend(flag);
                prims
                    .curves
                    .push((format!("def CadSketchSpline {}", quoted(&name)), lines));
                if form == "open" {
                    if let (Some(first), Some(last)) = (controls.first(), controls.last()) {
                        prims.start.insert(id.clone(), first.clone());
                        prims.end.insert(id.clone(), last.clone());
                    }
                }
                prims.entity.insert(id, name);
            }
            _ => return false,
        }
        true
    }

    /// The type, constrained targets, reference targets, and value of a
    /// constraint, or `None` when no constraint type states it.
    fn constraint_parts(
        &self,
        constraint: &SketchConstraint,
        prims: &Prims,
    ) -> Option<ConstraintParts> {
        use SketchConstraintDefinitionInput as Input;
        let entity = |id: &SketchEntityId| prims.entity.get(id).cloned();
        let locus = |locus: &SketchLocus| prims.locus(locus).cloned();
        let value = |id| self.parameter_value(id);
        Some(match constraint.definition.kind() {
            Input::CoincidentLoci { loci } => match loci.as_slice() {
                [a, b] => (
                    "CadCoincidentConstraint",
                    vec![locus(a)?, locus(b)?],
                    Vec::new(),
                    None,
                ),
                _ => return None,
            },
            Input::Horizontal { entity: line } => (
                "CadHorizontalConstraint",
                vec![entity(line)?],
                Vec::new(),
                None,
            ),
            Input::Vertical { entity: line } => (
                "CadVerticalConstraint",
                vec![entity(line)?],
                Vec::new(),
                None,
            ),
            Input::SameCoordinate { relation } => (
                match relation.axis() {
                    SketchCoordinateAxis::V => "CadHorizontalConstraint",
                    SketchCoordinateAxis::U => "CadVerticalConstraint",
                },
                vec![locus(relation.first())?, locus(relation.second())?],
                Vec::new(),
                None,
            ),
            Input::Parallel { first, second } => (
                "CadParallelConstraint",
                vec![entity(first)?, entity(second)?],
                Vec::new(),
                None,
            ),
            Input::Perpendicular { first, second } => (
                "CadPerpendicularConstraint",
                vec![entity(first)?, entity(second)?],
                Vec::new(),
                None,
            ),
            Input::Equal { first, second } => (
                "CadEqualConstraint",
                vec![entity(first)?, entity(second)?],
                Vec::new(),
                None,
            ),
            Input::Tangent { first, second } => (
                "CadTangentConstraint",
                vec![entity(first)?, entity(second)?],
                Vec::new(),
                None,
            ),
            Input::Fixed { entity: target } => (
                "CadFixedConstraint",
                vec![entity(target)?],
                Vec::new(),
                None,
            ),
            Input::PointCoordinateValues { point, .. } => {
                ("CadFixedConstraint", vec![locus(point)?], Vec::new(), None)
            }
            Input::PointOnObject {
                point,
                entity: curve,
            } => (
                "CadPointOnCurveConstraint",
                vec![locus(point)?],
                vec![entity(curve)?],
                None,
            ),
            Input::Symmetric {
                first,
                second,
                axis,
            } => (
                "CadSymmetricConstraint",
                vec![locus(first)?, locus(second)?],
                vec![entity(axis)?],
                None,
            ),
            Input::PointSymmetric {
                first,
                second,
                center,
            } => (
                "CadSymmetricConstraint",
                vec![locus(first)?, locus(second)?],
                vec![locus(center)?],
                None,
            ),
            Input::Radius {
                entity: curve,
                parameter,
            } => (
                "CadRadiusConstraint",
                vec![entity(curve)?],
                Vec::new(),
                Some(value(parameter)?),
            ),
            Input::Diameter {
                entity: curve,
                parameter,
            } => (
                "CadRadiusConstraint",
                vec![entity(curve)?],
                Vec::new(),
                Some(value(parameter)? / 2.0),
            ),
            Input::DistanceLoci {
                first,
                second,
                parameter,
            } => (
                "CadDistanceConstraint",
                vec![locus(first)?, locus(second)?],
                Vec::new(),
                Some(value(parameter)?),
            ),
            Input::Angle {
                first,
                second,
                parameter,
            } => (
                "CadAngleConstraint",
                vec![entity(second)?],
                vec![entity(first)?],
                Some(value(parameter)?),
            ),
            _ => return None,
        })
    }

    fn sketch(&mut self, sketch: &Sketch, names: &mut Names, parent: Option<&Transform>) {
        let model = &self.ir.model;
        let tolerance = self.ir.tolerances.linear.get() / super::MILLIMETRES_PER_UNIT;
        let mut prims = Prims::default();
        let mut point_names = Names::default();
        // Entities are written in an order of their geometry, so that the
        // prim names do not depend on the arena order.
        let mut entities: Vec<&SketchEntity> = model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
            .collect();
        entities.sort_by(|left, right| order_key(left).total_cmp_key(&order_key(right)));
        for entity in entities {
            if !Self::sketch_entity(entity, &mut prims, &mut point_names, tolerance) {
                self.unwritten.sketch_entities += 1;
            }
        }
        let name = names.unique(sketch.name.as_deref().unwrap_or("Sketch"));
        let frame = sketch
            .placement
            .resolved()
            .and_then(|(origin, normal, u_axis)| {
                let (origin, normal, u_axis) = (origin.get(), normal.get(), u_axis.get());
                let unit = |vector: cadmpeg_ir::math::Vector3| {
                    let length =
                        (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z).sqrt();
                    [vector.x / length, vector.y / length, vector.z / length]
                };
                let (n, u) = (unit(normal), unit(u_axis));
                let v = [
                    n[1] * u[2] - n[2] * u[1],
                    n[2] * u[0] - n[0] * u[2],
                    n[0] * u[1] - n[1] * u[0],
                ];
                Transform::affine([
                    [u[0], v[0], n[0], origin.x],
                    [u[1], v[1], n[1], origin.y],
                    [u[2], v[2], n[2], origin.z],
                ])
            });
        let frame = match (frame, parent) {
            (Some(frame), Some(parent)) => parent
                .try_inverse_affine()
                .ok()
                .and_then(|inverse| inverse.compose(frame).ok()),
            (frame, _) => frame,
        };
        self.prim_head("def CadSketch", &name, &[]);
        self.out.open();
        let path = self.out.path();
        self.out.line(&format!(
            "uniform string cad:id = {}",
            quoted(sketch.id.as_str())
        ));
        self.placement(&Placement {
            transform: frame.as_ref(),
            ..Placement::default()
        });
        let mut low = [f64::INFINITY; 2];
        let mut high = [f64::NEG_INFINITY; 2];
        for (_, position, _) in &prims.points {
            for axis in 0..2 {
                low[axis] = low[axis].min(position[axis]);
                high[axis] = high[axis].max(position[axis]);
            }
        }
        for (center, radius) in &prims.discs {
            for axis in 0..2 {
                low[axis] = low[axis].min(center[axis] - radius);
                high[axis] = high[axis].max(center[axis] + radius);
            }
        }
        if low[0].is_finite() {
            self.out.line(&format!(
                "float3[] extent = [{}, {}]",
                tuple(&[low[0], low[1], 0.0]),
                tuple(&[high[0], high[1], 0.0])
            ));
        }
        for (name, position, construction) in &prims.points {
            self.prim_head("def CadSketchPoint", name, &[]);
            self.out.open();
            if let Some((id, _)) = prims.entity.iter().find(|(_, prim)| *prim == name) {
                self.out
                    .line(&format!("uniform string cad:id = {}", quoted(id.as_str())));
            }
            self.out.line(&format!(
                "uniform double2 sketch:point:position = {}",
                tuple(position)
            ));
            if *construction {
                self.out.line("uniform bool sketch:point:construction = 1");
            }
            self.out.close();
        }
        for (head, lines) in &prims.curves {
            self.out.line(head);
            self.out.open();
            for line in lines {
                self.out.line(&line.replace("{sketch}", &path));
            }
            self.out.close();
        }
        let mut constraint_names = Names::default();
        for constraint in model
            .sketch_constraints
            .iter()
            .filter(|constraint| constraint.sketch == sketch.id)
        {
            let Some((kind, constrained, reference, value)) =
                self.constraint_parts(constraint, &prims)
            else {
                self.unwritten.sketch_constraints += 1;
                continue;
            };
            let name = constraint_names.unique(constraint.name.as_deref().unwrap_or("constraint"));
            self.prim_head(&format!("def {kind}"), &name, &[]);
            self.out.open();
            self.out.line(&format!(
                "uniform string cad:id = {}",
                quoted(constraint.id.as_str())
            ));
            let targets = |targets: &[String]| {
                targets
                    .iter()
                    .map(|target| format!("<{path}/{target}>"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            self.out.line(&format!(
                "rel sketch:constraint:constrained = [{}]",
                targets(&constrained)
            ));
            if !reference.is_empty() {
                self.out.line(&format!(
                    "rel sketch:constraint:reference = [{}]",
                    targets(&reference)
                ));
            }
            if let Some(value) = value {
                self.out.line(&format!(
                    "uniform double sketch:constraint:value = {}",
                    real(value)
                ));
            }
            if let Some(parameter) =
                dimension_parameter(constraint).and_then(|id| self.parameter_path(id))
            {
                self.out
                    .line(&format!("rel sketch:constraint:parameter = <{parameter}>"));
            }
            if constraint.driving == Some(false) {
                self.out.line("uniform bool sketch:constraint:driving = 0");
            }
            self.out.close();
        }
        self.out.close();
    }

    /// Writes the `Sketches` scope. `parent` is the placement of the prim
    /// that holds the scope.
    pub(super) fn sketches(&mut self, names: &mut Names, parent: Option<&OccurrenceId>) {
        if self.ir.model.sketches.is_empty() {
            return;
        }
        let parent = parent.and_then(|id| {
            self.ir
                .model
                .occurrences
                .iter()
                .find(|occurrence| &occurrence.id == id)
                .map(|occurrence| occurrence.transform)
        });
        names.reserve(SKETCHES);
        self.prim_head("def Scope", SKETCHES, &[]);
        self.out.open();
        let mut sketch_names = Names::default();
        for sketch in &self.ir.model.sketches {
            self.sketch(sketch, &mut sketch_names, parent.as_ref());
        }
        self.out.close();
    }
}

/// The design parameter that a dimension constraint states.
fn dimension_parameter(
    constraint: &SketchConstraint,
) -> Option<&cadmpeg_ir::features::ParameterId> {
    use SketchConstraintDefinitionInput as Input;
    match constraint.definition.kind() {
        Input::Radius { parameter, .. }
        | Input::Diameter { parameter, .. }
        | Input::DistanceLoci { parameter, .. }
        | Input::Angle { parameter, .. } => Some(parameter),
        _ => None,
    }
}
