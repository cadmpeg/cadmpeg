use super::*;

fn cylinder_with_radius(radius: f64) -> Entity {
    let mut cylinder = entity("GdtCylinder");
    let mut geometry = Entity {
        class: "PrizMetrik.Geometry.GeoCylinder".into(),
        ..Entity::default()
    };
    geometry.doubles.insert("R".into(), radius);
    geometry.doubles.insert("I".into(), 0.0);
    geometry.doubles.insert("J".into(), 0.0);
    geometry.doubles.insert("K".into(), 1.0);
    cylinder.related.push(RelatedObject {
        name: "NomCylinder".into(),
        class: geometry.class.clone(),
        entity: geometry,
    });
    cylinder
}

fn cylinder_with_radius_and_depth(radius: f64, depth: f64) -> Entity {
    let mut cylinder = cylinder_with_radius(radius);
    for (name, z) in [("NomTop", depth), ("NomBottom", 0.0)] {
        let mut plane = Entity {
            class: "PrizMetrik.Geometry.GeoPlane".into(),
            ..Entity::default()
        };
        plane.doubles.insert("X".into(), 0.0);
        plane.doubles.insert("Y".into(), 0.0);
        plane.doubles.insert("Z".into(), z);
        cylinder.related.push(RelatedObject {
            name: name.into(),
            class: plane.class.clone(),
            entity: plane,
        });
    }
    cylinder
}

fn cone_with_angle_and_top(angle: f64, top: f64) -> Entity {
    let mut cone = entity("GdtCone");
    let mut geometry = Entity {
        class: "PrizMetrik.Geometry.GeoCone".into(),
        ..Entity::default()
    };
    for (name, value) in [
        ("FullAngle", angle),
        ("I", 0.0),
        ("J", 0.0),
        ("K", 1.0),
        ("X", 0.0),
        ("Y", 0.0),
        ("Z", 0.0),
    ] {
        geometry.doubles.insert(name.into(), value);
    }
    cone.related.push(RelatedObject {
        name: "NomCone".into(),
        class: geometry.class.clone(),
        entity: geometry,
    });
    let mut plane = Entity {
        class: "PrizMetrik.Geometry.GeoPlane".into(),
        ..Entity::default()
    };
    for (name, value) in [
        ("I", 0.0),
        ("J", 0.0),
        ("K", -1.0),
        ("X", 0.0),
        ("Y", 0.0),
        ("Z", top),
    ] {
        plane.doubles.insert(name.into(), value);
    }
    cone.related.push(RelatedObject {
        name: "NomTop".into(),
        class: plane.class.clone(),
        entity: plane,
    });
    cone
}

fn plane_with_origin(origin_z: f64) -> Entity {
    let mut feature = entity("GdtPlane");
    let mut plane = Entity {
        class: "PrizMetrik.Geometry.GeoPlane".into(),
        ..Entity::default()
    };
    for (name, value) in [
        ("I", 0.0),
        ("J", 0.0),
        ("K", 1.0),
        ("X", 5.0),
        ("Y", 0.0),
        ("Z", 0.0),
    ] {
        plane.doubles.insert(name.into(), value);
    }
    feature.related.push(RelatedObject {
        name: "NomPlane".into(),
        class: plane.class.clone(),
        entity: plane,
    });
    let mut origin = Entity {
        class: "PrizMetrik.Geometry.GeoPoint".into(),
        ..Entity::default()
    };
    for (name, value) in [("X", 0.0), ("Y", 0.0), ("Z", origin_z)] {
        origin.doubles.insert(name.into(), value);
    }
    feature.related.push(RelatedObject {
        name: "NomOrigin".into(),
        class: origin.class.clone(),
        entity: origin,
    });
    feature
}

fn plane_at(point: [f64; 3], normal: [f64; 3]) -> Entity {
    let mut feature = entity("GdtPlane");
    let mut plane = Entity {
        class: "PrizMetrik.Geometry.GeoPlane".into(),
        ..Entity::default()
    };
    for (name, value) in ["X", "Y", "Z"].into_iter().zip(point) {
        plane.doubles.insert(name.into(), value);
    }
    for (name, value) in ["I", "J", "K"].into_iter().zip(normal) {
        plane.doubles.insert(name.into(), value);
    }
    feature.related.push(RelatedObject {
        name: "NomPlane".into(),
        class: plane.class.clone(),
        entity: plane,
    });
    feature
}

fn cylinder_at(point: [f64; 3], axis: [f64; 3]) -> Entity {
    let mut feature = cylinder_with_radius(3.0);
    let cylinder = &mut feature
        .related
        .first_mut()
        .expect("nominal cylinder")
        .entity;
    for (name, value) in ["X", "Y", "Z"].into_iter().zip(point) {
        cylinder.doubles.insert(name.into(), value);
    }
    for (name, value) in ["I", "J", "K"].into_iter().zip(axis) {
        cylinder.doubles.insert(name.into(), value);
    }
    feature
}

