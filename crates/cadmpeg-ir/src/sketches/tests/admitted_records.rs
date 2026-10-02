// SPDX-License-Identifier: Apache-2.0
use crate::math::{Point2, Point3, Vector3};
use crate::scalar::{Angle, FiniteReal, Length, PositiveLength, PositiveReal};
use crate::sketches::{
    EllipseRadii, SketchGeometry, SketchGeometryDefinition, SpatialSketchConstraintDefinition,
    SpatialSketchConstraintDefinitionInput, SpatialSketchEntityId, SpatialSketchEntityUse,
    SpatialSketchGeometry, SpatialSketchGeometryDefinition, SpatialSketchProfile, TextPlacement,
};
use crate::units::{FinitePoint2, UnitVector3};

fn length(value: f64) -> Length {
    Length::new(value).unwrap()
}

#[test]
fn spatial_profile_uniqueness_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let boundary = vec![SpatialSketchEntityUse {
        entity: SpatialSketchEntityId::mint("test:model:entity#profile-edge").unwrap(),
        reversed: false,
    }];
    let result = SpatialSketchProfile::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        boundary,
        &ctx,
        "test spatial profile uniqueness",
    );
    assert!(matches!(result, Err(CodecError::ResourceLimit(failure))
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "test spatial profile uniqueness"));
}

#[test]
fn polygon_uniqueness_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let entities = ["first", "second", "third"]
        .map(|suffix| {
            crate::sketches::SketchEntityId::mint(format!("test:model:entity#{suffix}")).unwrap()
        })
        .into_iter()
        .collect();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 2;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = crate::sketches::SketchPolygon::try_new(
        entities,
        &ctx,
        "test polygon uniqueness",
    );
    assert!(matches!(result, Err(CodecError::ResourceLimit(failure))
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "test polygon uniqueness"));
}

#[test]
fn planar_offset_parameter_setter_preserves_admitted_pairs() {
    use crate::features::ParameterId;
    use crate::sketches::{
        OffsetParameter, SketchConstraintDefinition, SketchConstraintDefinitionInput,
        SketchEntityId, SketchOffsetPair,
    };

    let source = SketchEntityId::mint("test:model:entity#source").unwrap();
    let result = SketchEntityId::mint("test:model:entity#result").unwrap();
    let pair = SketchOffsetPair {
        source,
        result,
        source_reversed: false,
    };
    let mut definition =
        SketchConstraintDefinition::try_from(SketchConstraintDefinitionInput::Offset {
            pairs: vec![pair.clone()],
            distance: length(5.0),
            parameter: None,
        })
        .unwrap();
    let parameter = OffsetParameter {
        id: ParameterId::mint("test:model:parameter#distance").unwrap(),
        negated: true,
    };
    assert!(definition.set_offset_parameter(parameter.clone()));
    assert!(matches!(definition.kind(),
        SketchConstraintDefinitionInput::Offset { pairs, parameter: Some(driving), .. }
            if pairs == &[pair] && driving == &parameter));
}

#[test]
fn sketch_ellipse_serialization_keeps_its_wire_fields() {
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
        center: Point2::new(1.0, 2.0),
        major_angle: Angle::new(0.5).unwrap(),
        radii: EllipseRadii {
            major_radius: length(3.0),
            minor_radius: length(2.0),
        },
        bounds: None,
    })
    .unwrap();
    assert_eq!(
        serde_json::to_string(&geometry).unwrap(),
        r#"{"kind":"ellipse","center":{"u":1.0,"v":2.0},"major_angle":0.5,"major_radius":3.0,"minor_radius":2.0}"#
    );
}

