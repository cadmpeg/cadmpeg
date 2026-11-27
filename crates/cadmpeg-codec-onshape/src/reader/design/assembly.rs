// SPDX-License-Identifier: Apache-2.0
//! The design scope of an Onshape Assembly (§5.1, §6.7).
//!
//! The root assembly definition is one scope whose ordered timeline holds a
//! `Joint` feature for each mate, in the order the assemblies list them, and
//! whose occurrences state which are fixed. Each connector is the mate's
//! solved coordinate system in the coordinates of the mated occurrence's
//! prototype: a `Whole` `Coincident` input on that occurrence's component,
//! moved by the solved frame. Onshape mates move along and about the
//! connector's z axis.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::construction::{
    kinds, Construction, ConstructionInput, FrameAxis, FrameValue, InputReference, InputRole,
    PostStep, Relation, Subject,
};
use cadmpeg_ir::design::container::Container;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::design::ops::shared::{
    Alignment, Joint, JointConnector, JointKind, JointMotion, JointOffset,
};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::scope::{DesignScope, OccurrenceDesign};
use cadmpeg_ir::design::selection::{
    Anchor, Members, OccurrencePath, PathStep, PickContext, PickTarget, Picked, Picks, Selection,
    StepRef,
};
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{JointId, JointOperands, PairedJointKind, PlaneSense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::CadIr;

/// The occurrence path of each connector of a mate, from the definition that
/// lists the mate, and the sub-assembly that lists it, where a sub-assembly
/// does.
#[derive(Debug)]
pub(in crate::reader) struct Paths {
    pub(in crate::reader) connectors: [Vec<OccurrenceId>; 2],
    pub(in crate::reader) component: Option<ProductDefinitionId>,
}

/// The paths of each mate, by its joint.
pub(in crate::reader) type JointPaths = BTreeMap<JointId, Paths>;

/// Project the design scope of the root assembly `root` of `ir`, whose
/// mates' connector paths are `paths`. An assembly with no mate and no fixed
/// occurrence states no scope.
///
/// # Errors
///
/// A resource refusal, or a design record the IR refuses.
pub(in crate::reader) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    root: &ProductDefinitionId,
    paths: &JointPaths,
) -> Result<(), CodecError> {
    let mut features = Vec::new();
    let mut grounded = BTreeSet::new();
    for joint in &ir.model.assembly_joints {
        match joint.operands() {
            JointOperands::Grounded { connector, .. } => {
                if let cadmpeg_ir::products::OperandContainer::Occurrence { occurrence, .. } =
                    &connector.operand.container
                {
                    grounded.insert(occurrence.clone());
                }
            }
            JointOperands::Pair {
                kind, connectors, ..
            } => {
                let Some(paths) = paths.get(&joint.id) else {
                    continue;
                };
                let connector = |index: usize| -> Result<JointConnector, CodecError> {
                    frame_connector(&connectors[index].frame, &paths.connectors[index])
                };
                features.push(Feature {
                    id: joint_feature(&joint.id)?,
                    name: None,
                    suppressed: Some(Dimension::Literal(joint.suppressed)),
                    definition: FeatureDefinition {
                        operation: FeatureOperation::Joint(Box::new(Joint {
                            connectors: [connector(0)?, connector(1)?],
                            kind: JointKind::Motion {
                                motion: motion(kind),
                                axes: None,
                                offset: JointOffset {
                                    x: None,
                                    y: None,
                                    z: None,
                                    angle: None,
                                },
                                limits: Vec::new(),
                                drives: Vec::new(),
                            },
                            alignment: joint.frame_sense().map(|sense| match sense {
                                PlaneSense::Aligned => Alignment::Aligned,
                                PlaneSense::Opposed => Alignment::Opposed,
                            }),
                            native_options: NativeOptions::new(),
                            source_settings: NativeOptions::new(),
                        })),
                        assembly: None,
                        refine: None,
                        fuzzy_tolerance: None,
                    },
                    placement: None,
                    component: paths.component.clone(),
                    driven_by: None,
                    native_ref: None,
                });
            }
        }
    }
    if features.is_empty() && grounded.is_empty() {
        return Ok(());
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(features.len()) + 2,
        "Onshape assembly design",
    )?;
    let container = super::super::id("container", "Assembly");
    let container_id: cadmpeg_ir::design::ids::ContainerId = container
        .clone()
        .try_into()
        .map_err(|_| CodecError::malformed("the assembly container id is not an id"))?;
    let roots = if features.is_empty() {
        Vec::new()
    } else {
        ir.design.containers.push(Container::Ordered {
            id: container_id.clone(),
            kind: cadmpeg_ir::design::container::OrderedKind::Timeline,
            members: features
                .iter()
                .map(|feature| Member::Feature(feature.id.clone()))
                .collect(),
            folders: Vec::new(),
            rollback: None,
            placement: None,
        });
        vec![container_id]
    };
    ir.design.features.extend(features);
    let spanned = ir.spanned_definitions(root);
    let unproduced = ir
        .model
        .product_definitions
        .iter()
        .filter(|definition| spanned.contains(definition.id.as_str()))
        .flat_map(|definition| definition.bodies.iter().cloned())
        .collect();
    let occurrences = grounded
        .into_iter()
        .map(|occurrence| {
            (
                occurrence,
                OccurrenceDesign {
                    configuration: None,
                    arrangements: Vec::new(),
                    grounded: Some(true),
                    flexible: None,
                    parameters: BTreeMap::new(),
                },
            )
        })
        .collect();
    ir.design.scopes.push(DesignScope {
        definition: root.clone(),
        roots,
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences,
        current: Vec::new(),
        unproduced,
    });
    Ok(())
}

