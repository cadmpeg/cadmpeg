// SPDX-License-Identifier: Apache-2.0
//! The joints of a `FreeCAD` Assembly (§6.7). Each paired joint is a `Joint`
//! feature whose connectors are frames on the picked elements, and each
//! grounded object is occurrence state. A `JointGroup` is an unordered
//! `Group` of its joints.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::construction::{
    Construction, ConstructionInput, FrameAxis, InputReference, InputRole, PostStep, Relation,
    Subject,
};
use cadmpeg_ir::design::container::{Container, UnorderedKind};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::design::ids::ContainerId;
use cadmpeg_ir::design::names::{CodecName, NativeName, NativeNameValue};
use cadmpeg_ir::design::ops::shared::{
    AxisLimits, Bounds, Coupling, Joint, JointConnector, JointKind, JointMotion, JointOffset,
};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::scope::OccurrenceDesign;
use cadmpeg_ir::design::selection::{
    Anchor, Members, OccurrencePath, PathStep, PickContext, PickTarget, Picked, Picks, Selection,
    StepRef,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{Occurrence, ProductDefinition};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};
use cadmpeg_ir::transform::Transform;

use crate::loss::FreecadLossCode;
use crate::native::frame::FiniteFrame;
use crate::native::joint::{JointBody, JointParameter, JointParameters};
use crate::native::{LinkTarget, ObjectRecord, PropertyRecord};

mod distance;

/// The joint records of a document as design records.
pub(super) struct Joints {
    /// One `Joint` feature for each paired joint.
    pub(super) features: Vec<Feature>,
    /// One unordered `Group` for each `JointGroup` that lists a joint feature.
    pub(super) groups: Vec<Container>,
    /// The joint features no `JointGroup` lists.
    pub(super) loose: BTreeSet<Member>,
    /// The occurrences a grounded joint fixes.
    pub(super) grounded: BTreeMap<OccurrenceId, OccurrenceDesign>,
}

