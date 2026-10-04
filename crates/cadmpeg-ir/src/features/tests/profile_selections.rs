use crate::features::PlanarProfileRef;
use crate::features::{FeatureId, GeneratedCurveRef, PathRef, ProfileRef};
use crate::ids::{FeatureInputTopologyId, HistoricalEdgeId, HistoricalFaceId};
use crate::sketches::{SketchEntityId, SketchId, SpatialSketchEntityId, SpatialSketchId};

#[test]
fn profile_selection_members_are_checked_at_construction_and_on_wire() {
    let sketch = SketchId::mint("test:test:sketch#one").unwrap();
    let spatial = SpatialSketchId::mint("test:test:spatial-sketch#one").unwrap();
    let entity = SketchEntityId::mint("test:test:sketch-entity#one").unwrap();
    let state = FeatureInputTopologyId::mint("test:model:feature-input#one").unwrap();
    let face = HistoricalFaceId::mint("test:model:historical-face#one").unwrap();
    let profiles = [
        (
            ProfileRef::Planar(
                PlanarProfileRef::sketch_profiles(
                    sketch.clone(),
                    vec![1, 0],
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("profile membership admission")
                .unwrap(),
            ),
            "profiles",
        ),
        (
            ProfileRef::spatial_sketch_profiles(
                spatial.clone(),
                vec![1, 0],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("profile membership admission")
            .unwrap(),
            "profiles",
        ),
        (
            ProfileRef::Planar(
                PlanarProfileRef::sketch_entities(
                    sketch.clone(),
                    vec![entity.clone()],
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("profile membership admission")
                .unwrap(),
            ),
            "entities",
        ),
        (
            ProfileRef::Planar(
                PlanarProfileRef::sketch_selection(
                    sketch.clone(),
                    vec!["group".into()],
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("profile membership admission")
                .unwrap(),
            ),
            "selections",
        ),
        (
            ProfileRef::spatial_sketch_selection(
                spatial.clone(),
                vec!["group".into()],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("profile membership admission")
            .unwrap(),
            "selections",
        ),
        (
            ProfileRef::Planar(
                PlanarProfileRef::historical_faces(
                    state.clone(),
                    vec![face.clone()],
                    vec!["group".into()],
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("selection storage is admitted")
                .unwrap(),
            ),
            "faces",
        ),
    ];
    for (profile, field) in profiles {
        let wire = serde_json::to_value(&profile).unwrap();
        assert_eq!(
            serde_json::from_value::<ProfileRef>(wire.clone()).unwrap(),
            profile
        );
        let first = wire["value"][field][0].clone();
        for invalid in [serde_json::json!([]), serde_json::json!([first, first])] {
            let mut invalid_wire = wire.clone();
            invalid_wire["value"][field] = invalid;
            assert!(serde_json::from_value::<ProfileRef>(invalid_wire.clone()).is_err());
            if profile.planar().is_some() {
                let message = serde_json::from_value::<PlanarProfileRef>(invalid_wire)
                    .unwrap_err()
                    .to_string();
                assert!(message.contains(field), "{message}");
            }
        }
    }
    for indices in [vec![], vec![0, 0]] {
        assert!(PlanarProfileRef::sketch_profiles(
            sketch.clone(),
            indices.clone(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("profile membership admission")
        .is_err());
        assert!(ProfileRef::spatial_sketch_profiles(
            spatial.clone(),
            indices,
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("profile membership admission")
        .is_err());
    }
    for entities in [vec![], vec![entity.clone(), entity]] {
        assert!(PlanarProfileRef::sketch_entities(
            sketch.clone(),
            entities,
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("profile membership admission")
        .is_err());
    }
    for names in [
        vec![],
        vec![String::new()],
        vec![" \t".into()],
        vec!["group".into(), "group".into()],
    ] {
        assert!(PlanarProfileRef::sketch_selection(
            sketch.clone(),
            names.clone(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("profile membership admission")
        .is_err());
        assert!(ProfileRef::spatial_sketch_selection(
            spatial.clone(),
            names.clone(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("profile membership admission")
        .is_err());
        assert!(PlanarProfileRef::historical_faces(
            state.clone(),
            vec![face.clone()],
            names,
            &cadmpeg_test_support::service_decode_context()
        )
        .expect("selection storage is admitted")
        .is_err());
    }
    assert!(PlanarProfileRef::historical_faces(
        state.clone(),
        vec![],
        vec!["group".into()],
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("selection storage is admitted")
    .is_err());
    assert!(PlanarProfileRef::historical_faces(
        state,
        vec![face.clone(), face],
        vec!["group".into()],
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("selection storage is admitted")
    .is_err());
}

#[test]
fn path_selection_members_are_checked_at_construction_and_on_wire() {
    let sketch = SketchId::mint("test:test:sketch#one").unwrap();
    let spatial = SpatialSketchId::mint("test:test:spatial-sketch#one").unwrap();
    let entity = SketchEntityId::mint("test:test:sketch-entity#one").unwrap();
    let spatial_entity = SpatialSketchEntityId::mint("test:test:spatial-entity#one").unwrap();
    let state = FeatureInputTopologyId::mint("test:model:feature-input#one").unwrap();
    let edge = HistoricalEdgeId::mint("test:model:historical-edge#one").unwrap();
    let paths = [
        (
            PathRef::sketch_curves(
                sketch.clone(),
                vec![entity.clone()],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("profile membership admission")
            .unwrap(),
            "curves",
        ),
        (
            PathRef::spatial_sketch_curves(
                spatial.clone(),
                vec![spatial_entity.clone()],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("profile membership admission")
            .unwrap(),
            "curves",
        ),
        (
            crate::features::NativeSelections::try_from(vec!["group".into()])
                .map(
                    |selections| crate::features::PathRef::SpatialSketchSelection {
                        sketch: spatial.clone(),
                        selections,
                    },
                )
                .unwrap(),
            "selections",
        ),
        (
            PathRef::historical_edges(
                state.clone(),
                vec![edge.clone()],
                "group".into(),
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("selection storage is admitted")
            .unwrap(),
            "edges",
        ),
    ];
    for (path, field) in paths {
        let wire = serde_json::to_value(&path).unwrap();
        assert_eq!(
            serde_json::from_value::<PathRef>(wire.clone()).unwrap(),
            path
        );
        let first = wire["value"][field][0].clone();
        for invalid in [serde_json::json!([]), serde_json::json!([first, first])] {
            let mut invalid_wire = wire.clone();
            invalid_wire["value"][field] = invalid;
            assert!(serde_json::from_value::<PathRef>(invalid_wire)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }
    for curves in [vec![], vec![entity.clone(), entity]] {
        assert!(PathRef::sketch_curves(
            sketch.clone(),
            curves,
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("profile membership admission")
        .is_err());
    }
    for curves in [vec![], vec![spatial_entity.clone(), spatial_entity]] {
        assert!(PathRef::spatial_sketch_curves(
            spatial.clone(),
            curves,
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("profile membership admission")
        .is_err());
    }
    for names in [
        vec![],
        vec![String::new()],
        vec![" \t".into()],
        vec!["group".into(), "group".into()],
    ] {
        assert!(crate::features::NativeSelections::try_from(names)
            .map(
                |selections| crate::features::PathRef::SpatialSketchSelection {
                    sketch: spatial.clone(),
                    selections
                }
            )
            .is_err());
    }
    assert!(PathRef::historical_edges(
        state.clone(),
        vec![],
        "group".into(),
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("selection storage is admitted")
    .is_err());
    assert!(PathRef::historical_edges(
        state.clone(),
        vec![edge.clone(), edge.clone()],
        "group".into(),
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("selection storage is admitted")
    .is_err());
    assert!(PathRef::historical_edges(
        state.clone(),
        vec![edge.clone()],
        String::new(),
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("selection storage is admitted")
    .is_err());
    assert!(PathRef::historical_edges(
        state.clone(),
        vec![edge.clone()],
        " ".into(),
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("selection storage is admitted")
    .is_err());
    assert!(PathRef::historical_edges(
        state,
        vec![edge],
        " g ".into(),
        &cadmpeg_test_support::service_decode_context()
    )
    .expect("selection storage is admitted")
    .is_ok());
}

#[test]
fn generated_profiles_require_references_and_preserve_repeated_curves() {
    let feature = FeatureId::mint("test:test:feature#one").unwrap();
    for invalid in ["", " \t"] {
        assert!(GeneratedCurveRef::new(
            feature.clone(),
            invalid.into(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("selection reference admission")
        .is_err());
        assert!(serde_json::from_value::<GeneratedCurveRef>(
            serde_json::json!({"feature": feature, "local_id": invalid})
        )
        .unwrap_err()
        .to_string()
        .contains("local_id"));
    }
    let curve = GeneratedCurveRef::new(
        feature,
        "curve".into(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("selection reference admission")
    .unwrap();
    assert!(PlanarProfileRef::generated(
        vec![],
        "group".into(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("selection reference admission")
    .is_err());
    for native in ["", " \t"] {
        assert!(PlanarProfileRef::generated(
            vec![curve.clone()],
            native.into(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("selection reference admission")
        .is_err());
    }
    let profile = ProfileRef::Planar(
        PlanarProfileRef::generated(
            vec![curve.clone(), curve.clone()],
            "group".into(),
            &cadmpeg_test_support::service_decode_context(),
        )
        .expect("selection reference admission")
        .unwrap(),
    );
    let wire = serde_json::to_value(&profile).unwrap();
    assert_eq!(wire["value"]["curves"], serde_json::json!([curve, curve]));
    assert_eq!(
        serde_json::from_value::<ProfileRef>(wire.clone()).unwrap(),
        profile
    );
    for (field, value) in [
        ("curves", serde_json::json!([])),
        ("native", serde_json::json!(" \t")),
    ] {
        let mut invalid = wire.clone();
        invalid["value"][field] = value;
        assert!(serde_json::from_value::<ProfileRef>(invalid.clone()).is_err());
        assert!(serde_json::from_value::<PlanarProfileRef>(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
}

#[test]
fn a_spatial_profile_where_a_planar_one_is_required_is_refused_as_an_unknown_variant() {
    let spatial = ProfileRef::spatial_sketch_profiles(
        SpatialSketchId::mint("test:test:spatial-sketch#one").unwrap(),
        vec![0],
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("profile membership admission")
    .unwrap();
    let wire = serde_json::to_value(&spatial).unwrap();
    assert_eq!(wire["kind"], "spatial_sketch_profiles");
    let error = serde_json::from_value::<PlanarProfileRef>(wire.clone())
        .expect_err("a spatial profile is not a planar profile");
    assert!(error.to_string().contains("unknown variant"), "{error}");
    let section = serde_json::json!({ "kind": "profile", "value": wire });
    assert!(serde_json::from_value::<crate::features::SweepSection>(section).is_err());
    assert!(spatial.planar().is_none());
}

#[test]
fn profile_and_path_constructors_propagate_scoped_index_refusals() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let sketch = SketchId::mint("test:test:sketch#one").unwrap();
    let spatial = SpatialSketchId::mint("test:test:spatial-sketch#one").unwrap();
    let entity = SketchEntityId::mint("test:test:sketch-entity#one").unwrap();
    for error in [
        PlanarProfileRef::sketch_profiles(sketch.clone(), vec![0], &ctx).unwrap_err(),
        ProfileRef::spatial_sketch_selection(spatial, vec!["group".into()], &ctx).unwrap_err(),
        PathRef::sketch_curves(sketch, vec![entity], &ctx).unwrap_err(),
    ] {
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("profile membership resource refusal required");
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes
        );
    }
}

#[test]
fn every_profile_constructor_uses_the_caller_session_for_membership() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let sketch = SketchId::mint("test:test:sketch#one").unwrap();
    let spatial = SpatialSketchId::mint("test:test:spatial-sketch#one").unwrap();
    let entity = SketchEntityId::mint("test:test:sketch-entity#one").unwrap();
    let spatial_entity = SpatialSketchEntityId::mint("test:test:spatial-entity#one").unwrap();
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
    ] {
        for kind in 0..7 {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => panic!("membership admission dimensions"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = match kind {
                0 => PlanarProfileRef::sketch_profiles(sketch.clone(), vec![0], &ctx)
                    .map(|result| result.map(|_| ())),
                1 => PlanarProfileRef::sketch_entities(sketch.clone(), vec![entity.clone()], &ctx)
                    .map(|result| result.map(|_| ())),
                2 => PlanarProfileRef::sketch_selection(sketch.clone(), vec!["group".into()], &ctx)
                    .map(|result| result.map(|_| ())),
                3 => ProfileRef::spatial_sketch_profiles(spatial.clone(), vec![0], &ctx)
                    .map(|result| result.map(|_| ())),
                4 => ProfileRef::spatial_sketch_selection(
                    spatial.clone(),
                    vec!["group".into()],
                    &ctx,
                )
                .map(|result| result.map(|_| ())),
                5 => PathRef::sketch_curves(sketch.clone(), vec![entity.clone()], &ctx)
                    .map(|result| result.map(|_| ())),
                _ => PathRef::spatial_sketch_curves(
                    spatial.clone(),
                    vec![spatial_entity.clone()],
                    &ctx,
                )
                .map(|result| result.map(|_| ())),
            };
            let cadmpeg_core::CodecError::ResourceLimit(limit) =
                result.expect_err("constructor must use its supplied session")
            else {
                panic!("profile membership resource refusal required");
            };
            assert_eq!(limit.dimension, dimension);
            assert!(
                matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit)
            );
        }
    }
}
