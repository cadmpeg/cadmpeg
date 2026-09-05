// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

#[test]
fn spatial_line_distance_requires_parallel_geometry_and_exact_value() {
    use cadmpeg_ir::sketches::SpatialSketchGeometry::Line;

    let first = Line {
        start: Point3::new(0.0, 0.0, 0.0),
        end: Point3::new(0.0, 10.0, 0.0),
    };
    let second = Line {
        start: Point3::new(3.0, 0.0, 4.0),
        end: Point3::new(3.0, -5.0, 4.0),
    };
    let crossing = Line {
        start: Point3::new(0.0, 0.0, 0.0),
        end: Point3::new(1.0, 0.0, 0.0),
    };

    assert!(spatial_parallel_line_distance_matches(&first, &second, 5.0));
    assert!(!spatial_parallel_line_distance_matches(
        &first, &second, 4.0
    ));
    assert!(!spatial_parallel_line_distance_matches(
        &first, &crossing, 0.0
    ));
}

#[test]
fn spatial_point_distance_requires_point_geometry_and_exact_value() {
    use cadmpeg_ir::sketches::SpatialSketchGeometry::{Line, Point};

    let first = Point {
        position: Point3::new(1.0, 2.0, 3.0),
    };
    let second = Point {
        position: Point3::new(4.0, 6.0, 3.0),
    };
    let line = Line {
        start: Point3::new(1.0, 2.0, 3.0),
        end: Point3::new(4.0, 6.0, 3.0),
    };

    assert!(spatial_point_distance_matches(&first, &second, 5.0));
    assert!(!spatial_point_distance_matches(&first, &second, 4.0));
    assert!(!spatial_point_distance_matches(&first, &line, 5.0));
}

#[test]
fn owner_scoped_radial_dimensions_preserve_repeated_measurements() {
    let mut entity = SketchEntity::new(
        SketchEntityId("f3d:model:sketch-entity#circle".into()),
        SketchId("f3d:model:sketch#radial".into()),
        SketchGeometry::Circle {
            center: Point2::new(2.0, 3.0),
            radius: Length(5.0),
        },
    );
    let radius_parameter = cadmpeg_ir::features::ParameterId("parameter#radius".into());
    assert!(matches!(
        radial_dimension_definition(
            &entity,
            "Radius Dimension-2",
            0.5,
            radius_parameter.clone(),
        ),
        Some(SketchConstraintDefinition::Radius { entity: ref actual, parameter: ref p })
            if actual == entity.id() && p == &radius_parameter
    ));
    assert!(matches!(
        radial_dimension_definition(
            &entity,
            "Radial Dimension-3",
            0.5,
            radius_parameter.clone(),
        ),
        Some(SketchConstraintDefinition::Radius { entity: ref actual, .. })
            if actual == entity.id()
    ));
    let diameter_parameter = cadmpeg_ir::features::ParameterId("parameter#diameter".into());
    assert!(matches!(
        radial_dimension_definition(
            &entity,
            "Diameter Dimension-2",
            1.0,
            diameter_parameter.clone(),
        ),
        Some(SketchConstraintDefinition::Diameter { entity: ref actual, parameter: ref p })
            if actual == entity.id() && p == &diameter_parameter
    ));
    assert!(radial_dimension_definition(
        &entity,
        "Diameter Dimension-2",
        0.5,
        diameter_parameter.clone(),
    )
    .is_none());
    let parameter = parse_design_parameter(&parameter_record(
        Some(1),
        "10 mm",
        "Diameter Dimension-2",
        Some("mm"),
        "d1",
        1.0,
    ))
    .expect("diameter parameter");
    assert!(matches!(
        owner_scoped_radial_dimension_definition(
            std::slice::from_ref(&entity),
            &entity.sketch,
            &parameter,
            &diameter_parameter,
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::Diameter {
            entity: ref actual,
            ..
        }) if actual == entity.id()
    ));
    let mut duplicate = SketchEntity::new(
        SketchEntityId("f3d:model:sketch-entity#duplicate-circle".into()),
        entity.sketch.clone(),
        entity.geometry.clone(),
    )
    .with_construction(entity.construction)
    .with_native_ref(entity.native_ref.clone())
    .with_geometry_ref(entity.geometry_ref.clone())
    .with_endpoint_refs(entity.endpoint_refs.clone());
    let SketchGeometry::Circle { radius, .. } = &mut duplicate.geometry else {
        unreachable!("test entity is circular")
    };
    radius.0 += 5.0e-7;
    assert!(matches!(
        owner_scoped_radial_dimension_definition(
            &[entity.clone(), duplicate.clone()],
            &entity.sketch,
            &parameter,
            &diameter_parameter,
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::RepeatedDiameter {
            entities,
            parameter,
        }) if entities == vec![entity.id().clone(), duplicate.id().clone()]
            && parameter == diameter_parameter
    ));

    let radial_parameter = parse_design_parameter(&parameter_record(
        Some(1),
        "5 mm",
        "Radial Dimension-2",
        Some("mm"),
        "d2",
        0.5,
    ))
    .expect("radial parameter");
    assert!(matches!(
        owner_scoped_radial_dimension_definition(
            &[entity.clone(), duplicate.clone()],
            &entity.sketch,
            &radial_parameter,
            &radius_parameter,
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::RepeatedRadius {
            entities,
            parameter,
        }) if entities == vec![entity.id().clone(), duplicate.id().clone()]
            && parameter == radius_parameter
    ));

    entity.geometry = SketchGeometry::Arc {
        center: Point2::new(2.0, 3.0),
        radius: Length(5.0),
        start_angle: cadmpeg_ir::features::Angle(0.0),
        end_angle: cadmpeg_ir::features::Angle(1.0),
    };
    assert!(
        radial_dimension_definition(&entity, "Diameter Dimension", 1.0, diameter_parameter,)
            .is_some()
    );
    entity.geometry = SketchGeometry::Ellipse {
        center: Point2::new(2.0, 3.0),
        major_angle: cadmpeg_ir::features::Angle(0.0),
        major_radius: Length(5.0),
        minor_radius: Length(3.0),
        bounds: None,
    };
    assert!(
        radial_dimension_definition(&entity, "Radius Dimension-2", 0.5, radius_parameter,)
            .is_none()
    );
}

