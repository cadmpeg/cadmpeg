// SPDX-License-Identifier: Apache-2.0
use crate::math::{Point2, Point3, Vector3};
use crate::scalar::{Angle, FiniteReal, Length, PositiveLength, PositiveReal};
use crate::sketches::{
    SketchGeometry, SketchGeometryDefinition, SpatialSketchConstraintDefinition,
    SpatialSketchConstraintDefinitionInput, SpatialSketchEntityId, SpatialSketchEntityUse,
    SpatialSketchGeometry, SpatialSketchGeometryDefinition, SpatialSketchProfile, TextPlacement,
};
use crate::units::{FinitePoint2, UnitVector3};

fn length(value: f64) -> Length {
    Length::new(value).unwrap()
}

#[test]
fn a_sketch_geometry_holds_its_admitted_definition_and_takes_admitted_parts() {
    let ellipse = SketchGeometryDefinition::Ellipse {
        center: Point2::new(1.0, 2.0),
        major_angle: Angle::new(0.5).unwrap(),
        major_radius: length(3.0),
        minor_radius: length(2.0),
        bounds: None,
    };
    let geometry = SketchGeometry::try_from(ellipse.clone()).unwrap();
    let SketchGeometryDefinition::Ellipse {
        center,
        major_radius,
        ..
    } = geometry.definition()
    else {
        panic!("ellipse")
    };
    assert_eq!(*center, FinitePoint2::new(Point2::new(1.0, 2.0)).unwrap());
    assert_eq!(*major_radius, PositiveLength::new(3.0).unwrap());
    assert_eq!(geometry.definition().to_raw(), ellipse);

    // The typed route tests only the conditions between fields.
    let admitted = |major: f64, minor: f64| SketchGeometryDefinition::Ellipse {
        center: FinitePoint2::new(Point2::new(0.0, 0.0)).unwrap(),
        major_angle: Angle::new(0.0).unwrap(),
        major_radius: PositiveLength::new(major).unwrap(),
        minor_radius: PositiveLength::new(minor).unwrap(),
        bounds: None,
    };
    assert!(SketchGeometry::from_parts(admitted(3.0, 2.0)).is_ok());
    assert_eq!(
        SketchGeometry::from_parts(admitted(2.0, 3.0)).unwrap_err(),
        "sketch ellipse major_radius must be at least minor_radius"
    );
    assert_eq!(
        SketchGeometry::from_parts(SketchGeometryDefinition::ReferenceLine {
            origin: FinitePoint2::new(Point2::new(0.0, 0.0)).unwrap(),
            direction: FinitePoint2::new(Point2::new(0.0, 0.0)).unwrap(),
        })
        .unwrap_err(),
        "sketch reference line requires finite origin and nonzero finite direction"
    );

    // The raw route keeps its refusal text and order.
    let text = |height: f64, width_factor: f64, anchor: f64| SketchGeometryDefinition::Text {
        text: cadmpeg_core::nonblank_literal!("A"),
        font_family: cadmpeg_core::nonblank_literal!("Sans"),
        font_weight: crate::sketches::SketchFontWeight::Regular,
        height: length(height),
        width_factor: Some(width_factor),
        placement: Some(TextPlacement {
            anchor: Point2::new(anchor, 0.0),
            rotation: Angle::new(0.0).unwrap(),
        }),
        horizontal_alignment: None,
        vertical_alignment: None,
    };
    for (definition, message) in [
        (
            text(0.0, f64::NAN, f64::INFINITY),
            "sketch text height must be positive and finite",
        ),
        (
            text(1.0, f64::NAN, f64::INFINITY),
            "sketch text width_factor must be positive and finite",
        ),
        (
            text(1.0, 2.0, f64::INFINITY),
            "sketch text anchor and rotation must be finite",
        ),
    ] {
        assert_eq!(SketchGeometry::try_from(definition).unwrap_err(), message);
    }
    let admitted_text = SketchGeometry::try_from(text(1.0, 2.0, 3.0)).unwrap();
    let SketchGeometryDefinition::Text { width_factor, .. } = admitted_text.definition() else {
        panic!("text")
    };
    assert_eq!(*width_factor, Some(PositiveReal::new(2.0).unwrap()));
    let parabola = SketchGeometryDefinition::Parabola {
        vertex: Point2::new(0.0, 0.0),
        axis_angle: Angle::new(0.0).unwrap(),
        focal_length: length(1.0),
        bounds: Some([-1.0, 2.0]),
    };
    let SketchGeometryDefinition::Parabola { bounds, .. } = SketchGeometry::try_from(parabola)
        .unwrap()
        .into_definition()
    else {
        panic!("parabola")
    };
    assert_eq!(bounds.map(FiniteReal::raw_array), Some([-1.0, 2.0]));
}

