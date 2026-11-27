// SPDX-License-Identifier: Apache-2.0
//! The form in which `FreeCAD` solves a `Distance` joint, by the types of the
//! elements it names (`AssemblyUtils.cpp` `getDistanceType`,
//! `AssemblyObject.cpp` `makeMbdJointDistance`), restated as a §6.7 kind.

use std::collections::HashMap;

use cadmpeg_ir::design::construction::{
    kinds, AxisValue, Construction, ConstructionKind, FrameAxis, FrameValue, PlaneValue,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::ops::shared::{
    Alignment, JointConnector, JointKind, JointMotion, JointOffset,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::{
    CurveGeometry, ProceduralSurface, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::topology::{EdgeCarrier, Face};

use crate::loss::FreecadLossCode;
use crate::native::{LinkTarget, ObjectRecord, PropertyRecord};
use crate::topology_transfer::TopologyOccurrence;

/// The links, body levels, and trimmed surfaces followed to reach a shape.
const DEPTH: usize = 16;

/// A surface type that `getDistanceType` reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum SurfaceType {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    Other,
}

/// A curve type that `getDistanceType` reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum CurveType {
    Line,
    Circle,
    Other,
}

/// A face or edge, with the radius `FreeCAD` adds to a distance: a cylinder,
/// sphere, or circle radius, else zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Element {
    Face { surface: SurfaceType, radius: f64 },
    Edge { curve: CurveType, radius: f64 },
}

/// The shape of an element is in another document or is not decoded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Unresolved;

/// One element of a `Distance` joint.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Side {
    /// The letters of the element name: `Face`, `Edge`, `Vertex`, or other
    /// text.
    pub(super) kind: String,
    /// The face or edge. `None` where the reference names no shape feature,
    /// so that `FreeCAD` reads no surface or curve type.
    pub(super) element: Result<Option<Element>, Unresolved>,
}

/// The joint `FreeCAD` solves, by `getDistanceType` case. `extra` is the sum
/// of the radii the case adds to the distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Form {
    /// `ASMTPlanarJoint`: the second frame's origin at the distance along the
    /// first frame's z axis, the z axes parallel.
    Planar,
    /// `ASMTPointInPlaneJoint`: the second origin at the offset along the
    /// first frame's z axis.
    PointInPlane { extra: f64 },
    /// `ASMTLineInPlaneJoint`: the second z axis in the plane at the offset
    /// along the first frame's z axis.
    LineInPlane { extra: f64 },
    /// `ASMTRevCylJoint`: the z axes parallel at the distance.
    RevCyl { extra: f64 },
    /// `ASMTCylSphJoint`: the second origin at the distance from the first z
    /// axis.
    CylSph { extra: f64 },
    /// `ASMTSphSphJoint`: the origins at the distance.
    SphSph { extra: f64 },
    /// Two vertices: `ASMTSphSphJoint`, or `ASMTSphericalJoint` below
    /// `Precision::Confusion()`.
    PointPoint,
}

/// The form of a joint between `sides`, and whether `FreeCAD` swaps the two
/// connectors first (`swapJCS`) so that the plane, line, or face is first.
pub(super) fn classify(sides: [&Side; 2]) -> Result<(bool, Form), Unresolved> {
    let mut sides = sides;
    let mut swap = false;
    let form = match (sides[0].kind.as_str(), sides[1].kind.as_str()) {
        ("Vertex", "Vertex") => Form::PointPoint,
        ("Edge", "Edge") => edges(&mut sides, &mut swap)?,
        ("Face", "Face") => faces(&mut sides, &mut swap)?,
        ("Vertex", "Face") | ("Face", "Vertex") => {
            order(sides[0].kind == "Face", &mut sides, &mut swap);
            let [first, _] = sides;
            if face(first, SurfaceType::Plane)? {
                Form::PointInPlane { extra: 0.0 }
            } else if face(first, SurfaceType::Cylinder)? {
                Form::CylSph {
                    extra: radius(first),
                }
            } else if face(first, SurfaceType::Sphere)? {
                Form::SphSph {
                    extra: radius(first),
                }
            } else {
                Form::Planar
            }
        }
        ("Edge", "Face") | ("Face", "Edge") => {
            order(sides[0].kind == "Face", &mut sides, &mut swap);
            let [first, second] = sides;
            if edge(second, CurveType::Line)? && face(first, SurfaceType::Plane)? {
                Form::LineInPlane { extra: 0.0 }
            } else {
                Form::Planar
            }
        }
        ("Vertex", "Edge") | ("Edge", "Vertex") => {
            order(sides[0].kind == "Edge", &mut sides, &mut swap);
            if edge(sides[0], CurveType::Line)? {
                Form::CylSph { extra: 0.0 }
            } else {
                Form::PointInPlane { extra: 0.0 }
            }
        }
        _ => Form::Planar,
    };
    Ok((swap, form))
}

