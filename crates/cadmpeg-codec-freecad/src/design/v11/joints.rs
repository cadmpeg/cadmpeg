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
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::transform::Transform;

use crate::loss::FreecadLossCode;
use crate::native::frame::FiniteFrame;
use crate::native::joint::{JointBody, JointParameter, JointParameters, JointRecord};
use crate::native::{LinkTarget, ObjectRecord, PropertyRecord};

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

/// The design records of joint records `records` in the scope of definition
/// `root`. A joint whose operand the scope cannot name is stated by no record,
/// with a loss in `losses`.
pub(super) fn project(
    ir: &CadIr,
    root: &ProductDefinitionId,
    records: &[JointRecord],
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    losses: &mut Vec<LossNote>,
) -> Joints {
    let products = Products::of(ir, objects, properties);
    let mut joints = Joints {
        features: Vec::new(),
        groups: Vec::new(),
        loose: BTreeSet::new(),
        grounded: BTreeMap::new(),
    };
    let mut grouped = BTreeMap::<&str, BTreeSet<Member>>::new();
    for record in records {
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
                let [first, second] = connectors.as_ref();
                let connectors = [first, second].map(|connector| {
                    let reference = connector.reference.as_ref()?;
                    products.connector(definition, reference, connector.placement, connector.offset)
                });
                let [Some(first), Some(second)] = connectors else {
                    losses.push(FreecadLossCode::FeatureJointUnresolved.note(format!(
                        "joint {} names no object for a connector; the joint stays in its \
                         native record",
                        record.object()
                    )));
                    continue;
                };
                let Some(joint) = joint(kind.as_str(), record.parameters(), [first, second]) else {
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
                    continue;
                };
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
/// connectors `connectors`. Each parameter the joint does not hold is a native
/// option under its own name.
/// `None` where the family is blank.
fn joint(
    label: &str,
    parameters: &JointParameters,
    connectors: [JointConnector; 2],
) -> Option<Joint> {
    let family = label.to_ascii_lowercase();
    let mut read = Read {
        parameters,
        used: BTreeSet::from(["Suppressed", "Detach1", "Detach2"]),
        degrees: if family == "angle" {
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
        "slider" | "prismatic" => motion(
            JointMotion::Slider { axis: FrameAxis::Z },
            read.translation().into_iter().collect(),
        ),
        "ball" | "spherical" => motion(JointMotion::Ball, Vec::new()),
        "distance" => JointKind::Distance {
            distance: read.length("Distance"),
            limits: None,
        },
        "parallel" => JointKind::Parallel,
        "perpendicular" => JointKind::Perpendicular,
        "angle" => JointKind::Angle {
            angle: read
                .angle("Angle")
                .or_else(|| read.unsigned_angle("Distance")),
            limits: None,
        },
        "rackpinion" | "rack_pinion" => coupling(Coupling::RackPinion {
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
    let detach = ["Detach1", "Detach2"]
        .into_iter()
        .filter(|name| parameters.bool_value(name) == Some(true))
        .filter_map(|name| Some((key(name)?, NativeOption::Bool(Dimension::Literal(true)))))
        .collect();
    Some(Joint {
        connectors,
        kind,
        alignment: None,
        native_options: read.rest(),
        source_settings: detach,
    })
}

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

    /// Angle `name`, stored in degrees.
    fn angle(&mut self, name: &'static str) -> Option<Dimension<Angle>> {
        let value = Angle::new(self.parameters.scalar_value(name)?.get().to_radians())?;
        self.used.insert(name);
        Some(Dimension::Literal(value))
    }

    /// The magnitude of angle `name`, stored in degrees.
    fn unsigned_angle(&mut self, name: &'static str) -> Option<Dimension<Angle>> {
        let value = Angle::new(self.parameters.scalar_value(name)?.get().abs().to_radians())?;
        self.used.insert(name);
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

    /// Each parameter no typed field holds, as a native option: an angle in
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
            let pick = match NonBlankString::new(element) {
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
            };
            if !picks.contains(&pick) {
                picks.push(pick);
            }
        }
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
        let offset = offset.transform();
        Some(JointConnector::Frame(Construction {
            inputs: vec![ConstructionInput {
                role,
                reference: selection.map(InputReference::Picks),
                value: None,
                reversed: None,
            }],
            after: (offset != Transform::identity())
                .then_some(PostStep::Fixed(offset))
                .into_iter()
                .collect(),
            solution: None,
            mode: None,
            resolved: super::frame_value(placement),
            resolved_in: BTreeMap::new(),
        }))
    }
}