/// The design records of the joint records of `assembly` in the scope of
/// definition `root`. A joint whose operand the scope cannot name is stated by
/// no record, with a loss in `losses`.
pub(super) fn project(
    ir: &CadIr,
    root: &ProductDefinitionId,
    assembly: &super::Assembly<'_>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    losses: &mut Vec<LossNote>,
) -> Joints {
    let products = Products::of(ir, objects, properties);
    let mut shapes = None;
    let mut joints = Joints {
        features: Vec::new(),
        groups: Vec::new(),
        loose: BTreeSet::new(),
        grounded: BTreeMap::new(),
    };
    let mut grouped = BTreeMap::<&str, BTreeSet<Member>>::new();
    for record in assembly.joints {
        let definition = products.definition(record.object()).unwrap_or(root);
        match &record.body {
            JointBody::Grounded { reference, .. } => {
                let occurrence = reference
                    .as_ref()
                    .filter(|reference| reference.document().is_none())
                    .and_then(LinkTarget::object)
                    .and_then(|object| products.resolve(definition, object, "").0.pop());
                match occurrence {
                    Some(occurrence) => {
                        joints.grounded.insert(
                            occurrence,
                            OccurrenceDesign {
                                configuration: None,
                                arrangements: Vec::new(),
                                grounded: Some(true),
                                flexible: None,
                                parameters: BTreeMap::new(),
                            },
                        );
                    }
                    None => losses.push(FreecadLossCode::FeatureJointUnresolved.note(format!(
                        "grounded joint {} fixes an object that the scope holds no \
                             occurrence of; the grounding stays in its native record",
                        record.object()
                    ))),
                }
            }
            JointBody::Pair { kind, connectors } => {
                let owned = |name: &str| {
                    properties
                        .iter()
                        .find(|property| property.owner == record.object() && property.name == name)
                };
                let [first, second] = connectors.as_ref();
                let float_distance = owned("Distance")
                    .is_some_and(|property| property.type_name == "App::PropertyFloat");
                let classified = kind.as_str().eq_ignore_ascii_case("distance").then(|| {
                    let shapes = shapes.get_or_insert_with(|| {
                        distance::Shapes::of(ir, objects, properties, assembly.topology)
                    });
                    let [Some(first), Some(second)] = [first, second]
                        .map(|connector| connector.reference.as_ref().map(|r| shapes.side(r)))
                    else {
                        return Err(distance::Unresolved);
                    };
                    distance::classify([&first, &second])
                });
                // `swapJCS` exchanges the references and placements of the two
                // slots; each slot keeps its offset and detach flag.
                let placed = match classified {
                    Some(Ok((true, _))) => [second, first],
                    _ => [first, second],
                };
                let mut native_options = NativeOptions::new();
                let connectors = [("1", first, placed[0]), ("2", second, placed[1])].map(
                    |(index, slot, placed)| {
                        let reference = placed.reference.as_ref()?;
                        if record.parameters().bool_value(&format!("Detach{index}")) != Some(true) {
                            return products.connector(
                                definition,
                                reference,
                                placed.placement,
                                slot.offset,
                            );
                        }
                        let offset = (slot.offset.transform() != Transform::identity())
                            .then(|| owned(&format!("Offset{index}")))
                            .flatten()
                            .and_then(crate::placement::placement_values);
                        let (connector, options) = products.detached(
                            definition,
                            reference,
                            placed.placement,
                            index,
                            offset,
                        )?;
                        native_options.extend(options);
                        Some(connector)
                    },
                );
                let [Some(first), Some(second)] = connectors else {
                    losses.push(FreecadLossCode::FeatureJointUnresolved.note(format!(
                        "joint {} names no object for a connector; the joint stays in its \
                         native record",
                        record.object()
                    )));
                    continue;
                };
                let Some((joint, charge)) = joint(
                    kind.as_str(),
                    record.parameters(),
                    [first, second],
                    native_options,
                    float_distance,
                    classified.map(|classified| classified.map(|(_, form)| form)),
                ) else {
                    losses.push(FreecadLossCode::FeatureJointUnresolved.note(format!(
                        "joint {} stores a blank joint type; the joint stays in its native \
                         record",
                        record.object()
                    )));
                    continue;
                };
                let Ok(id) = FeatureId::mint(crate::native::model_id(
                    "joint",
                    record.object(),
                    "constraint",
                )) else {
                    losses.push(FreecadLossCode::FeatureJointUnresolved.note(format!(
                        "joint {} has an object name that mints no feature id; the joint \
                         stays in its native record",
                        record.object()
                    )));
                    continue;
                };
                match charge {
                    Some(code @ FreecadLossCode::FeatureJointDistanceUnresolved) => {
                        losses.push(code.note(format!(
                            "distance joint {} names an element whose shape is in another \
                             document or is not decoded; its distance stays the native option \
                             fcstd:Distance",
                            record.object()
                        )));
                    }
                    Some(code) => losses.push(code.note(format!(
                        "distance joint {} states a distance that the solver cannot satisfy \
                         for its elements; it stays the native option fcstd:Distance",
                        record.object()
                    ))),
                    None => {}
                }
                let member = Member::Feature(id.clone());
                match products.group_of(record.object()) {
                    Some(group) => {
                        grouped.entry(group).or_default().insert(member);
                    }
                    None => {
                        joints.loose.insert(member);
                    }
                }
                joints.features.push(Feature {
                    id,
                    name: products
                        .names
                        .get(record.object())
                        .map(|name| (*name).to_owned()),
                    suppressed: record
                        .parameters()
                        .bool_value("Suppressed")
                        .map(Dimension::Literal),
                    definition: FeatureDefinition {
                        operation: FeatureOperation::Joint(Box::new(joint)),
                        assembly: None,
                        refine: None,
                        fuzzy_tolerance: None,
                    },
                    placement: None,
                    component: (definition != root).then(|| definition.clone()),
                    driven_by: None,
                    native_ref: Some(record.object().to_owned()),
                });
            }
        }
    }
    for (group, members) in grouped {
        let Ok(id) = ContainerId::mint(format!(
            "fcstd:design:container#{}",
            crate::native::id_key(group)
        )) else {
            joints.loose.extend(members);
            continue;
        };
        joints.groups.push(Container::Unordered {
            id,
            kind: UnorderedKind::Group,
            members,
        });
    }
    joints
}