/// Swaps `sides` where the first does not hold the element the case puts
/// first.
fn order(first_holds: bool, sides: &mut [&Side; 2], swap: &mut bool) {
    if !first_holds {
        sides.swap(0, 1);
        *swap = true;
    }
}

/// The form of two edges: a line first, else a circle first.
fn edges(sides: &mut [&Side; 2], swap: &mut bool) -> Result<Form, Unresolved> {
    for kind in [CurveType::Line, CurveType::Circle] {
        if !(edge(sides[0], kind)? || edge(sides[1], kind)?) {
            continue;
        }
        order(edge(sides[0], kind)?, sides, swap);
        let [first, second] = *sides;
        return Ok(match kind {
            CurveType::Line if edge(second, CurveType::Line)? => Form::RevCyl { extra: 0.0 },
            CurveType::Line if edge(second, CurveType::Circle)? => Form::RevCyl {
                extra: radius(second),
            },
            CurveType::Circle if edge(second, CurveType::Circle)? => Form::RevCyl {
                extra: radius(first) + radius(second),
            },
            _ => Form::Planar,
        });
    }
    Ok(Form::Planar)
}

/// The form of two faces: the first type of plane, cylinder, cone, torus,
/// and sphere that either face has goes first.
fn faces(sides: &mut [&Side; 2], swap: &mut bool) -> Result<Form, Unresolved> {
    use SurfaceType::{Cone, Cylinder, Plane, Sphere, Torus};
    let blocks: [(SurfaceType, &[SurfaceType]); 5] = [
        (Plane, &[Plane, Cylinder, Sphere, Cone, Torus]),
        (Cylinder, &[Cylinder, Sphere, Cone, Torus]),
        (Cone, &[Cone, Torus, Sphere]),
        (Torus, &[Torus, Sphere]),
        (Sphere, &[Sphere]),
    ];
    for (kind, partners) in blocks {
        if !(face(sides[0], kind)? || face(sides[1], kind)?) {
            continue;
        }
        order(face(sides[0], kind)?, sides, swap);
        let [first, second] = *sides;
        let mut partner = None;
        for candidate in partners {
            if face(second, *candidate)? {
                partner = Some(*candidate);
                break;
            }
        }
        let both = radius(first) + radius(second);
        return Ok(match (kind, partner) {
            (Plane, Some(Cylinder)) => Form::LineInPlane {
                extra: radius(second),
            },
            (Plane, Some(Sphere)) => Form::PointInPlane {
                extra: radius(second),
            },
            (Cylinder, Some(Cylinder | Torus)) => Form::RevCyl { extra: both },
            (Cylinder | Torus, Some(Sphere)) => Form::CylSph { extra: both },
            (Sphere, Some(Sphere)) => Form::SphSph { extra: both },
            _ => Form::Planar,
        });
    }
    Ok(Form::Planar)
}

/// Whether `side` is a face of type `kind` (`isFaceType`).
fn face(side: &Side, kind: SurfaceType) -> Result<bool, Unresolved> {
    Ok(matches!(side.element?, Some(Element::Face { surface, .. }) if surface == kind))
}

/// Whether `side` is an edge of type `kind` (`isEdgeType`).
fn edge(side: &Side, kind: CurveType) -> Result<bool, Unresolved> {
    Ok(matches!(side.element?, Some(Element::Edge { curve, .. }) if curve == kind))
}

/// The radius of an element a type test has resolved.
fn radius(side: &Side) -> f64 {
    match side.element {
        Ok(Some(Element::Face { radius, .. } | Element::Edge { radius, .. })) => radius,
        _ => 0.0,
    }
}

/// The connector variant a restated joint relates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Shape {
    Frame,
    Plane,
    Axis,
    Point,
}

/// A `Distance` joint restated in the form `FreeCAD` solves it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Restatement {
    pub(super) kind: JointKind,
    pub(super) shapes: [Shape; 2],
    /// The axes also stay parallel, a fact the kind does not state.
    pub(super) parallel: bool,
    /// `Nearest` where the solver holds the z axes parallel in either sense.
    pub(super) alignment: Option<Alignment>,
}

