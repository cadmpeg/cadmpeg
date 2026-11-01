// SPDX-License-Identifier: Apache-2.0
//! The declared joints of a composed layer (spec §18.3).
//!
//! A mate becomes an assembly joint whose connectors name their faces by IR
//! face identity, with the connector's witness frame. A mate that does not
//! resolve gives way to the `UsdPhysics` joints it holds.

use cadmpeg_ir::products::{
    AssemblyJoint, JointConnector, JointId, JointLimits, JointOperand, PairedJointKind, PlaneSense,
    TangentContact,
};
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use cadmpeg_ir::transform::Transform;

use crate::brep::frame::{OriginRule, RowFrame};
use crate::schema::{boolean, token, vector};
use crate::stage::Prim;

/// `cadMate:tolerance` when it is not authored.
const DEFAULT_TOLERANCE: f64 = 1e-6;

/// The mate types (spec §18.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MateType {
    Fastened,
    Revolute,
    Slider,
    Cylindrical,
    Planar,
    Ball,
    Distance,
    Parallel,
    Perpendicular,
    Angle,
    Concentric,
    PointOnAxis,
    Insert,
    Tangent,
    Screw,
    Gear,
    RackPinion,
    Belt,
    Native,
    Ground,
}

fn mate_type(type_name: Option<&str>) -> Option<MateType> {
    Some(match type_name? {
        "CadFastenedMate" => MateType::Fastened,
        "CadRevoluteMate" => MateType::Revolute,
        "CadSliderMate" => MateType::Slider,
        "CadCylindricalMate" => MateType::Cylindrical,
        "CadPlanarMate" => MateType::Planar,
        "CadBallMate" => MateType::Ball,
        "CadDistanceMate" => MateType::Distance,
        "CadParallelMate" => MateType::Parallel,
        "CadPerpendicularMate" => MateType::Perpendicular,
        "CadAngleMate" => MateType::Angle,
        "CadConcentricMate" => MateType::Concentric,
        "CadPointOnAxisMate" => MateType::PointOnAxis,
        "CadInsertMate" => MateType::Insert,
        "CadTangentMate" => MateType::Tangent,
        "CadScrewMate" => MateType::Screw,
        "CadGearMate" => MateType::Gear,
        "CadRackPinionMate" => MateType::RackPinion,
        "CadBeltMate" => MateType::Belt,
        "CadNativeMate" => MateType::Native,
        "CadGroundMate" => MateType::Ground,
        _ => return None,
    })
}

/// The value attributes of a mate prim, in IR units (spec §18.3).
#[derive(Default)]
struct Values {
    angle: Option<FiniteReal>,
    distance: Option<FiniteReal>,
    distance2: Option<FiniteReal>,
    offset: Option<FiniteReal>,
    ratio: Option<f64>,
    translation: Option<[FiniteReal; 3]>,
    angular_limits: Option<JointLimits>,
    linear_limits: Option<JointLimits>,
    contact: Option<TangentContact>,
    family: Option<String>,
    coupled: Vec<JointId>,
}

/// Whether a prim type is a mate type.
pub(super) fn is_mate_type(type_name: Option<&str>) -> bool {
    mate_type(type_name).is_some()
}

/// The rows of a row-vector `matrix4d`, without their last column.
fn row_frame(values: &[f64]) -> Option<RowFrame> {
    let values = <[f64; 16]>::try_from(values).ok()?;
    Some([0, 1, 2, 3].map(|row| [0, 1, 2].map(|column| values[4 * row + column])))
}

/// `left · right` for row frames.
fn product(left: &RowFrame, right: &RowFrame) -> RowFrame {
    let mut out = [[0.0; 3]; 4];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (column, value) in out_row.iter_mut().enumerate() {
            *value = (0..3).map(|k| left[row][k] * right[k][column]).sum();
        }
    }
    for column in 0..3 {
        out[3][column] += right[3][column];
    }
    out
}

fn single_target(prim: &Prim<'_>, name: &str) -> Option<String> {
    match prim.relationship_targets(name).as_slice() {
        [target] => Some(target.clone()),
        _ => None,
    }
}

/// A resolved connector: its operand, its frame in the definition of its
/// occurrence, and whether its rule is `faceFrame`.
struct Resolved {
    operand: JointOperand,
    frame: Transform,
    face_frame: bool,
}

impl super::Builder<'_, '_, '_, '_> {
    /// The IR placement of row frame `frame`, with its origin in millimetres.
    fn row_transform(&self, frame: &RowFrame) -> Option<Transform> {
        // The row frame's rows are the columns of the IR placement.
        let mut rows = [[0.0; 4]; 3];
        for (axis, row) in rows.iter_mut().enumerate() {
            for (column, value) in row.iter_mut().take(3).enumerate() {
                *value = frame[column][axis];
            }
            row[3] = frame[3][axis] * self.scale;
        }
        Transform::affine(rows)
    }

