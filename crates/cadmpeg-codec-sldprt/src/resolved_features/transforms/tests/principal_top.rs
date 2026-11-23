use crate::resolved_features::compact_reference_planes::principal_sketch_frame;
use crate::resolved_features::transforms::{
    component_sketch_marker_transform, feature_sketch_marker_transform,
    record_component_marker_coordinates,
};
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureId, FeatureOperation, PrincipalPlane, SketchFeatureBinding,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchId, SketchPlacement};

fn sketch() -> Sketch {
    let (origin, normal, u_axis) = principal_sketch_frame(PrincipalPlane::Top);
    Sketch {
        id: SketchId::mint("synthetic:test:id#top-component").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(origin, normal, u_axis).unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }
}

#[test]
fn top_component_rectangle_and_blind_travel_use_intrinsic_coordinates() {
    let sketch = sketch();
    let (origin, normal, u_axis) = principal_sketch_frame(PrincipalPlane::Top);
    let v_axis = normal.cross(u_axis);
    assert_eq!(normal, Vector3::new(0.0, 1.0, 0.0));
    let transform = component_sketch_marker_transform(&sketch, 1.0, true).unwrap();
    for (stored, expected) in [
        ((2, 3), Point3::new(2.0, 0.0, -3.0)),
        ((7, 3), Point3::new(7.0, 0.0, -3.0)),
        ((7, 11), Point3::new(7.0, 0.0, -11.0)),
        ((2, 11), Point3::new(2.0, 0.0, -11.0)),
    ] {
        let (u, v) = transform.apply(stored).unwrap();
        let point = origin
            .translated(u_axis, cadmpeg_core::convert::f64_from_i64(u).unwrap())
            .translated(v_axis, cadmpeg_core::convert::f64_from_i64(v).unwrap());
        assert_eq!(point, expected);
        let end = point.translated(normal, 13.0);
        assert_eq!(end.y, 13.0);
        assert_ne!(point.translated(normal.scale(-1.0), 13.0).y, end.y);
        let mirrored = origin
            .translated(u_axis, cadmpeg_core::convert::f64_from_i64(u).unwrap())
            .translated(v_axis, -cadmpeg_core::convert::f64_from_i64(v).unwrap());
        assert_ne!(mirrored, expected);
    }
}

#[test]
fn custom_top_frame_keeps_ascending_model_axis_projection() {
    let sketch = sketch();
    assert_eq!(
        component_sketch_marker_transform(&sketch, 1.0, false)
            .unwrap()
            .apply((2, 3)),
        Some((2, -3))
    );
    assert_eq!(
        component_sketch_marker_transform(&sketch, 1.0, true)
            .unwrap()
            .apply((2, 3)),
        Some((2, 3))
    );
}

#[test]
fn top_coordinate_provenance_follows_the_sketch_binding() {
    let sketch = sketch();
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#top-feature").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        native_ref: None,
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch.id.clone())),
            },
        )),
    };
    record_component_marker_coordinates(&mut feature, Some(PrincipalPlane::Top));
    assert_eq!(
        feature_sketch_marker_transform(&sketch, 1.0, &[feature])
            .unwrap()
            .apply((2, 3)),
        Some((2, 3))
    );
    assert_eq!(
        feature_sketch_marker_transform(&sketch, 1.0, &[])
            .unwrap()
            .apply((2, 3)),
        Some((2, -3))
    );
}

/// The assumed-binding loss names the sketches whose frame the Top plane
/// reference gave, by the route recorded where the frame was chosen. A compact
/// sketch that a principal-plane component record placed, the unbound twin of
/// a marker sketch of the same name, is not named; nor is a recorded sketch
/// that no longer lies on the Top frame.
#[test]
fn top_plane_binding_names_only_sketches_the_plane_reference_placed() {
    use crate::resolved_features::compact_reference_planes::{PlaneBinding, PlaneBindings};
    let named = |id: &str, name: &str| Sketch {
        id: SketchId::mint(id).unwrap(),
        name: Some(name.into()),
        ..sketch()
    };
    let marker = named("sldprt:model:sketch#markers:1:7", "Sketch3");
    let twin = named("sldprt:model:sketch#compact:1:7", "Sketch3");
    let mut routes = PlaneBindings::default();
    routes.record(&marker.id, PlaneBinding::Stored);
    routes.record(&twin.id, PlaneBinding::Stored);
    assert!(routes.note(&[marker.clone(), twin.clone()]).is_none());

    let referenced = named("sldprt:model:sketch#markers:1:9", "Sketch5");
    let moved = Sketch {
        id: SketchId::mint("sldprt:model:sketch#markers:1:11").unwrap(),
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 4.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        ..referenced.clone()
    };
    routes.record(&referenced.id, PlaneBinding::TopReference);
    routes.record(&moved.id, PlaneBinding::TopReference);
    let note = routes
        .note(&[marker, twin, referenced, moved])
        .expect("one note");
    assert_eq!(
        note.code,
        crate::loss::SldprtLossCode::SketchPlaneBindingAssumed.kind()
    );
    assert_eq!(
        note.message,
        "1 profile sketch(es) lie on a plane the decoder assumes (sldprt-open-items.md DI-73): \
         1 lie on the Top plane (origin 0,0,0, normal +Y, u-axis +X) that their feature-input \
         plane reference names, and the B-rep of some sketches bound this way lies on another \
         principal plane (Sketch5)."
    );
}