/// The joint of form `form` with stored distance `distance`. The solver
/// squares a separation from an axis, so a negative one is its magnitude.
/// The code is the loss of a form with no joint: a negative separation of two
/// points, which no placement satisfies, or a distance and radii with no
/// finite sum.
pub(super) fn restatement(form: Form, distance: f64) -> Result<Restatement, FreecadLossCode> {
    let length = |value: f64| {
        Length::new(value)
            .map(Dimension::Literal)
            .ok_or(FreecadLossCode::FeatureJointDistanceUnresolved)
    };
    let restated = |kind, shapes, alignment| Restatement {
        kind,
        shapes,
        parallel: false,
        alignment,
    };
    let distance_kind = |distance| JointKind::Distance {
        distance: Some(distance),
        limits: None,
    };
    Ok(match form {
        Form::Planar => restated(
            JointKind::Motion {
                motion: JointMotion::Planar { axis: FrameAxis::Z },
                axes: None,
                offset: JointOffset {
                    x: None,
                    y: None,
                    z: Some(length(distance)?),
                    angle: None,
                },
                limits: Vec::new(),
                drives: Vec::new(),
            },
            [Shape::Frame, Shape::Frame],
            Some(Alignment::Nearest),
        ),
        Form::PointInPlane { extra } => restated(
            JointKind::Coincident {
                offset: Some(length(distance + extra)?),
            },
            [Shape::Plane, Shape::Point],
            None,
        ),
        Form::LineInPlane { extra } => restated(
            JointKind::Coincident {
                offset: Some(length(distance + extra)?),
            },
            [Shape::Plane, Shape::Axis],
            None,
        ),
        Form::RevCyl { extra } => {
            let value = (distance + extra).abs();
            let separation = length(value)?;
            if value > 0.0 {
                Restatement {
                    kind: distance_kind(separation),
                    shapes: [Shape::Axis, Shape::Axis],
                    parallel: true,
                    alignment: Some(Alignment::Nearest),
                }
            } else {
                restated(
                    JointKind::Concentric,
                    [Shape::Axis, Shape::Axis],
                    Some(Alignment::Nearest),
                )
            }
        }
        Form::CylSph { extra } => restated(
            distance_kind(length((distance + extra).abs())?),
            [Shape::Axis, Shape::Point],
            None,
        ),
        Form::SphSph { extra } => {
            let separation = length(distance + extra)?;
            if distance + extra < 0.0 {
                return Err(FreecadLossCode::FeatureJointDistanceUnsatisfiable);
            }
            restated(
                distance_kind(separation),
                [Shape::Point, Shape::Point],
                None,
            )
        }
        Form::PointPoint if distance < super::CONFUSION => restated(
            JointKind::Coincident { offset: None },
            [Shape::Point, Shape::Point],
            None,
        ),
        Form::PointPoint => restated(
            distance_kind(length(distance)?),
            [Shape::Point, Shape::Point],
            None,
        ),
    })
}

/// Frame connector `connector` as the element `shape` of its frame: the xy
/// plane, the z axis, or the origin. The picks and steps stay.
pub(super) fn connector(connector: JointConnector, shape: Shape) -> JointConnector {
    let JointConnector::Frame(frame) = connector else {
        return connector;
    };
    match shape {
        Shape::Frame => JointConnector::Frame(frame),
        Shape::Plane => JointConnector::Plane(convert(frame, |frame| PlaneValue {
            origin: frame.origin,
            normal: frame.z_axis,
            x_axis: frame.x_axis,
        })),
        Shape::Axis => JointConnector::Axis(convert(frame, |frame| AxisValue {
            origin: frame.origin,
            direction: frame.z_axis,
        })),
        Shape::Point => JointConnector::Point(convert(frame, |frame| frame.origin)),
    }
}

fn convert<K: ConstructionKind>(
    frame: Construction<kinds::Frame>,
    value: impl Fn(FrameValue) -> K::Value,
) -> Construction<K> {
    Construction {
        inputs: frame.inputs,
        after: frame.after,
        solution: frame.solution,
        mode: frame.mode,
        resolved: frame.resolved.map(&value),
        resolved_in: frame
            .resolved_in
            .into_iter()
            .map(|(configuration, frame)| (configuration, value(frame)))
            .collect(),
    }
}

