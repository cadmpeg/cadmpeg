// SPDX-License-Identifier: Apache-2.0
//! The Alibre feature history of the `STEP_DATA` stream.
//!
//! The history lists one feature for each `DESIGNSTATE` of the part state
//! context, in state order. Lengths are millimetres and angles are radians,
//! converted from the centimetre model unit of the stream.

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::AlibreLossCode;
use crate::part21::{Instance, Part21, Value};

/// Millimetres per model unit of the stream.
const CM_TO_MM: f64 = 10.0;

fn bad(what: &str) -> CodecError {
    CodecError::malformed(format_args!("STEP_DATA feature model: {what}"))
}

pub(crate) type Vec2 = [f64; 2];
pub(crate) type Vec3 = [f64; 3];

/// A right-handed frame: `y` is `z` crossed with `x`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    pub(crate) origin: Vec3,
    pub(crate) x: Vec3,
    pub(crate) y: Vec3,
    pub(crate) z: Vec3,
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The unit of a quantity a parameter holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParameterKind {
    Length,
    Angle,
}

/// A `DOUBLEPARAMETER` or `ANGLEPARAMETER` record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Parameter {
    pub(crate) record: u32,
    pub(crate) name: String,
    pub(crate) guid: Option<String>,
    pub(crate) kind: ParameterKind,
    /// Millimetres or radians.
    pub(crate) value: f64,
}

/// The point of a sketch that a line or circle of the sketch owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointRole {
    Start,
    End,
    Center,
}

/// The curve of a sketch primitive, in millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Curve {
    Point(Vec2),
    Line { start: Vec2, end: Vec2 },
    Circle { center: Vec2, radius: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Primitive {
    pub(crate) record: u32,
    pub(crate) curve: Curve,
}

/// A `REFERENCEPOINTUV` record that an owner primitive bounds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SketchPoint {
    pub(crate) record: u32,
    pub(crate) position: Vec2,
    pub(crate) owner: Option<(u32, PointRole)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchAxis {
    X,
    Y,
}

/// A decoded `CONSTRAINTPAIRUV` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Constraint {
    /// Two sketch points coincide.
    Coincident { record: u32, a: u32, b: u32 },
    /// A primitive line is parallel to a reference axis of the sketch.
    Parallel {
        record: u32,
        line: u32,
        axis: SketchAxis,
    },
}

