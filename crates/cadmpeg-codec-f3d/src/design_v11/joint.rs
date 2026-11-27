// SPDX-License-Identifier: Apache-2.0
//! The design `Joint` and `RigidGroup` features of Fusion joints (CADIR
//! §6.7, Joints and Rigid groups), restated from the decoder's assembly
//! joints and the native motion record of each joint's scope.
//!
//! A joint's kind is its motion record's freedom code (§6.7, the Fusion
//! row), its connectors are the joint's solved frames, and the limits and
//! rest are the motion record's lanes. The assembly joint of a revolute
//! about the joint's y axis turns its frames so that y becomes z; the design
//! joint states the frames as stored and names the y axis.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::construction::{
    Construction, ConstructionInput, FrameAxis, FrameValue, InputReference, InputRole, PostStep,
    Relation, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::design::names::{CodecName, NativeName, NativeNameValue};
use cadmpeg_ir::design::ops::assembly::RigidGroup;
use cadmpeg_ir::design::ops::shared::{
    Alignment, AxisLimits, Bounds, Coupling, Joint, JointConnector, JointKind, JointMotion,
    JointOffset,
};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::projection::restated::Restater;
use cadmpeg_ir::design::selection::{
    Anchor, Members, OccurrencePath, OrderedSelection, PathStep, PickContext, PickTarget, Picked,
    Picks, Selection, StepRef,
};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureDefinition as LegacyDefinition, FeatureId,
    FeatureOperation as LegacyOperation,
};
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{
    self, AssemblyJoint, JointId, JointLimits, JointOperands, Occurrence, OperandContainer,
    PairedJointKind, PlaneSense, ProductDefinition,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::UnitVector3;

use crate::design::assembly::operand_keys::OperandKeys;
use crate::loss::F3dLossCode;
use crate::records::feature::joint_motion::DesignJointMotion;

/// Millimetres per stored centimetre of a translation lane.
const MM_PER_CM: f64 = 10.0;

/// What a joint's scope record states beside its assembly joint.
#[derive(Clone, Copy, Default)]
pub(super) struct Source<'a> {
    /// Whether the scope is an `As-built` joint.
    pub(super) as_built: bool,
    /// Whether the scope is an as-built rigid group.
    pub(super) group: bool,
    /// The motion record that binds to the scope.
    pub(super) motion: Option<&'a DesignJointMotion>,
}

/// The occurrences a connector's context path walks.
pub(super) struct Places<'a> {
    pub(super) occurrences: &'a [Occurrence],
    pub(super) parents: BTreeMap<OccurrenceId, OccurrenceId>,
    /// The definition that lists each occurrence.
    pub(super) owners: HashMap<&'a OccurrenceId, &'a ProductDefinitionId>,
    pub(super) keys: OperandKeys<'a>,
}

impl<'a> Places<'a> {
    pub(super) fn new(
        definitions: &'a [ProductDefinition],
        occurrences: &'a [Occurrence],
        keys: OperandKeys<'a>,
    ) -> Self {
        Self {
            occurrences,
            parents: crate::tree::parent_ids(definitions, occurrences),
            owners: definitions
                .iter()
                .flat_map(|definition| {
                    definition
                        .occurrences
                        .iter()
                        .map(move |occurrence| (occurrence, &definition.id))
                })
                .collect(),
            keys,
        }
    }

    /// The path from `owner` to `occurrence`: the chain of parents from the
    /// first occurrence `owner` lists, else from the outermost occurrence.
    fn path(&self, occurrence: &OccurrenceId, owner: &ProductDefinitionId) -> OccurrencePath {
        let mut chain = vec![occurrence.clone()];
        let mut at = occurrence;
        while self.owners.get(at) != Some(&owner) && chain.len() <= self.occurrences.len() {
            let Some(parent) = self.parents.get(at) else {
                break;
            };
            chain.push(parent.clone());
            at = parent;
        }
        OccurrencePath(
            chain
                .into_iter()
                .rev()
                .map(|occurrence| PathStep {
                    occurrence: StepRef::Occurrence(occurrence),
                    configuration: None,
                })
                .collect(),
        )
    }

    /// The occurrence that `occurrence` and the residual native keys `path`
    /// name, with the keys that name no single child.
    fn descend(&self, occurrence: &OccurrenceId, path: &[String]) -> (OccurrenceId, Vec<String>) {
        let mut occurrence = occurrence.clone();
        let mut path = path.to_vec();
        self.keys
            .descend(self.occurrences, &self.parents, &mut occurrence, &mut path);
        (occurrence, path)
    }
}