/// The objects, shapes, and decoded topology a joint's references name.
pub(super) struct Shapes<'a> {
    /// The id and runtime type of each object, by name.
    objects: HashMap<&'a str, (&'a str, &'a str)>,
    /// The name and runtime type of each object, by id.
    ids: HashMap<&'a str, (&'a str, &'a str)>,
    /// The `LinkedObject` target of each object, by id.
    linked: HashMap<&'a str, &'a LinkTarget>,
    /// The `Part::PropertyPartShape` `Shape` property of each object, by id.
    shape_properties: HashMap<&'a str, &'a str>,
    topology: HashMap<(&'a str, &'a str, usize), Option<&'a str>>,
    faces: HashMap<&'a str, &'a Face>,
    edges: HashMap<&'a str, &'a EdgeCarrier>,
    surfaces: HashMap<&'a str, &'a Surface>,
    procedural: HashMap<&'a str, &'a ProceduralSurface>,
    curves: HashMap<&'a str, &'a CurveGeometry>,
}

impl<'a> Shapes<'a> {
    pub(super) fn of(
        ir: &'a CadIr,
        objects: &'a [ObjectRecord],
        properties: &'a [PropertyRecord],
        topology: &'a [TopologyOccurrence],
    ) -> Self {
        let mut linked = HashMap::new();
        let mut shape_properties = HashMap::new();
        for property in properties {
            match (property.name.as_str(), property.type_name.as_str()) {
                ("LinkedObject", _) => {
                    if let [Some(target)] = property.links() {
                        linked.entry(property.owner.as_str()).or_insert(target);
                    }
                }
                ("Shape", "Part::PropertyPartShape") => {
                    shape_properties
                        .entry(property.owner.as_str())
                        .or_insert(property.id.as_str());
                }
                _ => {}
            }
        }
        let geometry = &ir.geometry;
        Self {
            objects: objects
                .iter()
                .map(|object| {
                    (
                        object.name().as_str(),
                        (object.id().as_str(), object.type_name.as_str()),
                    )
                })
                .collect(),
            ids: objects
                .iter()
                .map(|object| {
                    (
                        object.id().as_str(),
                        (object.name().as_str(), object.type_name.as_str()),
                    )
                })
                .collect(),
            linked,
            shape_properties,
            topology: crate::design::index_occurrences(topology),
            faces: geometry
                .faces
                .iter()
                .map(|face| (face.id.as_str(), face))
                .collect(),
            edges: geometry
                .edges
                .iter()
                .map(|edge| (edge.id.as_str(), &edge.carrier))
                .collect(),
            surfaces: geometry
                .surfaces
                .iter()
                .map(|surface| (surface.id.as_str(), surface))
                .collect(),
            procedural: geometry
                .procedural_surfaces
                .iter()
                .map(|surface| (surface.id.as_str(), surface))
                .collect(),
            curves: geometry
                .curves
                .iter()
                .map(|curve| (curve.id.as_str(), &curve.geometry))
                .collect(),
        }
    }

    /// The element reference `reference` names: the last segment of its first
    /// sub-name, on the shape `getLinkedObjFromRef` reaches.
    pub(super) fn side(&self, reference: &LinkTarget) -> Side {
        let sub = reference.subelements().first().map(String::as_str);
        let element = sub
            .and_then(|sub| sub.rsplit('.').next())
            .unwrap_or_default();
        let kind = element
            .chars()
            .filter(char::is_ascii_alphabetic)
            .collect::<String>();
        let element = match kind.as_str() {
            "Face" | "Edge" => self
                .shape(reference, sub)
                .and_then(|shape| shape.map(|shape| self.element(shape, element)).transpose()),
            _ => Ok(None),
        };
        Side { kind, element }
    }