/// What a sketch lies on.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Support {
    /// The plane datum of the feature at this index.
    Plane(usize),
    /// A face the topology signature names.
    Face(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Sketch {
    pub(crate) record: u32,
    pub(crate) name: String,
    pub(crate) frame: Frame,
    pub(crate) support: Support,
    pub(crate) primitives: Vec<Primitive>,
    pub(crate) points: Vec<SketchPoint>,
    pub(crate) constraints: Vec<Constraint>,
}

/// How a hole or an extrusion ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum End {
    /// A blind depth, the parameter at this index.
    Blind(usize),
    ThroughAll,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Operation {
    Origin(Vec3),
    Axis {
        origin: Vec3,
        direction: Vec3,
    },
    Plane {
        frame: Frame,
    },
    OffsetPlane {
        base: usize,
        offset: usize,
        frame: Frame,
    },
    Sketch(Box<Sketch>),
    Extrude {
        sketch: usize,
        end: End,
        reversed: bool,
    },
    Revolve {
        sketch: usize,
        axis_origin: Vec3,
        axis_direction: Vec3,
        axis_feature: Option<usize>,
        angle: usize,
    },
    Fillet {
        radius: usize,
        /// The topology signature text of each edge, in list order.
        edge_signatures: Vec<String>,
    },
    Hole {
        sketch: usize,
        diameter: usize,
        end: End,
    },
    Loft {
        sections: Vec<usize>,
    },
    /// An operation record the history does not map.
    Unmapped(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Feature {
    pub(crate) record: u32,
    pub(crate) name: String,
    pub(crate) operation: Operation,
}

#[derive(Debug, Default)]
pub(crate) struct History {
    pub(crate) parameters: Vec<Parameter>,
    pub(crate) features: Vec<Feature>,
    pub(crate) losses: Vec<LossNote>,
}

struct Reader<'a> {
    part: &'a Part21,
    sketch_at: BTreeMap<u32, usize>,
    plane_at: BTreeMap<u32, usize>,
    axis_at: BTreeMap<u32, usize>,
    history: History,
}

fn point(part: &Part21, id: u32, dims: usize) -> Result<Vec<f64>, CodecError> {
    let record = part
        .of(id, "CARTESIAN_POINT")
        .ok_or_else(|| bad("a point reference does not name a CARTESIAN_POINT"))?;
    reals(record.arg(1), dims)
}

fn reals(value: Option<&Value>, dims: usize) -> Result<Vec<f64>, CodecError> {
    let list = value
        .and_then(Value::as_list)
        .ok_or_else(|| bad("a coordinate list is missing"))?;
    let out: Option<Vec<f64>> = list.iter().map(Value::as_real).collect();
    let out = out.ok_or_else(|| bad("a coordinate is not a number"))?;
    if out.len() == dims {
        Ok(out)
    } else {
        Err(bad("a coordinate list has the wrong length"))
    }
}

fn mm2(values: &[f64]) -> Vec2 {
    [values[0] * CM_TO_MM, values[1] * CM_TO_MM]
}

fn mm3(values: &[f64]) -> Vec3 {
    [
        values[0] * CM_TO_MM,
        values[1] * CM_TO_MM,
        values[2] * CM_TO_MM,
    ]
}

fn direction3(part: &Part21, id: u32) -> Result<Vec3, CodecError> {
    let record = part
        .of(id, "DIRECTION")
        .ok_or_else(|| bad("a direction reference does not name a DIRECTION"))?;
    let v = reals(record.arg(1), 3)?;
    Ok([v[0], v[1], v[2]])
}

fn direction2(part: &Part21, id: u32) -> Result<Vec2, CodecError> {
    let record = part
        .of(id, "DIRECTION")
        .ok_or_else(|| bad("a direction reference does not name a DIRECTION"))?;
    let v = reals(record.arg(1), 2)?;
    Ok([v[0], v[1]])
}

fn reference(record: &Instance, index: usize, what: &str) -> Result<u32, CodecError> {
    record
        .arg(index)
        .and_then(Value::as_ref)
        .ok_or_else(|| bad(what))
}

fn frame(part: &Part21, plane: u32) -> Result<Frame, CodecError> {
    let plane = part
        .of(plane, "PLANE")
        .ok_or_else(|| bad("a plane reference does not name a PLANE"))?;
    let placement = part
        .of(
            reference(plane, 1, "a PLANE has no placement")?,
            "AXIS2_PLACEMENT_3D",
        )
        .ok_or_else(|| bad("a PLANE placement is not an AXIS2_PLACEMENT_3D"))?;
    let origin = mm3(&point(
        part,
        reference(placement, 1, "a placement has no location")?,
        3,
    )?);
    let z = direction3(part, reference(placement, 2, "a placement has no axis")?)?;
    let x = direction3(
        part,
        reference(placement, 3, "a placement has no reference direction")?,
    )?;
    Ok(Frame {
        origin,
        x,
        y: cross(z, x),
        z,
    })
}

impl<'a> Reader<'a> {
    fn get(&self, id: u32, name: &str, what: &str) -> Result<&'a Instance, CodecError> {
        self.part.of(id, name).ok_or_else(|| bad(what))
    }

    fn parameter(&mut self, record: u32) -> Result<usize, CodecError> {
        if let Some(at) = self
            .history
            .parameters
            .iter()
            .position(|parameter| parameter.record == record)
        {
            return Ok(at);
        }
        let instance = self
            .part
            .get(record)
            .ok_or_else(|| bad("a parameter reference names no record"))?;
        let name = instance
            .arg(0)
            .and_then(Value::as_str)
            .ok_or_else(|| bad("a parameter has no name"))?
            .to_owned();
        let guid = instance.arg(2).and_then(Value::as_str).map(str::to_owned);
        let (kind, value) = match instance.name.as_str() {
            "DOUBLEPARAMETER" => (
                ParameterKind::Length,
                instance
                    .arg(11)
                    .and_then(Value::as_real)
                    .ok_or_else(|| bad("a DOUBLEPARAMETER has no value"))?
                    * CM_TO_MM,
            ),
            "ANGLEPARAMETER" => {
                let angle = reference(instance, 11, "an ANGLEPARAMETER has no angle")?;
                let angle = self.get(angle, "ANGLEUV", "an angle value is not an ANGLEUV")?;
                (
                    ParameterKind::Angle,
                    angle
                        .arg(0)
                        .and_then(Value::as_real)
                        .ok_or_else(|| bad("an ANGLEUV has no value"))?,
                )
            }
            _ => return Err(bad("a parameter reference names another record")),
        };
        self.history.parameters.push(Parameter {
            record,
            name,
            guid,
            kind,
            value,
        });
        Ok(self.history.parameters.len() - 1)
    }

    fn end(&mut self, record: u32) -> Result<End, CodecError> {
        let end = self
            .part
            .get(record)
            .ok_or_else(|| bad("an end condition names no record"))?;
        match end.name.as_str() {
            "BLINDENDCONDITION" => {
                let depth = reference(end, 3, "a BLINDENDCONDITION has no depth")?;
                Ok(End::Blind(self.parameter(depth)?))
            }
            "THROUGHALLENDCONDITION" => Ok(End::ThroughAll),
            _ => Err(bad("an end condition is not blind or through-all")),
        }
    }

    fn unmapped(&mut self, name: &str, state: &str) -> Operation {
        self.history
            .losses
            .push(AlibreLossCode::HistoryOperationUnmapped.note(format!(
                "state {state} holds the operation record {name}, which the history does not map"
            )));
        Operation::Unmapped(name.to_owned())
    }

    fn sketch(&mut self, sketch: &Instance) -> Result<Sketch, CodecError> {
        let name = sketch
            .arg(0)
            .and_then(Value::as_str)
            .ok_or_else(|| bad("a DESIGNSKETCH has no name"))?
            .to_owned();
        let frame = frame(
            self.part,
            reference(sketch, 3, "a DESIGNSKETCH has no plane")?,
        )?;
        let proxy = self.get(
            reference(sketch, 1, "a DESIGNSKETCH has no target")?,
            "OPERATIONTARGETPROXY",
            "a sketch target is not an OPERATIONTARGETPROXY",
        )?;
        let target = reference(proxy, 1, "a target proxy has no target")?;
        let support = match self.part.get(target) {
            Some(record)
                if matches!(
                    record.name.as_str(),
                    "DESIGNPRIMARYPLANE" | "DESIGNASSOCIATEDPLANE"
                ) =>
            {
                Support::Plane(
                    *self
                        .plane_at
                        .get(&target)
                        .ok_or_else(|| bad("a sketch lies on a plane that no state defines"))?,
                )
            }
            Some(record) if record.name == "TOPOLOGYSIGNATURE" => Support::Face(
                record
                    .arg(0)
                    .and_then(Value::as_str)
                    .ok_or_else(|| bad("a TOPOLOGYSIGNATURE has no text"))?
                    .to_owned(),
            ),
            _ => return Err(bad("a sketch target is neither a plane nor a face")),
        };
        let block = self.get(
            reference(sketch, 2, "a DESIGNSKETCH has no block")?,
            "BLOCKDEFINITION",
            "a sketch block is not a BLOCKDEFINITION",
        )?;
        let axes = [
            (reference(block, 9, "a block has no x axis")?, SketchAxis::X),
            (
                reference(block, 10, "a block has no y axis")?,
                SketchAxis::Y,
            ),
        ];
        let mut out = Sketch {
            record: sketch.id,
            name,
            frame,
            support,
            primitives: Vec::new(),
            points: Vec::new(),
            constraints: Vec::new(),
        };
        let mut owner_of = BTreeMap::new();
        for primitive in block
            .arg(1)
            .and_then(Value::refs)
            .ok_or_else(|| bad("a block has no primitives"))?
        {
            let primitive = self.get(
                primitive,
                "BLOCKPRIMITIVE",
                "a block member is not a BLOCKPRIMITIVE",
            )?;
            for member in primitive.arg(0).and_then(Value::refs).unwrap_or_default() {
                owner_of.insert(member, primitive.id);
            }
            let geometry = reference(primitive, 1, "a BLOCKPRIMITIVE has no geometry")?;
            self.primitive(&mut out, primitive, geometry)?;
        }
        let context = self.get(
            reference(block, 8, "a block has no constraint context")?,
            "CONSTRAINTCONTEXTUV",
            "a block context is not a CONSTRAINTCONTEXTUV",
        )?;
        for constraint in context.arg(0).and_then(Value::refs).unwrap_or_default() {
            match self.constraint(constraint, &out, &owner_of, &axes)? {
                Read::Mapped(constraint) => out.constraints.push(constraint),
                Read::Implied => {}
                Read::Unmapped => self.history.losses.push(
                    AlibreLossCode::HistoryConstraintUnmapped.note(format!(
                        "sketch {} holds the constraint record #{constraint}, which the history does not map",
                        out.name
                    )),
                ),
            }
        }
        Ok(out)
    }

    fn primitive(
        &mut self,
        sketch: &mut Sketch,
        primitive: &Instance,
        geometry: u32,
    ) -> Result<(), CodecError> {
        let geometry = self
            .part
            .get(geometry)
            .ok_or_else(|| bad("a BLOCKPRIMITIVE names no geometry record"))?;
        let curve = match geometry.name.as_str() {
            "LINE" => {
                let start = mm2(&point(
                    self.part,
                    reference(geometry, 1, "a LINE has no start")?,
                    2,
                )?);
                let vector = self.get(
                    reference(geometry, 2, "a LINE has no vector")?,
                    "VECTOR",
                    "a LINE vector is not a VECTOR",
                )?;
                let direction = direction2(
                    self.part,
                    reference(vector, 1, "a VECTOR has no direction")?,
                )?;
                let length = vector
                    .arg(2)
                    .and_then(Value::as_real)
                    .ok_or_else(|| bad("a VECTOR has no length"))?
                    * CM_TO_MM;
                Curve::Line {
                    start,
                    end: [
                        start[0] + direction[0] * length,
                        start[1] + direction[1] * length,
                    ],
                }
            }
            "CIRCLE" => {
                let placement = self.get(
                    reference(geometry, 1, "a CIRCLE has no placement")?,
                    "AXIS2_PLACEMENT_2D",
                    "a CIRCLE placement is not an AXIS2_PLACEMENT_2D",
                )?;
                let center = mm2(&point(
                    self.part,
                    reference(placement, 1, "a placement has no location")?,
                    2,
                )?);
                let radius = geometry
                    .arg(2)
                    .and_then(Value::as_real)
                    .ok_or_else(|| bad("a CIRCLE has no radius"))?
                    * CM_TO_MM;
                Curve::Circle { center, radius }
            }
            "NODEUV" => Curve::Point(mm2(&point(
                self.part,
                reference(geometry, 1, "a NODEUV has no point")?,
                2,
            )?)),
            other => {
                self.history
                    .losses
                    .push(AlibreLossCode::HistorySketchGeometryUnmapped.note(format!(
                    "sketch {} holds the geometry record {other}, which the history does not map",
                    sketch.name
                )));
                return Ok(());
            }
        };
        sketch.primitives.push(Primitive {
            record: primitive.id,
            curve,
        });
        for member in primitive.arg(0).and_then(Value::refs).unwrap_or_default() {
            let Some(reference) = self.part.of(member, "REFERENCEPOINTUV") else {
                continue;
            };
            let position = mm2(&point(
                self.part,
                self::reference(reference, 2, "a REFERENCEPOINTUV has no position")?,
                2,
            )?);
            let mut owner = None;
            for dependent in reference.arg(3).and_then(Value::refs).unwrap_or_default() {
                let dependent = self.get(
                    dependent,
                    "REFERENCEDEPENDENTUV",
                    "a point dependent is not a REFERENCEDEPENDENTUV",
                )?;
                if self::reference(dependent, 0, "a dependent has no owner")? != geometry.id {
                    continue;
                }
                owner = match (
                    geometry.name.as_str(),
                    dependent.arg(1).and_then(Value::as_int),
                ) {
                    ("LINE", Some(0)) => Some((primitive.id, PointRole::Start)),
                    ("LINE", Some(1)) => Some((primitive.id, PointRole::End)),
                    ("CIRCLE", Some(0)) => Some((primitive.id, PointRole::Center)),
                    _ => None,
                };
            }
            if owner.is_some() {
                sketch.points.push(SketchPoint {
                    record: member,
                    position,
                    owner,
                });
            }
        }
        Ok(())
    }

    fn constraint(
        &self,
        record: u32,
        sketch: &Sketch,
        owner_of: &BTreeMap<u32, u32>,
        axes: &[(u32, SketchAxis); 2],
    ) -> Result<Read, CodecError> {
        let pair = self.get(
            record,
            "CONSTRAINTPAIRUV",
            "a constraint is not a CONSTRAINTPAIRUV",
        )?;
        let members = pair
            .arg(0)
            .and_then(Value::refs)
            .ok_or_else(|| bad("a CONSTRAINTPAIRUV has no members"))?;
        let (Some(kind), [a, b]) = (pair.arg(2).and_then(Value::as_int), members.as_slice()) else {
            return Ok(Read::Unmapped);
        };
        let point = |id: u32| sketch.points.iter().find(|point| point.record == id);
        let axis_of = |id: u32| {
            axes.iter()
                .find(|(axis, _)| *axis == id)
                .map(|(_, axis)| *axis)
        };
        let binds = |p: u32, q: u32| {
            point(p)
                .and_then(|point| point.owner)
                .is_some_and(|(owner, _)| owner_of.get(&q) == Some(&owner))
        };
        Ok(match kind {
            6 if point(*a).is_some() && point(*b).is_some() => {
                Read::Mapped(Constraint::Coincident {
                    record,
                    a: *a,
                    b: *b,
                })
            }
            2 | 6 if binds(*a, *b) || binds(*b, *a) => Read::Implied,
            4 => match (axis_of(*a), axis_of(*b)) {
                (Some(axis), None) => owner_of.get(b).map_or(Read::Unmapped, |line| {
                    Read::Mapped(Constraint::Parallel {
                        record,
                        line: *line,
                        axis,
                    })
                }),
                (None, Some(axis)) => owner_of.get(a).map_or(Read::Unmapped, |line| {
                    Read::Mapped(Constraint::Parallel {
                        record,
                        line: *line,
                        axis,
                    })
                }),
                _ => Read::Unmapped,
            },
            _ => Read::Unmapped,
        })
    }
}

enum Read {
    Mapped(Constraint),
    /// The point roles of the primitives state the constraint.
    Implied,
    Unmapped,
}

/// Reads the feature history, or `None` where the stream names another schema
/// or holds no part state context.
pub(crate) fn read(part: &Part21) -> Result<Option<History>, CodecError> {
    if !part.is_alibre() {
        return Ok(None);
    }
    let mut contexts = part
        .instances
        .values()
        .filter(|instance| instance.name == "DESIGNPARTSTATECONTEXT");
    let Some(context) = contexts.next() else {
        return Ok(None);
    };
    if contexts.next().is_some() {
        return Err(bad("the stream holds more than one part state context"));
    }
    let states = context
        .arg(0)
        .and_then(Value::refs)
        .ok_or_else(|| bad("the part state context lists no states"))?;
    let mut reader = Reader {
        part,
        sketch_at: BTreeMap::new(),
        plane_at: BTreeMap::new(),
        axis_at: BTreeMap::new(),
        history: History::default(),
    };
    let mut operations = Vec::new();
    for (at, state) in states.iter().enumerate() {
        let state = reader.get(*state, "DESIGNSTATE", "a state is not a DESIGNSTATE")?;
        let operation = reader.get_any(reference(state, 2, "a state has no operation")?)?;
        let target = operation.arg(1).and_then(Value::as_ref);
        match (operation.name.as_str(), target) {
            ("DEFINESKETCHOPERATION", Some(sketch)) => {
                reader.sketch_at.insert(sketch, at);
            }
            ("DESIGNPLANEOPERATION", Some(plane)) => {
                reader.plane_at.insert(plane, at);
            }
            ("DESIGNAXISOPERATION", Some(axis)) => {
                reader.axis_at.insert(axis, at);
            }
            _ => {}
        }
        operations.push((state, operation));
    }
    for table in part
        .instances
        .values()
        .filter(|instance| instance.name == "PARAMETERTABLE")
    {
        for parameter in table.arg(0).and_then(Value::refs).unwrap_or_default() {
            reader.parameter(parameter)?;
        }
    }
    for (state, operation) in operations {
        let feature = reader.feature(state, operation)?;
        reader.history.features.push(feature);
    }
    Ok(Some(reader.history))
}

impl<'a> Reader<'a> {
    fn get_any(&self, id: u32) -> Result<&'a Instance, CodecError> {
        self.part
            .get(id)
            .ok_or_else(|| bad("a reference names no record"))
    }

    fn sketch_feature(&self, record: u32) -> Result<usize, CodecError> {
        self.sketch_at
            .get(&record)
            .copied()
            .ok_or_else(|| bad("an operation names a sketch that no state defines"))
    }

    fn feature(&mut self, state: &Instance, operation: &Instance) -> Result<Feature, CodecError> {
        let name = state
            .arg(0)
            .and_then(Value::as_str)
            .ok_or_else(|| bad("a state has no name"))?
            .to_owned();
        let part = self.part;
        let target = operation.arg(1).and_then(Value::as_ref);
        let record = |index: usize, what: &str| reference(operation, index, what);
        let kind = match operation.name.as_str() {
            "DESIGNPOINTOPERATION" => {
                let point_record = self.get(
                    target.ok_or_else(|| bad("a point operation has no point"))?,
                    "DESIGNPRIMARYPOINT",
                    "a point operation names another record",
                )?;
                Operation::Origin(mm3(&point(
                    part,
                    reference(point_record, 2, "a point has no location")?,
                    3,
                )?))
            }
            "DESIGNAXISOPERATION" => {
                let axis = self.get(
                    target.ok_or_else(|| bad("an axis operation has no axis"))?,
                    "DESIGNPRIMARYAXIS",
                    "an axis operation names another record",
                )?;
                let (origin, direction) =
                    self.axis1(reference(axis, 2, "an axis has no placement")?)?;
                Operation::Axis { origin, direction }
            }
            "DESIGNPLANEOPERATION" => {
                let plane_id = target.ok_or_else(|| bad("a plane operation has no plane"))?;
                let plane = self.get_any(plane_id)?;
                let frame = frame(part, reference(plane, 2, "a plane has no placement")?)?;
                match plane.name.as_str() {
                    "DESIGNPRIMARYPLANE" => Operation::Plane { frame },
                    "DESIGNASSOCIATEDPLANE" => {
                        let producer = self.get(
                            reference(plane, 3, "an associated plane has no producer")?,
                            "OFFSETPLANEPRODUCER",
                            "an associated plane producer is not an OFFSETPLANEPRODUCER",
                        )?;
                        let base = self.get(
                            reference(producer, 0, "a producer has no base")?,
                            "OPERATIONTARGETPROXY",
                            "a producer base is not an OPERATIONTARGETPROXY",
                        )?;
                        let base_plane = reference(base, 1, "a proxy has no target")?;
                        let base = *self.plane_at.get(&base_plane).ok_or_else(|| {
                            bad("an offset plane lies on a plane that no state defines")
                        })?;
                        let offset =
                            self.parameter(reference(producer, 1, "a producer has no offset")?)?;
                        Operation::OffsetPlane {
                            base,
                            offset,
                            frame,
                        }
                    }
                    other => self.unmapped(other, &name),
                }
            }
            "DEFINESKETCHOPERATION" => {
                let sketch = self.get(
                    target.ok_or_else(|| bad("a sketch operation has no sketch"))?,
                    "DESIGNSKETCH",
                    "a sketch operation names another record",
                )?;
                let sketch = self.sketch(sketch)?;
                Operation::Sketch(Box::new(sketch))
            }
            "EXTRUDESKETCHOPERATION" => {
                let sketch = self.sketch_feature(record(2, "an extrusion has no sketch")?)?;
                let condition = self.get(
                    record(3, "an extrusion has no end condition")?,
                    "BLINDENDCONDITION",
                    "an extrusion end condition is not a BLINDENDCONDITION",
                )?;
                let depth = reference(condition, 3, "an end condition has no depth")?;
                let reversed = condition
                    .arg(2)
                    .and_then(Value::as_logical)
                    .ok_or_else(|| bad("an end condition has no direction flag"))?;
                self.dimension_annotations(operation, &name);
                Operation::Extrude {
                    sketch,
                    end: End::Blind(self.parameter(depth)?),
                    reversed,
                }
            }
            "REVOLVESKETCHOPERATION" => {
                let sketch = self.sketch_feature(record(2, "a revolution has no sketch")?)?;
                let (axis_origin, axis_direction) =
                    self.axis1(record(7, "a revolution has no axis placement")?)?;
                let proxy = self.get(
                    record(3, "a revolution has no axis target")?,
                    "OPERATIONTARGETPROXY",
                    "a revolution axis target is not an OPERATIONTARGETPROXY",
                )?;
                let axis_feature = self
                    .axis_at
                    .get(&reference(proxy, 1, "a proxy has no target")?)
                    .copied();
                let condition = self.get(
                    record(8, "a revolution has no end condition")?,
                    "BLINDANGLEENDCONDITION",
                    "a revolution end condition is not a BLINDANGLEENDCONDITION",
                )?;
                let angle =
                    self.parameter(reference(condition, 1, "an end condition has no angle")?)?;
                self.dimension_annotations(operation, &name);
                Operation::Revolve {
                    sketch,
                    axis_origin,
                    axis_direction,
                    axis_feature,
                    angle,
                }
            }
            "FILLETOPERATION" => {
                let radius = self.parameter(record(2, "a fillet has no radius")?)?;
                let mut signatures = Vec::new();
                for edge in operation.arg(1).and_then(Value::refs).unwrap_or_default() {
                    let proxy = self.get(
                        edge,
                        "OPERATIONTARGETPROXY",
                        "a fillet edge is not an OPERATIONTARGETPROXY",
                    )?;
                    let target = self.get_any(reference(proxy, 1, "a proxy has no target")?)?;
                    if target.name == "TOPOLOGYSIGNATURE" {
                        signatures.extend(target.arg(0).and_then(Value::as_str).map(str::to_owned));
                    }
                }
                self.history.losses.push(AlibreLossCode::HistoryEdgeSelectionUnresolved.note(
                    format!("state {name} selects its edges by topology signature, which names no edge of the body"),
                ));
                Operation::Fillet {
                    radius,
                    edge_signatures: signatures,
                }
            }
            "HOLEOPERATION" => {
                let sketch = self.sketch_feature(record(2, "a hole has no sketch")?)?;
                let shape = self.get(
                    record(3, "a hole has no shape")?,
                    "SIMPLEHOLE",
                    "a hole shape is not a SIMPLEHOLE",
                )?;
                let diameters = shape
                    .arg(1)
                    .and_then(Value::refs)
                    .ok_or_else(|| bad("a SIMPLEHOLE has no diameter"))?;
                let [diameter] = diameters.as_slice() else {
                    return Err(bad("a SIMPLEHOLE does not hold one diameter"));
                };
                let diameter = self.parameter(*diameter)?;
                let end = self.end(reference(shape, 2, "a SIMPLEHOLE has no end condition")?)?;
                Operation::Hole {
                    sketch,
                    diameter,
                    end,
                }
            }
            "LOFTOPERATION" => {
                let sections = operation
                    .arg(2)
                    .and_then(Value::refs)
                    .ok_or_else(|| bad("a loft has no sections"))?
                    .into_iter()
                    .map(|sketch| self.sketch_feature(sketch))
                    .collect::<Result<Vec<_>, _>>()?;
                self.history.losses.push(AlibreLossCode::HistoryLoftSettingsUnmapped.note(
                    format!("state {name} holds loft settings after the section list, which the history does not map"),
                ));
                Operation::Loft { sections }
            }
            other => self.unmapped(other, &name),
        };
        Ok(Feature {
            record: state.id,
            name,
            operation: kind,
        })
    }

    /// Charges the `DESIGNDIMENSION` list of attribute 0, which the history
    /// does not map.
    fn dimension_annotations(&mut self, operation: &Instance, state: &str) {
        if operation
            .arg(0)
            .and_then(Value::as_list)
            .is_some_and(|dimensions| !dimensions.is_empty())
        {
            self.history
                .losses
                .push(
                    AlibreLossCode::HistoryDimensionAnnotationUnmapped.note(format!(
                    "state {state} holds a dimension annotation, which the history does not map"
                )),
                );
        }
    }

    fn axis1(&self, id: u32) -> Result<(Vec3, Vec3), CodecError> {
        let placement = self.get(
            id,
            "AXIS1_PLACEMENT",
            "an axis placement is not an AXIS1_PLACEMENT",
        )?;
        Ok((
            mm3(&point(
                self.part,
                reference(placement, 1, "a placement has no location")?,
                3,
            )?),
            direction3(
                self.part,
                reference(placement, 2, "a placement has no direction")?,
            )?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::report::Severity;

    fn charged(text: &[u8]) -> Vec<LossNote> {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
        let part = crate::part21::parse(&ctx, text).unwrap();
        let mut reader = Reader {
            part: &part,
            sketch_at: BTreeMap::new(),
            plane_at: BTreeMap::new(),
            axis_at: BTreeMap::new(),
            history: History::default(),
        };
        reader.dimension_annotations(part.get(1).unwrap(), "R1");
        reader.history.losses
    }

    #[test]
    fn a_dimension_list_of_an_operation_is_charged() {
        let notes = charged(b"DATA;#1=REVOLVESKETCHOPERATION((#2));#2=DESIGNDIMENSION();ENDSEC;");
        let [note] = notes.as_slice() else {
            panic!("one loss");
        };
        assert_eq!(
            note.code,
            AlibreLossCode::HistoryDimensionAnnotationUnmapped.kind()
        );
        assert_eq!(note.severity, Severity::Warning);
    }

    #[test]
    fn an_empty_dimension_list_charges_nothing() {
        assert!(charged(b"DATA;#1=REVOLVESKETCHOPERATION(());ENDSEC;").is_empty());
    }
}
