// SPDX-License-Identifier: Apache-2.0
//! The declared joints of a composed layer (spec §18.3).
//!
//! A mate becomes an assembly joint whose connectors name their faces by IR
//! face identity, with the connector's witness frame. A mate that does not
//! resolve gives way to the `UsdPhysics` joints it holds.

use cadmpeg_ir::products::{
    AssemblyJoint, JointConnector, JointOperand, PairedJointKind, PlaneSense,
};
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
        "CadGroundMate" => MateType::Ground,
        _ => return None,
    })
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
        // The row frame's rows are the columns of the IR placement.
        let mut rows = [[0.0; 4]; 3];
        for (axis, row) in rows.iter_mut().enumerate() {
            for (column, value) in row.iter_mut().take(3).enumerate() {
                *value = frame[column][axis];
            }
            row[3] = frame[3][axis] * self.scale;
        }
        let local = Transform::affine(rows)?;
        let frame = match &entry.body_transform {
            Some(placement) => placement.compose(local).ok()?,
            None => local,
        };
        Some(Resolved {
            operand: JointOperand::occurrence(
                crate::ids::occurrence(&occurrence),
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
            let id = crate::ids::joint(&prim.path);
            let joint = if kind == MateType::Ground {
                single_target(prim, "cadMate:grounded")
                    .filter(|target| {
                        self.ir
                            .model
                            .occurrences
                            .iter()
                            .any(|occurrence| occurrence.id == crate::ids::occurrence(target))
                    })
                    .map(|target| {
                        AssemblyJoint::grounded(
                            id,
                            JointConnector {
                                operand: JointOperand::occurrence(
                                    crate::ids::occurrence(&target),
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
                    let paired = match kind {
                        MateType::Fastened => PairedJointKind::Fixed {
                            angle: None,
                            translation_offset: None,
                            angular_limits: None,
                            linear_limits: None,
                        },
                        MateType::Revolute => PairedJointKind::Revolute {
                            angle: None,
                            angular_limits: None,
                        },
                        MateType::Slider => PairedJointKind::Slider {
                            distance: None,
                            translation_offset: None,
                            linear_limits: None,
                        },
                        MateType::Cylindrical => PairedJointKind::Cylindrical {
                            angle: None,
                            distance: None,
                            angular_limits: None,
                            linear_limits: None,
                        },
                        MateType::Planar => PairedJointKind::PlaneCoincidence {
                            sense,
                            offset: None,
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
                        None,
                    );
                    joint.set_frame_sense(Some(sense));
                    Some(joint)
                })
            };
            match joint {
                Some(mut joint) => {
                    joint.suppressed = !enabled;
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
