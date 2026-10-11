// SPDX-License-Identifier: Apache-2.0
//! Fixed solid primitives and rigid-transform geometry.

use crate::records::feature::{extrude::DesignExtrudeOperation, scope::DesignParameterScope};
use cadmpeg_ir::math::Vector3;

const EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E12: f64 = 1.0e-12;
const EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E8: f64 = 1.0e-8;

pub(super) fn project_solid_primitive(
    scope: &DesignParameterScope,
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, PrimitiveSolid, PrimitiveSolidKind,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::{Angle, Length};
    let operation = |operation| match operation {
        DesignExtrudeOperation::Join => cadmpeg_ir::features::BooleanOp::Join,
        DesignExtrudeOperation::Cut => cadmpeg_ir::features::BooleanOp::Cut,
        DesignExtrudeOperation::Intersect => cadmpeg_ir::features::BooleanOp::Intersect,
        DesignExtrudeOperation::NewBody => cadmpeg_ir::features::BooleanOp::NewBody,
    };
    Some(match &scope.payload() {
        crate::records::feature::scope::DesignScopePayload::BoxPrimitive(Some(
            crate::records::feature::primitives::DesignBoxPrimitive {
                length,
                width,
                height,
                offset_x,
                offset_y,
                operation: result,
                ..
            },
        )) => {
            let placement = cadmpeg_ir::transform::Transform::affine([
                [1.0, 0.0, 0.0, offset_x.get() * 10.0],
                [0.0, 1.0, 0.0, offset_y.get() * 10.0],
                [0.0, 0.0, 1.0, 0.0],
            ])?;
            FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions: Some([
                    cadmpeg_ir::scalar::PositiveLength::new(length.get() * 10.0)?,
                    cadmpeg_ir::scalar::PositiveLength::new(width.get() * 10.0)?,
                    cadmpeg_ir::scalar::PositiveLength::new(height.get() * 10.0)?,
                ]),
                placement: Some(cadmpeg_ir::features::FeatureRigidPlacement::new(placement)?),
                op: operation(*result),
            })
        }
        crate::records::feature::scope::DesignScopePayload::CylinderPrimitive(Some(
            crate::records::feature::primitives::DesignCylinderPrimitive {
                height,
                diameter,
                operation: result,
                ..
            },
        )) => FeatureDefinition::Operation(FeatureOperation::Primitive {
            solid: PrimitiveSolid::new(PrimitiveSolidKind::Cylinder {
                radius: Length::new(diameter.get() * 5.0)?,
                height: Length::new(height.get() * 10.0)?,
                angle: Angle::FULL_TURN,
            })
            .ok()?,
            op: operation(*result),
        }),
        crate::records::feature::scope::DesignScopePayload::SpherePrimitive(Some(
            crate::records::feature::primitives::DesignSpherePrimitive {
                transform,
                diameter,
                operation: result,
                ..
            },
        )) => FeatureDefinition::Operation(FeatureOperation::Sphere {
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                transform[0][3] * 10.0,
                transform[1][3] * 10.0,
                transform[2][3] * 10.0,
            ))?,
            radius: cadmpeg_ir::scalar::PositiveLength::new(diameter.get() * 5.0)?,
            op: operation(*result),
        }),
        crate::records::feature::scope::DesignScopePayload::TorusPrimitive(Some(
            crate::records::feature::primitives::DesignTorusPrimitive {
                transform,
                major_diameter,
                minor_diameter,
                operation: result,
                ..
            },
        )) => FeatureDefinition::Operation(FeatureOperation::Torus {
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                transform[0][3] * 10.0,
                transform[1][3] * 10.0,
                transform[2][3] * 10.0,
            ))?,
            axis: cadmpeg_ir::units::UnitVector3::new(Vector3::new(
                transform[0][2],
                transform[1][2],
                transform[2][2],
            ))?,
            major_radius: cadmpeg_ir::scalar::PositiveLength::new(major_diameter.get() * 5.0)?,
            minor_radius: cadmpeg_ir::scalar::PositiveLength::new(minor_diameter.get() * 5.0)?,
            op: operation(*result),
        }),
        _ => return None,
    })
}

pub(super) fn matrix_axis_angle(transform: &[[f64; 4]; 4]) -> Option<cadmpeg_ir::features::AxisAngle> {
    use cadmpeg_ir::features::AxisAngle;
    use cadmpeg_ir::scalar::Angle;

    let trace = transform[0][0] + transform[1][1] + transform[2][2];
    let sine = 0.5
        * (transform[2][1] - transform[1][2])
            .hypot(transform[0][2] - transform[2][0])
            .hypot(transform[1][0] - transform[0][1]);
    let angle = sine.atan2(((trace - 1.0) * 0.5).clamp(-1.0, 1.0));
    if angle.abs() <= EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E12 {
        return None;
    }
    let (x, y, z) =
        if (std::f64::consts::PI - angle).abs() <= EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E8 {
            // The largest diagonal supplies a nonzero axis component at a half-turn.
            // Recover the other signs from its row; the X component may be zero.
            let pivot = (0..3).max_by(|&a, &b| transform[a][a].total_cmp(&transform[b][b]))?;
            let mut axis = [0.0; 3];
            axis[pivot] = ((transform[pivot][pivot] + 1.0) * 0.5).max(0.0).sqrt();
            if axis[pivot] == 0.0 {
                return None;
            }
            for other in 0..3 {
                if other != pivot {
                    axis[other] =
                        (transform[pivot][other] + transform[other][pivot]) / (4.0 * axis[pivot]);
                }
            }
            // Away from an exact half-turn the skew terms still state the
            // orientation. The diagonal alone cannot distinguish opposite axes.
            let skew = [
                transform[2][1] - transform[1][2],
                transform[0][2] - transform[2][0],
                transform[1][0] - transform[0][1],
            ];
            if axis
                .iter()
                .zip(skew)
                .map(|(axis, skew)| axis * skew)
                .sum::<f64>()
                < 0.0
            {
                axis = axis.map(|value| -value);
            }
            (axis[0], axis[1], axis[2])
        } else {
            let scale = 2.0 * angle.sin();
            (
                (transform[2][1] - transform[1][2]) / scale,
                (transform[0][2] - transform[2][0]) / scale,
                (transform[1][0] - transform[0][1]) / scale,
            )
        };
    let norm = x.hypot(y).hypot(z);
    (norm > EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E12).then_some(AxisAngle {
        origin: cadmpeg_ir::features::FinitePoint3::ZERO,
        direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
            x / norm,
            y / norm,
            z / norm,
        ))?,
        angle: Angle::new(angle)?,
    })
}