/// The joint of family `label` with parameters `parameters` between
/// connectors `connectors`, whose detached connectors keep `native_options`.
/// `float_distance` states that the joint stores `Distance` as a float, the
/// form in which an Angle joint stores its angle there in degrees.
/// `classified` is the form of a `Distance` joint, whose connectors are
/// already in the order `FreeCAD` solves them. Each parameter the joint does not hold is a source setting
/// under its own name, and a native option of a family the decoder does not
/// read. The code is the loss a `Distance` joint charges. `None` where the
/// family is blank.
fn joint(
    label: &str,
    parameters: &JointParameters,
    mut connectors: [JointConnector; 2],
    mut native_options: NativeOptions,
    float_distance: bool,
    classified: Option<Result<distance::Form, distance::Unresolved>>,
) -> Option<(Joint, Option<FreecadLossCode>)> {
    let family = label.to_ascii_lowercase();
    let mut charge = None;
    let old_angle = family == "angle" && float_distance;
    let mut read = Read {
        parameters,
        used: BTreeSet::from(["Suppressed"]),
        degrees: if old_angle {
            &["Angle", "AngleMin", "AngleMax", "Distance"]
        } else {
            &["Angle", "AngleMin", "AngleMax"]
        },
    };
    let motion = |motion, limits| JointKind::Motion {
        motion,
        axes: None,
        offset: JointOffset {
            x: None,
            y: None,
            z: None,
            angle: None,
        },
        limits,
        drives: Vec::new(),
    };
    let coupling = |coupling| JointKind::Coupling {
        coupling,
        joints: None,
    };
    let kind = match family.as_str() {
        "fixed" => motion(JointMotion::Rigid, Vec::new()),
        "revolute" => motion(
            JointMotion::Revolute { axis: FrameAxis::Z },
            read.rotation().into_iter().collect(),
        ),
        "cylindrical" => motion(
            JointMotion::Cylindrical { axis: FrameAxis::Z },
            read.rotation()
                .into_iter()
                .chain(read.translation())
                .collect(),
        ),
        "slider" => motion(
            JointMotion::Slider { axis: FrameAxis::Z },
            read.translation().into_iter().collect(),
        ),
        "ball" => motion(JointMotion::Ball, Vec::new()),
        "distance" => {
            let stored = read.scalar("Distance");
            let restated = classified
                .unwrap_or(Err(distance::Unresolved))
                .map_err(|_| FreecadLossCode::FeatureJointDistanceUnresolved)
                .and_then(|form| {
                    distance::restatement(form, stored.unwrap_or(0.0))
                        .ok_or(FreecadLossCode::FeatureJointDistanceUnsatisfiable)
                });
            match restated {
                Ok(restatement) => {
                    let [first, second] = connectors;
                    let [first_shape, second_shape] = restatement.shapes;
                    connectors = [
                        distance::connector(first, first_shape),
                        distance::connector(second, second_shape),
                    ];
                    if restatement.parallel {
                        native_options.extend(
                            key("parallelAxes")
                                .map(|key| (key, NativeOption::Bool(Dimension::Literal(true)))),
                        );
                    }
                    restatement.kind
                }
                Err(code) => {
                    charge = Some(code);
                    native_options.extend(key("Distance").zip(stored.and_then(Length::new)).map(
                        |(key, length)| (key, NativeOption::Length(Dimension::Literal(length))),
                    ));
                    JointKind::Distance {
                        distance: None,
                        limits: None,
                    }
                }
            }
        }
        "parallel" => JointKind::Parallel,
        "perpendicular" => JointKind::Perpendicular,
        "angle" => {
            let name = if old_angle { "Distance" } else { "Angle" };
            match parameters.scalar_value(name) {
                Some(angle)
                    if (angle.get().to_radians().abs() % std::f64::consts::TAU) < CONFUSION =>
                {
                    JointKind::Parallel
                }
                _ => JointKind::Angle {
                    angle: read.magnitude(name),
                    limits: None,
                },
            }
        }
        "rackpinion" => coupling(Coupling::RackPinion {
            ratio: None,
            pitch_radius: read.length("Distance"),
        }),
        "screw" => coupling(Coupling::Screw {
            pitch: read.length("Distance"),
        }),
        "gears" => coupling(Coupling::Gear {
            ratio: None,
            radii: [read.length("Distance"), read.length("Distance2")],
        }),
        "belt" => coupling(Coupling::Belt {
            ratio: None,
            radii: [read.length("Distance"), read.length("Distance2")],
        }),
        _ => JointKind::Native(NonBlankString::new(label.to_owned())?),
    };
    let rest = read.rest();
    let source_settings = if matches!(kind, JointKind::Native(_)) {
        native_options.extend(rest);
        NativeOptions::new()
    } else {
        rest
    };
    Some((
        Joint {
            connectors,
            kind,
            alignment: None,
            native_options,
            source_settings,
        },
        charge,
    ))
}

