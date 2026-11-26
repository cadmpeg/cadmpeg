// SPDX-License-Identifier: Apache-2.0
//! The design `Joint` feature of a `UsdPhysics` joint (CADIR §6.7), and the
//! assembly joint that the writer writes from a design joint.
//!
//! A design joint's connector frames are the joint's local frames, and its
//! motion names the joint's axis. An assembly joint's frames are turned so
//! that their z axis is the axis of a revolute or prismatic joint
//! ([`alignment`]); the turn is a cyclic permutation of the axes, so both
//! forms hold the same values.

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::construction::{
    Construction, ConstructionInput, FrameAxis, FrameValue, InputReference, InputRole, PostStep,
    Relation, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::ops::shared::{
    AxisLimits, Bounds, Coupling, DriveAxis, DriveMode, Joint, JointConnector, JointDrive,
    JointKind, JointMotion, JointOffset,
};
use cadmpeg_ir::design::selection::{
    Anchor, Members, OccurrencePath, OrderedSelection, PathStep, PickContext, PickTarget, Picked,
    Picks, Selection, StepRef,
};
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{
    self, AssemblyJoint, JointId, JointLimits, JointOperand, JointOperands, PairedJointKind,
};
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length, NonZeroReal};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::UnitVector3;

/// The prefix of the native motion of a generic joint whose degrees of
/// freedom have no neutral motion.
const NATIVE: &str = "usd:";

/// The index of an axis: X 0, Y 1, Z 2.
const fn index(axis: FrameAxis) -> usize {
    match axis {
        FrameAxis::X => 0,
        FrameAxis::Y => 1,
        FrameAxis::Z => 2,
    }
}

const fn axis_at(index: usize) -> FrameAxis {
    match index % 3 {
        0 => FrameAxis::X,
        1 => FrameAxis::Y,
        _ => FrameAxis::Z,
    }
}

/// The design axis of a `physics:axis` token.
pub(crate) fn source_axis(letter: char) -> FrameAxis {
    match letter {
        'X' => FrameAxis::X,
        'Y' => FrameAxis::Y,
        _ => FrameAxis::Z,
    }
}

