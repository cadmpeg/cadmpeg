// SPDX-License-Identifier: Apache-2.0
//! Length scaling of admitted sketch geometry.

use super::{
    FinitePoint2, FinitePoint3, FiniteReal, OrderedMajorRadius, Point2, PositiveLength,
    PositiveReal, SketchConstraintDefinition, SketchConstraintDefinitionInput, SketchGeometry,
    SketchGeometryDefinition, SpatialSketchConstraintDefinition,
    SpatialSketchConstraintDefinitionInput, SpatialSketchGeometry, SpatialSketchGeometryDefinition,
    EPS_POLAR_DISTANCE_ZERO, EPS_SPATIAL_LINE_LENGTH,
};
use crate::geometry::nurbs::NurbsError;

/// A condition that scaling checked sketch geometry can break.
#[derive(Debug)]
pub enum SketchLengthScaleError {
    /// A source length product overflowed before geometry admission.
    LengthOverflow,
    /// A changed coordinate, positive length, or bound failed its field contract.
    Field(&'static str),
    /// A planar or spatial curve control point failed admission.
    CurveControlPoints(NurbsError),
    /// A spatial surface control point failed admission.
    SurfaceControlPoints(NurbsError),
}

/// A condition that scaling a stored sketch constraint distance can break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchConstraintScaleError {
    /// A length product overflowed.
    LengthOverflow,
    /// The scaled distance no longer satisfies its relation contract.
    InvalidLocalValue,
}

fn constraint_length(
    length: &mut crate::scalar::Length,
    scale: PositiveReal,
) -> Result<(), SketchConstraintScaleError> {
    *length = crate::scalar::Length::new(length.get() * scale.get())
        .ok_or(SketchConstraintScaleError::LengthOverflow)?;
    Ok(())
}

impl SketchConstraintDefinition {
    /// Scale only distance values. Stored arity, references, and unrelated
    /// scalar fields retain their admission.
    pub fn scale_lengths(&mut self, scale: PositiveReal) -> Result<(), SketchConstraintScaleError> {
        use SketchConstraintDefinitionInput as Kind;
        let mut kind = self.0.clone();
        match &mut kind {
            Kind::PointCoordinateValues { values, .. } => {
                for value in values {
                    constraint_length(value, scale)?;
                }
            }
            Kind::MidpointCoordinate { value, .. }
            | Kind::DistanceLociValue {
                distance: value, ..
            }
            | Kind::PolarDistance {
                distance: value, ..
            }
            | Kind::Offset {
                distance: value, ..
            } => constraint_length(value, scale)?,
            _ => {}
        }
        match &kind {
            Kind::PolarDistance {
                distance, angle, ..
            } if (distance.get() <= EPS_POLAR_DISTANCE_ZERO) == angle.is_some() => {
                return Err(SketchConstraintScaleError::InvalidLocalValue);
            }
            Kind::Offset { distance, .. } if distance.get() <= 0.0 => {
                return Err(SketchConstraintScaleError::InvalidLocalValue);
            }
            _ => {}
        }
        self.0 = kind;
        Ok(())
    }
}

impl SpatialSketchConstraintDefinition {
    /// Scale the optional offset distance while carrying admitted directions
    /// and entity relationships unchanged.
    pub fn scale_lengths(&mut self, scale: PositiveReal) -> Result<(), SketchConstraintScaleError> {
        let mut kind = self.0.clone();
        if let SpatialSketchConstraintDefinitionInput::Offset { distance, .. } = &mut kind {
            let product = distance.get() * scale.get();
            if !product.is_finite() {
                return Err(SketchConstraintScaleError::LengthOverflow);
            }
            *distance = PositiveLength::new(product)
                .ok_or(SketchConstraintScaleError::InvalidLocalValue)?;
        }
        self.0 = kind;
        Ok(())
    }
}

fn planar_point(point: FinitePoint2, scale: PositiveReal) -> Option<FinitePoint2> {
    let point = point.get();
    FinitePoint2::new(Point2::new(point.u * scale.get(), point.v * scale.get()))
}

fn spatial_point(point: FinitePoint3, scale: PositiveReal) -> Option<FinitePoint3> {
    point.scaled(scale)
}

fn length_product(
    length: PositiveLength,
    scale: PositiveReal,
) -> Result<f64, SketchLengthScaleError> {
    let product = length.get() * scale.get();
    product
        .is_finite()
        .then_some(product)
        .ok_or(SketchLengthScaleError::LengthOverflow)
}