/// `Precision::Confusion()`, the tolerance of `FreeCAD`'s joint cases: an
/// Angle joint within it of a multiple of a full turn is a parallel joint, and
/// a point-to-point distance below it is a coincidence.
const CONFUSION: f64 = 1e-7;

/// The option key `fcstd:<name>`.
fn key(name: &str) -> Option<CodecName> {
    CodecName::new(&cadmpeg_ir::codec_id!("fcstd"), name)
}

/// The parameters of one joint, with the names a typed field holds.
struct Read<'a> {
    parameters: &'a JointParameters,
    used: BTreeSet<&'static str>,
    /// The parameters the joint stores in degrees.
    degrees: &'static [&'static str],
}

impl Read<'_> {
    fn scalar(&mut self, name: &'static str) -> Option<f64> {
        let value = self.parameters.scalar_value(name)?.get();
        self.used.insert(name);
        Some(value)
    }

    /// Length `name`, in millimetres.
    fn length(&mut self, name: &'static str) -> Option<Dimension<Length>> {
        let value = Length::new(self.parameters.scalar_value(name)?.get())?;
        self.used.insert(name);
        Some(Dimension::Literal(value))
    }

    /// The magnitude of angle `name`, stored in degrees, which is the angle
    /// `FreeCAD` solves for. A negative stored value stays a parameter the
    /// joint does not hold, since its sign has no effect.
    fn magnitude(&mut self, name: &'static str) -> Option<Dimension<Angle>> {
        let stored = self.parameters.scalar_value(name)?.get();
        let value = Angle::new(stored.abs().to_radians())?;
        if stored >= 0.0 {
            self.used.insert(name);
        }
        Some(Dimension::Literal(value))
    }

    /// Bound `name` where flag `enable` is true.
    fn bound(&mut self, name: &'static str, enable: &'static str) -> Option<f64> {
        if self.parameters.bool_value(enable) != Some(true) {
            return None;
        }
        let value = self.scalar(name)?;
        self.used.insert(enable);
        Some(value)
    }

    /// The enabled limits of the rotation about z, stored in degrees.
    fn rotation(&mut self) -> Option<AxisLimits> {
        let bound = |value: f64| Angle::new(value.to_radians()).map(Dimension::Literal);
        let min = self.bound("AngleMin", "EnableAngleMin").and_then(bound);
        let max = self.bound("AngleMax", "EnableAngleMax").and_then(bound);
        (min.is_some() || max.is_some()).then_some(AxisLimits::Rotation {
            axis: FrameAxis::Z,
            limits: Some(Bounds { min, max }),
            rest: None,
        })
    }

    /// The enabled limits of the translation along z, in millimetres.
    fn translation(&mut self) -> Option<AxisLimits> {
        let bound = |value: f64| Length::new(value).map(Dimension::Literal);
        let min = self.bound("LengthMin", "EnableLengthMin").and_then(bound);
        let max = self.bound("LengthMax", "EnableLengthMax").and_then(bound);
        (min.is_some() || max.is_some()).then_some(AxisLimits::Translation {
            axis: FrameAxis::Z,
            limits: Some(Bounds { min, max }),
            rest: None,
        })
    }

    /// Each parameter no typed field holds, as an option: an angle in
    /// radians, a length in millimetres, a flag, or the stored text.
    fn rest(self) -> NativeOptions {
        self.parameters
            .iter()
            .filter(|(name, _)| !self.used.contains(name))
            .filter_map(|(name, parameter)| {
                let option = match parameter {
                    JointParameter::Scalar { value, .. } if self.degrees.contains(&name) => {
                        NativeOption::Angle(Dimension::Literal(Angle::new(
                            value.get().to_radians(),
                        )?))
                    }
                    JointParameter::Scalar { value, .. } => {
                        NativeOption::Length(Dimension::Literal(Length::new(value.get())?))
                    }
                    JointParameter::Boolean { value, .. } => {
                        NativeOption::Bool(Dimension::Literal(*value))
                    }
                    JointParameter::Native { raw } => {
                        NativeOption::Text(Dimension::Literal(NonBlankString::new(raw.clone())?))
                    }
                };
                Some((key(name)?, option))
            })
            .collect()
    }
}