fn distance_annotation(first: Reference, second: Reference, direction: [f64; 3]) -> Entity {
    let mut annotation = entity("GdtDistanceBetween");
    annotation.doubles.insert("Nominal".into(), 0.0);
    annotation.doubles.insert("MinusTolerance".into(), -0.5);
    annotation.doubles.insert("PlusTolerance".into(), 0.5);
    annotation.doubles.insert("LowerLimit".into(), 0.0);
    annotation.doubles.insert("UpperLimit".into(), 0.0);
    annotation.integers.insert("ComputeAnswerBy".into(), 0);
    annotation.integers.insert("Dimension".into(), 0);
    annotation.integers.insert("Direction".into(), 4);
    annotation.integers.insert("NormalTo".into(), 1);
    annotation.features.references = vec![first, second];
    let mut transform = Entity {
        class: "PrizMetrik.Geometry.GeoTransform".into(),
        ..Entity::default()
    };
    for (name, value) in [
        ("R1C1", 1.0),
        ("R1C2", 0.0),
        ("R1C3", 0.0),
        ("R2C1", 0.0),
        ("R2C2", 1.0),
        ("R2C3", 0.0),
        ("R3C1", 0.0),
        ("R3C2", 0.0),
        ("R3C3", 1.0),
        ("X", 0.0),
        ("Y", 0.0),
        ("Z", 0.0),
    ] {
        transform.doubles.insert(name.into(), value);
    }
    annotation.related.push(RelatedObject {
        name: "NominalTransform".into(),
        class: transform.class.clone(),
        entity: transform,
    });
    let mut vector = Entity {
        class: "PrizMetrik.Geometry.GeoUnitVector".into(),
        ..Entity::default()
    };
    for (name, value) in ["I", "J", "K"].into_iter().zip(direction) {
        vector.doubles.insert(name.into(), value);
    }
    annotation.related.push(RelatedObject {
        name: "DirectionVector".into(),
        class: vector.class.clone(),
        entity: vector,
    });
    annotation
}

fn feature_with_nominal_measurement(
    feature_class: &str,
    object_name: &str,
    geometry_class: &str,
    field: &str,
    value: f64,
) -> Entity {
    let mut feature = entity(feature_class);
    let mut geometry = Entity {
        class: format!("PrizMetrik.Geometry.{geometry_class}"),
        ..Entity::default()
    };
    geometry.doubles.insert(field.into(), value);
    feature.related.push(RelatedObject {
        name: object_name.into(),
        class: geometry.class.clone(),
        entity: geometry,
    });
    feature
}

#[test]
fn rendered_diameter_resolves_rounded_applied_geometry() {
    let mut root = semantic_root();
    *root.features.entities.get_mut(1).expect("first cylinder") = cylinder_with_radius(1.984_375);
    *root.features.entities.get_mut(2).expect("second cylinder") = cylinder_with_radius(1.984_375);
    let diameter = root.annotations.entities.get_mut(2).expect("diameter");
    diameter
        .integers
        .insert("BlockToleranceDecimalPlaces".into(), 3);
    diameter.doubles.insert("MinusTolerance".into(), 0.0);
    diameter.doubles.insert("PlusTolerance".into(), 0.0);
    diameter.doubles.insert("LowerLimit".into(), 3.8);
    diameter.doubles.insert("UpperLimit".into(), 4.1);
    let displayed = [RenderedDimension {
        kind: RenderedDimensionKind::Diameter,
        value: 0.156,
        decimal_places: 3,
    }];
    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &displayed, &mut annotations);
    let diameter = annotations
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Diameter 1"))
        .expect("diameter annotation");
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        tolerance:
            Some(DimensionTolerance::PlusMinus {
                lower: lower_deviation,
                upper: upper_deviation,
            }),
        ..
    } = &diameter.definition
    else {
        panic!("dimension definition");
    };
    assert!(approximately_equal(nominal.value.get(), 3.962_4));
    assert!(approximately_equal(lower_deviation.value.get(), -0.162_4));
    assert!(approximately_equal(upper_deviation.value.get(), 0.137_6));

    root.annotations
        .entities
        .get_mut(2)
        .expect("diameter")
        .features
        .references = vec![reference("FP", "GdtPattern")];
    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &displayed, &mut annotations);
    let diameter = annotations
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Diameter 1"))
        .expect("diameter annotation");
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &diameter.definition
    else {
        panic!("dimension definition");
    };
    assert!(approximately_equal(nominal.value.get(), 3.962_4));
}