#[test]
fn owner_scoped_line_lengths_preserve_repeated_entities() {
    let sketch = SketchId("f3d:model:sketch#line-length".into());
    let line = |name: &str, v: f64, length: f64| {
        SketchEntity::new(
            SketchEntityId(format!("f3d:model:sketch-entity#{name}")),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(0.0, v),
                end: Point2::new(length, v),
            },
        )
    };
    let first = line("first", 0.0, 4.0);
    let second = line("second", 2.0, 4.0 + 5.0e-7);
    let parameter = parse_design_parameter(&parameter_record(
        Some(1),
        "4 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        0.4,
    ))
    .expect("linear parameter");
    let parameter_id = cadmpeg_ir::features::ParameterId("parameter#line-length".into());

    assert!(matches!(
        owner_scoped_line_length_dimension_definition(
            std::slice::from_ref(&first),
            &sketch,
            &parameter,
            &parameter_id,
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::DistanceLoci {
            first: SketchLocus::Start(ref entity),
            second: SketchLocus::End(ref other),
            parameter: ref actual_parameter,
        }) if entity == first.id() && other == first.id() && actual_parameter == &parameter_id
    ));
    assert!(matches!(
        owner_scoped_line_length_dimension_definition(
            &[first.clone(), second.clone()],
            &sketch,
            &parameter,
            &parameter_id,
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::RepeatedLength {
            entities,
            parameter,
        }) if entities == vec![first.id().clone(), second.id().clone()]
            && parameter == parameter_id
    ));
}

#[test]
fn owner_scoped_angular_dimension_requires_one_matching_line_pair() {
    let sketch = SketchId("f3d:model:sketch#angular".into());
    let line = |name: &str, angle: f64| {
        SketchEntity::new(
            SketchEntityId(format!("f3d:model:sketch-entity#{name}")),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(angle.cos(), angle.sin()),
            },
        )
    };
    let horizontal = line("horizontal", 0.0);
    let sloped = line("sloped", std::f64::consts::FRAC_PI_6);
    let vertical = line("vertical", std::f64::consts::FRAC_PI_2);
    let parameter = parse_design_parameter(&parameter_record(
        Some(1),
        "30 deg",
        "Angular Dimension-2",
        Some("deg"),
        "d1",
        std::f64::consts::FRAC_PI_6,
    ))
    .expect("angular parameter");
    let parameter_id = cadmpeg_ir::features::ParameterId("parameter#angle".into());

    assert!(matches!(
        owner_scoped_angular_dimension_definition(
            &[horizontal.clone(), sloped.clone(), vertical.clone()],
            &sketch,
            &parameter,
            &parameter_id,
        ),
        Some(SketchConstraintDefinition::Angle {
            first,
            second,
            parameter,
        }) if first == horizontal.id().clone() && second == sloped.id().clone() && parameter == parameter_id
    ));

    let other_sloped = line("other-sloped", -std::f64::consts::FRAC_PI_6);
    assert!(owner_scoped_angular_dimension_definition(
        &[horizontal, sloped, vertical, other_sloped],
        &sketch,
        &parameter,
        &parameter_id,
    )
    .is_none());
}