/// What the joint projection reads from the product structure.
struct Products<'a> {
    definitions: &'a [ProductDefinition],
    occurrences: &'a [Occurrence],
    /// The name of each object, by id.
    names: HashMap<&'a str, &'a str>,
    /// The id of each object, by name.
    ids: HashMap<&'a str, &'a str>,
    /// The `Assembly` or `JointGroup` whose `Group` lists each object.
    owners: HashMap<&'a str, &'a str>,
    /// The `JointGroup` objects.
    joint_groups: BTreeSet<&'a str>,
}

impl<'a> Products<'a> {
    fn of(ir: &'a CadIr, objects: &'a [ObjectRecord], properties: &'a [PropertyRecord]) -> Self {
        let owners_kind = objects
            .iter()
            .filter(|object| {
                matches!(
                    object.type_name.as_str(),
                    "Assembly::AssemblyObject" | "Assembly::JointGroup"
                )
            })
            .map(|object| (object.id().as_str(), object.type_name.as_str()))
            .collect::<HashMap<_, _>>();
        let mut owners = HashMap::new();
        for property in properties
            .iter()
            .filter(|property| property.name == "Group")
            .filter(|property| owners_kind.contains_key(property.owner.as_str()))
        {
            for object in property
                .links()
                .iter()
                .flatten()
                .filter_map(LinkTarget::object)
            {
                owners.entry(object).or_insert(property.owner.as_str());
            }
        }
        Self {
            definitions: &ir.model.product_definitions,
            occurrences: &ir.model.occurrences,
            names: objects
                .iter()
                .map(|object| (object.id().as_str(), object.name().as_str()))
                .collect(),
            ids: objects
                .iter()
                .map(|object| (object.name().as_str(), object.id().as_str()))
                .collect(),
            owners,
            joint_groups: owners_kind
                .into_iter()
                .filter(|(_, kind)| *kind == "Assembly::JointGroup")
                .map(|(object, _)| object)
                .collect(),
        }
    }