#[test]
fn conflicting_pattern_sizes_do_not_resolve_a_nominal() {
    let mut root = semantic_root();
    *root.features.entities.get_mut(1).expect("first cylinder") = cylinder_with_radius(2.5);
    *root.features.entities.get_mut(2).expect("second cylinder") = cylinder_with_radius(3.0);
    let diameter = root.annotations.entities.get_mut(2).expect("diameter");
    diameter
        .integers
        .insert("BlockToleranceDecimalPlaces".into(), 1);
    diameter.features.references = vec![reference("FP", "GdtPattern")];
    let mut annotations = project(&root);
    enrich_implicit_nominals(
        &root,
        &[RenderedDimension {
            kind: RenderedDimensionKind::Diameter,
            value: 5.0,
            decimal_places: 1,
        }],
        &mut annotations,
    );
    assert_eq!(dimension_nominal(&annotations, "A30"), None);
}

#[test]
fn numerically_equivalent_pattern_sizes_supply_diameter_without_rendered_text() {
    let mut root = semantic_root();
    *root.features.entities.get_mut(1).expect("first cylinder") = cylinder_with_radius(2.5);
    *root.features.entities.get_mut(2).expect("second cylinder") =
        cylinder_with_radius(2.500_002_5);
    root.annotations
        .entities
        .get_mut(2)
        .expect("diameter")
        .features
        .references = vec![reference("FP", "GdtPattern")];
    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("A30").unwrap())
        .expect("pattern diameter")
        .definition
    else {
        panic!("dimension definition");
    };
    assert_eq!(*nominal, length(5.0).expect("finite length"));
}

#[test]
fn diameter_equivalence_does_not_merge_distinct_sizes() {
    assert_eq!(unique_diameter(&[10.0, 10.000_005]), Some(10.0));
    assert_eq!(unique_diameter(&[10.0, 10.000_02]), None);
}

#[test]
fn empty_pattern_does_not_bind_an_unrelated_rendered_diameter() {
    let mut root = semantic_root();
    root.features
        .entities
        .first_mut()
        .and_then(|pattern| pattern.related.first_mut())
        .expect("pattern members")
        .entity
        .related
        .clear();
    root.annotations
        .entities
        .get_mut(2)
        .expect("diameter")
        .features
        .references = vec![reference("FP", "GdtPattern")];

    let mut annotations = project(&root);
    enrich_implicit_nominals(
        &root,
        &[RenderedDimension {
            kind: RenderedDimensionKind::Diameter,
            value: 0.25,
            decimal_places: 3,
        }],
        &mut annotations,
    );
    assert_eq!(dimension_nominal(&annotations, "A30"), None);
}

#[test]
fn counterbore_pattern_supplies_distinct_hole_diameter() {
    let mut root = semantic_root();
    *root
        .features
        .entities
        .get_mut(1)
        .expect("counterbore cylinder") = cylinder_with_radius(5.0);
    *root.features.entities.get_mut(2).expect("hole cylinder") = cylinder_with_radius(3.0);
    root.annotations
        .entities
        .get_mut(2)
        .expect("diameter")
        .features
        .references = vec![reference("FP", "GdtPattern")];

    let mut counterbore = entity("GdtCounterBore");
    counterbore.doubles.insert("Nominal".into(), 0.0);
    counterbore.features.references = vec![
        reference("FP", "GdtPattern"),
        reference("F20", "GdtCylinder"),
    ];
    root.annotations
        .references
        .push(reference("A50", "GdtCounterBore"));
    root.annotations.entities.push(counterbore);

    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("A30").unwrap())
        .expect("pattern diameter")
        .definition
    else {
        panic!("dimension definition");
    };
    assert_eq!(*nominal, length(6.0).expect("finite length"));
}

#[test]
fn unrelated_counterbore_size_does_not_select_a_pattern_diameter() {
    let mut root = semantic_root();
    *root.features.entities.get_mut(1).expect("first cylinder") = cylinder_with_radius(5.0);
    *root.features.entities.get_mut(2).expect("second cylinder") = cylinder_with_radius(3.0);
    root.annotations
        .entities
        .get_mut(2)
        .expect("diameter")
        .features
        .references = vec![reference("FP", "GdtPattern")];
    root.features
        .references
        .push(reference("FCB", "GdtCylinder"));
    root.features.entities.push(cylinder_with_radius(4.0));

    let mut counterbore = entity("GdtCounterBore");
    counterbore.doubles.insert("Nominal".into(), 0.0);
    counterbore.features.references = vec![
        reference("FP", "GdtPattern"),
        reference("FCB", "GdtCylinder"),
    ];
    root.annotations
        .references
        .push(reference("A50", "GdtCounterBore"));
    root.annotations.entities.push(counterbore);

    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    assert_eq!(dimension_nominal(&annotations, "A30"), None);
}