impl SketchGeometry {
    /// Scale length-bearing fields while carrying stored directions, angles,
    /// bounds unrelated to length, and text attributes unchanged.
    pub fn scaled_lengths(&self, scale: PositiveReal) -> Result<Self, SketchLengthScaleError> {
        use SketchGeometryDefinition as Definition;
        let mut definition = self.definition().clone();
        match &mut definition {
            Definition::Point { position } => {
                *position = planar_point(*position, scale).ok_or(SketchLengthScaleError::Field(
                    "sketch point position must be finite",
                ))?;
            }
            Definition::Line { start, end } => {
                *start = planar_point(*start, scale).ok_or(SketchLengthScaleError::Field(
                    "sketch line endpoints must be finite",
                ))?;
                *end = planar_point(*end, scale).ok_or(SketchLengthScaleError::Field(
                    "sketch line endpoints must be finite",
                ))?;
            }
            Definition::ReferenceLine { origin, .. } => {
                *origin = planar_point(*origin, scale).ok_or(SketchLengthScaleError::Field(
                    "sketch reference line requires finite origin and nonzero finite direction",
                ))?;
            }
            Definition::Circle { center, radius } | Definition::Arc { center, radius, .. } => {
                let product = length_product(*radius, scale)?;
                *center = planar_point(*center, scale).ok_or(SketchLengthScaleError::Field(
                    "sketch circular geometry requires finite center and positive finite radius",
                ))?;
                *radius = PositiveLength::new(product).ok_or(SketchLengthScaleError::Field(
                    "sketch circular geometry requires finite center and positive finite radius",
                ))?;
            }
            Definition::Ellipse {
                center,
                major_radius,
                minor_radius,
                ..
            } => {
                let major_product = length_product(major_radius.major(), scale)?;
                let minor_product = length_product(*minor_radius, scale)?;
                let radii_message = "sketch ellipse radii must be positive and finite";
                *center = planar_point(*center, scale).ok_or(SketchLengthScaleError::Field(
                    "sketch ellipse center and major_angle must be finite",
                ))?;
                let major = PositiveLength::new(major_product)
                    .ok_or(SketchLengthScaleError::Field(radii_message))?;
                let minor = PositiveLength::new(minor_product)
                    .ok_or(SketchLengthScaleError::Field(radii_message))?;
                *major_radius = OrderedMajorRadius::new(major, minor)
                    .ok_or(SketchLengthScaleError::Field(radii_message))?;
                *minor_radius = minor;
            }
            Definition::Hyperbola {
                center,
                major_radius,
                minor_radius,
                ..
            } => {
                let major_product = length_product(*major_radius, scale)?;
                let minor_product = length_product(*minor_radius, scale)?;
                let radii_message = "sketch hyperbola radii must be positive and finite";
                *center = planar_point(*center, scale).ok_or(SketchLengthScaleError::Field(
                    "sketch hyperbola center and major_angle must be finite",
                ))?;
                *major_radius = PositiveLength::new(major_product)
                    .ok_or(SketchLengthScaleError::Field(radii_message))?;
                *minor_radius = PositiveLength::new(minor_product)
                    .ok_or(SketchLengthScaleError::Field(radii_message))?;
            }
            Definition::Parabola {
                vertex,
                focal_length,
                bounds,
                ..
            } => {
                let product = length_product(*focal_length, scale)?;
                let scaled_bounds =
                    (*bounds).map(|values| values.map(|value| value.get() * scale.get()));
                *vertex = planar_point(*vertex, scale).ok_or(SketchLengthScaleError::Field(
                    "sketch parabola vertex and axis_angle must be finite",
                ))?;
                *focal_length =
                    PositiveLength::new(product).ok_or(SketchLengthScaleError::Field(
                        "sketch parabola focal_length must be positive and finite",
                    ))?;
                *bounds = scaled_bounds
                    .map(|values| {
                        FiniteReal::array(values).ok_or(SketchLengthScaleError::Field(
                            "sketch parabola bounds must be finite",
                        ))
                    })
                    .transpose()?;
            }
            Definition::Nurbs { curve } => {
                curve
                    .edit_control_points(|point| {
                        point.u *= scale.get();
                        point.v *= scale.get();
                        Ok(())
                    })
                    .map_err(SketchLengthScaleError::CurveControlPoints)?;
            }
            Definition::Text {
                height, placement, ..
            } => {
                let product = length_product(*height, scale)?;
                let scaled_anchor = placement.as_ref().map(|placement| placement.anchor);
                *height = PositiveLength::new(product).ok_or(SketchLengthScaleError::Field(
                    "sketch text height must be positive and finite",
                ))?;
                if let (Some(placement), Some(anchor)) = (placement, scaled_anchor) {
                    placement.anchor =
                        planar_point(anchor, scale).ok_or(SketchLengthScaleError::Field(
                            "sketch text anchor and rotation must be finite",
                        ))?;
                }
            }
            Definition::ExternalReference { .. } | Definition::Native { .. } => {}
        }
        if let Definition::Hyperbola {
            bounds: Some(bounds),
            ..
        } = &mut definition
        {
            let scaled = bounds.map(|value| value.get() * scale.get());
            *bounds = FiniteReal::array(scaled).ok_or(SketchLengthScaleError::Field(
                "sketch hyperbola bounds must be finite",
            ))?;
        }
        Ok(Self(definition))
    }
}