    /// The `JointGroup` that lists object `object`.
    fn group_of(&self, object: &str) -> Option<&'a str> {
        self.owners
            .get(object)
            .copied()
            .filter(|owner| self.joint_groups.contains(owner))
    }

    /// The definition of the `Assembly` that lists joint `object`, directly
    /// or in its `JointGroup`.
    fn definition(&self, object: &str) -> Option<&'a ProductDefinitionId> {
        let owner = self.owners.get(object)?;
        let assembly = match self.group_of(object) {
            Some(group) => self.owners.get(group)?,
            None => owner,
        };
        self.definitions
            .iter()
            .find(|definition| definition.native_ref.as_deref() == Some(*assembly))
            .map(|definition| &definition.id)
    }

    /// The one occurrence of object `object` among `candidates`: an
    /// occurrence a link places before a `:container` occurrence.
    fn sole<'o>(
        object: &str,
        candidates: impl Iterator<Item = &'o Occurrence>,
    ) -> Option<&'o Occurrence> {
        let matches = candidates
            .filter(|occurrence| occurrence.native_ref.as_deref() == Some(object))
            .collect::<Vec<_>>();
        if let [occurrence] = matches.as_slice() {
            return Some(*occurrence);
        }
        let container = crate::native::model_id("occurrence", object, "container");
        let mut placed = matches
            .into_iter()
            .filter(|occurrence| occurrence.id.as_str() != container);
        let occurrence = placed.next()?;
        placed.next().is_none().then_some(occurrence)
    }

    /// The occurrence path from definition `root` to occurrence `occurrence`.
    fn path_to(
        &self,
        root: &ProductDefinitionId,
        occurrence: &'a Occurrence,
    ) -> Option<Vec<&'a Occurrence>> {
        let mut route = vec![occurrence];
        for _ in 0..=self.occurrences.len() {
            let last = route.last()?;
            let owner = cadmpeg_ir::products::owner(self.definitions, &last.id)?;
            if &owner.id == root {
                route.reverse();
                return Some(route);
            }
            route.push(cadmpeg_ir::products::sole_placement(
                self.occurrences,
                &owner.id,
            )?);
        }
        None
    }

    /// The occurrence path from definition `root` that object `object` and
    /// sub-name `sub` name, with the rest of `sub`, the element. The object of
    /// `root` itself names the empty path. Each leading segment of `sub` that
    /// names an object placed in the prototype reached extends the path. An
    /// object with no occurrence names an element of `root` by `object.sub`.
    fn resolve(
        &self,
        root: &ProductDefinitionId,
        object: &str,
        sub: &str,
    ) -> (Vec<OccurrenceId>, String) {
        let own = self.definitions.iter().any(|definition| {
            &definition.id == root && definition.native_ref.as_deref() == Some(object)
        });
        let path = if own {
            Some(Vec::new())
        } else {
            Self::sole(object, self.occurrences.iter())
                .and_then(|occurrence| self.path_to(root, occurrence))
        };
        let Some(mut path) = path else {
            let name = self.names.get(object).copied().unwrap_or(object);
            let element = if sub.is_empty() {
                name.to_owned()
            } else {
                format!("{name}.{sub}")
            };
            return (Vec::new(), element);
        };
        let mut rest = sub;
        while let Some((segment, tail)) = rest.split_once('.') {
            let prototype = path.last().map_or(root, |last| &last.prototype);
            let Some(next) = self.ids.get(segment).and_then(|segment| {
                let children = self
                    .definitions
                    .iter()
                    .find(|definition| &definition.id == prototype)?
                    .occurrences
                    .iter()
                    .filter_map(|id| {
                        self.occurrences
                            .iter()
                            .find(|occurrence| &occurrence.id == id)
                    });
                Self::sole(segment, children)
            }) else {
                break;
            };
            path.push(next);
            rest = tail;
        }
        (
            path.into_iter()
                .map(|occurrence| occurrence.id.clone())
                .collect(),
            rest.to_owned(),
        )
    }

    /// The connector of reference `reference`, whose frame is `placement`
    /// after `offset`, in the coordinates of the definition its picks resolve
    /// in. `None` where the reference names no object.
    fn connector(
        &self,
        root: &ProductDefinitionId,
        reference: &LinkTarget,
        placement: FiniteFrame,
        offset: FiniteFrame,
    ) -> Option<JointConnector> {
        let object = reference.object()?;
        let subs = reference
            .subelements()
            .iter()
            .map(String::as_str)
            .filter(|sub| !sub.is_empty())
            .collect::<Vec<_>>();
        let subs = if subs.is_empty() { vec![""] } else { subs };
        let mut picks = Vec::<Picked>::new();
        for sub in subs {
            let (path, element) = match reference.document() {
                Some(document) => {
                    let element = if sub.is_empty() {
                        format!("{}#{object}", document.as_str())
                    } else {
                        format!("{}#{object}.{sub}", document.as_str())
                    };
                    (Vec::new(), element)
                }
                None => self.resolve(root, object, sub),
            };
            let pick = pick(path, element);
            if !picks.contains(&pick) {
                picks.push(pick);
            }
        }
        Some(frame(picks, offset.transform(), placement))
    }

    /// The connector of reference `reference` that the source detaches from
    /// it (`DetachN`): a pick of the occurrence the reference names, moved by
    /// its stored frame `placement` (§6.7, Frames). Each element the
    /// reference names is the native option `fcstd:Reference<index>.sub<i>`,
    /// and each of the components `offset` of its inert offset the native
    /// option `fcstd:Offset<index>.<component>`. `None` where the reference
    /// names no object.
    fn detached(
        &self,
        root: &ProductDefinitionId,
        reference: &LinkTarget,
        placement: FiniteFrame,
        index: &str,
        offset: Option<[FiniteReal; 7]>,
    ) -> Option<(JointConnector, NativeOptions)> {
        let object = reference.object()?;
        let subs = reference
            .subelements()
            .iter()
            .map(String::as_str)
            .filter(|sub| !sub.is_empty())
            .collect::<Vec<_>>();
        let (path, element) = match reference.document() {
            Some(document) => (Vec::new(), format!("{}#{object}", document.as_str())),
            None => {
                let (path, _) =
                    self.resolve(root, object, subs.first().copied().unwrap_or_default());
                if path.is_empty() {
                    self.resolve(root, object, "")
                } else {
                    (path, String::new())
                }
            }
        };
        let mut options = NativeOptions::new();
        for (position, sub) in subs.into_iter().enumerate() {
            if let (Some(key), Some(text)) = (
                key(&format!("Reference{index}.sub{position}")),
                NonBlankString::new(sub.to_owned()),
            ) {
                options.insert(key, NativeOption::Text(Dimension::Literal(text)));
            }
        }
        let components = ["px", "py", "pz", "qx", "qy", "qz", "qw"];
        for (component, value) in components.into_iter().zip(offset.into_iter().flatten()) {
            if let Some(key) = key(&format!("Offset{index}.{component}")) {
                options.insert(key, NativeOption::Real(Dimension::Literal(value)));
            }
        }
        Some((
            frame(vec![pick(path, element)], placement.transform(), placement),
            options,
        ))
    }
}