    /// The `Shape` property of the object reference `reference` and its first
    /// sub-name `sub` reach (`getObjFromRef`, then `getLinkedObject`).
    /// `None` where that is no shape feature.
    fn shape(
        &self,
        reference: &LinkTarget,
        sub: Option<&str>,
    ) -> Result<Option<&'a str>, Unresolved> {
        if reference.document().is_some() {
            return Err(Unresolved);
        }
        let (Some(object), Some(sub)) = (reference.object(), sub) else {
            return Ok(None);
        };
        let Some((&id, &(name, _))) = self.ids.get_key_value(object) else {
            return Ok(None);
        };
        let names = std::iter::once(name)
            .chain(sub.split('.'))
            .collect::<Vec<_>>();
        let selected = if names.len() <= 2 {
            Some(id)
        } else {
            self.walk(&names)?
        };
        let Some(selected) = selected else {
            return Ok(None);
        };
        let Some(target) = self.follow(selected)? else {
            return Ok(None);
        };
        Ok(self.shape_properties.get(target).copied())
    }

    /// The object `getObjFromRef` selects along `names`.
    fn walk(&self, names: &[&str]) -> Result<Option<&'a str>, Unresolved> {
        for (position, name) in names.iter().enumerate() {
            let Some(&(id, kind)) = self.objects.get(name) else {
                return Ok(None);
            };
            if is_group(kind) {
                continue;
            }
            if position + 2 == names.len() {
                return Ok(Some(id));
            }
            let rest = &names[position + 1..];
            if is_part(kind) {
                continue;
            }
            if kind == "PartDesign::Body" {
                return Ok(Some(self.body_child(id, rest)));
            }
            if self.shape_properties.contains_key(id) {
                return Ok(Some(id));
            }
            if is_link(kind) {
                let Some(linked) = self.follow(id)? else {
                    continue;
                };
                if self.ids.get(linked).map(|(_, kind)| *kind) == Some("PartDesign::Body") {
                    let child = self.body_child(linked, rest);
                    return Ok(Some(if child == linked { id } else { child }));
                }
                if self.shape_properties.contains_key(linked) {
                    return Ok(Some(id));
                }
            }
        }
        Ok(None)
    }

    /// The sketch, datum, or coordinate system of body `body` that the next of
    /// `rest` names, else the body (`handlePartDesignBody`).
    fn body_child(&self, body: &'a str, rest: &[&str]) -> &'a str {
        let Some(&(child, kind)) = rest.first().and_then(|name| self.objects.get(name)) else {
            return body;
        };
        if !is_body_child(kind) {
            return body;
        }
        if is_coordinate_system(kind) {
            if let Some(&(element, kind)) = rest.get(1).and_then(|name| self.objects.get(name)) {
                if matches!(kind, "App::Plane" | "App::Line" | "App::Point") {
                    return element;
                }
            }
        }
        child
    }

    /// The object link `object` reaches through its link chain
    /// (`getLinkedObject(true)`); the object itself where it is no link.
    fn follow(&self, object: &'a str) -> Result<Option<&'a str>, Unresolved> {
        let mut current = object;
        for _ in 0..DEPTH {
            let is_link = self.ids.get(current).is_some_and(|(_, kind)| is_link(kind));
            let Some(target) = self.linked.get(current).filter(|_| is_link) else {
                return Ok(Some(current));
            };
            if target.document().is_some() {
                return Err(Unresolved);
            }
            let Some(next) = target.object() else {
                return Ok(Some(current));
            };
            let Some((&next, _)) = self.ids.get_key_value(next) else {
                return Ok(None);
            };
            current = next;
        }
        Err(Unresolved)
    }

    /// Element `element` (`FaceN` or `EdgeN`) of shape property `shape`.
    fn element(&self, shape: &str, element: &str) -> Result<Element, Unresolved> {
        let split = element
            .find(|character: char| !character.is_ascii_alphabetic())
            .ok_or(Unresolved)?;
        let (kind, index) = element.split_at(split);
        let index = index
            .parse::<usize>()
            .ok()
            .filter(|index| *index > 0)
            .ok_or(Unresolved)?;
        let id = self
            .topology
            .get(&(shape, kind, index))
            .copied()
            .flatten()
            .ok_or(Unresolved)?;
        match kind {
            "Face" => {
                let face = self.faces.get(id).ok_or(Unresolved)?;
                let (surface, radius) = self.surface(face.surface.as_str(), 0)?;
                Ok(Element::Face { surface, radius })
            }
            "Edge" => {
                let (EdgeCarrier::Curve(curve) | EdgeCarrier::Bounded(curve, _)) =
                    self.edges.get(id).ok_or(Unresolved)?
                else {
                    return Err(Unresolved);
                };
                let CurveGeometry::Solved(geometry) =
                    self.curves.get(curve.as_str()).ok_or(Unresolved)?
                else {
                    return Err(Unresolved);
                };
                let (curve, radius) = curve_type(geometry, 0)?;
                Ok(Element::Edge { curve, radius })
            }
            _ => Err(Unresolved),
        }
    }

    /// The type and radius `BRepAdaptor_Surface` reads of surface `id`. A
    /// trimmed surface reads as its support.
    fn surface(&self, id: &str, depth: usize) -> Result<(SurfaceType, f64), Unresolved> {
        if depth >= DEPTH {
            return Err(Unresolved);
        }
        match &self.surfaces.get(id).ok_or(Unresolved)?.geometry {
            SurfaceGeometry::Solved(geometry) => surface_type(geometry, 0),
            SurfaceGeometry::Procedural { construction, .. } => {
                match self
                    .procedural
                    .get(construction.as_str())
                    .ok_or(Unresolved)?
                    .definition()
                {
                    ProceduralSurfaceDefinition::Subset(subset) => {
                        self.surface(subset.support().as_str(), depth + 1)
                    }
                    ProceduralSurfaceDefinition::Extrusion(_)
                    | ProceduralSurfaceDefinition::Revolution(_)
                    | ProceduralSurfaceDefinition::Offset(_) => Ok((SurfaceType::Other, 0.0)),
                    _ => Err(Unresolved),
                }
            }
        }
    }
}

/// The type and radius of solved surface `geometry`.
fn surface_type(
    geometry: &SolvedSurfaceGeometry,
    depth: usize,
) -> Result<(SurfaceType, f64), Unresolved> {
    Ok(match geometry {
        SolvedSurfaceGeometry::Plane(_) => (SurfaceType::Plane, 0.0),
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            (SurfaceType::Cylinder, cylinder.radius().get())
        }
        SolvedSurfaceGeometry::Cone(_) => (SurfaceType::Cone, 0.0),
        SolvedSurfaceGeometry::Sphere(sphere) => (SurfaceType::Sphere, sphere.radius().get().abs()),
        SolvedSurfaceGeometry::Torus(_) => (SurfaceType::Torus, 0.0),
        SolvedSurfaceGeometry::Nurbs(_) => (SurfaceType::Other, 0.0),
        SolvedSurfaceGeometry::Transformed(placed)
            if depth < DEPTH && placed.transform().is_proper_rigid() =>
        {
            return surface_type(placed.basis(), depth + 1);
        }
        _ => return Err(Unresolved),
    })
}