impl SpatialSketchGeometry {
    /// Scale the model-space coordinates and radii while carrying admitted
    /// unit directions, angles, and native labels unchanged.
    pub fn scaled_lengths(&self, scale: PositiveReal) -> Result<Self, SketchLengthScaleError> {
        use SpatialSketchGeometryDefinition as Definition;
        let mut definition = self.definition().clone();
        match &mut definition {
            Definition::Point { position } => {
                *position = spatial_point(*position, scale).ok_or(
                    SketchLengthScaleError::Field("spatial sketch point position must be finite"),
                )?;
            }
            Definition::Line { start, end } => {
                let first = spatial_point(*start, scale);
                let second = spatial_point(*end, scale);
                let (Some(first), Some(second)) = (first, second) else {
                    return Err(SketchLengthScaleError::Field(
                        "spatial sketch line endpoints must be finite and separated",
                    ));
                };
                let first_point = first.get();
                let second_point = second.get();
                let distance = (second_point.x - first_point.x)
                    .hypot(second_point.y - first_point.y)
                    .hypot(second_point.z - first_point.z);
                if distance <= EPS_SPATIAL_LINE_LENGTH {
                    return Err(SketchLengthScaleError::Field(
                        "spatial sketch line endpoints must be finite and separated",
                    ));
                }
                *start = first;
                *end = second;
            }
            Definition::Circle { center, radius, .. } | Definition::Arc { center, radius, .. } => {
                let product = length_product(*radius, scale)?;
                *center = spatial_point(*center, scale).ok_or(SketchLengthScaleError::Field(
                    "spatial circular geometry requires finite center and positive finite radius",
                ))?;
                *radius = PositiveLength::new(product).ok_or(SketchLengthScaleError::Field(
                    "spatial circular geometry requires finite center and positive finite radius",
                ))?;
            }
            Definition::Nurbs { curve } => {
                curve
                    .edit_control_points(|point| {
                        point.x *= scale.get();
                        point.y *= scale.get();
                        point.z *= scale.get();
                        Ok(())
                    })
                    .map_err(SketchLengthScaleError::CurveControlPoints)?;
            }
            Definition::NurbsSurface { surface } => {
                surface
                    .edit_control_points(|point| {
                        point.x *= scale.get();
                        point.y *= scale.get();
                        point.z *= scale.get();
                        Ok(())
                    })
                    .map_err(SketchLengthScaleError::SurfaceControlPoints)?;
            }
            Definition::Native { .. } => {}
        }
        Ok(Self(definition))
    }
}

#[cfg(test)]
mod tests {
    use super::{SketchConstraintScaleError, SketchLengthScaleError, EPS_POLAR_DISTANCE_ZERO};
    use crate::math::{Point3, Vector3};
    use crate::scalar::{Angle, FiniteReal, Length};
    use crate::sketches::{
        Point2, PositiveReal, SketchConstraintDefinition, SketchConstraintDefinitionInput,
        SketchGeometry, SketchGeometryDefinition, SpatialSketchConstraintDefinition,
        SpatialSketchConstraintDefinitionInput, SpatialSketchGeometry,
        SpatialSketchGeometryDefinition,
    };

