// SPDX-License-Identifier: Apache-2.0
//! Writes assembly joints between faces of written B-reps as mates
//! (spec §18.3).
//!
//! A joint whose connectors name faces of written `BrepArray` prims becomes a
//! mate of the `Physics` scope, with its connectors in the `Connectors`
//! scope. The connector offset is the IR frame relative to the frame of the
//! origin rule, so the connector frame is the IR frame. The mate holds the
//! joint's `UsdPhysics` joint.

use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{
    AssemblyJoint, JointConnector, JointOperands, OperandContainer, PairedJointKind, PlaneSense,
    PrototypeReference,
};
use cadmpeg_ir::transform::Transform;

use super::text::{quoted, real, tuple, Names};
use super::Writer;
use crate::brep::frame::{
    derived, face_point, intrinsic_params, surface_class, OriginRule, RowFrame,
};
use crate::brep::SurfaceKind;

/// Largest axis-component deviation of an offset written as the identity.
const ROTATION_SNAP: f64 = 1e-12;
/// Fraction of the B-rep tolerance below which an offset origin is zero.
const ORIGIN_SNAP: f64 = 1e-6;

/// Name of the scope of the connectors in the `Physics` scope.
pub(super) const CONNECTORS: &str = "Connectors";

/// A connector to write.
#[derive(Debug, Clone)]
pub(super) struct ConnectorPlan {
    body: String,
    face_id: i64,
    rule: OriginRule,
    up: [f64; 3],
    direction: [f64; 3],
    offset: RowFrame,
    /// The surface class token, its intrinsic parameters, a point on the
    /// face, and the derived frame.
    witness: (&'static str, Vec<f64>, [f64; 3], RowFrame),
    tolerance: f64,
}

/// The rows of a frame: the IR placement's columns and its translation.
fn row_frame(transform: &Transform) -> RowFrame {
    let rows = transform.affine_rows();
    let column = |axis: usize| [rows[0][axis], rows[1][axis], rows[2][axis]];
    [column(0), column(1), column(2), column(3)]
}

/// `left · right⁻¹` for rigid row frames.
fn relative(left: &RowFrame, right: &RowFrame) -> RowFrame {
    // right⁻¹ has rotation rows equal to right's rotation columns and
    // origin −t·Rᵀ.
    let transpose = |row: usize, column: usize| right[column][row];
    let mut inverse = [[0.0; 3]; 4];
    for (row, values) in inverse.iter_mut().take(3).enumerate() {
        for (column, value) in values.iter_mut().enumerate() {
            *value = transpose(row, column);
        }
    }
    inverse[3] = [0, 1, 2].map(|column| {
        -(0..3)
            .map(|k| right[3][k] * transpose(k, column))
            .sum::<f64>()
    });
    let mut out = [[0.0; 3]; 4];
    for (row, values) in out.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().enumerate() {
            *value = (0..3).map(|k| left[row][k] * inverse[k][column]).sum();
        }
    }
    for column in 0..3 {
        out[3][column] += inverse[3][column];
    }
    out
}

fn matrix_text(frame: &RowFrame) -> String {
    let row = |values: [f64; 3], last: f64| tuple(&[values[0], values[1], values[2], last]);
    format!(
        "({}, {}, {}, {})",
        row(frame[0], 0.0),
        row(frame[1], 0.0),
        row(frame[2], 0.0),
        row(frame[3], 1.0)
    )
}

/// Whether a frame is the identity to within `tolerance` in its origin and
/// [`ROTATION_SNAP`] in its axes.
fn is_identity(frame: &RowFrame, tolerance: f64) -> bool {
    let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    frame.iter().take(3).zip(identity).all(|(row, expected)| {
        row.iter()
            .zip(expected)
            .all(|(value, expected)| (value - expected).abs() <= ROTATION_SNAP)
    }) && frame[3].iter().all(|value| value.abs() <= tolerance)
}

/// The mate type of a joint family, or `None` when no mate type states it.
fn mate_type(joint: &AssemblyJoint) -> Option<&'static str> {
    Some(match joint.operands() {
        JointOperands::Grounded { .. } => "CadGroundMate",
        JointOperands::Pair { kind, .. } => match kind {
            PairedJointKind::Fixed { .. } => "CadFastenedMate",
            PairedJointKind::Revolute { .. } => "CadRevoluteMate",
            PairedJointKind::Slider { .. } => "CadSliderMate",
            PairedJointKind::Cylindrical { .. } => "CadCylindricalMate",
            PairedJointKind::PlaneCoincidence { .. } => "CadPlanarMate",
            PairedJointKind::Ball {} => "CadBallMate",
            _ => return None,
        },
    })
}