#[test]
fn direct_cylinder_and_sphere_supply_diameter_without_rendered_text() {
    let mut root = semantic_root();
    *root.features.entities.get_mut(1).expect("direct cylinder") = cylinder_with_radius(17.5);
    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("A30").unwrap())
        .expect("direct diameter")
        .definition
    else {
        panic!("dimension definition");
    };
    assert_eq!(*nominal, length(35.0).expect("finite length"));

    *root.features.entities.get_mut(1).expect("direct sphere") =
        feature_with_nominal_measurement("GdtSphere", "NomSphere", "GeoSphere", "R", 15.875);
    root.features
        .references
        .get_mut(1)
        .expect("direct feature reference")
        .class = "PrizMetrik.GdtAnalysis.GdtSphere,gdtanalysis.net".into();
    root.annotations
        .entities
        .get_mut(2)
        .and_then(|diameter| diameter.features.references.first_mut())
        .expect("diameter feature reference")
        .class = "PrizMetrik.GdtAnalysis.GdtSphere,gdtanalysis.net".into();
    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("A30").unwrap())
        .expect("direct diameter")
        .definition
    else {
        panic!("dimension definition");
    };
    assert_eq!(*nominal, length(31.75).expect("finite length"));
}

#[test]
fn conflicting_rendered_units_do_not_resolve_a_nominal() {
    assert_eq!(
        rendered_nominal(
            5.0,
            1,
            RenderedDimensionKind::Diameter,
            &[
                RenderedDimension {
                    kind: RenderedDimensionKind::Diameter,
                    value: 5.0,
                    decimal_places: 1,
                },
                RenderedDimension {
                    kind: RenderedDimensionKind::Diameter,
                    value: 0.2,
                    decimal_places: 1,
                },
            ],
        ),
        None
    );
}

#[test]
fn directional_plane_distance_supplies_location_nominal() {
    let mut root = semantic_root();
    root.features.references.push(reference("FL1", "GdtPlane"));
    root.features
        .entities
        .push(plane_at([3.0, 4.0, 5.0], [0.0, 0.0, 1.0]));
    root.features.references.push(reference("FL2", "GdtPlane"));
    root.features
        .entities
        .push(plane_at([8.0, 9.0, 25.0], [0.0, 0.0, 1.0]));
    root.annotations
        .references
        .push(reference("A50", "GdtDistanceBetween"));
    root.annotations.entities.push(distance_annotation(
        reference("FL1", "GdtPlane"),
        reference("FL2", "GdtPlane"),
        [0.0, 0.0, -1.0],
    ));

    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        tolerance:
            Some(DimensionTolerance::PlusMinus {
                lower: lower_deviation,
                upper: upper_deviation,
            }),
        ..
    } = &annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("A50").unwrap())
        .expect("location dimension")
        .definition
    else {
        panic!("dimension definition");
    };
    assert_eq!(*nominal, length(20.0).expect("finite length"));
    assert_eq!(*lower_deviation, length(-0.5).expect("finite length"));
    assert_eq!(*upper_deviation, length(0.5).expect("finite length"));

    *root.features.entities.last_mut().expect("second plane") =
        plane_at([8.0, 9.0, 25.0], [1.0, 0.0, 0.0]);
    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    assert_eq!(dimension_nominal(&annotations, "A50"), None);
}

#[test]
fn directional_compound_hole_axes_supply_location_nominal() {
    let mut root = semantic_root();
    for (hole_id, cylinder_id, y) in [("FH1", "FC1", 210.0), ("FH2", "FC2", 285.0)] {
        let mut hole = entity("GdtCompoundHole");
        hole.features
            .references
            .push(reference(cylinder_id, "GdtCylinder"));
        root.features
            .references
            .push(reference(hole_id, "GdtCompoundHole"));
        root.features.entities.push(hole);
        root.features
            .references
            .push(reference(cylinder_id, "GdtCylinder"));
        root.features
            .entities
            .push(cylinder_at([230.0, y, 27.0], [0.0, 0.0, 1.0]));
    }
    root.annotations
        .references
        .push(reference("A50", "GdtDistanceBetween"));
    root.annotations.entities.push(distance_annotation(
        reference("FH1", "GdtCompoundHole"),
        reference("FH2", "GdtCompoundHole"),
        [0.0, 1.0, 0.0],
    ));

    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("A50").unwrap())
        .expect("hole-axis location")
        .definition
    else {
        panic!("dimension definition");
    };
    assert_eq!(*nominal, length(75.0).expect("finite length"));
}