/// The pick of element `element` in the occurrence at `path`, which picks the
/// whole occurrence where `element` is empty.
fn pick(path: Vec<OccurrenceId>, element: String) -> Picked {
    let context = (!path.is_empty()).then(|| {
        PickContext::Occurrence(OccurrencePath(
            path.into_iter()
                .map(|occurrence| PathStep {
                    occurrence: StepRef::Occurrence(occurrence),
                    configuration: None,
                })
                .collect(),
        ))
    });
    match NonBlankString::new(element) {
        None => Picked {
            target: PickTarget::Component,
            context,
            native: None,
            domain: None,
            witness: None,
            resolved: None,
        },
        Some(element) => Picked {
            target: PickTarget::Native { within: None },
            context,
            native: Some(NativeName {
                codec: cadmpeg_ir::codec_id!("fcstd"),
                value: NativeNameValue::Text(element),
            }),
            domain: None,
            witness: None,
            resolved: None,
        },
    }
}

/// The frame connector on `picks`, moved by `step`, whose frame is
/// `placement`.
fn frame(picks: Vec<Picked>, step: Transform, placement: FiniteFrame) -> JointConnector {
    let role = picks
        .iter()
        .all(|pick| matches!(pick.target, PickTarget::Component))
        .then_some(InputRole {
            subject: Subject::Whole,
            relation: Relation::Coincident,
        });
    let selection = Members::new(picks).ok().map(|members| Selection {
        at: Anchor::Current,
        picks: Picks::Named(members),
        native: None,
    });
    JointConnector::Frame(Construction {
        inputs: vec![ConstructionInput {
            role,
            reference: selection.map(InputReference::Picks),
            value: None,
            reversed: None,
        }],
        after: (step != Transform::identity())
            .then_some(PostStep::Fixed(step))
            .into_iter()
            .collect(),
        solution: None,
        mode: None,
        resolved: super::frame_value(placement),
        resolved_in: BTreeMap::new(),
    })
}