#[test]
fn spatial_sketch_records_hold_their_admitted_frames_and_scalars() {
    let circle = SpatialSketchGeometryDefinition::Circle {
        center: Point3::new(1.0, 2.0, 3.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        reference_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: length(2.0),
    };
    let geometry = SpatialSketchGeometry::try_from(circle.clone()).unwrap();
    let SpatialSketchGeometryDefinition::Circle { normal, radius, .. } = geometry.definition()
    else {
        panic!("circle")
    };
    assert_eq!(
        *normal,
        UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap()
    );
    assert_eq!(*radius, PositiveLength::new(2.0).unwrap());
    assert_eq!(geometry.definition().to_raw(), circle);
    // Center and radius are refused before the frame.
    assert_eq!(
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Circle {
            center: Point3::new(f64::NAN, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 2.0),
            reference_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: length(2.0),
        })
        .unwrap_err(),
        "spatial circular geometry requires finite center and positive finite radius"
    );
    assert_eq!(
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            reference_direction: Vector3::new(0.0, 0.0, 1.0),
            radius: length(2.0),
        })
        .unwrap_err(),
        "spatial circular normal and reference_direction must be unit and orthogonal"
    );

    let entity = SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#a").unwrap();
    let offset = SpatialSketchConstraintDefinitionInput::Offset {
        sources: vec![entity.clone()],
        results: vec![
            SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#b").unwrap(),
        ],
        normal: Vector3::new(0.0, 1.0, 0.0),
        distance: length(0.5),
        parameter: None,
    };
    let definition = SpatialSketchConstraintDefinition::try_from(offset.clone()).unwrap();
    let SpatialSketchConstraintDefinitionInput::Offset { distance, .. } = definition.kind() else {
        panic!("offset")
    };
    assert_eq!(*distance, PositiveLength::new(0.5).unwrap());
    assert_eq!(definition.kind().to_raw(), offset);
    for invalid in [
        SpatialSketchConstraintDefinitionInput::ParallelToDirection {
            entity: entity.clone(),
            direction: Vector3::new(0.0, 2.0, 0.0),
        },
        SpatialSketchConstraintDefinitionInput::Offset {
            sources: vec![entity.clone()],
            results: vec![entity.clone()],
            normal: Vector3::new(0.0, 1.0, 0.0),
            distance: length(0.0),
            parameter: None,
        },
    ] {
        assert_eq!(
            SpatialSketchConstraintDefinition::try_from(invalid).unwrap_err(),
            "invalid spatial sketch constraint local arity or scalar value"
        );
    }

    let boundary = vec![SpatialSketchEntityUse {
        entity,
        reversed: false,
    }];
    let unit = |x, y, z| UnitVector3::new(Vector3::new(x, y, z)).unwrap();
    let origin = crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap();
    assert!(SpatialSketchProfile::from_parts(
        origin,
        unit(0.0, 0.0, 1.0),
        unit(1.0, 0.0, 0.0),
        boundary.clone()
    )
    .is_ok());
    assert_eq!(
        SpatialSketchProfile::from_parts(
            origin,
            unit(0.0, 0.0, 1.0),
            unit(0.0, 0.0, 1.0),
            boundary
        )
        .unwrap_err(),
        "spatial profile normal and u_axis must be unit and orthogonal"
    );
}