#[test]
fn closed_slot_end_feature_supplies_length_location_nominal() {
    let mut root = semantic_root();
    root.features
        .references
        .push(reference("FSC", "GdtCylinder"));
    root.features
        .entities
        .push(cylinder_at([38.1, 3.037_84, -95.25], [0.0, 1.0, 0.0]));
    root.features
        .entities
        .last_mut()
        .and_then(|cylinder| cylinder.related.first_mut())
        .expect("nominal cylinder")
        .entity
        .doubles
        .insert("R".into(), 3.175);
    let mut slot = entity("GdtCompoundClosedSlot3D");
    slot.features
        .references
        .push(reference("FSC", "GdtCylinder"));
    let mut geometry = Entity {
        class: "PrizMetrik.Geometry.GeoClosedSlot".into(),
        ..Entity::default()
    };
    for (name, value) in [
        ("I", 0.0),
        ("J", -1.0),
        ("K", 0.0),
        ("LongitudeI", -1.0),
        ("LongitudeJ", 0.0),
        ("LongitudeK", 0.0),
        ("X", 47.625),
        ("Y", 3.037_84),
        ("Z", -95.25),
        ("Length", 25.4),
        ("Width", 6.35),
    ] {
        geometry.doubles.insert(name.into(), value);
    }
    slot.related.push(RelatedObject {
        name: "NomClosedSlot".into(),
        class: geometry.class.clone(),
        entity: geometry,
    });
    root.features
        .references
        .push(reference("FS", "GdtCompoundClosedSlot3D"));
    root.features.entities.push(slot);
    root.annotations
        .references
        .push(reference("A50", "GdtDistanceBetween"));
    let mut distance = distance_annotation(
        reference("FSC", "GdtCylinder"),
        reference("FS", "GdtCompoundClosedSlot3D"),
        [-1.0, 0.0, 0.0],
    );
    distance.integers.insert("FeatureFosUsage".into(), 2);
    distance.integers.insert("OriginFeatureFosUsage".into(), 2);
    root.annotations.entities.push(distance);

    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("A50").unwrap())
        .expect("slot length location")
        .definition
    else {
        panic!("dimension definition");
    };
    assert_eq!(*nominal, length(25.4).expect("finite length"));

    root.features
        .entities
        .last_mut()
        .and_then(|slot| slot.related.first_mut())
        .expect("nominal slot")
        .entity
        .doubles
        .insert("Width".into(), 7.0);
    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    assert_eq!(dimension_nominal(&annotations, "A50"), None);
}

#[test]
fn rendered_depth_resolves_axial_nominal_planes() {
    let mut root = semantic_root();
    *root.features.entities.get_mut(1).expect("first cylinder") =
        cylinder_with_radius_and_depth(2.5, 7.625);
    let mut depth = entity("GdtDepth");
    depth.strings.insert("ObjectName".into(), "Depth 1".into());
    depth
        .integers
        .insert("BlockToleranceDecimalPlaces".into(), 2);
    depth.doubles.insert("Nominal".into(), 0.0);
    depth.doubles.insert("MinusTolerance".into(), -0.254);
    depth.doubles.insert("PlusTolerance".into(), 0.254);
    depth.doubles.insert("LowerLimit".into(), 0.0);
    depth.doubles.insert("UpperLimit".into(), 0.0);
    depth
        .features
        .references
        .push(reference("F20", "GdtCylinder"));
    root.annotations
        .references
        .push(reference("A50", "GdtDepth"));
    root.annotations.entities.push(depth);

    let mut annotations = project(&root);
    enrich_implicit_nominals(
        &root,
        &[RenderedDimension {
            kind: RenderedDimensionKind::Depth,
            value: 0.3,
            decimal_places: 2,
        }],
        &mut annotations,
    );
    let depth = annotations
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Depth 1"))
        .expect("depth annotation");
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &depth.definition
    else {
        panic!("dimension definition");
    };
    assert!(approximately_equal(nominal.value.get(), 7.62));
}

#[test]
fn direct_and_thread_cylinders_supply_depth_without_rendered_text() {
    let mut root = semantic_root();
    *root.features.entities.get_mut(1).expect("direct cylinder") =
        cylinder_with_radius_and_depth(5.0, 14.2875);
    let mut depth = entity("GdtDepth");
    depth.doubles.insert("Nominal".into(), 0.0);
    depth
        .features
        .references
        .push(reference("F20", "GdtCylinder"));
    root.annotations
        .references
        .push(reference("A50", "GdtDepth"));
    root.annotations.entities.push(depth);

    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("A50").unwrap())
        .expect("direct depth annotation")
        .definition
    else {
        panic!("dimension definition");
    };
    assert!(approximately_equal(nominal.value.get(), 14.2875));

    root.annotations
        .entities
        .last_mut()
        .expect("thread-depth annotation")
        .integers
        .insert("IsThreadDepth".into(), 1);
    let cylinder = root
        .features
        .entities
        .get_mut(1)
        .expect("threaded cylinder");
    cylinder.integers.insert("IsThreaded".into(), 1);
    cylinder.doubles.insert("ThreadDepth".into(), 12.0);
    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("A50").unwrap())
        .expect("thread depth annotation")
        .definition
    else {
        panic!("dimension definition");
    };
    assert_eq!(*nominal, length(12.0).expect("finite length"));
}