/// The records a joint feature restates.
pub(super) struct Inputs<'a> {
    /// The assembly joint each legacy joint feature binds, by feature id.
    bound: HashMap<&'a str, &'a JointId>,
    /// The feature each assembly joint binds to.
    features: HashMap<&'a JointId, &'a FeatureId>,
    joints: HashMap<&'a JointId, &'a AssemblyJoint>,
    /// The joints of each scope.
    scoped: BTreeMap<&'a str, Vec<&'a AssemblyJoint>>,
    /// What each scope record states, by scope id.
    pub(super) sources: HashMap<&'a str, Source<'a>>,
    pub(super) places: Places<'a>,
}

impl<'a> Inputs<'a> {
    pub(super) fn new(
        legacy: &'a [LegacyFeature],
        joints: &'a [AssemblyJoint],
        sources: HashMap<&'a str, Source<'a>>,
        places: Places<'a>,
    ) -> Self {
        let mut bound = HashMap::new();
        let mut features = HashMap::new();
        for feature in legacy {
            if let LegacyDefinition::Operation(LegacyOperation::AssemblyJoint { joint }) =
                feature.evaluation.definition()
            {
                bound.insert(feature.id.as_str(), joint);
                features.entry(joint).or_insert(&feature.id);
            }
        }
        let mut scoped = BTreeMap::<&str, Vec<&AssemblyJoint>>::new();
        for joint in joints {
            if let Some(scope) = joint.native_ref.as_deref() {
                scoped.entry(scope).or_default().push(joint);
            }
        }
        Self {
            bound,
            features,
            joints: joints.iter().map(|joint| (&joint.id, joint)).collect(),
            scoped,
            sources,
            places,
        }
    }
}

