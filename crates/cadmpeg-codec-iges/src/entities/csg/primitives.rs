// SPDX-License-Identifier: Apache-2.0
//! Analytic IGES primitive constructions with their authored rigid frame.

use cadmpeg_ir::features::{
    BooleanOp, Dimension, FeatureOperation, FeatureRigidPlacement, PrimitiveSolid,
    PrimitiveSolidKind, RadialDimension,
};
use cadmpeg_ir::scalar::{Angle, Length};

pub(super) fn operation(
    entity_type: i64,
    dimensions: &[f64],
    factor: f64,
    placement: FeatureRigidPlacement,
) -> Option<FeatureOperation> {
    let length = |index: usize| {
        Length::new(dimensions.get(index).copied()? * factor).map(Dimension::Literal)
    };
    let radial = |index: usize| length(index).map(RadialDimension::Radius);
    let angle = |value| Angle::new(value).map(Dimension::Literal);
    let kind = match entity_type {
        150 => PrimitiveSolidKind::Box {
            length: length(0)?,
            width: length(1)?,
            height: length(2)?,
        },
        152 => PrimitiveSolidKind::Wedge {
            xmin: Dimension::Literal(Length::new(0.0)?),
            ymin: Dimension::Literal(Length::new(0.0)?),
            zmin: Dimension::Literal(Length::new(0.0)?),
            xmax: length(0)?,
            ymax: length(1)?,
            zmax: length(2)?,
            x2min: Dimension::Literal(Length::new(0.0)?),
            x2max: length(3)?,
            z2min: Dimension::Literal(Length::new(0.0)?),
            z2max: length(2)?,
        },
        154 => PrimitiveSolidKind::Cylinder {
            radius: radial(1)?,
            height: length(0)?,
            angle: angle(std::f64::consts::TAU)?,
        },
        156 => PrimitiveSolidKind::Cone {
            radius1: radial(1)?,
            radius2: radial(2)?,
            height: length(0)?,
            angle: angle(std::f64::consts::TAU)?,
        },
        158 => PrimitiveSolidKind::Sphere {
            radius: radial(0)?,
            latitude1: angle(-std::f64::consts::FRAC_PI_2)?,
            latitude2: angle(std::f64::consts::FRAC_PI_2)?,
            longitude: angle(std::f64::consts::TAU)?,
        },
        160 => PrimitiveSolidKind::Torus {
            major_radius: radial(0)?,
            minor_radius: radial(1)?,
            latitude1: angle(-std::f64::consts::PI)?,
            latitude2: angle(std::f64::consts::PI)?,
            longitude: angle(std::f64::consts::TAU)?,
        },
        168 => PrimitiveSolidKind::Ellipsoid {
            x_radius: radial(0)?,
            y_radius: radial(1)?,
            z_radius: radial(2)?,
            latitude1: angle(-std::f64::consts::FRAC_PI_2)?,
            latitude2: angle(std::f64::consts::FRAC_PI_2)?,
            longitude: angle(std::f64::consts::TAU)?,
        },
        _ => return None,
    };
    Some(FeatureOperation::Primitive {
        solid: PrimitiveSolid::new(kind).ok()?,
        placement: Some(placement),
        op: BooleanOp::NewBody,
    })
}
