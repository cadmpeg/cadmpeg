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

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::nonblank_literal;
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
use cadmpeg_ir::design::projection::fields::{Field, ModelFeatureField};
use cadmpeg_ir::design::projection::restated::Restater;
use cadmpeg_ir::design::selection::{
    Anchor, DefinitionRef, Members, OccurrencePath, OrderedSelection, PathStep, PickContext,
    PickTarget, Picked, Picks, Selection, StepRef,
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

/// The wire fields of an assembly joint a design feature leaves out.
const OPERANDS: &str = "operands";
const CONNECTORS: &str = "operands.connectors";
const SUPPRESSED: &str = "suppressed";
const DRIVING_PARAMETER: &str = "driving_parameter";
const DRIVES: &str = "drives";
const NATIVE_REF: &str = "native_ref";

/// What a joint's scope record states beside its assembly joint.
#[derive(Clone, Copy, Default)]
pub(super) struct Source<'a> {
    /// Whether the scope is an `As-built` joint.
    pub(super) as_built: bool,
    /// Whether the scope is an as-built rigid group.
    pub(super) group: bool,
    /// The motion record that binds to the scope.
    pub(super) motion: Option<&'a DesignJointMotion>,
    /// Whether two or more motion records bind to the scope.
    pub(super) ambiguous: bool,
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
    /// first occurrence `owner` lists. `None` where no occurrence of the
    /// chain is one that `owner` lists.
    fn path(
        &self,
        occurrence: &OccurrenceId,
        owner: &ProductDefinitionId,
    ) -> Option<OccurrencePath> {
        let mut chain = vec![occurrence.clone()];
        let mut at = occurrence;
        while self.owners.get(at) != Some(&owner) {
            if chain.len() > self.occurrences.len() {
                return None;
            }
            at = self.parents.get(at)?;
            chain.push(at.clone());
        }
        Some(OccurrencePath(
            chain
                .into_iter()
                .rev()
                .map(|occurrence| PathStep {
                    occurrence: StepRef::Occurrence(occurrence),
                    configuration: None,
                })
                .collect(),
        ))
    }

    /// The occurrence an `Occurrence` operand names: its occurrence, moved
    /// down by the native occurrence keys below it (its `occurrence_path`,
    /// then its `subelements`), with the keys that name no single child.
    /// `None` for another container.
    fn reach(&self, operand: &products::JointOperand) -> Option<(OccurrenceId, Vec<String>)> {
        let OperandContainer::Occurrence {
            occurrence,
            occurrence_path,
        } = &operand.container
        else {
            return None;
        };
        let mut occurrence = occurrence.clone();
        let mut keys = occurrence_path
            .iter()
            .chain(&operand.subelements)
            .cloned()
            .collect();
        self.keys
            .descend(self.occurrences, &self.parents, &mut occurrence, &mut keys);
        Some((occurrence, keys))
    }
}

/// The records a joint feature restates.
pub(super) struct Inputs<'a> {
    /// The assembly joint each legacy joint feature binds, by feature id.
    bound: HashMap<&'a str, &'a JointId>,
    /// The feature each assembly joint binds to.
    features: HashMap<&'a JointId, &'a FeatureId>,
    joints: HashMap<&'a JointId, &'a AssemblyJoint>,
    /// Each scope that a joint names, by scope id.
    scopes: HashMap<&'a str, Scope<'a>>,
    pub(super) places: Places<'a>,
}

/// A joint's scope record.
struct Scope<'a> {
    source: Source<'a>,
    /// The joints of the scope, in model order.
    joints: Vec<&'a AssemblyJoint>,
}

impl<'a> Inputs<'a> {
    pub(super) fn new(
        legacy: &'a [LegacyFeature],
        joints: &'a [AssemblyJoint],
        mut sources: HashMap<&'a str, Source<'a>>,
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
        let mut scopes = HashMap::<&str, Scope<'_>>::new();
        for joint in joints {
            if let Some(scope) = joint.native_ref.as_deref() {
                scopes
                    .entry(scope)
                    .or_insert_with(|| Scope {
                        source: sources.remove(scope).unwrap_or_default(),
                        joints: Vec::new(),
                    })
                    .joints
                    .push(joint);
            }
        }
        Self {
            bound,
            features,
            joints: joints.iter().map(|joint| (&joint.id, joint)).collect(),
            scopes,
            places,
        }
    }