/// Restate each joint feature of `features`, a scope's features, from the
/// assembly joint its legacy feature binds. `owner` is the scope's
/// definition. A feature that does not restate keeps its operation, and a
/// loss names it. Returns each assembly joint a restated feature states,
/// with the feature's id: a rigid group states every joint of its scope.
pub(super) fn project(
    features: &mut [Feature],
    owner: &ProductDefinitionId,
    inputs: &Inputs<'_>,
    losses: &mut Vec<LossNote>,
) -> Vec<(String, String)> {
    let ids = features
        .iter()
        .map(|feature| feature.id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let mut unprojected = Vec::new();
    let mut restated = Vec::new();
    for feature in features.iter_mut() {
        if !matches!(
            feature.definition.operation,
            FeatureOperation::Unresolved {
                family: OperationFamily::Joint
            }
        ) {
            continue;
        }
        let Some(joint) = inputs.bound.get(feature.id.as_str()) else {
            continue;
        };
        let site = Site {
            owner: feature.component.as_ref().unwrap_or(owner),
            ids: &ids,
            inputs,
        };
        let projected = inputs
            .joints
            .get(joint)
            .ok_or("its assembly joint is absent")
            .and_then(|joint| site.operation(joint));
        match projected {
            Ok(operation) => {
                let stated = match (&operation, inputs.joints.get(joint)) {
                    (FeatureOperation::RigidGroup(_), Some(joint)) => joint
                        .native_ref
                        .as_deref()
                        .and_then(|scope| inputs.scoped.get(scope))
                        .map(|group| group.iter().map(|member| &member.id).collect())
                        .unwrap_or_default(),
                    _ => vec![*joint],
                };
                restated.extend(
                    stated
                        .into_iter()
                        .map(|id| (id.as_str().to_owned(), feature.id.as_str().to_owned())),
                );
                feature.definition.operation = operation;
            }
            Err(reason) => unprojected.push(format!("{} ({reason})", feature.id)),
        }
    }
    if !unprojected.is_empty() {
        losses.push(F3dLossCode::AssemblyDesignJointUnprojected.note(format!(
            "{} Fusion joint feature(s) keep an unresolved operation: {}",
            unprojected.len(),
            unprojected.join(", ")
        )));
    }
    restated
}

/// State in `model` that each feature of `restated`, as [`project`] returns
/// it, restates the assembly joint paired with it, whole.
pub(super) fn record(model: &mut cadmpeg_ir::document::Model, restated: Vec<(String, String)>) {
    for (joint, feature) in restated {
        model
            .restated_mut()
            .record(&joint, Restater::Record(feature), []);
    }
}

/// The context of one joint feature.
struct Site<'s, 'a> {
    /// The definition the feature's connector paths start at.
    owner: &'s ProductDefinitionId,
    /// The ids of the scope's features.
    ids: &'s BTreeSet<String>,
    inputs: &'s Inputs<'a>,
}

impl Site<'_, '_> {
    /// The design operation of `joint`.
    fn operation(&self, joint: &AssemblyJoint) -> Result<FeatureOperation, &'static str> {
        let source = joint
            .native_ref
            .as_deref()
            .and_then(|scope| self.inputs.sources.get(scope))
            .copied()
            .unwrap_or_default();
        let JointOperands::Pair {
            kind, connectors, ..
        } = joint.operands()
        else {
            return Err("it has one connector");
        };
        if source.group {
            return self
                .rigid_group(joint)
                .map(|group| FeatureOperation::RigidGroup(Box::new(group)));
        }
        let (kind, unturn, mut native_options, source_settings) = match kind {
            PairedJointKind::Gears {
                distance,
                distance2,
                ratio,
                coupled_joints,
            } => {
                let radius = |value: &Option<FiniteReal>| {
                    value
                        .and_then(|value| Length::new(value.get()))
                        .map(Dimension::Literal)
                };
                (
                    JointKind::Coupling {
                        coupling: Coupling::Gear {
                            ratio: ratio
                                .and_then(|ratio| FiniteReal::new(ratio.get()))
                                .map(Dimension::Literal),
                            radii: [radius(distance), radius(distance2)],
                        },
                        joints: self.coupled(coupled_joints),
                    },
                    false,
                    NativeOptions::new(),
                    NativeOptions::new(),
                )
            }
            PairedJointKind::Fixed { .. }
            | PairedJointKind::Revolute { .. }
            | PairedJointKind::Slider { .. } => {
                let freedom = freedom(kind, source.motion);
                let unturn = matches!(kind, PairedJointKind::Revolute { .. })
                    && source
                        .motion
                        .is_some_and(|motion| freedom_codes(motion) == [1]);
                let mut native_options = NativeOptions::new();
                let mut source_settings = NativeOptions::new();
                let limits = limits(
                    kind,
                    source.motion,
                    &freedom,
                    &mut native_options,
                    &mut source_settings,
                );
                (
                    JointKind::Motion {
                        motion: freedom.motion().ok_or("its freedom codes name no motion")?,
                        axes: None,
                        offset: offset(kind),
                        limits,
                        drives: Vec::new(),
                    },
                    unturn,
                    native_options,
                    source_settings,
                )
            }
            _ => return Err("its kind has no Fusion design form"),
        };
        if source.as_built {
            insert_text(&mut native_options, "joint_mode", "As-built".to_owned());
        }
        let [first, second] = connectors.as_ref();
        let connector = |connector: &products::JointConnector| {
            let frame = if unturn {
                unturned(connector.frame).ok_or("its frame does not turn back")?
            } else {
                connector.frame
            };
            self.connector(connector, frame)
                .ok_or("its connector names no pick")
        };
        let joint = Joint {
            connectors: [connector(first)?, connector(second)?],
            kind,
            alignment: joint.frame_sense().map(|sense| match sense {
                PlaneSense::Aligned => Alignment::Aligned,
                PlaneSense::Opposed => Alignment::Opposed,
            }),
            native_options,
            source_settings,
        };
        Ok(FeatureOperation::Joint(Box::new(joint)))
    }

    /// The `Feature` picks of the two joints a coupling acts on, where each
    /// binds a feature of the scope.
    fn coupled(&self, joints: &[JointId]) -> Option<OrderedSelection> {
        let [first, second] = joints else {
            return None;
        };
        let pick = |joint: &JointId| {
            let feature = *self.inputs.features.get(joint)?;
            self.ids.contains(feature.as_str()).then(|| Picked {
                target: PickTarget::Feature(feature.clone()),
                context: None,
                native: None,
                domain: None,
                witness: None,
                resolved: None,
            })
        };
        Some(Selection {
            at: Anchor::Current,
            picks: Picks::Named(Members::new(vec![pick(first)?, pick(second)?]).ok()?),
            native: None,
        })
    }

    /// A connector on `connector`'s operand at `frame`: a joint origin's
    /// frame datum, a frame on a whole occurrence moved by `frame`, or the
    /// native operand at `frame`.
    fn connector(
        &self,
        connector: &products::JointConnector,
        frame: Transform,
    ) -> Option<JointConnector> {
        let operand = &connector.operand;
        let (pick, after) = match &operand.container {
            OperandContainer::Root {}
                if operand.subelements.is_empty() && self.ids.contains(&operand.object) =>
            {
                (
                    Picked {
                        target: PickTarget::Datum {
                            feature: FeatureId::mint(operand.object.clone()).ok()?,
                            part: None,
                        },
                        context: None,
                        native: None,
                        domain: None,
                        witness: None,
                        resolved: None,
                    },
                    Vec::new(),
                )
            }
            OperandContainer::Occurrence {
                occurrence,
                occurrence_path,
            } => {
                let (occurrence, keys) = self.inputs.places.descend(occurrence, occurrence_path);
                let context = Some(PickContext::Occurrence(
                    self.inputs.places.path(&occurrence, self.owner),
                ));
                let native = native_name(
                    keys.iter()
                        .chain(std::iter::once(&operand.object))
                        .chain(&operand.subelements),
                );
                let target = if keys.is_empty() {
                    PickTarget::Component
                } else {
                    PickTarget::Native { within: None }
                };
                (
                    Picked {
                        target,
                        context,
                        native: Some(native?),
                        domain: None,
                        witness: None,
                        resolved: None,
                    },
                    vec![PostStep::Fixed(frame)],
                )
            }
            OperandContainer::Root {} | OperandContainer::External { .. } => (
                Picked {
                    target: PickTarget::Native { within: None },
                    context: None,
                    native: Some(native_name(
                        std::iter::once(&operand.object).chain(&operand.subelements),
                    )?),
                    domain: None,
                    witness: None,
                    resolved: None,
                },
                Vec::new(),
            ),
        };
        Some(JointConnector::Frame(Construction {
            inputs: vec![input(pick)?],
            after,
            solution: None,
            mode: None,
            resolved: frame_value(&frame),
            resolved_in: BTreeMap::new(),
        }))
    }

    /// The rigid group of the as-built scope `joint` names: the occurrences
    /// its joints connect, with each member's locator frame and each
    /// unresolved member as native options.
    fn rigid_group(&self, joint: &AssemblyJoint) -> Result<RigidGroup, &'static str> {
        let group = joint
            .native_ref
            .as_deref()
            .and_then(|scope| self.inputs.scoped.get(scope))
            .ok_or("its scope states no joints")?;
        let [first, second] = pair(joint)?;
        let mut connectors = vec![first, second];
        for member in group.iter().filter(|member| member.id != joint.id) {
            connectors.push(&pair(member)?[1]);
        }
        let mut members = BTreeSet::new();
        let mut native_options = NativeOptions::new();
        for (ordinal, connector) in connectors.into_iter().enumerate() {
            let operand = &connector.operand;
            let member = match &operand.container {
                OperandContainer::Occurrence {
                    occurrence,
                    occurrence_path,
                } => {
                    let (occurrence, keys) =
                        self.inputs.places.descend(occurrence, occurrence_path);
                    keys.is_empty()
                        .then(|| self.inputs.places.path(&occurrence, self.owner))
                }
                OperandContainer::Root {} | OperandContainer::External { .. } => None,
            };
            let named = match &member {
                Some(path) => steps(path),
                None => {
                    let listing = [&operand.object]
                        .into_iter()
                        .chain(&operand.subelements)
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join("/");
                    insert_text(
                        &mut native_options,
                        &format!("unresolved_member.{ordinal}"),
                        listing.clone(),
                    );
                    listing
                }
            };
            insert_text(
                &mut native_options,
                &format!("locator.{ordinal}"),
                format!("{named}: {}", rows(&connector.frame)),
            );
            if let Some(member) = member {
                members.insert(member);
            }
        }
        if members.len() < 2 {
            return Err("fewer than two of its members resolve to occurrences");
        }
        if let JointOperands::Pair {
            kind:
                PairedJointKind::Fixed {
                    angle,
                    translation_offset,
                    ..
                },
            ..
        } = joint.operands()
        {
            for (name, value) in ["offset_x", "offset_y", "offset_z"]
                .into_iter()
                .zip(translation_offset.iter().flatten())
                .filter(|(_, value)| value.get() != 0.0)
            {
                if let (Some(name), Some(length)) = (codec_name(name), Length::new(value.get())) {
                    native_options.insert(name, NativeOption::Length(Dimension::Literal(length)));
                }
            }
            if let Some(angle) = angle.filter(|angle| angle.get() != 0.0) {
                if let (Some(name), Some(angle)) = (codec_name("angle"), Angle::new(angle.get())) {
                    native_options.insert(name, NativeOption::Angle(Dimension::Literal(angle)));
                }
            }
        }
        Ok(RigidGroup {
            members,
            native_options,
            source_settings: NativeOptions::new(),
        })
    }
}