    /// The attachment offsets of mate `prim`, when both are authored.
    fn attachment_offsets(&mut self, prim: &Prim<'_>) -> Option<[Transform; 2]> {
        let mut offset = |name: &str| {
            vector(prim, name, 16, &mut self.issues).and_then(|values| row_frame(&values))
        };
        let (first, second) = (
            offset("cadMate:attachmentOffset0")?,
            offset("cadMate:attachmentOffset1")?,
        );
        Some([self.row_transform(&first)?, self.row_transform(&second)?])
    }

    /// The drives of the `UsdPhysics` joint that mate `prim` holds.
    fn mate_drives(&mut self, prim: &Prim<'_>) -> Vec<cadmpeg_ir::products::JointDrive> {
        let stage = self.stage;
        let mut out = Vec::new();
        for child in prim
            .children
            .iter()
            .filter_map(|child| stage.prims.get(*child))
        {
            let Some(kind) = child
                .type_name()
                .and_then(crate::schema::physics::JointType::of)
            else {
                continue;
            };
            let data = crate::schema::physics::joint(child, kind, &mut self.issues);
            for drive in &data.drives {
                match self.drive(data.kind, drive) {
                    Some(drive) => out.push(drive),
                    None => self
                        .tallies
                        .joint_properties
                        .push(format!("{}: drive {}", child.path, drive.instance)),
                }
            }
        }
        out
    }