#[test]
fn counterbore_bottom_plane_resolves_sibling_cylinder_depth() {
    let mut root = semantic_root();
    root.features
        .references
        .push(reference("FCB", "GdtCylinder"));
    root.features
        .entities
        .push(cylinder_with_radius_and_depth(5.0, 12.7));
    root.features.references.push(reference("FDP", "GdtPlane"));
    root.features.entities.push(plane_with_origin(0.0));

    let mut counterbore = entity("GdtCounterBore");
    counterbore.doubles.insert("Nominal".into(), 0.0);
    counterbore.features.references = vec![
        reference("FP", "GdtPattern"),
        reference("FCB", "GdtCylinder"),
    ];
    root.annotations
        .references
        .push(reference("ACB", "GdtCounterBore"));
    root.annotations.entities.push(counterbore);
    let mut depth = entity("GdtDepth");
    depth.doubles.insert("Nominal".into(), 0.0);
    depth.features.references = vec![reference("FP", "GdtPattern"), reference("FDP", "GdtPlane")];
    root.annotations
        .references
        .push(reference("AD", "GdtDepth"));
    root.annotations.entities.push(depth);

    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("AD").unwrap())
        .expect("counterbore depth")
        .definition
    else {
        panic!("dimension definition");
    };
    assert_eq!(*nominal, length(12.7).expect("finite length"));

    root.features
        .entities
        .last_mut()
        .and_then(|plane| plane.related.get_mut(1))
        .expect("depth-plane origin")
        .entity
        .doubles
        .insert("Z".into(), 1.0);
    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    assert_eq!(dimension_nominal(&annotations, "AD"), None);
}

#[test]
fn semantic_slot_dimensions_resolve_exact_nominals() {
    let mut root = semantic_root();
    root.features
        .references
        .push(reference("FW", "GdtCompoundWidth"));
    root.features
        .entities
        .push(feature_with_nominal_measurement(
            "GdtCompoundWidth",
            "NomCompoundWidth",
            "GeoOpenSlot",
            "Width",
            12.7,
        ));
    root.features
        .references
        .push(reference("FL", "GdtCompoundClosedSlot3D"));
    root.features
        .entities
        .push(feature_with_nominal_measurement(
            "GdtCompoundClosedSlot3D",
            "NomClosedSlot",
            "GeoClosedSlot",
            "Length",
            38.1,
        ));
    let mut width = entity("GdtWidth");
    width.strings.insert("ObjectName".into(), "Width 1".into());
    width.doubles.insert("Nominal".into(), 0.0);
    width.doubles.insert("MinusTolerance".into(), 0.0);
    width.doubles.insert("PlusTolerance".into(), 0.0);
    width.doubles.insert("LowerLimit".into(), 12.5);
    width.doubles.insert("UpperLimit".into(), 12.9);
    width
        .features
        .references
        .push(reference("FW", "GdtCompoundWidth"));
    root.annotations
        .references
        .push(reference("A50", "GdtWidth"));
    root.annotations.entities.push(width);
    let mut length = entity("GdtLength");
    length
        .strings
        .insert("ObjectName".into(), "Length 1".into());
    length.doubles.insert("Nominal".into(), 0.0);
    length
        .features
        .references
        .push(reference("FL", "GdtCompoundClosedSlot3D"));
    root.annotations
        .references
        .push(reference("A60", "GdtLength"));
    root.annotations.entities.push(length);

    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    let width = annotations
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Width 1"))
        .expect("width annotation");
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        tolerance:
            Some(DimensionTolerance::PlusMinus {
                lower: lower_deviation,
                upper: upper_deviation,
            }),
        ..
    } = &width.definition
    else {
        panic!("dimension definition");
    };
    assert!(approximately_equal(nominal.value.get(), 12.7));
    assert!(approximately_equal(lower_deviation.value.get(), -0.2));
    assert!(approximately_equal(upper_deviation.value.get(), 0.2));
    let length = annotations
        .iter()
        .find(|annotation| annotation.name.as_deref() == Some("Length 1"))
        .expect("length annotation");
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &length.definition
    else {
        panic!("dimension definition");
    };
    assert!(approximately_equal(nominal.value.get(), 38.1));
}