/// The two connectors of a paired joint.
fn pair(joint: &AssemblyJoint) -> Result<&[products::JointConnector; 2], &'static str> {
    match joint.operands() {
        JointOperands::Pair { connectors, .. } => Ok(connectors.as_ref()),
        JointOperands::Grounded { .. } => Err("a member joint has one connector"),
    }
}

/// The freedoms a joint leaves, from its motion record's codes (§6.7, the
/// Fusion row).
#[derive(Clone, PartialEq, Debug)]
enum Freedom {
    Rigid,
    Rotation(FrameAxis),
    Translation(FrameAxis),
    Native(String),
}

impl Freedom {
    fn of(codes: &[u32]) -> Self {
        match codes {
            [] => Self::Rigid,
            [1] => Self::Rotation(FrameAxis::Y),
            [2] => Self::Rotation(FrameAxis::Z),
            [8] => Self::Translation(FrameAxis::Z),
            codes => Self::Native(format!(
                "f3d:freedom:{}",
                codes
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            )),
        }
    }

    fn motion(&self) -> Option<JointMotion> {
        Some(match self {
            Self::Rigid => JointMotion::Rigid,
            Self::Rotation(axis) => JointMotion::Revolute { axis: *axis },
            Self::Translation(axis) => JointMotion::Slider { axis: *axis },
            Self::Native(name) => JointMotion::Native(NonBlankString::new(name.clone())?),
        })
    }
}