/// The rotation that turns a joint frame so that its z axis is `axis`, by
/// a cyclic permutation of the axes.
pub(crate) fn alignment(axis: FrameAxis) -> crate::schema::xform::Affine {
    match axis {
        FrameAxis::X => [
            [0.0, 0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
        ],
        FrameAxis::Y => [
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
        ],
        FrameAxis::Z => crate::schema::xform::IDENTITY,
    }
}

/// The axis of a joint frame that axis `turned` of the frame turned to
/// `axis` is.
fn unturned(turned: products::FrameAxis, axis: FrameAxis) -> FrameAxis {
    let turned = match turned {
        products::FrameAxis::X => 0,
        products::FrameAxis::Y => 1,
        products::FrameAxis::Z => 2,
    };
    axis_at(turned + index(axis) + 1)
}

/// The axis of the frame turned to `axis` that axis `source` of the joint
/// frame is.
fn turned(source: FrameAxis, axis: FrameAxis) -> products::FrameAxis {
    match (index(source) + 5 - index(axis)) % 3 {
        0 => products::FrameAxis::X,
        1 => products::FrameAxis::Y,
        _ => products::FrameAxis::Z,
    }
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

/// One side of a joint: the occurrence path to its body from the joint's
/// definition, or `None` for the world, and its local frame relative to the
/// body or the world, in millimetres.
pub(crate) struct Side {
    pub(crate) body: Option<Vec<OccurrenceId>>,
    pub(crate) frame: Transform,
}

/// A connector: a frame on the whole occurrence of its body, moved by its
/// local frame, or the joint definition's frame so moved.
fn connector(side: Side) -> JointConnector {
    let inputs = match side.body {
        None => Vec::new(),
        Some(path) => {
            let pick = Picked {
                target: PickTarget::Component,
                context: Some(PickContext::Occurrence(OccurrencePath(
                    path.into_iter()
                        .map(|occurrence| PathStep {
                            occurrence: StepRef::Occurrence(occurrence),
                            configuration: None,
                        })
                        .collect(),
                ))),
                native: None,
                domain: None,
                witness: None,
                resolved: None,
            };
            vec![ConstructionInput {
                role: Some(InputRole {
                    subject: Subject::Whole,
                    relation: Relation::Coincident,
                }),
                reference: Members::new(vec![pick]).ok().map(|members| {
                    InputReference::Picks(Selection {
                        at: Anchor::Current,
                        picks: Picks::Named(members),
                        native: None,
                    })
                }),
                value: None,
                reversed: None,
            }]
        }
    };
    JointConnector::Frame(Construction {
        inputs,
        after: vec![PostStep::Fixed(side.frame)],
        solution: None,
        mode: None,
        resolved: frame_value(&side.frame),
        resolved_in: BTreeMap::new(),
    })
}

fn bounds<Q>(limits: Option<&JointLimits>, value: impl Fn(f64) -> Option<Q>) -> Option<Bounds<Q>> {
    let (min, max) = match limits? {
        JointLimits::Minimum { minimum } => (Some(minimum.get()), None),
        JointLimits::Maximum { maximum } => (None, Some(maximum.get())),
        JointLimits::Range(range) => (Some(range.minimum().get()), Some(range.maximum().get())),
    };
    let min = min.and_then(&value).map(Dimension::Literal);
    let max = max.and_then(&value).map(Dimension::Literal);
    (min.is_some() || max.is_some()).then_some(Bounds { min, max })
}

fn rotation(axis: FrameAxis, limits: Option<&JointLimits>) -> Option<AxisLimits> {
    bounds(limits, Angle::new).map(|limits| AxisLimits::Rotation {
        axis,
        limits: Some(limits),
        rest: None,
    })
}

fn translation(axis: FrameAxis, limits: Option<&JointLimits>) -> Option<AxisLimits> {
    bounds(limits, Length::new).map(|limits| AxisLimits::Translation {
        axis,
        limits: Some(limits),
        rest: None,
    })
}

/// The `Feature` picks of the joints a coupling acts on.
fn coupled(
    joints: &[JointId],
    feature: &dyn Fn(&JointId) -> Option<FeatureId>,
) -> Option<OrderedSelection> {
    if joints.len() != 2 {
        return None;
    }
    let picks = joints
        .iter()
        .map(|joint| {
            Some(Picked {
                target: PickTarget::Feature(feature(joint)?),
                context: None,
                native: None,
                domain: None,
                witness: None,
                resolved: None,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Selection {
        at: Anchor::Current,
        picks: Picks::Named(Members::new(picks).ok()?),
        native: None,
    })
}

fn motion(motion: JointMotion, limits: Vec<AxisLimits>, drives: Vec<JointDrive>) -> JointKind {
    JointKind::Motion {
        motion,
        axes: None,
        offset: JointOffset {
            x: None,
            y: None,
            z: None,
            angle: None,
        },
        limits,
        drives,
    }
}

/// The design joint of assembly joint `joint`, read from a joint whose
/// frames turn to `axis`, with `sides` its bodies and unturned frames in
/// source order. `feature` names the feature of a coupled joint. `None`
/// where the joint has no design form here: a coupling that names no two
/// joint features and has a world side.
pub(crate) fn design(
    joint: &AssemblyJoint,
    axis: FrameAxis,
    sides: [Side; 2],
    feature: &dyn Fn(&JointId) -> Option<FeatureId>,
) -> Option<Joint> {
    let drives = joint
        .drives
        .iter()
        .map(|drive| JointDrive {
            axis: match drive.axis {
                products::DriveAxis::Rotation { axis: turned } => DriveAxis::Rotation {
                    axis: unturned(turned, axis),
                },
                products::DriveAxis::Translation { axis: turned } => DriveAxis::Translation {
                    axis: unturned(turned, axis),
                },
            },
            mode: match drive.mode {
                products::DriveMode::Force => DriveMode::Force,
                products::DriveMode::Acceleration => DriveMode::Acceleration,
            },
            target_position: drive.target_position,
            target_velocity: drive.target_velocity,
            stiffness: drive.stiffness,
            damping: drive.damping,
            max_force: drive.max_force,
        })
        .collect();
    let world = sides.iter().any(|side| side.body.is_none());
    let kind = match joint.operands() {
        JointOperands::Grounded { .. } => motion(JointMotion::Rigid, Vec::new(), drives),
        JointOperands::Pair { kind, .. } => match kind {
            PairedJointKind::Fixed { .. } => motion(JointMotion::Rigid, Vec::new(), drives),
            PairedJointKind::Revolute { angular_limits, .. } => motion(
                JointMotion::Revolute { axis },
                rotation(axis, angular_limits.as_ref())
                    .into_iter()
                    .collect(),
                drives,
            ),
            PairedJointKind::Slider { linear_limits, .. } => motion(
                JointMotion::Slider { axis },
                translation(axis, linear_limits.as_ref())
                    .into_iter()
                    .collect(),
                drives,
            ),
            PairedJointKind::Cylindrical {
                angular_limits,
                linear_limits,
                ..
            } => motion(
                JointMotion::Cylindrical { axis },
                rotation(axis, angular_limits.as_ref())
                    .into_iter()
                    .chain(translation(axis, linear_limits.as_ref()))
                    .collect(),
                drives,
            ),
            PairedJointKind::Ball {} => motion(JointMotion::Ball, Vec::new(), drives),
            PairedJointKind::Native { name, .. } => motion(
                JointMotion::Native(NonBlankString::new(format!("{NATIVE}{name}"))?),
                Vec::new(),
                drives,
            ),
            PairedJointKind::Distance { distance } => JointKind::Distance {
                distance: distance
                    .and_then(|distance| Length::new(distance.get()))
                    .map(Dimension::Literal),
                limits: None,
            },
            PairedJointKind::Gears {
                distance,
                distance2,
                ratio,
                coupled_joints,
            } => {
                let joints = coupled(coupled_joints, feature);
                if joints.is_none() && world {
                    return None;
                }
                let radius = |value: &Option<FiniteReal>| {
                    value
                        .and_then(|value| Length::new(value.get()))
                        .map(Dimension::Literal)
                };
                JointKind::Coupling {
                    coupling: Coupling::Gear {
                        ratio: ratio
                            .and_then(|ratio| FiniteReal::new(ratio.get()))
                            .map(Dimension::Literal),
                        radii: [radius(distance), radius(distance2)],
                    },
                    joints,
                }
            }
            PairedJointKind::RackPinion {
                ratio,
                coupled_joints,
                ..
            } => {
                let joints = coupled(coupled_joints, feature);
                if joints.is_none() && world {
                    return None;
                }
                JointKind::Coupling {
                    coupling: Coupling::RackPinion {
                        ratio: ratio
                            .and_then(|ratio| FiniteReal::new(ratio.get()))
                            .map(Dimension::Literal),
                        pitch_radius: None,
                    },
                    joints,
                }
            }
            _ => return None,
        },
    };
    let [first, second] = sides;
    Some(Joint {
        connectors: [connector(first), connector(second)],
        kind,
        alignment: None,
        native_options: BTreeMap::default(),
        source_settings: BTreeMap::default(),
    })
}

/// The occurrence and the local frame of a connector, or `None` for the
/// world, read back from [`connector`]'s form; `Err` for another form.
fn side(connector: &JointConnector) -> Result<(Option<OccurrenceId>, Transform), ()> {
    let JointConnector::Frame(construction) = connector else {
        return Err(());
    };
    let mut frame = Transform::identity();
    for step in &construction.after {
        let PostStep::Fixed(step) = step else {
            return Err(());
        };
        frame = frame.compose(*step).map_err(|_| ())?;
    }
    let body = match construction.inputs.as_slice() {
        [] => None,
        [input] => {
            let Some(InputReference::Picks(selection)) = &input.reference else {
                return Err(());
            };
            let Picks::Named(members) = &selection.picks else {
                return Err(());
            };
            match members.as_slice() {
                [Picked {
                    target: PickTarget::Component,
                    context: Some(PickContext::Occurrence(OccurrencePath(path))),
                    ..
                }] => match path.last() {
                    Some(PathStep {
                        occurrence: StepRef::Occurrence(occurrence),
                        ..
                    }) => Some(occurrence.clone()),
                    _ => return Err(()),
                },
                _ => return Err(()),
            }
        }
        _ => return Err(()),
    };
    Ok((body, frame))
}

fn literal<T: Copy>(dimension: Option<&Dimension<T>>, dropped: &mut usize) -> Option<T> {
    match dimension? {
        Dimension::Literal(value) => Some(*value),
        _ => {
            *dropped += 1;
            None
        }
    }
}

fn limits<Q: Copy>(
    bounds: Option<&Bounds<Q>>,
    value: impl Fn(Q) -> f64,
    dropped: &mut usize,
) -> Option<JointLimits> {
    let bounds = bounds?;
    let min = literal(bounds.min.as_ref(), dropped).map(&value);
    let max = literal(bounds.max.as_ref(), dropped).map(&value);
    JointLimits::new(min, max)
}

/// An assembly joint that the writer writes, and the count of the joint's
/// values it has no place for.
pub(crate) struct Assembly {
    pub(crate) joint: AssemblyJoint,
    pub(crate) dropped: usize,
}

/// The assembly joint of design joint `joint` of feature `id`, with frames
/// turned to the axis of its motion. `None` where the joint has no
/// assembly-joint form: a connector of another form than [`design`] states,
/// or a kind with no `UsdPhysics` joint.
pub(crate) fn assembly(id: &FeatureId, joint: &Joint) -> Option<Assembly> {
    let mut dropped = 0;
    let mut sides = Vec::new();
    for connector in &joint.connectors {
        sides.push(side(connector).ok()?);
    }
    let coupled = |joints: &Option<OrderedSelection>| -> Vec<JointId> {
        let Some(Picks::Named(members)) = joints.as_ref().map(|joints| &joints.picks) else {
            return Vec::new();
        };
        members
            .as_slice()
            .iter()
            .filter_map(|pick| match &pick.target {
                PickTarget::Feature(feature) => JointId::mint(feature.as_str()).ok(),
                _ => None,
            })
            .collect()
    };
    let (kind, axis, drives) = match &joint.kind {
        JointKind::Motion {
            motion,
            axes,
            offset,
            limits: axis_limits,
            drives,
        } => {
            if axes.is_some() {
                return None;
            }
            dropped += [
                offset.x.is_some(),
                offset.y.is_some(),
                offset.z.is_some(),
                offset.angle.is_some(),
            ]
            .into_iter()
            .filter(|stated| *stated)
            .count();
            let axis = match motion {
                JointMotion::Revolute { axis }
                | JointMotion::Slider { axis }
                | JointMotion::Cylindrical { axis } => *axis,
                _ => FrameAxis::Z,
            };
            let mut angular = None;
            let mut linear = None;
            for limit in axis_limits {
                match limit {
                    AxisLimits::Rotation {
                        axis: limited,
                        limits: bounds,
                        rest,
                    } if *limited == axis && angular.is_none() => {
                        angular = limits(bounds.as_ref(), Angle::get, &mut dropped);
                        dropped += usize::from(rest.is_some());
                    }
                    AxisLimits::Translation {
                        axis: limited,
                        limits: bounds,
                        rest,
                    } if *limited == axis && linear.is_none() => {
                        linear = limits(bounds.as_ref(), Length::get, &mut dropped);
                        dropped += usize::from(rest.is_some());
                    }
                    _ => dropped += 1,
                }
            }
            let kind = match motion {
                JointMotion::Rigid => PairedJointKind::Fixed {
                    angle: None,
                    translation_offset: None,
                    angular_limits: None,
                    linear_limits: None,
                },
                JointMotion::Revolute { .. } => PairedJointKind::Revolute {
                    angle: None,
                    angular_limits: angular,
                },
                JointMotion::Slider { .. } => PairedJointKind::Slider {
                    distance: None,
                    translation_offset: None,
                    linear_limits: linear,
                },
                JointMotion::Cylindrical { .. } => PairedJointKind::Cylindrical {
                    angle: None,
                    distance: None,
                    angular_limits: angular,
                    linear_limits: linear,
                },
                JointMotion::Ball => PairedJointKind::Ball {},
                JointMotion::Native(name) => PairedJointKind::Native {
                    name: name
                        .as_str()
                        .strip_prefix(NATIVE)
                        .unwrap_or(name.as_str())
                        .to_owned(),
                    angle: None,
                    translation_offset: None,
                    distance: None,
                    distance2: None,
                    angular_limits: None,
                    linear_limits: None,
                },
                JointMotion::Planar { .. } | JointMotion::PinSlot { .. } => return None,
            };
            (kind, axis, drives.as_slice())
        }
        JointKind::Distance {
            distance,
            limits: range,
        } => {
            dropped += usize::from(range.is_some());
            let distance = literal(distance.as_ref(), &mut dropped)
                .and_then(|distance| FiniteReal::new(distance.get()));
            (
                PairedJointKind::Distance { distance },
                FrameAxis::Z,
                &[][..],
            )
        }
        JointKind::Coupling { coupling, joints } => {
            let ratio = |ratio: &Option<Dimension<FiniteReal>>, dropped: &mut usize| {
                literal(ratio.as_ref(), dropped).and_then(|ratio| NonZeroReal::new(ratio.get()))
            };
            let length = |length: &Option<Dimension<Length>>, dropped: &mut usize| {
                literal(length.as_ref(), dropped).and_then(|length| FiniteReal::new(length.get()))
            };
            let kind = match coupling {
                Coupling::Gear {
                    ratio: gear_ratio,
                    radii,
                } => PairedJointKind::Gears {
                    distance: length(&radii[0], &mut dropped),
                    distance2: length(&radii[1], &mut dropped),
                    ratio: ratio(gear_ratio, &mut dropped),
                    coupled_joints: coupled(joints),
                },
                Coupling::RackPinion {
                    ratio: rack_ratio,
                    pitch_radius,
                } => PairedJointKind::RackPinion {
                    distance: length(pitch_radius, &mut dropped),
                    distance2: None,
                    ratio: ratio(rack_ratio, &mut dropped),
                    coupled_joints: coupled(joints),
                },
                _ => return None,
            };
            (kind, FrameAxis::Z, &[][..])
        }
        _ => return None,
    };
    let turn = Transform::affine(alignment(axis))?;
    let mut connectors = Vec::new();
    for (body, frame) in sides {
        let frame = frame.compose(turn).ok()?;
        connectors.push((body, frame));
    }
    let connector = |(body, frame): (Option<OccurrenceId>, Transform)| products::JointConnector {
        operand: match body {
            Some(occurrence) => JointOperand::occurrence(occurrence, String::new(), Vec::new()),
            None => JointOperand::root("/", Vec::new()),
        },
        frame,
        detached: false,
    };
    let joint_id = JointId::mint(id.as_str()).ok()?;
    let world = connectors.iter().filter(|(body, _)| body.is_none()).count();
    let fixed = matches!(kind, PairedJointKind::Fixed { .. });
    let mut written = match (fixed, world) {
        (_, 2) => return None,
        (true, 1) => {
            let (grounded, world): (Vec<_>, Vec<_>) =
                connectors.into_iter().partition(|(body, _)| body.is_some());
            let grounded = grounded.into_iter().next()?;
            let world = world.into_iter().next().map(|(_, frame)| frame);
            AssemblyJoint::grounded(joint_id, connector(grounded), world)
        }
        _ => {
            let mut connectors = connectors.into_iter();
            let (first, second) = (connectors.next()?, connectors.next()?);
            AssemblyJoint::paired(joint_id, kind, [connector(first), connector(second)], None)
        }
    };
    for drive in drives {
        written.drives.push(products::JointDrive {
            axis: match drive.axis {
                DriveAxis::Rotation { axis: source } => products::DriveAxis::Rotation {
                    axis: turned(source, axis),
                },
                DriveAxis::Translation { axis: source } => products::DriveAxis::Translation {
                    axis: turned(source, axis),
                },
            },
            mode: match drive.mode {
                DriveMode::Force => products::DriveMode::Force,
                DriveMode::Acceleration => products::DriveMode::Acceleration,
            },
            target_position: drive.target_position,
            target_velocity: drive.target_velocity,
            stiffness: drive.stiffness,
            damping: drive.damping,
            max_force: drive.max_force,
        });
    }
    Some(Assembly {
        joint: written,
        dropped,
    })
}

#[cfg(test)]
mod tests;