    /// The scope record of `joint`.
    fn scope(&self, joint: &AssemblyJoint) -> Option<&Scope<'a>> {
        joint
            .native_ref
            .as_deref()
            .and_then(|scope| self.scopes.get(scope))
    }

    /// What the scope record of `joint` states.
    fn source(&self, joint: &AssemblyJoint) -> Source<'a> {
        self.scope(joint)
            .map(|scope| scope.source)
            .unwrap_or_default()
    }
}

/// A record a design feature restates, with the record's wire fields the
/// feature leaves out.
pub(super) struct Restated {
    legacy: String,
    by: String,
    cut: Vec<&'static str>,
}

/// Restate each joint feature of `features`, a scope's features, from the
/// assembly joint its legacy feature binds. `owner` is the scope's
/// definition. A feature that does not restate keeps its operation, and a
/// loss names it. Returns each assembly joint a restated feature states, with
/// the feature and the joint's fields it leaves out: a rigid group states
/// every joint of its scope.
///
/// A feature keeps its component. Where the connector paths do not start at
/// a component other than the scope's definition and do start at the
/// scope's definition, a joint's picks reach their occurrences through the
/// scope's definition; a rigid group's member paths start at the scope's
/// definition, and the group leaves out the legacy feature's
/// `product_definition`.
pub(super) fn project(
    features: &mut [Feature],
    owner: &ProductDefinitionId,
    inputs: &Inputs<'_>,
    losses: &mut Vec<LossNote>,
) -> Vec<Restated> {
    let definitions = features
        .iter()
        .map(|feature| {
            (
                feature.id.as_str().to_owned(),
                feature.component.clone().unwrap_or_else(|| owner.clone()),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut unprojected = Vec::new();
    let mut assumed = Vec::new();
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
        let Some(bound) = inputs.bound.get(feature.id.as_str()) else {
            continue;
        };
        let Some(joint) = inputs.joints.get(bound).copied() else {
            unprojected.push(format!("{} (its assembly joint is absent)", feature.id));
            continue;
        };
        let site = |at, in_context| Site {
            owner: at,
            in_context,
            scope: owner,
            definitions: &definitions,
            inputs,
        };
        let source = inputs.source(joint);
        let component = feature.component.clone();
        let (site, cleared) = match &component {
            Some(component)
                if component != owner
                    && !site(component, false).roots(joint)
                    && (source.group || site(owner, false).roots(joint)) =>
            {
                if source.group {
                    (site(owner, false), true)
                } else {
                    (site(owner, true), false)
                }
            }
            Some(component) => (site(component, false), false),
            None => (site(owner, false), false),
        };
        let operation = match site.operation(joint) {
            Ok(operation) => operation,
            Err(reason) => {
                unprojected.push(format!("{} ({reason})", feature.id));
                continue;
            }
        };
        let by = feature.id.as_str().to_owned();
        if cleared {
            feature.component = None;
            restated.push(Restated {
                legacy: by.clone(),
                by: by.clone(),
                cut: vec![ModelFeatureField::ProductDefinition.key()],
            });
        }
        let rigid = matches!(
            &operation,
            FeatureOperation::Joint(design) if matches!(
                design.kind,
                JointKind::Motion { motion: JointMotion::Rigid, .. }
            )
        );
        if rigid && source.ambiguous {
            assumed.push(format!("{} (assembly joint {})", feature.id, joint.id));
        }
        match &operation {
            FeatureOperation::RigidGroup(_) => {
                let placed = site.placed(joint).unwrap_or_default();
                for member in inputs
                    .scope(joint)
                    .into_iter()
                    .flat_map(|scope| &scope.joints)
                {
                    restated.push(Restated {
                        legacy: member.id.as_str().to_owned(),
                        by: by.clone(),
                        cut: member_cut(member, joint, &placed, feature),
                    });
                }
            }
            operation => restated.push(Restated {
                legacy: joint.id.as_str().to_owned(),
                by,
                cut: joint_cut(joint, feature, operation),
            }),
        }
        feature.definition.operation = operation;
    }
    if !unprojected.is_empty() {
        losses.push(F3dLossCode::AssemblyDesignJointUnprojected.note(format!(
            "{} Fusion joint feature(s) keep an unresolved operation: {}",
            unprojected.len(),
            unprojected.join(", ")
        )));
    }
    if !assumed.is_empty() {
        losses.push(F3dLossCode::AssemblyJointMotionAssumedRigid.note(format!(
            "{} Fusion joint feature(s) are stated rigid: two or more motion records bind to their scope: {}",
            assumed.len(),
            assumed.join(", ")
        )));
    }
    restated
}

/// State in `model` each restatement of `restated`, as [`project`] returns
/// it.
pub(super) fn record(model: &mut cadmpeg_ir::document::Model, restated: Vec<Restated>) {
    for Restated { legacy, by, cut } in restated {
        model
            .restated_mut()
            .record(&legacy, Restater::Record(by), cut);
    }
}

/// The fields of `joint` that `feature` leaves out beside its operation: the
/// suppression where it differs, the driving parameter, the drives, and the
/// scope record where the feature names another.
fn scalar_cut(joint: &AssemblyJoint, feature: &Feature) -> Vec<&'static str> {
    let suppressed = match &feature.suppressed {
        None => joint.suppressed,
        Some(Dimension::Literal(suppressed)) => *suppressed != joint.suppressed,
        Some(Dimension::Parameter(_) | Dimension::Negated(_)) => false,
    };
    [
        (SUPPRESSED, suppressed),
        (DRIVING_PARAMETER, joint.driving_parameter.is_some()),
        (DRIVES, !joint.drives.is_empty()),
        (
            NATIVE_REF,
            joint.native_ref.is_some() && joint.native_ref != feature.native_ref,
        ),
    ]
    .into_iter()
    .filter_map(|(field, cut)| cut.then_some(field))
    .collect()
}

/// The fields of `joint` that the design joint `operation` of `feature`
/// leaves out: the connectors where a connector states no frame.
fn joint_cut(
    joint: &AssemblyJoint,
    feature: &Feature,
    operation: &FeatureOperation,
) -> Vec<&'static str> {
    let mut cut = scalar_cut(joint, feature);
    let FeatureOperation::Joint(design) = operation else {
        return cut;
    };
    let framed = |connector: &JointConnector| match connector {
        JointConnector::Frame(construction) => {
            construction.resolved.is_some() || !construction.after.is_empty()
        }
        _ => false,
    };
    if !design.connectors.iter().all(framed) {
        cut.push(CONNECTORS);
    }
    cut
}