/// The design feature id of the mate `joint`: the joint's own key.
fn joint_feature(joint: &JointId) -> Result<cadmpeg_ir::features::FeatureId, CodecError> {
    cadmpeg_ir::features::FeatureId::mint(joint.as_str())
        .map_err(|_| CodecError::malformed("the mate id is not a feature id"))
}

/// The motion an Onshape mate leaves, about or along the connector's z axis.
fn motion(kind: &PairedJointKind) -> JointMotion {
    match kind {
        PairedJointKind::Fixed { .. } => JointMotion::Rigid,
        PairedJointKind::Revolute { .. } => JointMotion::Revolute { axis: FrameAxis::Z },
        PairedJointKind::Slider { .. } => JointMotion::Slider { axis: FrameAxis::Z },
        PairedJointKind::Cylindrical { .. } => JointMotion::Cylindrical { axis: FrameAxis::Z },
        PairedJointKind::PlaneCoincidence { .. } => JointMotion::Planar { axis: FrameAxis::Z },
        PairedJointKind::Ball {} => JointMotion::Ball,
        _ => JointMotion::Native(cadmpeg_core::nonblank_const!("onshape:mate")),
    }
}

/// The frame connector at `frame` in the prototype at the end of `path`.
fn frame_connector(frame: &Transform, path: &[OccurrenceId]) -> Result<JointConnector, CodecError> {
    let resolved = frame_value(frame)?;
    let component = Picked {
        target: PickTarget::Component,
        context: Some(PickContext::Occurrence(OccurrencePath(
            path.iter()
                .map(|occurrence| PathStep {
                    occurrence: StepRef::Occurrence(occurrence.clone()),
                    configuration: None,
                })
                .collect(),
        ))),
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    };
    Ok(JointConnector::Frame(Construction::<kinds::Frame> {
        inputs: vec![ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            }),
            reference: Some(InputReference::Picks(Selection {
                at: Anchor::Current,
                picks: Picks::Named(
                    Members::new(vec![component])
                        .map_err(|reason| CodecError::malformed(reason.to_owned()))?,
                ),
                native: None,
            })),
            value: None,
            reversed: None,
        }],
        after: vec![PostStep::Fixed(*frame)],
        solution: None,
        mode: None,
        resolved: Some(resolved),
        resolved_in: BTreeMap::new(),
    }))
}

/// The frame of a rigid transform: its translation and its rotated axes.
fn frame_value(frame: &Transform) -> Result<FrameValue, CodecError> {
    let rows = frame.affine_rows();
    let axis = |column: usize| {
        UnitVector3::normalized(cadmpeg_ir::math::Vector3::new(
            rows[0][column],
            rows[1][column],
            rows[2][column],
        ))
        .ok_or_else(|| CodecError::malformed("the mate frame axis is degenerate"))
    };
    Ok(FrameValue {
        origin: cadmpeg_core::serde_erase::json::from_value(cadmpeg_core::json!({
            "x": rows[0][3], "y": rows[1][3], "z": rows[2][3]
        }))
        .map_err(|_| CodecError::malformed("the mate frame origin is not finite"))?,
        x_axis: axis(0)?,
        y_axis: axis(1)?,
        z_axis: axis(2)?,
    })
}