    #[test]
    fn planar_scaling_carries_reference_direction_and_hyperbola_angle() {
        let reference = SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
            origin: Point2::new(2.0, -3.0),
            direction: Point2::new(0.25, -0.5),
        })
        .unwrap();
        let scaled = reference
            .scaled_lengths(PositiveReal::new(10.0).unwrap())
            .unwrap();
        assert!(matches!(
            scaled.definition(),
            SketchGeometryDefinition::ReferenceLine { origin, direction }
                if origin.get() == Point2::new(20.0, -30.0)
                    && direction.get() == Point2::new(0.25, -0.5)
        ));

        let angle = Angle::new(0.25).unwrap();
        let hyperbola = SketchGeometry::try_from(SketchGeometryDefinition::Hyperbola {
            center: Point2::new(1.0, 2.0),
            major_angle: angle,
            major_radius: Length::new(4.0).unwrap(),
            minor_radius: Length::new(2.0).unwrap(),
            bounds: Some([-3.0, 5.0]),
        })
        .unwrap();
        let scaled = hyperbola
            .scaled_lengths(PositiveReal::new(10.0).unwrap())
            .unwrap();
        assert!(matches!(
            scaled.definition(),
            SketchGeometryDefinition::Hyperbola {
                center,
                major_angle,
                major_radius,
                minor_radius,
                bounds: Some(bounds),
            } if center.get() == Point2::new(10.0, 20.0)
                && *major_angle == angle
                && major_radius.get() == 40.0
                && minor_radius.get() == 20.0
                && bounds.map(FiniteReal::get) == [-30.0, 50.0]
        ));
    }

    #[test]
    fn planar_scaling_refuses_a_length_overflow_before_a_center_overflow() {
        let original = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(1.0e300, 0.0),
            radius: Length::new(1.0e300).unwrap(),
        })
        .unwrap();
        let result = original.scaled_lengths(PositiveReal::new(1.0e300).unwrap());
        assert!(matches!(
            result,
            Err(SketchLengthScaleError::LengthOverflow)
        ));
        assert_eq!(
            original.definition().to_raw(),
            SketchGeometryDefinition::Circle {
                center: Point2::new(1.0e300, 0.0),
                radius: Length::new(1.0e300).unwrap(),
            }
        );
    }

    #[test]
    fn spatial_scaling_carries_the_admitted_circle_frame() {
        let original = SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Arc {
            center: Point3::new(1.0, -2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            reference_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: Length::new(4.0).unwrap(),
            start_angle: Angle::new(0.0).unwrap(),
            end_angle: Angle::new(1.0).unwrap(),
        })
        .unwrap();
        let scaled = original
            .scaled_lengths(PositiveReal::new(2.0).unwrap())
            .unwrap();
        assert!(matches!(
            scaled.definition(),
            SpatialSketchGeometryDefinition::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } if center.get() == Point3::new(2.0, -4.0, 6.0)
                && *normal.as_raw() == Vector3::new(0.0, 0.0, 1.0)
                && *reference_direction.as_raw() == Vector3::new(1.0, 0.0, 0.0)
                && radius.get() == 8.0
                && start_angle.get() == 0.0
                && end_angle.get() == 1.0
        ));
    }

    #[test]
    fn polar_distance_scaling_checks_only_the_changed_threshold_relation() {
        use crate::sketches::{SketchEntityId, SketchLocus};

        let first = SketchLocus::Start(SketchEntityId::mint("test:test:sketch-entity#a").unwrap());
        let second = SketchLocus::End(SketchEntityId::mint("test:test:sketch-entity#b").unwrap());
        let mut definition =
            SketchConstraintDefinition::try_from(SketchConstraintDefinitionInput::PolarDistance {
                first,
                second,
                distance: Length::new(2.0 * EPS_POLAR_DISTANCE_ZERO).unwrap(),
                angle: Some(Angle::new(0.5).unwrap()),
                distance_parameter: None,
            })
            .unwrap();
        let original = definition.clone();
        assert_eq!(
            definition.scale_lengths(PositiveReal::new(0.25).unwrap()),
            Err(SketchConstraintScaleError::InvalidLocalValue)
        );
        assert_eq!(definition, original);
        definition
            .scale_lengths(PositiveReal::new(2.0).unwrap())
            .unwrap();
        assert!(matches!(
            definition.kind(),
            SketchConstraintDefinitionInput::PolarDistance { distance, angle: Some(angle), .. }
                if distance.get() == 4.0 * EPS_POLAR_DISTANCE_ZERO && angle.get() == 0.5
        ));
    }

    #[test]
    fn spatial_offset_scaling_carries_the_unit_normal_and_refuses_zero() {
        use crate::sketches::SpatialSketchEntityId;

        let mut definition = SpatialSketchConstraintDefinition::try_from(
            SpatialSketchConstraintDefinitionInput::Offset {
                sources: vec![SpatialSketchEntityId::mint("test:test:spatial-entity#a").unwrap()],
                results: vec![SpatialSketchEntityId::mint("test:test:spatial-entity#b").unwrap()],
                normal: Vector3::new(0.0, 0.0, 1.0),
                distance: Length::new(2.0).unwrap(),
                parameter: None,
            },
        )
        .unwrap();
        assert_eq!(
            definition.scale_lengths(PositiveReal::new(f64::MIN_POSITIVE).unwrap()),
            Ok(())
        );
        assert!(matches!(
            definition.kind(),
            SpatialSketchConstraintDefinitionInput::Offset { normal, distance, .. }
                if *normal.as_raw() == Vector3::new(0.0, 0.0, 1.0)
                    && distance.get() == 2.0 * f64::MIN_POSITIVE
        ));
        let before_failure = definition.clone();
        assert_eq!(
            definition.scale_lengths(PositiveReal::new(f64::MIN_POSITIVE).unwrap()),
            Err(SketchConstraintScaleError::InvalidLocalValue)
        );
        assert_eq!(definition, before_failure);
    }
}