/// The freedom of the motion record that binds to a joint's scope, else the
/// freedom its assembly joint's kind states: a joint with no motion record is
/// rigid.
fn freedom(kind: &PairedJointKind, motion: Option<&DesignJointMotion>) -> Freedom {
    match (motion, kind) {
        (Some(motion), _) => Freedom::of(&freedom_codes(motion)),
        (None, PairedJointKind::Revolute { .. }) => Freedom::Rotation(FrameAxis::Z),
        (None, PairedJointKind::Slider { .. }) => Freedom::Translation(FrameAxis::Z),
        (None, _) => Freedom::Rigid,
    }
}

fn freedom_codes(motion: &DesignJointMotion) -> Vec<u32> {
    motion
        .freedoms
        .iter()
        .map(|freedom| freedom.value)
        .collect()
}

/// The stored displacement of the second connector: the offsets and the
/// angle the assembly joint's kind carries, zero where its kind requires
/// zero.
fn offset(kind: &PairedJointKind) -> JointOffset {
    let length = |value: Option<FiniteReal>| {
        value
            .and_then(|value| Length::new(value.get()))
            .map(Dimension::Literal)
    };
    let angle = |value: Option<FiniteReal>| {
        value
            .and_then(|value| Angle::new(value.get()))
            .map(Dimension::Literal)
    };
    let zero = FiniteReal::new(0.0);
    let (translation, rotation) = match kind {
        PairedJointKind::Fixed {
            angle,
            translation_offset,
            ..
        } => (*translation_offset, *angle),
        PairedJointKind::Revolute { angle, .. } => (zero.map(|zero| [zero; 3]), *angle),
        PairedJointKind::Slider {
            translation_offset, ..
        } => (*translation_offset, zero),
        _ => (None, None),
    };
    let [x, y, z] = translation.map_or([None; 3], |offset| offset.map(Some));
    JointOffset {
        x: length(x),
        y: length(y),
        z: length(z),
        angle: angle(rotation),
    }
}