/// The fields of `member`, a joint of the as-built rigid group that
/// `feature` restates from `bound`, that the group leaves out: the operands
/// where the member is no fixed joint without limits at the group's
/// placement, or a connector of it is none of `placed`.
fn member_cut(
    member: &AssemblyJoint,
    bound: &AssemblyJoint,
    placed: &[&products::JointConnector],
    feature: &Feature,
) -> Vec<&'static str> {
    let mut cut = scalar_cut(member, feature);
    let stated = match (unlimited_fixed(member), unlimited_fixed(bound)) {
        (Some((kind, connectors)), Some((group_kind, _))) => {
            kind == group_kind
                && connectors
                    .iter()
                    .all(|connector| placed.contains(&connector))
        }
        _ => false,
    };
    if !stated {
        cut.push(OPERANDS);
    }
    cut
}

/// The context of one joint feature.
struct Site<'s, 'a> {
    /// The definition the feature's connector paths start at.
    owner: &'s ProductDefinitionId,
    /// Whether the paths reach through `owner` from a feature whose
    /// definition is another.
    in_context: bool,
    /// The scope's definition: the document root a `Root` operand names.
    scope: &'s ProductDefinitionId,
    /// The definition of each feature of the scope, by feature id.
    definitions: &'s HashMap<String, ProductDefinitionId>,
    inputs: &'s Inputs<'a>,
}