#[test]
fn compound_hole_dimensions_use_direct_operation_geometry() {
    let mut root = semantic_root();
    *root
        .features
        .entities
        .get_mut(1)
        .expect("first pattern member") = cylinder_with_radius(3.0);
    *root
        .features
        .entities
        .get_mut(2)
        .expect("second pattern member") = cylinder_with_radius(3.0);
    root.features
        .references
        .push(reference("FCB", "GdtCylinder"));
    root.features.entities.push(cylinder_with_radius(7.9375));
    root.features.references.push(reference("FCS", "GdtCone"));
    root.features
        .entities
        .push(cone_with_angle_and_top(std::f64::consts::FRAC_PI_2, 10.0));

    for (id, class, feature) in [
        ("ACB", "GdtCounterBore", "FCB"),
        ("ACSD", "GdtCounterSinkDiameter", "FCS"),
        ("ACSA", "GdtCounterSinkAngle", "FCS"),
    ] {
        let mut annotation = entity(class);
        annotation.doubles.insert("Nominal".into(), 0.0);
        annotation
            .features
            .references
            .push(reference("FP", "GdtPattern"));
        annotation.features.references.push(reference(
            feature,
            if feature == "FCB" {
                "GdtCylinder"
            } else {
                "GdtCone"
            },
        ));
        root.annotations.references.push(reference(id, class));
        root.annotations.entities.push(annotation);
    }

    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    for (id, expected, quantity) in [
        ("ACB", 15.875, PmiQuantity::Length),
        ("ACSD", 20.0, PmiQuantity::Length),
        ("ACSA", std::f64::consts::FRAC_PI_2, PmiQuantity::Angle),
    ] {
        let PmiDefinition::Dimension {
            nominal: Some(nominal),
            ..
        } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id(id).unwrap())
            .expect("compound-hole annotation")
            .definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(nominal.quantity, quantity);
        assert!(approximately_equal(nominal.value.get(), expected));
    }
}

#[test]
fn semantic_slot_width_traverses_patterns_and_rejects_disagreement() {
    let mut root = semantic_root();
    for (index, value) in [(1, 9.525), (2, 9.525)] {
        *root
            .features
            .entities
            .get_mut(index)
            .expect("pattern member") = feature_with_nominal_measurement(
            "GdtCompoundClosedSlot3D",
            "NomClosedSlot",
            "GeoClosedSlot",
            "Width",
            value,
        );
    }
    let pattern_members = root
        .features
        .entities
        .first_mut()
        .and_then(|pattern| pattern.related.first_mut())
        .expect("pattern members");
    for applied in &mut pattern_members.entity.related {
        applied
            .entity
            .features
            .references
            .first_mut()
            .expect("pattern member reference")
            .class = "PrizMetrik.GdtAnalysis.GdtCompoundClosedSlot3D,gdtanalysis.net".into();
    }
    let mut width = entity("GdtWidth");
    width.doubles.insert("Nominal".into(), 0.0);
    width
        .features
        .references
        .push(reference("FP", "GdtPattern"));
    root.annotations
        .references
        .push(reference("A50", "GdtWidth"));
    root.annotations.entities.push(width);

    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        ..
    } = &annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("A50").unwrap())
        .expect("width annotation")
        .definition
    else {
        panic!("dimension definition");
    };
    assert!(approximately_equal(nominal.value.get(), 9.525));

    root.features
        .entities
        .get_mut(2)
        .and_then(|feature| feature.related.first_mut())
        .expect("second nominal slot")
        .entity
        .doubles
        .insert("Width".into(), 6.35);
    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    assert_eq!(dimension_nominal(&annotations, "A50"), None);
}