#[test]
fn preceding_incident_angular_dimension_excludes_later_symmetric_geometry() {
    let stream = "f3d:A";
    let sketch = SketchId("f3d:model:sketch#angular-incidence".into());
    let curve = |record_index, byte_offset, angle: f64| SketchCurveIdentity {
        id: format!("{stream}:sketch-curve#{record_index}"),
        record_index,
        owner_reference: Some(100),
        class_tag: "301".into(),
        byte_offset,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: u64::from(record_index),
        secondary_id: 0,
        geometry: Some(SketchCurveGeometry::Line {
            start: Point3::new(0.0, 0.0, 0.0),
            end: Point3::new(angle.cos(), angle.sin(), 0.0),
            direction: Vector3::new(angle.cos(), angle.sin(), 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }),
    };
    let curves = vec![
        curve(10, 10, 0.0),
        curve(11, 20, 3.0 * std::f64::consts::FRAC_PI_4),
        curve(12, 110, std::f64::consts::FRAC_PI_2),
        curve(13, 120, -std::f64::consts::FRAC_PI_4),
    ];
    let point = |record_index, byte_offset, incident_curves| SketchPoint {
        id: format!("{stream}:sketch-point#{record_index}"),
        record_index,
        owner_reference: Some(100),
        class_tag: "300".into(),
        byte_offset,
        coordinate_offset: 0,
        entity_genesis: None,
        record_form: crate::records::SketchPointRecordForm::version11(
            u64::from(record_index),
            crate::records::SketchPointClosure::Selector0State0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(0.0, 0.0),
        depth: 0.0,
        companion: Some(crate::records::SketchPointCompanion {
            prefix_present_zero: false,
            reference_encoding: Default::default(),
            incident_curves,
        }),
    };
    let points = vec![point(20, 30, vec![10, 11]), point(21, 130, vec![12, 13])];
    let entity = |record_index, angle: f64| {
        SketchEntity::new(
            SketchEntityId(format!("f3d:model:sketch-entity#line-{record_index}")),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(angle.cos(), angle.sin()),
            },
        )
    };
    let entities = [
        entity(10, 0.0),
        entity(11, 3.0 * std::f64::consts::FRAC_PI_4),
        entity(12, std::f64::consts::FRAC_PI_2),
        entity(13, -std::f64::consts::FRAC_PI_4),
    ];
    let projected = HashMap::from([
        ((stream, 10), &entities[0]),
        ((stream, 11), &entities[1]),
        ((stream, 12), &entities[2]),
        ((stream, 13), &entities[3]),
    ]);
    let mut parameter = parse_design_parameter(&parameter_record(
        Some(1),
        "135 deg",
        "Angular Dimension-2",
        Some("deg"),
        "d1",
        3.0 * std::f64::consts::FRAC_PI_4,
    ))
    .expect("angular parameter");
    parameter.byte_offset = 100;
    let parameter_id = ParameterId("parameter#angle".into());

    assert!(matches!(
        preceding_incident_angular_dimension_definition(
            stream,
            &points,
            &curves,
            &projected,
            &sketch,
            &parameter,
            &parameter_id,
        ),
        Some(SketchConstraintDefinition::Angle {
            first,
            second,
            parameter,
        }) if first == entities[0].id().clone() && second == entities[1].id().clone() && parameter == parameter_id
    ));
}

#[test]
fn owner_scoped_point_dimensions_quotient_coincident_identities() {
    let sketch = SketchId("f3d:model:sketch#point-classes".into());
    let point = |name: &str, u: f64, v: f64| {
        SketchEntity::new(
            SketchEntityId(format!("f3d:model:sketch-entity#{name}")),
            sketch.clone(),
            SketchGeometry::Point {
                position: Point2::new(u, v),
            },
        )
    };
    let lower = point("lower", -53.0, -20.875);
    let lower_duplicate = point("lower-duplicate", -53.0, -20.875 + 5.0e-7);
    let upper = point("upper", -53.0, -7.875);
    let parameter = parse_design_parameter(&parameter_record(
        Some(1),
        "13 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d19",
        1.3,
    ))
    .expect("linear parameter");
    let parameter_id = cadmpeg_ir::features::ParameterId("parameter#point-classes".into());

    assert!(matches!(
        unique_point_class_dimension_definition(
            &[lower.clone(), lower_duplicate, upper.clone()],
            &sketch,
            &parameter,
            &parameter_id,
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::VerticalDistance {
            first: SketchLocus::Entity(first),
            second: SketchLocus::Entity(second),
            parameter,
        }) if first == lower.id().clone() && second == upper.id().clone() && parameter == parameter_id
    ));

    let another_upper = point("another-upper", -40.0, -7.875);
    assert!(unique_point_class_dimension_definition(
        &[lower, upper, another_upper],
        &sketch,
        &parameter,
        &parameter_id,
        1.0e-6,
    )
    .is_none());
}

#[test]
fn radial_locus_groups_use_direct_curves_then_unique_center_witnesses() {
    let sketch = SketchId("f3d:model:sketch#radial-loci".into());
    let point = |id: &str, u, v| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            sketch.clone(),
            SketchGeometry::Point {
                position: Point2::new(u, v),
            },
        )
    };
    let circle = |id: &str, u, v, radius| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            sketch.clone(),
            SketchGeometry::Circle {
                center: Point2::new(u, v),
                radius: Length(radius),
            },
        )
    };
    let center = point("center", 2.0, 3.0);
    let annotation = point("annotation", 7.0, 3.0);
    let measured = circle("measured", 2.0, 3.0, 5.0);
    let other_center = circle("other-center", 20.0, 30.0, 5.0);
    let other_radius = circle("other-radius", 2.0, 3.0, 7.0);
    let all = [
        center.clone(),
        annotation.clone(),
        measured.clone(),
        other_center,
        other_radius,
    ];
    let parameter = cadmpeg_ir::features::ParameterId("parameter#radial-loci".into());

    assert!(matches!(
        radial_locus_dimension_definition(
            &[&measured, &annotation],
            &all,
            "Radial Dimension-2",
            0.5,
            &parameter,
        ),
        Some(SketchConstraintDefinition::Radius { entity, .. }) if entity == measured.id().clone()
    ));
    let repeated = circle("repeated", 12.0, 3.0, 5.0);
    assert!(matches!(
        radial_locus_dimension_definition(
            &[&measured, &annotation, &repeated],
            &all,
            "Diameter Dimension-3",
            1.0,
            &parameter,
        ),
        Some(SketchConstraintDefinition::RepeatedDiameter { entities, parameter: actual })
            if entities == vec![measured.id().clone(), repeated.id().clone()] && actual == parameter
    ));
    assert!(matches!(
        radial_locus_dimension_definition(
            &[&center],
            &all,
            "Diameter Dimension-2",
            1.0,
            &parameter,
        ),
        Some(SketchConstraintDefinition::Diameter { entity, .. }) if entity == measured.id().clone()
    ));
}