/// The limits and rest of a joint on its free axis, from its motion
/// record's lanes and its scope's stored limits. A value no free axis holds
/// goes to `native_options`; a rest stored with its flag cleared goes to
/// `source_settings` (§6.7, Kinds).
fn limits(
    kind: &PairedJointKind,
    motion: Option<&DesignJointMotion>,
    freedom: &Freedom,
    native_options: &mut NativeOptions,
    source_settings: &mut NativeOptions,
) -> Vec<AxisLimits> {
    let mut bounds = None;
    let mut rest = None;
    let scale = match freedom {
        Freedom::Translation(_) => MM_PER_CM,
        _ => 1.0,
    };
    let free = matches!(freedom, Freedom::Rotation(_) | Freedom::Translation(_));
    if let Some(lanes) = motion.and_then(|motion| motion.limits.as_ref().map(|l| (motion, l))) {
        let (motion, lanes) = lanes;
        let flags = (lanes.first_flag, lanes.second_flag);
        let ordered = lanes.minimum <= lanes.maximum;
        match flags {
            (0, 0) => {}
            (1, 1) if free && crate::design::assembly::zero_lane_rotation_limits(motion) => {
                insert_angle(native_options, "zero_lane_limit_minimum", lanes.minimum);
                insert_angle(native_options, "zero_lane_limit_maximum", lanes.maximum);
            }
            (1, 1) if free && ordered => {
                bounds = Some((Some(lanes.minimum * scale), Some(lanes.maximum * scale)));
            }
            (first, second) => {
                insert_integer(native_options, "limit_first_flag", i64::from(first));
                insert_integer(native_options, "limit_second_flag", i64::from(second));
                insert_real(native_options, "limit_minimum", lanes.minimum);
                insert_real(native_options, "limit_maximum", lanes.maximum);
            }
        }
        if let Some(stored) = lanes.rest.filter(|_| !lanes.rest_malformed) {
            match (stored.flag, free) {
                (1, true) => rest = Some(stored.value * scale),
                (0, true) => insert_quantity(
                    source_settings,
                    "rest_value_disabled",
                    freedom,
                    stored.value * scale,
                ),
                (0, false) => insert_real(source_settings, "rest_value_disabled", stored.value),
                (1, false) => insert_real(native_options, "rest_value", stored.value),
                (flag, _) => {
                    insert_integer(native_options, "rest_flag", i64::from(flag));
                    insert_real(native_options, "rest_value", stored.value);
                }
            }
        }
    }
    if let PairedJointKind::Fixed {
        angular_limits,
        linear_limits,
        ..
    } = kind
    {
        for (stored, rotation) in [(angular_limits, true), (linear_limits, false)] {
            let Some(stored) = stored else {
                continue;
            };
            let (min, max) = range(stored);
            let fits = match freedom {
                Freedom::Rotation(_) => rotation,
                Freedom::Translation(_) => !rotation,
                _ => false,
            };
            if fits && bounds.is_none() {
                bounds = Some((min, max));
            } else {
                let quantity = if rotation {
                    Freedom::Rotation(FrameAxis::Z)
                } else {
                    Freedom::Translation(FrameAxis::Z)
                };
                for (name, value) in [("scope_limit_minimum", min), ("scope_limit_maximum", max)] {
                    if let Some(value) = value {
                        insert_quantity(native_options, name, &quantity, value);
                    }
                }
            }
        }
    }
    let entry = match freedom {
        Freedom::Rotation(axis) => {
            let limits = bounds.and_then(|(min, max)| axis_bounds(min, max, Angle::new));
            let rest = rest.and_then(Angle::new).map(Dimension::Literal);
            (limits.is_some() || rest.is_some()).then_some(AxisLimits::Rotation {
                axis: *axis,
                limits,
                rest,
            })
        }
        Freedom::Translation(axis) => {
            let limits = bounds.and_then(|(min, max)| axis_bounds(min, max, Length::new));
            let rest = rest.and_then(Length::new).map(Dimension::Literal);
            (limits.is_some() || rest.is_some()).then_some(AxisLimits::Translation {
                axis: *axis,
                limits,
                rest,
            })
        }
        Freedom::Rigid | Freedom::Native(_) => None,
    };
    entry.into_iter().collect()
}

fn range(limits: &JointLimits) -> (Option<f64>, Option<f64>) {
    match limits {
        JointLimits::Minimum { minimum } => (Some(minimum.get()), None),
        JointLimits::Maximum { maximum } => (None, Some(maximum.get())),
        JointLimits::Range(range) => (Some(range.minimum().get()), Some(range.maximum().get())),
    }
}