#[test]
fn semantic_radius_resolves_fillets_cylinders_and_spheres() {
    let mut root = semantic_root();
    let mut fillet = entity("GdtFillet");
    fillet.doubles.insert("Radius".into(), 3.175);
    *root.features.entities.get_mut(1).expect("first member") = fillet;
    *root.features.entities.get_mut(2).expect("second member") = cylinder_with_radius(3.175);
    root.features
        .references
        .get_mut(1)
        .expect("first feature reference")
        .class = "PrizMetrik.GdtAnalysis.GdtFillet,gdtanalysis.net".into();
    let pattern_members = root
        .features
        .entities
        .first_mut()
        .and_then(|pattern| pattern.related.first_mut())
        .expect("pattern members");
    for (applied, class) in pattern_members
        .entity
        .related
        .iter_mut()
        .zip(["GdtFillet", "GdtCylinder"])
    {
        applied
            .entity
            .features
            .references
            .first_mut()
            .expect("pattern member reference")
            .class = format!("PrizMetrik.GdtAnalysis.{class},gdtanalysis.net");
    }
    let mut radius = entity("GdtRadius");
    radius.doubles.insert("Nominal".into(), 0.0);
    radius.doubles.insert("MinusTolerance".into(), -0.1);
    radius.doubles.insert("PlusTolerance".into(), 0.0);
    radius.doubles.insert("UpperLimit".into(), 0.0);
    radius
        .features
        .references
        .push(reference("FP", "GdtPattern"));
    root.annotations
        .references
        .push(reference("A50", "GdtRadius"));
    root.annotations.entities.push(radius);

    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    let PmiDefinition::Dimension {
        nominal: Some(nominal),
        tolerance:
            Some(DimensionTolerance::PlusMinus {
                upper: upper_deviation,
                ..
            }),
        ..
    } = &annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("A50").unwrap())
        .expect("radius annotation")
        .definition
    else {
        panic!("dimension definition");
    };
    assert!(approximately_equal(nominal.value.get(), 3.175));
    assert_eq!(*upper_deviation, length(0.0).expect("finite length"));

    *root.features.entities.get_mut(2).expect("second member") =
        feature_with_nominal_measurement("GdtSphere", "NomSphere", "GeoSphere", "R", 4.0);
    root.features
        .references
        .get_mut(2)
        .expect("second feature reference")
        .class = "PrizMetrik.GdtAnalysis.GdtSphere,gdtanalysis.net".into();
    root.features
        .entities
        .first_mut()
        .and_then(|pattern| pattern.related.first_mut())
        .and_then(|members| members.entity.related.get_mut(1))
        .and_then(|applied| applied.entity.features.references.first_mut())
        .expect("second pattern member reference")
        .class = "PrizMetrik.GdtAnalysis.GdtSphere,gdtanalysis.net".into();
    let mut annotations = project(&root);
    enrich_implicit_nominals(&root, &[], &mut annotations);
    assert_eq!(dimension_nominal(&annotations, "A50"), None);
}

#[test]
fn scans_explicit_rendered_diameter_literals() {
    let mut payload = Vec::new();
    for text in [
        "<COUNT=#X ><MOD-DIAM> .156",
        "<MOD-DIAM> <sft_holeDia>",
        "<MOD-DIAM> .281<HOLE-SPOT><MOD-DIAM> .438",
        "<MOD-DIAM> .250 <HOLE-DEPTH> .30",
    ] {
        payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
        payload.push(u8::try_from(text.encode_utf16().count()).expect("fixture length"));
        for unit in text.encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
    }
    assert_eq!(
        rendered_dimensions(&payload),
        [
            RenderedDimension {
                kind: RenderedDimensionKind::Diameter,
                value: 0.156,
                decimal_places: 3,
            },
            RenderedDimension {
                kind: RenderedDimensionKind::Diameter,
                value: 0.281,
                decimal_places: 3,
            },
            RenderedDimension {
                kind: RenderedDimensionKind::Diameter,
                value: 0.438,
                decimal_places: 3,
            },
            RenderedDimension {
                kind: RenderedDimensionKind::Diameter,
                value: 0.25,
                decimal_places: 3,
            },
            RenderedDimension {
                kind: RenderedDimensionKind::Depth,
                value: 0.3,
                decimal_places: 2,
            },
        ]
    );
}
fn zero_nominal_angle_root() -> Entity {
    let mut angle = entity("GdtAngleBetween");
    angle.strings.insert("ObjectName".into(), "Angle 1".into());
    angle.integers.insert("Dimension".into(), 0);
    angle.doubles.insert("Nominal".into(), 0.0);
    angle.doubles.insert("MinusTolerance".into(), -0.5);
    angle.doubles.insert("PlusTolerance".into(), 0.5);

    let mut root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    root.annotations.references = vec![reference("A60", "GdtAngleBetween")];
    root.annotations.entities = vec![angle];
    root
}

#[test]
fn zero_nominal_dimension_keeps_the_annotation_without_a_nominal() {
    let root = zero_nominal_angle_root();
    let annotations = project(&root);
    let annotation = annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("A60").unwrap())
        .expect("zero nominal angle");
    let PmiDefinition::Dimension {
        dimension,
        nominal,
        tolerance,
    } = &annotation.definition
    else {
        panic!("dimension definition");
    };
    assert_eq!(*dimension, DimensionKind::Angular);
    assert_eq!(*nominal, None);
    assert_eq!(
        *tolerance,
        Some(DimensionTolerance::PlusMinus {
            lower: pmi_value(-0.5, PmiQuantity::Angle).expect("finite angle"),
            upper: pmi_value(0.5, PmiQuantity::Angle).expect("finite angle"),
        })
    );
}

#[test]
fn dimension_without_a_nominal_key_is_skipped() {
    let mut root = zero_nominal_angle_root();
    root.annotations
        .entities
        .get_mut(0)
        .expect("angle")
        .doubles
        .remove("Nominal");
    let annotations = project(&root);
    assert!(!annotations
        .iter()
        .any(|annotation| annotation.id == pmi_id("A60").unwrap()));
}