#[test]
fn radial_extension_annotations_require_a_point_on_the_line_carrier() {
    let sketch = SketchId("f3d:model:sketch#radial-extension".into());
    let entity =
        |id: &str, geometry| SketchEntity::new(SketchEntityId(id.into()), sketch.clone(), geometry);
    let line = entity(
        "line",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(6.0, 0.0),
        },
    );
    let extension_point = entity(
        "extension-point",
        SketchGeometry::Point {
            position: Point2::new(6.5, 0.0),
        },
    );
    let off_carrier = entity(
        "off-carrier",
        SketchGeometry::Point {
            position: Point2::new(6.5, 0.25),
        },
    );
    let parameter = parse_design_parameter(&parameter_record(
        Some(1),
        "5 mm",
        "Radial Dimension-2",
        Some("mm"),
        "d1",
        0.5,
    ))
    .expect("radial parameter");

    assert!(radial_extension_annotation_group(
        &[&extension_point, &line],
        &parameter,
    ));
    assert!(!radial_extension_annotation_group(
        &[&off_carrier, &line],
        &parameter,
    ));

    let mut linear = parameter;
    linear.source_kind = "Linear Dimension-2".into();
    assert!(!radial_extension_annotation_group(
        &[&extension_point, &line],
        &linear,
    ));
}