#[test]
fn sketch_ellipse_rejects_unknown_and_duplicate_wire_fields() {
    for (wire, expected) in [
        (
            r#"{"kind":"ellipse","center":{"u":1.0,"v":2.0},"major_angle":0.5,"major_radius":3.0,"minor_radius":2.0,"zz_bogus":1}"#,
            "zz_bogus",
        ),
        (
            r#"{"kind":"ellipse","center":{"u":1.0,"v":2.0},"major_angle":0.5,"major_radius":3.0,"major_radius":4.0,"minor_radius":2.0}"#,
            "duplicate field `major_radius`",
        ),
    ] {
        let error = serde_json::from_str::<SketchGeometry>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn a_sketch_geometry_holds_its_admitted_definition_and_takes_admitted_parts() {
    let ellipse = SketchGeometryDefinition::Ellipse {
        center: Point2::new(1.0, 2.0),
        major_angle: Angle::new(0.5).unwrap(),
        radii: EllipseRadii {
            major_radius: length(3.0),
            minor_radius: length(2.0),
        },
        bounds: None,
    };
    let geometry = SketchGeometry::try_from(ellipse.clone()).unwrap();
    let SketchGeometryDefinition::Ellipse { center, radii, .. } = geometry.definition() else {
        panic!("ellipse")
    };
    assert_eq!(*center, FinitePoint2::new(Point2::new(1.0, 2.0)).unwrap());
    assert_eq!(radii.major(), PositiveLength::new(3.0).unwrap());
    assert_eq!(radii.minor(), PositiveLength::new(2.0).unwrap());
    assert_eq!(geometry.definition().to_raw(), ellipse);

    // The typed route tests only the conditions between fields.
    let admitted = |major: f64, minor: f64| SketchGeometryDefinition::Ellipse {
        center: FinitePoint2::new(Point2::new(0.0, 0.0)).unwrap(),
        major_angle: Angle::new(0.0).unwrap(),
        radii: EllipseRadii {
            major_radius: PositiveLength::new(major).unwrap(),
            minor_radius: PositiveLength::new(minor).unwrap(),
        },
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
        boundary.clone(), &cadmpeg_test_support::service_decode_context(), "spatial profile uniqueness").expect("fixture collection admission")
    .is_ok());
    assert_eq!(
        SpatialSketchProfile::from_parts(
            origin,
            unit(0.0, 0.0, 1.0),
            unit(0.0, 0.0, 1.0),
            boundary, &cadmpeg_test_support::service_decode_context(), "spatial profile uniqueness").expect("fixture collection admission")
        .unwrap_err()
        .to_string(),
        "spatial profile normal and u_axis must be unit and orthogonal"
    );
}

#[test]
fn polygon_uniqueness_refuses_scoped_storage_and_work() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::WorkUnits,
    ] {
        let entities = ["first", "second", "third"]
            .map(|suffix| {
                crate::sketches::SketchEntityId::mint(format!("test:model:entity#{suffix}"))
                    .unwrap()
            })
            .into_iter()
            .collect();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if dimension == ResourceDimension::MaterializedBytes {
            policy.limits.max_materialized_bytes = 0;
        } else {
            policy.limits.max_work_units = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = crate::sketches::SketchPolygon::try_new(
            entities,
            &ctx,
            "polygon temporary uniqueness",
        );
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == dimension)
        );
    }
}

#[test]
fn sketch_member_comparisons_refuse_before_first_and_later_visits_and_shifts() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    for cap in [0, 3, 4, 10, 11, 12, 13] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) = crate::sketches::distinct_sketch_members(
            &ctx, ["z", "x", "x-a"].into_iter(), "sketch member comparison test",
        ) else { panic!("member scan must refuse"); };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "sketch member comparison test");
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(crate::sketches::distinct_sketch_members(&ctx, ["z", "x", "x-a"].into_iter(), "sketch member comparison test").unwrap());
    assert!(!crate::sketches::distinct_sketch_members(&ctx, ["z", "x", "x-a", "x"].into_iter(), "sketch member comparison test").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn sketch_constructors_keep_original_refusals_without_retaining_temporary_slots() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let members = ["prefix-long", "prefix", "alpha"].map(|suffix| {
        crate::sketches::SketchEntityId::mint(format!("test:model:entity#{suffix}")).unwrap()
    });
    let boundary = ["prefix-long", "prefix", "alpha"].map(|suffix| SpatialSketchEntityUse {
        entity: SpatialSketchEntityId::mint(format!("test:model:entity#{suffix}")).unwrap(),
        reversed: true,
    });
    let origin = Point3::new(1.0, 2.0, 3.0);
    let normal = Vector3::new(0.0, 0.0, 1.0);
    let u_axis = Vector3::new(1.0, 0.0, 0.0);
    for profile in [false, true] {
        for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems, ResourceDimension::WorkUnits] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 1,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 3,
                _ => panic!("test dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = if profile {
                SpatialSketchProfile::try_new(origin, normal, u_axis, boundary.to_vec(), &ctx, "sketch constructor test").map(|_| ())
            } else {
                crate::sketches::SketchPolygon::try_new(members.to_vec(), &ctx, "sketch constructor test").map(|_| ())
            };
            let Err(CodecError::ResourceLimit(limit)) = result else { panic!("constructor must refuse"); };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(limit.operation, "sketch constructor test");
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 512;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if profile {
            let profile = SpatialSketchProfile::try_new(origin, normal, u_axis, boundary.to_vec(), &ctx, "sketch constructor test").unwrap().unwrap();
            assert_eq!(profile.boundary(), boundary);
            let invalid = SpatialSketchProfile::try_new(origin, normal, u_axis, vec![boundary[0].clone(), boundary[1].clone(), boundary[0].clone()], &ctx, "sketch constructor test").unwrap().unwrap_err();
            assert_eq!(invalid, "spatial profile boundary must be nonempty and contain distinct entities");
        } else {
            let polygon = crate::sketches::SketchPolygon::try_new(members.to_vec(), &ctx, "sketch constructor test").unwrap().unwrap();
            assert_eq!(polygon.entities(), members);
            let invalid = crate::sketches::SketchPolygon::try_new(vec![members[0].clone(), members[1].clone(), members[0].clone()], &ctx, "sketch constructor test").unwrap().unwrap_err();
            assert_eq!(invalid, "entities requires at least three distinct polygon members");
        }
        let reservation = ctx.reserve_scoped_limit(512, "released sketch member index").unwrap();
        drop(reservation);
        ctx.finish_session().unwrap();
    }
}