/// Each assumed route is named with the plane it gave; a later stored record
/// of the same sketch clears it. A sketch bound by its own record is never
/// named.
#[test]
fn assumed_plane_binding_names_each_route_and_its_plane() {
    use crate::resolved_features::compact_reference_planes::{PlaneBinding, PlaneBindings};
    let on = |id: &str, name: &str, plane: PrincipalPlane| {
        let (origin, normal, u_axis) = principal_sketch_frame(plane);
        Sketch {
            id: SketchId::mint(id).unwrap(),
            name: Some(name.into()),
            placement: SketchPlacement::try_resolved(origin, normal, u_axis).unwrap(),
            ..sketch()
        }
    };
    let lane = on(
        "sldprt:model:sketch#markers:1:3",
        "Sketch1",
        PrincipalPlane::Front,
    );
    let context = on(
        "sldprt:model:sketch#markers:1:5",
        "Sketch4",
        PrincipalPlane::Right,
    );
    let conflicting = on(
        "sldprt:model:sketch#markers:1:6",
        "Sketch6",
        PrincipalPlane::Front,
    );
    let legacy = on(
        "sldprt:model:sketch#legacy:1:7",
        "Sketch7",
        PrincipalPlane::Front,
    );
    let fallback = Sketch {
        id: SketchId::mint("sldprt:model:sketch#markers:1:8").unwrap(),
        name: Some("Sketch8".into()),
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
        )
        .unwrap(),
        ..sketch()
    };
    let restored = on(
        "sldprt:model:sketch#markers:1:4",
        "Sketch2",
        PrincipalPlane::Front,
    );
    let own = on(
        "sldprt:model:sketch#markers:1:9",
        "Sketch9",
        PrincipalPlane::Front,
    );
    let mut routes = PlaneBindings::default();
    let borrowed = PlaneBinding::Borrowed { own_records: false };
    routes.record(&lane.id, borrowed);
    routes.record(&context.id, borrowed);
    routes.record(
        &conflicting.id,
        PlaneBinding::Borrowed { own_records: true },
    );
    routes.record(&legacy.id, PlaneBinding::LaneFrame);
    routes.record(&fallback.id, PlaneBinding::FrontFallback);
    routes.record(&restored.id, borrowed);
    routes.record(&restored.id, PlaneBinding::Stored);
    routes.record(&own.id, PlaneBinding::Stored);
    let note = routes
        .note(&[lane, context, conflicting, legacy, fallback, restored, own])
        .expect("one note");
    assert_eq!(
        note.code,
        crate::loss::SldprtLossCode::SketchPlaneBindingAssumed.kind()
    );
    assert_eq!(
        note.message,
        "5 profile sketch(es) lie on a plane the decoder assumes (sldprt-open-items.md DI-73): \
         2 take the plane that the plane reference of the preceding feature object or the \
         lane's default reference names, and their own object holds no plane record the \
         decoder reads (1 on the Front plane (Sketch1), 1 on the Right plane (Sketch4)); 1 take \
         the plane that the plane reference of the preceding feature object or the lane's \
         default reference names, because the plane records of their own object give no single \
         plane (1 on the Front plane (Sketch6)); 1 are legacy configuration sketches with no \
         frame of their own that take the only frame of the other sketches of their \
         configuration lane (1 on the Front plane (Sketch7)); 1 lie on the Front lane fallback \
         frame (origin 0,0,0, normal -Y, u-axis -Z), which no plane of the part stores, because \
         their reference names the Front plane and the principal-plane component records of \
         their own object bind them to no plane by sldprt.md §2: the records name two planes, a \
         component frame record lies from the preceding object through the profile, or their \
         own object holds a plane reference other than a declared Front record, open item \
         SF-01 (Sketch8)."
    );
}

#[test]
fn right_component_markers_are_sketch_coordinates_on_the_right_plane() {
    let (origin, normal, u_axis) = principal_sketch_frame(PrincipalPlane::Right);
    let sketch = Sketch {
        placement: SketchPlacement::try_resolved(origin, normal, u_axis).unwrap(),
        ..sketch()
    };
    let v_axis = normal.cross(u_axis);
    let transform = component_sketch_marker_transform(&sketch, 1.0, true).unwrap();
    assert_eq!(transform.apply((2, 3)), Some((2, 3)));
    let point = origin.translated(u_axis, 2.0).translated(v_axis, 3.0);
    assert_eq!(point, Point3::new(0.0, 3.0, -2.0));
    assert_eq!(
        component_sketch_marker_transform(&sketch, 1.0, false)
            .unwrap()
            .apply((2, 3)),
        Some((-3, 2))
    );
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#right-feature").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        native_ref: None,
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch.id.clone())),
            },
        )),
    };
    record_component_marker_coordinates(&mut feature, Some(PrincipalPlane::Right));
    assert_eq!(
        feature_sketch_marker_transform(&sketch, 1.0, &[feature])
            .unwrap()
            .apply((2, 3)),
        Some((2, 3))
    );
}