impl Writer<'_> {
    /// The `CadBrep` prim path of the written body that holds face `face` of
    /// the definition placed by `occurrence`, the body record, and the face
    /// record index.
    fn placed_face(
        &self,
        occurrence: &OccurrenceId,
        face: &str,
    ) -> Option<(String, &cadmpeg_ir::topology::Body, usize)> {
        let model = &self.ir.model;
        let placed = model
            .occurrences
            .iter()
            .find(|candidate| &candidate.id == occurrence)?;
        let PrototypeReference::Local { definition } = &placed.prototype else {
            return None;
        };
        let bodies = &model
            .product_definitions
            .iter()
            .find(|candidate| &candidate.id == definition)?
            .bodies;
        for body_id in bodies {
            let Some((_, faces)) = self.exported.get(body_id) else {
                continue;
            };
            let Some(index) = faces
                .iter()
                .position(|candidate| candidate.as_str() == face)
            else {
                continue;
            };
            let recorded = self.brep_paths.get(body_id)?;
            // A shared definition is written once under the prototypes;
            // its instance places it at the occurrence's prim.
            let path = match self.shared.get(definition) {
                Some(prototype) => {
                    let prefix = format!("/{}/{prototype}", super::PROTOTYPES);
                    format!(
                        "{}{}",
                        self.paths.get(occurrence)?,
                        recorded.strip_prefix(&prefix)?
                    )
                }
                None => recorded.clone(),
            };
            let body = model
                .bodies
                .iter()
                .find(|candidate| &candidate.id == body_id)?;
            return Some((path, body, index));
        }
        None
    }

    fn connector_plan(&self, connector: &JointConnector) -> Option<ConnectorPlan> {
        let OperandContainer::Occurrence {
            occurrence,
            occurrence_path,
        } = &connector.operand.container
        else {
            return None;
        };
        if !occurrence_path.is_empty() {
            return None;
        }
        let [face] = connector.operand.subelements.as_slice() else {
            return None;
        };
        let (path, body, index) = self.placed_face(occurrence, face)?;
        let (records, _) = self.exported.get(&body.id)?;
        let local = match &body.transform {
            Some(placement) => placement
                .try_inverse_affine()
                .ok()?
                .compose(connector.frame)
                .ok()?,
            None => connector.frame,
        };
        let mut frame = row_frame(&local);
        frame[3] = frame[3].map(|value| value / super::MILLIMETRES_PER_UNIT);
        let class = surface_class(records, index);
        let rule = match class {
            Some(SurfaceKind::Plane) | None => OriginRule::FaceFrame,
            Some(SurfaceKind::Sphere) => OriginRule::Centre,
            Some(_) => OriginRule::Axis { high: false },
        };
        let (up, direction) = (frame[1], frame[2]);
        let derived_frame = derived(records, index, rule, up, direction).unwrap_or(frame);
        let token = match class {
            Some(SurfaceKind::Plane) => "plane",
            Some(SurfaceKind::Cylinder) => "cylinder",
            Some(SurfaceKind::Cone) => "cone",
            Some(SurfaceKind::Sphere) => "sphere",
            Some(SurfaceKind::Torus) => "torus",
            _ => "other",
        };
        Some(ConnectorPlan {
            body: path,
            face_id: i64::try_from(index).ok()?,
            rule,
            up,
            direction,
            offset: relative(&frame, &derived_frame),
            witness: (
                token,
                intrinsic_params(records, index).unwrap_or_default(),
                face_point(records, index)?,
                derived_frame,
            ),
            tolerance: records.tolerance,
        })
    }

    fn write_connector(&mut self, plan: &ConnectorPlan, name: &str) {
        self.prim_head("def CadConnector", name, &[]);
        self.out.open();
        self.out
            .line(&format!("rel cadConnector:body = <{}>", plan.body));
        self.out.line(&format!(
            "uniform int64 cadConnector:faceId = {}",
            plan.face_id
        ));
        self.out.line(&format!(
            "uniform token cadConnector:origin = {}",
            quoted(plan.rule.token())
        ));
        if plan.rule == (OriginRule::Axis { high: true }) {
            self.out
                .line("uniform token cadConnector:axisEnd = \"high\"");
        }
        self.out.line(&format!(
            "uniform vector3d cadConnector:up = {}",
            tuple(&plan.up)
        ));
        if plan.rule != OriginRule::FaceFrame {
            self.out.line(&format!(
                "uniform vector3d cadConnector:direction = {}",
                tuple(&plan.direction)
            ));
        }
        self.out.line(&format!(
            "uniform double cadConnector:tolerance = {}",
            real(plan.tolerance)
        ));
        if !is_identity(&plan.offset, plan.tolerance * ORIGIN_SNAP) {
            self.out.line(&format!(
                "uniform matrix4d cadConnector:offset = {}",
                matrix_text(&plan.offset)
            ));
        }
        let (class, params, point, frame) = &plan.witness;
        self.out.line(&format!(
            "uniform token cadConnector:witness:surfaceClass = {}",
            quoted(class)
        ));
        self.out.array(
            "uniform double[] cadConnector:witness:surfaceParams",
            &params.iter().map(|value| real(*value)).collect::<Vec<_>>(),
            "",
        );
        self.out.line(&format!(
            "uniform point3d cadConnector:witness:point = {}",
            tuple(point)
        ));
        self.out.line(&format!(
            "uniform matrix4d cadConnector:witness:frame = {}",
            matrix_text(frame)
        ));
        self.out.close();
    }

    /// Writes `joint` as a mate holding its `UsdPhysics` joint, or returns
    /// `false` when it has no mate form.
    pub(super) fn mate(
        &mut self,
        joint: &AssemblyJoint,
        names: &mut Names,
        connectors: &mut Names,
    ) -> bool {
        let Some(mate_type) = mate_type(joint) else {
            return false;
        };
        let base = joint
            .native_ref
            .as_deref()
            .and_then(|path| path.rsplit('/').next())
            .unwrap_or("Mate")
            .to_owned();
        if let JointOperands::Grounded { connector, .. } = joint.operands() {
            let OperandContainer::Occurrence { occurrence, .. } = &connector.operand.container
            else {
                return false;
            };
            let Some(target) = self.paths.get(occurrence).cloned() else {
                return false;
            };
            let name = names.unique(&base);
            self.prim_head(&format!("def {mate_type}"), &name, &[]);
            self.out.open();
            self.out.line(&format!("rel cadMate:grounded = <{target}>"));
            if joint.suppressed {
                self.out.line("uniform bool cadMate:enabled = 0");
            }
            let mut inner = Names::default();
            self.joint(joint, &mut inner, Some("Joint"));
            self.out.close();
            return true;
        }
        let plans: Option<Vec<ConnectorPlan>> = joint
            .connectors()
            .map(|connector| self.connector_plan(connector))
            .collect();
        let Some(plans) = plans else {
            return false;
        };
        if mate_type == "CadSliderMate"
            && plans
                .iter()
                .any(|plan| !matches!(plan.rule, OriginRule::Axis { .. }))
        {
            return false;
        }
        let both_faces = plans.iter().all(|plan| plan.rule == OriginRule::FaceFrame);
        let sense = joint.frame_sense().or_else(|| joint.kind_sense());
        let flip = sense.is_some_and(|sense| (sense == PlaneSense::Opposed) != both_faces);
        let name = names.unique(&base);
        let scope = format!("{}/{CONNECTORS}", self.out.path());
        let mut ends = Vec::new();
        for (side, plan) in plans.iter().enumerate() {
            let connector = connectors.unique(&format!("{name}_{side}"));
            ends.push(format!("{scope}/{connector}"));
            self.connector_queue.push((connector, plan.clone()));
        }
        self.prim_head(&format!("def {mate_type}"), &name, &[]);
        self.out.open();
        for (side, end) in ends.iter().enumerate() {
            self.out
                .line(&format!("rel cadMate:connector{side} = <{end}>"));
        }
        if flip {
            self.out.line("uniform bool cadMate:flip = 1");
        }
        if joint.suppressed {
            self.out.line("uniform bool cadMate:enabled = 0");
        }
        let mut inner = Names::default();
        self.joint(joint, &mut inner, Some("Joint"));
        self.out.close();
        true
    }

    /// Writes the `Connectors` scope of the mates written so far.
    pub(super) fn connectors(&mut self) {
        let queue = std::mem::take(&mut self.connector_queue);
        if queue.is_empty() {
            return;
        }
        self.prim_head("def Scope", CONNECTORS, &[]);
        self.out.open();
        for (name, plan) in &queue {
            self.write_connector(plan, name);
        }
        self.out.close();
    }
}