/// The type and radius `BRepAdaptor_Curve` reads of solved curve `geometry`.
fn curve_type(
    geometry: &SolvedCurveGeometry,
    depth: usize,
) -> Result<(CurveType, f64), Unresolved> {
    Ok(match geometry {
        SolvedCurveGeometry::Line(_) => (CurveType::Line, 0.0),
        SolvedCurveGeometry::Circle(circle) => (CurveType::Circle, circle.radius().get()),
        SolvedCurveGeometry::Ellipse(_)
        | SolvedCurveGeometry::Parabola(_)
        | SolvedCurveGeometry::Hyperbola(_)
        | SolvedCurveGeometry::Nurbs(_) => (CurveType::Other, 0.0),
        SolvedCurveGeometry::Transformed(placed)
            if depth < DEPTH && placed.transform().is_proper_rigid() =>
        {
            return curve_type(placed.basis(), depth + 1);
        }
        _ => return Err(Unresolved),
    })
}

/// `App::DocumentObjectGroup` and its derived types.
fn is_group(kind: &str) -> bool {
    matches!(
        kind,
        "App::DocumentObjectGroup"
            | "App::DocumentObjectGroupPython"
            | "Assembly::JointGroup"
            | "Assembly::ViewGroup"
            | "Assembly::BomGroup"
            | "Assembly::SimulationGroup"
            | "Assembly::SnapshotGroup"
    )
}

/// `App::Part` and its derived types, and a link group.
fn is_part(kind: &str) -> bool {
    matches!(
        kind,
        "App::Part" | "Assembly::AssemblyObject" | "Assembly::AssemblyLink" | "App::LinkGroup"
    )
}

fn is_link(kind: &str) -> bool {
    matches!(kind, "App::Link" | "App::LinkPython" | "App::LinkElement")
}

fn is_coordinate_system(kind: &str) -> bool {
    matches!(
        kind,
        "App::LocalCoordinateSystem" | "App::Origin" | "PartDesign::CoordinateSystem"
    )
}

/// A sketch, datum, datum element, or coordinate system of a body.
fn is_body_child(kind: &str) -> bool {
    is_coordinate_system(kind)
        || matches!(
            kind,
            "Sketcher::SketchObject"
                | "Sketcher::SketchObjectPython"
                | "PartDesign::Plane"
                | "PartDesign::Line"
                | "PartDesign::Point"
                | "App::Plane"
                | "App::Line"
                | "App::Point"
        )
}

#[cfg(test)]
mod tests;