impl<'a> Site<'_, 'a> {
    /// The connectors `joint` places: its own two, and for a rigid group
    /// the second connector of each other joint of its scope.
    fn placed<'j>(
        &self,
        joint: &'j AssemblyJoint,
    ) -> Result<Vec<&'j products::JointConnector>, &'static str>
    where
        'a: 'j,
    {
        let mut connectors = pair(joint)?.1.iter().collect::<Vec<_>>();
        if let Some(scope) = self.inputs.scope(joint).filter(|scope| scope.source.group) {
            for member in scope.joints.iter().filter(|member| member.id != joint.id) {
                connectors.push(&pair(member)?.1[1]);
            }
        }
        Ok(connectors)
    }

    /// The context of a pick in `occurrence`, or below it in the occurrences
    /// the native keys `below` name and the document does not hold: the path
    /// from `owner`, through `owner` where the feature's definition is
    /// another.
    fn context(&self, occurrence: &OccurrenceId, below: &[String]) -> Option<PickContext> {
        let mut path = self.inputs.places.path(occurrence, self.owner)?;
        for key in below {
            path.0.push(PathStep {
                occurrence: StepRef::Native(native_name([key.as_str()])?),
                configuration: None,
            });
        }
        self.through(Some(path))
    }

    /// The context of a pick at the end of `path` from `owner`, or in
    /// `owner` itself: through `owner` where the feature's definition is
    /// another.
    fn through(&self, path: Option<OccurrencePath>) -> Option<PickContext> {
        if self.in_context {
            Some(PickContext::InContext {
                assembly: DefinitionRef {
                    definition: self.owner.clone(),
                    configuration: None,
                },
                own: None,
                target: path,
            })
        } else {
            path.map(PickContext::Occurrence)
        }
    }

    /// The definition a `Root` operand's object lies in: a datum feature's
    /// own definition, else the scope's definition, the document root.
    fn home(&self, operand: &products::JointOperand) -> &ProductDefinitionId {
        self.datum(operand).unwrap_or(self.scope)
    }

    /// The definition of the datum feature a `Root` operand names.
    fn datum(&self, operand: &products::JointOperand) -> Option<&ProductDefinitionId> {
        operand
            .subelements
            .is_empty()
            .then(|| self.definitions.get(&operand.object))
            .flatten()
    }

    /// The path from `owner` to the one occurrence of `home`; `None` where
    /// `home` is `owner`. `Err` where `owner` places `home` other than once.
    fn placement(&self, home: &ProductDefinitionId) -> Result<Option<OccurrencePath>, ()> {
        if home == self.owner {
            return Ok(None);
        }
        let places = &self.inputs.places;
        let mut paths = places
            .occurrences
            .iter()
            .filter(|occurrence| &occurrence.prototype == home)
            .filter_map(|occurrence| places.path(&occurrence.id, self.owner));
        match (paths.next(), paths.next()) {
            (Some(path), None) => Ok(Some(path)),
            _ => Err(()),
        }
    }

    /// Whether the definition at `owner` has a path to each occurrence that
    /// the connectors `joint` places name, and to the definition of each
    /// `Root` operand a joint picks.
    fn roots(&self, joint: &AssemblyJoint) -> bool {
        let group = self.inputs.source(joint).group;
        self.placed(joint).map_or(true, |connectors| {
            connectors
                .iter()
                .all(|connector| match &connector.operand.container {
                    OperandContainer::Occurrence { .. } => {
                        self.inputs.places.reach(&connector.operand).is_none_or(
                            |(occurrence, _)| {
                                self.inputs.places.path(&occurrence, self.owner).is_some()
                            },
                        )
                    }
                    OperandContainer::Root {} => {
                        group || self.placement(self.home(&connector.operand)).is_ok()
                    }
                    OperandContainer::External { .. } => true,
                })
        })
    }

    /// The design operation of `joint`.
    fn operation(&self, joint: &AssemblyJoint) -> Result<FeatureOperation, Cow<'static, str>> {
        let source = self.inputs.source(joint);
        let (kind, connectors) = pair(joint)?;
        if source.group {
            return self
                .rigid_group(joint)
                .map(|group| FeatureOperation::RigidGroup(Box::new(group)))
                .map_err(Cow::Borrowed);
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
                        joints: Some(self.coupled(coupled_joints)?),
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
                        motion: freedom.motion(),
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
            _ => return Err("its kind has no Fusion design form".into()),
        };
        if source.as_built {
            insert_text(&mut native_options, "joint_mode", "As-built".to_owned());
        }
        let [first, second] = connectors;
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

    /// The `Feature` picks of the joints a coupling acts on (§6.7
    /// Couplings): the design feature of the scope each joint binds. A joint
    /// that binds none is named by its scope record.
    fn coupled(&self, joints: &[JointId]) -> Result<OrderedSelection, Cow<'static, str>> {
        let picks = joints
            .iter()
            .map(|joint| {
                let feature = self
                    .inputs
                    .features
                    .get(joint)
                    .filter(|feature| self.definitions.contains_key(feature.as_str()))
                    .ok_or_else(|| {
                        let named = self
                            .inputs
                            .joints
                            .get(joint)
                            .and_then(|joint| joint.native_ref.as_deref())
                            .unwrap_or(joint.as_str());
                        format!("its coupled joint {named} has no design feature")
                    })?;
                Ok::<_, String>(Picked {
                    target: PickTarget::Feature((*feature).clone()),
                    context: None,
                    native: None,
                    domain: None,
                    witness: None,
                    resolved: None,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Selection {
            at: Anchor::Current,
            picks: Picks::Named(Members::new(picks)?),
            native: None,
        })
    }

    /// A connector on `connector`'s operand at `frame`: a joint origin's
    /// frame datum, a frame on an occurrence moved by `frame`, or the native
    /// operand at `frame`. The frame is in the coordinates of the document at
    /// the end of the operand's path; an occurrence path that goes below the
    /// occurrences the document holds ends in their native keys. A `Root`
    /// operand lies in the document root, or in its datum feature's own
    /// definition, and its pick reaches that definition from `owner`. An
    /// external document's placement is not one occurrence's, so its frame
    /// states no resolved value.
    fn connector(
        &self,
        connector: &products::JointConnector,
        frame: Transform,
    ) -> Option<JointConnector> {
        let operand = &connector.operand;
        let pick = |target, context, native| Picked {
            target,
            context,
            native,
            domain: None,
            witness: None,
            resolved: None,
        };
        let (pick, after, resolved) = match &operand.container {
            OperandContainer::Root {} => {
                let (target, native) = match self.datum(operand) {
                    Some(_) => (
                        PickTarget::Datum {
                            feature: FeatureId::mint(operand.object.clone()).ok()?,
                            part: None,
                        },
                        None,
                    ),
                    None => (
                        PickTarget::Native { within: None },
                        Some(native_name(stored_parts(operand))?),
                    ),
                };
                let home = self.home(operand);
                let context = match self.placement(home) {
                    Ok(path) => self.through(path),
                    Err(()) => Some(PickContext::Definition(DefinitionRef {
                        definition: home.clone(),
                        configuration: None,
                    })),
                };
                (pick(target, context, native), Vec::new(), true)
            }
            OperandContainer::Occurrence { .. } => {
                let (occurrence, below) = self.inputs.places.reach(operand)?;
                (
                    pick(
                        PickTarget::Component,
                        Some(self.context(&occurrence, &below)?),
                        Some(native_name([operand.object.as_str()])?),
                    ),
                    vec![PostStep::Fixed(frame)],
                    true,
                )
            }
            OperandContainer::External { .. } => (
                pick(
                    PickTarget::Native { within: None },
                    None,
                    Some(native_name(stored_parts(operand))?),
                ),
                vec![PostStep::Fixed(frame)],
                false,
            ),
        };
        Some(JointConnector::Frame(Construction {
            inputs: vec![input(pick)?],
            after,
            solution: None,
            mode: None,
            resolved: if resolved { frame_value(&frame) } else { None },
            resolved_in: BTreeMap::new(),
        }))
    }

    /// The rigid group of the as-built scope `joint` names: the occurrences
    /// its joints connect, with each member's locator frame and each
    /// unresolved member as native options.
    fn rigid_group(&self, joint: &AssemblyJoint) -> Result<RigidGroup, &'static str> {
        let connectors = self.placed(joint)?;
        let mut members = BTreeSet::new();
        let mut native_options = NativeOptions::new();
        for (ordinal, connector) in connectors.into_iter().enumerate() {
            let member = self
                .inputs
                .places
                .reach(&connector.operand)
                .filter(|(_, below)| below.is_empty())
                .and_then(|(occurrence, _)| self.inputs.places.path(&occurrence, self.owner));
            let named = match &member {
                Some(path) => steps(path),
                None => {
                    let listing = stored_parts(&connector.operand).join("/");
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
        if let Ok((
            PairedJointKind::Fixed {
                angle,
                translation_offset,
                ..
            },
            _,
        )) = pair(joint)
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

/// The kind and the connectors of a fixed joint without limits.
fn unlimited_fixed(
    joint: &AssemblyJoint,
) -> Option<(&PairedJointKind, &[products::JointConnector; 2])> {
    pair(joint).ok().filter(|(kind, _)| {
        matches!(
            kind,
            PairedJointKind::Fixed {
                angular_limits: None,
                linear_limits: None,
                ..
            }
        )
    })
}

/// The kind and the two connectors of a paired joint.
fn pair(
    joint: &AssemblyJoint,
) -> Result<(&PairedJointKind, &[products::JointConnector; 2]), &'static str> {
    match joint.operands() {
        JointOperands::Pair {
            kind, connectors, ..
        } => Ok((kind, connectors.as_ref())),
        JointOperands::Grounded { .. } => Err("a joint has one connector"),
    }
}

/// The freedoms a joint leaves, from its motion record's codes (§6.7, the
/// Fusion row).
#[derive(Clone, PartialEq, Debug)]
enum Freedom {
    Rigid,
    Rotation(FrameAxis),
    Translation(FrameAxis),
    Native(NonBlankString),
}

impl Freedom {
    /// The freedom of a motion record's codes; a motion record stores one
    /// code or more.
    fn of(codes: &[u32]) -> Self {
        match codes {
            [1] => Self::Rotation(FrameAxis::Y),
            [2] => Self::Rotation(FrameAxis::Z),
            [8] => Self::Translation(FrameAxis::Z),
            codes => Self::Native(nonblank_literal!(
                "f3d:freedom:{}",
                codes
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            )),
        }
    }

    fn motion(&self) -> JointMotion {
        match self {
            Self::Rigid => JointMotion::Rigid,
            Self::Rotation(axis) => JointMotion::Revolute { axis: *axis },
            Self::Translation(axis) => JointMotion::Slider { axis: *axis },
            Self::Native(name) => JointMotion::Native(name.clone()),
        }
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

/// Every stored value of an operand, outermost first: its occurrence or its
/// external document's identity, its object, and the native keys below them.
fn stored_parts(operand: &products::JointOperand) -> Vec<&str> {
    let (container, path) = match &operand.container {
        OperandContainer::Root {} => (None, &[][..]),
        OperandContainer::Occurrence {
            occurrence,
            occurrence_path,
        } => (Some(occurrence.as_str()), occurrence_path.as_slice()),
        OperandContainer::External { external_document } => (
            external_document
                .document_id
                .as_ref()
                .map(NonBlankString::as_str),
            &[][..],
        ),
    };
    container
        .into_iter()
        .chain(std::iter::once(operand.object.as_str()))
        .chain(path.iter().chain(&operand.subelements).map(String::as_str))
        .collect()
}

/// A native name of `parts` joined by `/`.
fn native_name<'s>(parts: impl IntoIterator<Item = &'s str>) -> Option<NativeName> {
    Some(NativeName {
        codec: cadmpeg_ir::codec_id!("f3d"),
        value: NativeNameValue::Text(NonBlankString::new(
            parts.into_iter().collect::<Vec<_>>().join("/"),
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