    /// The value attributes of mate `prim`.
    fn mate_values(&mut self, prim: &Prim<'_>) -> Values {
        let scale = self.scale;
        let mut scalar = |name: &str| {
            vector(prim, &format!("cadMate:{name}"), 1, &mut self.issues)
                .and_then(|value| value.first().copied())
        };
        let length = |value: f64| FiniteReal::new(value * scale);
        let angle = |value: f64| FiniteReal::new(value.to_radians());
        let mut values = Values {
            angle: scalar("angle").and_then(angle),
            distance: scalar("distance").and_then(length),
            distance2: scalar("distance2").and_then(length),
            offset: scalar("offset").and_then(length),
            ratio: scalar("ratio"),
            ..Values::default()
        };
        values.translation = vector(prim, "cadMate:translationOffset", 3, &mut self.issues)
            .and_then(|value| {
                let [x, y, z] = <[f64; 3]>::try_from(value.as_slice()).ok()?;
                Some([length(x)?, length(y)?, length(z)?])
            });
        let mut limits = |name: &str, convert: &dyn Fn(f64) -> f64| {
            let value = vector(prim, &format!("cadMate:{name}"), 2, &mut self.issues)?;
            let bound = |value: f64| (!value.is_nan()).then(|| convert(value));
            JointLimits::new(bound(*value.first()?), bound(*value.get(1)?))
        };
        values.angular_limits = limits("angularLimits", &f64::to_radians);
        values.linear_limits = limits("linearLimits", &|value| value * scale);
        values.contact = match token(prim, "cadMate:contact", &mut self.issues).as_deref() {
            Some("outside") => Some(TangentContact::Outside),
            Some("inside") => Some(TangentContact::Inside),
            _ => None,
        };
        values.family = crate::schema::token(prim, "cadMate:family", &mut self.issues);
        values.coupled = crate::schema::tokens(prim, "cadMate:coupledJoints", &mut self.issues)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| JointId::mint(id).ok())
            .collect();
        values
    }

    /// The occurrence path and definition key of the `BrepArray` prim at
    /// `path`: its folding group, or the prim itself.
    fn brep_definition(&mut self, path: &str) -> Option<(String, String)> {
        let stage = self.stage;
        let index = stage.prims.iter().position(|prim| prim.path == path)?;
        let prim = stage.prims.get(index)?;
        let folded = prim
            .parent
            .and_then(|parent| stage.prims.get(parent))
            .filter(|parent| self.folded.get(&parent.path) == Some(&index));
        let holder = folded.unwrap_or(prim);
        let key = self.definition_key(holder);
        Some((holder.path.clone(), key))
    }

    fn connector(&mut self, path: &str) -> Option<Resolved> {
        let stage = self.stage;
        let connector = stage
            .find(path)
            .filter(|prim| prim.type_name() == Some("CadConnector"))?;
        let body = single_target(connector, "cadConnector:body")?;
        let face = crate::schema::integer(connector, "cadConnector:faceId", &mut self.issues)
            .and_then(|id| i64::try_from(id).ok())
            .filter(|id| *id != -1)?;
        let rule = match token(connector, "cadConnector:origin", &mut self.issues).as_deref() {
            None | Some("faceFrame") => OriginRule::FaceFrame,
            Some("axis") => OriginRule::Axis {
                high: token(connector, "cadConnector:axisEnd", &mut self.issues).as_deref()
                    == Some("high"),
            },
            Some("centre") => OriginRule::Centre,
            Some(_) => return None,
        };
        let offset = vector(connector, "cadConnector:offset", 16, &mut self.issues)
            .and_then(|values| row_frame(&values))
            .unwrap_or([
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0],
            ]);
        // A `BrepArray` carrier is NURBS, so the witness frame is the frame
        // of the origin rule (spec §18.3).
        let witness = vector(
            connector,
            "cadConnector:witness:frame",
            16,
            &mut self.issues,
        )
        .and_then(|values| row_frame(&values))?;
        let (occurrence, key) = self.brep_definition(&body)?;
        let entry = self.brep_entries.get(&key)?;
        let face_ir = entry.faces.get(&face)?.clone();
        let frame = product(&offset, &witness);
        let local = self.row_transform(&frame)?;
        let frame = match &entry.body_transform {
            Some(placement) => placement.compose(local).ok()?,
            None => local,
        };
        Some(Resolved {
            operand: JointOperand::occurrence(
                self.occurrence_id(&occurrence),
                body,
                vec![face_ir.to_string()],
            ),
            frame,
            face_frame: rule == OriginRule::FaceFrame,
        })
    }

    /// Adds the mates collected during the traversal.
    pub(super) fn mates(&mut self, pending: Vec<usize>) -> Result<(), cadmpeg_core::CodecError> {
        for index in pending {
            let stage = self.stage;
            let Some(prim) = stage.prims.get(index) else {
                continue;
            };
            let Some(kind) = mate_type(prim.type_name()) else {
                continue;
            };
            let enabled = boolean(prim, "cadMate:enabled", &mut self.issues).unwrap_or(true);
            let tolerance = vector(prim, "cadMate:tolerance", 1, &mut self.issues)
                .and_then(|value| value.first().copied());
            if tolerance.is_some_and(|value| value != DEFAULT_TOLERANCE) {
                self.tallies
                    .joint_properties
                    .push(format!("{}: cadMate:tolerance", prim.path));
            }
            let id = self.joint_id(&prim.path);
            let values = self.mate_values(prim);
            let offsets = self.attachment_offsets(prim);
            let drives = self.mate_drives(prim);
            let scale = self.scale;
            let driving =
                token(prim, "cadMate:drivingParameter", &mut self.issues).and_then(|text| {
                    self.ir
                        .model
                        .parameters
                        .iter()
                        .find(|parameter| parameter.id.as_str() == text)
                        .map(|parameter| parameter.id.clone())
                });
            let joint = if kind == MateType::Ground {
                single_target(prim, "cadMate:grounded")
                    .filter(|target| {
                        self.ir
                            .model
                            .occurrences
                            .iter()
                            .any(|occurrence| occurrence.id == self.occurrence_id(target))
                    })
                    .map(|target| {
                        AssemblyJoint::grounded(
                            id,
                            JointConnector {
                                operand: JointOperand::occurrence(
                                    self.occurrence_id(&target),
                                    target,
                                    Vec::new(),
                                ),
                                frame: Transform::identity(),
                                detached: false,
                            },
                            None,
                        )
                    })
            } else {
                let ends = [
                    single_target(prim, "cadMate:connector0"),
                    single_target(prim, "cadMate:connector1"),
                ];
                let resolved = ends
                    .iter()
                    .map(|end| end.as_deref().and_then(|path| self.connector(path)))
                    .collect::<Option<Vec<_>>>();
                resolved.and_then(|resolved| {
                    let [first, second] = <[Resolved; 2]>::try_from(resolved).ok()?;
                    let flip = boolean(prim, "cadMate:flip", &mut self.issues).unwrap_or(false);
                    let opposed = (first.face_frame && second.face_frame) != flip;
                    let sense = if opposed {
                        PlaneSense::Opposed
                    } else {
                        PlaneSense::Aligned
                    };
                    let ratio = |value: Option<f64>| value.and_then(PositiveReal::new);
                    let paired = match kind {
                        MateType::Fastened => PairedJointKind::Fixed {
                            angle: values.angle,
                            translation_offset: values.translation,
                            angular_limits: values.angular_limits,
                            linear_limits: values.linear_limits,
                        },
                        MateType::Revolute => PairedJointKind::Revolute {
                            angle: values.angle,
                            angular_limits: values.angular_limits,
                        },
                        MateType::Slider => PairedJointKind::Slider {
                            distance: values.distance,
                            translation_offset: values.translation,
                            linear_limits: values.linear_limits,
                        },
                        MateType::Cylindrical => PairedJointKind::Cylindrical {
                            angle: values.angle,
                            distance: values.distance,
                            angular_limits: values.angular_limits,
                            linear_limits: values.linear_limits,
                        },
                        MateType::Planar => PairedJointKind::PlaneCoincidence {
                            sense,
                            offset: values.offset,
                        },
                        MateType::Distance => PairedJointKind::Distance {
                            distance: values.distance,
                        },
                        MateType::Parallel => PairedJointKind::Parallel {},
                        MateType::Perpendicular => PairedJointKind::Perpendicular {},
                        MateType::Angle => PairedJointKind::Angle {
                            angle: values.angle,
                        },
                        MateType::Concentric => PairedJointKind::AxisCoincidence {},
                        MateType::PointOnAxis => PairedJointKind::PointOnAxis {
                            distance: values.distance,
                        },
                        MateType::Insert => PairedJointKind::Insert {
                            sense,
                            offset: values.offset,
                        },
                        MateType::Tangent => PairedJointKind::Tangent {
                            contact: values.contact?,
                        },
                        MateType::Screw => PairedJointKind::Screw {
                            distance: values.distance,
                        },
                        MateType::Gear => PairedJointKind::Gears {
                            distance: values.distance,
                            distance2: values.distance2,
                            ratio: ratio(values.ratio),
                            coupled_joints: values.coupled,
                        },
                        MateType::RackPinion => PairedJointKind::RackPinion {
                            distance: values.distance,
                            distance2: values.distance2,
                            ratio: ratio(values.ratio.map(|value| value.to_radians() / scale)),
                            coupled_joints: values.coupled,
                        },
                        MateType::Belt => PairedJointKind::Belt {
                            distance: values.distance,
                            distance2: values.distance2,
                        },
                        MateType::Native => PairedJointKind::Native {
                            name: values.family?,
                            angle: values.angle,
                            translation_offset: values.translation,
                            distance: values.distance,
                            distance2: values.distance2,
                            angular_limits: values.angular_limits,
                            linear_limits: values.linear_limits,
                        },
                        MateType::Ball | MateType::Ground => PairedJointKind::Ball {},
                    };
                    let mut joint = AssemblyJoint::paired(
                        id,
                        paired,
                        [
                            JointConnector {
                                operand: first.operand,
                                frame: first.frame,
                                detached: false,
                            },
                            JointConnector {
                                operand: second.operand,
                                frame: second.frame,
                                detached: false,
                            },
                        ],
                        offsets,
                    );
                    joint.set_frame_sense(Some(sense));
                    Some(joint)
                })
            };
            match joint {
                Some(mut joint) => {
                    joint.suppressed = !enabled;
                    joint.driving_parameter = driving;
                    joint.drives = drives;
                    joint.native_ref = Some(prim.path.clone());
                    self.ir.model.assembly_joints.push(joint);
                    self.admit("usd_mate")?;
                }
                None => {
                    self.tallies.rejected_joints.push(prim.path.clone());
                    for child in prim.children.clone() {
                        self.collect_unplaced(child, &crate::schema::xform::IDENTITY)?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl super::Builder<'_, '_, '_, '_> {
    /// Drops from each coupling the coupled joints that the stage does not
    /// give, after every joint is read.
    pub(super) fn couplings(&mut self) {
        let held: std::collections::BTreeSet<JointId> = self
            .ir
            .model
            .assembly_joints
            .iter()
            .map(|joint| joint.id.clone())
            .collect();
        for joint in &mut self.ir.model.assembly_joints {
            let cadmpeg_ir::products::JointOperands::Pair {
                kind,
                connectors,
                offset_frames,
                frame_sense,
            } = joint.operands()
            else {
                continue;
            };
            let mut kind = kind.clone();
            let (PairedJointKind::Gears { coupled_joints, .. }
            | PairedJointKind::RackPinion { coupled_joints, .. }) = &mut kind
            else {
                continue;
            };
            let listed = coupled_joints.len();
            coupled_joints.retain(|coupled| held.contains(coupled));
            if coupled_joints.len() == listed {
                continue;
            }
            self.tallies.joint_properties.push(format!(
                "{}: {} coupled joints",
                joint.id,
                listed - coupled_joints.len()
            ));
            let mut rebuilt = AssemblyJoint::paired(
                joint.id.clone(),
                kind,
                (**connectors).clone(),
                *offset_frames,
            );
            rebuilt.set_frame_sense(*frame_sense);
            rebuilt.suppressed = joint.suppressed;
            rebuilt.driving_parameter = joint.driving_parameter.take();
            rebuilt.drives = std::mem::take(&mut joint.drives);
            rebuilt.native_ref = joint.native_ref.take();
            *joint = rebuilt;
        }
    }
}