fn axis_bounds<Q>(
    min: Option<f64>,
    max: Option<f64>,
    value: impl Fn(f64) -> Option<Q>,
) -> Option<Bounds<Q>> {
    let min = min.and_then(&value).map(Dimension::Literal);
    let max = max.and_then(&value).map(Dimension::Literal);
    (min.is_some() || max.is_some()).then_some(Bounds { min, max })
}

/// The frame as stored, from the frame an assembly joint turned so that
/// its y axis became its z axis (axes (x, -z, y)).
fn unturned(frame: Transform) -> Option<Transform> {
    let mut rows = frame.affine_rows();
    for row in &mut rows {
        let (turned_y, turned_z) = (row[1], row[2]);
        row[1] = turned_z;
        row[2] = -turned_y;
    }
    Transform::affine(rows)
}

/// The frame value of a rigid transform.
fn frame_value(frame: &Transform) -> Option<FrameValue> {
    let [r0, r1, r2] = frame.affine_rows();
    let column = |c: usize| UnitVector3::normalized(Vector3::new(r0[c], r1[c], r2[c]));
    Some(FrameValue {
        origin: FinitePoint3::new(Point3::new(r0[3], r1[3], r2[3]))?,
        x_axis: column(0)?,
        y_axis: column(1)?,
        z_axis: column(2)?,
    })
}

/// The affine rows of a frame, row by row, as text.
fn rows(frame: &Transform) -> String {
    frame
        .affine_rows()
        .iter()
        .flatten()
        .map(f64::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The occurrences of a path, outermost first, as text.
fn steps(path: &OccurrencePath) -> String {
    path.0
        .iter()
        .map(|step| match &step.occurrence {
            StepRef::Occurrence(id) => id.to_string(),
            StepRef::Native(name) => format!("{:?}", name.value),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// A construction input that coincides with the whole of `pick`.
fn input(pick: Picked) -> Option<ConstructionInput> {
    Some(ConstructionInput {
        role: Some(InputRole {
            subject: Subject::Whole,
            relation: Relation::Coincident,
        }),
        reference: Some(InputReference::Picks(Selection {
            at: Anchor::Current,
            picks: Picks::Named(Members::new(vec![pick]).ok()?),
            native: None,
        })),
        value: None,
        reversed: None,
    })
}

/// The native name of an operand: its parts joined by `/`.
fn native_name<'s>(parts: impl Iterator<Item = &'s String>) -> Option<NativeName> {
    Some(NativeName {
        codec: cadmpeg_ir::codec_id!("f3d"),
        value: NativeNameValue::Text(NonBlankString::new(
            parts.map(String::as_str).collect::<Vec<_>>().join("/"),
        )?),
    })
}

fn codec_name(name: &str) -> Option<CodecName> {
    CodecName::new(&cadmpeg_ir::codec_id!("f3d"), name)
}

fn insert_text(options: &mut NativeOptions, name: &str, value: String) {
    if let (Some(name), Some(value)) = (codec_name(name), NonBlankString::new(value)) {
        options.insert(name, NativeOption::Text(Dimension::Literal(value)));
    }
}

fn insert_integer(options: &mut NativeOptions, name: &str, value: i64) {
    if let Some(name) = codec_name(name) {
        options.insert(name, NativeOption::Integer(Dimension::Literal(value)));
    }
}

fn insert_real(options: &mut NativeOptions, name: &str, value: f64) {
    if let (Some(name), Some(value)) = (codec_name(name), FiniteReal::new(value)) {
        options.insert(name, NativeOption::Real(Dimension::Literal(value)));
    }
}

fn insert_angle(options: &mut NativeOptions, name: &str, value: f64) {
    if let (Some(name), Some(value)) = (codec_name(name), Angle::new(value)) {
        options.insert(name, NativeOption::Angle(Dimension::Literal(value)));
    }
}

/// Insert `value` as the quantity of `freedom`'s axis: an angle for a
/// rotation, a length for a translation.
fn insert_quantity(options: &mut NativeOptions, name: &str, freedom: &Freedom, value: f64) {
    match freedom {
        Freedom::Translation(_) => {
            if let (Some(name), Some(value)) = (codec_name(name), Length::new(value)) {
                options.insert(name, NativeOption::Length(Dimension::Literal(value)));
            }
        }
        _ => insert_angle(options, name, value),
    }
}

#[cfg(test)]
mod tests;
