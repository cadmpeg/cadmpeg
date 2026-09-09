use crate::features::{FeatureId, GeneratedCurveRef, PathRef, ProfileRef};
use crate::ids::{FeatureInputTopologyId, HistoricalEdgeId, HistoricalFaceId};
use crate::sketches::{SketchEntityId, SketchId, SpatialSketchEntityId, SpatialSketchId};

#[test]
fn profile_selection_members_are_checked_at_construction_and_on_wire() {
    let sketch = SketchId("test:sketch#one".into());
    let spatial = SpatialSketchId("test:spatial-sketch#one".into());
    let entity = SketchEntityId("test:sketch-entity#one".into());
    let state = FeatureInputTopologyId::mint("test:model:feature-input#one").unwrap();
    let face = HistoricalFaceId::mint("test:model:historical-face#one").unwrap();
    let profiles = [
        (
            ProfileRef::sketch_profiles(sketch.clone(), vec![1, 0]).unwrap(),
            "profiles",
        ),
        (
            ProfileRef::spatial_sketch_profiles(spatial.clone(), vec![1, 0]).unwrap(),
            "profiles",
        ),
        (
            ProfileRef::sketch_entities(sketch.clone(), vec![entity.clone()]).unwrap(),
            "entities",
        ),
        (
            ProfileRef::sketch_selection(sketch.clone(), vec!["group".into()]).unwrap(),
            "selections",
        ),
        (
            ProfileRef::spatial_sketch_selection(spatial.clone(), vec!["group".into()]).unwrap(),
            "selections",
        ),
        (
            ProfileRef::historical_faces(state.clone(), vec![face.clone()], vec!["group".into()])
                .unwrap(),
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
            assert!(serde_json::from_value::<ProfileRef>(invalid_wire)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }
    for indices in [vec![], vec![0, 0]] {
        assert!(ProfileRef::sketch_profiles(sketch.clone(), indices.clone()).is_err());
        assert!(ProfileRef::spatial_sketch_profiles(spatial.clone(), indices).is_err());
    }
    for entities in [vec![], vec![entity.clone(), entity]] {
        assert!(ProfileRef::sketch_entities(sketch.clone(), entities).is_err());
    }
    for names in [
        vec![],
        vec![String::new()],
        vec![" \t".into()],
        vec!["group".into(), "group".into()],
    ] {
        assert!(ProfileRef::sketch_selection(sketch.clone(), names.clone()).is_err());
        assert!(ProfileRef::spatial_sketch_selection(spatial.clone(), names.clone()).is_err());
        assert!(ProfileRef::historical_faces(state.clone(), vec![face.clone()], names).is_err());
    }
    assert!(ProfileRef::historical_faces(state.clone(), vec![], vec!["group".into()]).is_err());
    assert!(
        ProfileRef::historical_faces(state, vec![face.clone(), face], vec!["group".into()])
            .is_err()
    );
}

#[test]
fn path_selection_members_are_checked_at_construction_and_on_wire() {
    let sketch = SketchId("test:sketch#one".into());
    let spatial = SpatialSketchId("test:spatial-sketch#one".into());
    let entity = SketchEntityId("test:sketch-entity#one".into());
    let spatial_entity = SpatialSketchEntityId("test:spatial-entity#one".into());
    let state = FeatureInputTopologyId::mint("test:model:feature-input#one").unwrap();
    let edge = HistoricalEdgeId::mint("test:model:historical-edge#one").unwrap();
    let paths = [
        (
            PathRef::sketch_curves(sketch.clone(), vec![entity.clone()]).unwrap(),
            "curves",
        ),
        (
            PathRef::spatial_sketch_curves(spatial.clone(), vec![spatial_entity.clone()]).unwrap(),
            "curves",
        ),
        (
            PathRef::spatial_sketch_selection(spatial.clone(), vec!["group".into()]).unwrap(),
            "selections",
        ),
        (
            PathRef::historical_edges(state.clone(), vec![edge.clone()], "group".into()).unwrap(),
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
        assert!(PathRef::sketch_curves(sketch.clone(), curves).is_err());
    }
    for curves in [vec![], vec![spatial_entity.clone(), spatial_entity]] {
        assert!(PathRef::spatial_sketch_curves(spatial.clone(), curves).is_err());
    }
    for names in [
        vec![],
        vec![String::new()],
        vec![" \t".into()],
        vec!["group".into(), "group".into()],
    ] {
        assert!(PathRef::spatial_sketch_selection(spatial.clone(), names).is_err());
    }
    assert!(PathRef::historical_edges(state.clone(), vec![], "group".into()).is_err());
    assert!(PathRef::historical_edges(
        state.clone(),
        vec![edge.clone(), edge.clone()],
        "group".into()
    )
    .is_err());
    assert!(PathRef::historical_edges(state.clone(), vec![edge.clone()], String::new()).is_err());
    assert!(PathRef::historical_edges(state, vec![edge], " ".into()).is_ok());
}

#[test]
fn generated_profiles_require_references_and_preserve_repeated_curves() {
    let feature = FeatureId::mint("test:feature#one").unwrap();
    for invalid in ["", " \t"] {
        assert!(GeneratedCurveRef::new(feature.clone(), invalid.into()).is_err());
        assert!(serde_json::from_value::<GeneratedCurveRef>(
            serde_json::json!({"feature": feature, "local_id": invalid})
        )
        .unwrap_err()
        .to_string()
        .contains("local_id"));
    }
    let curve = GeneratedCurveRef::new(feature, "curve".into()).unwrap();
    assert!(ProfileRef::generated(vec![], "group".into()).is_err());
    for native in ["", " \t"] {
        assert!(ProfileRef::generated(vec![curve.clone()], native.into()).is_err());
    }
    let profile =
        ProfileRef::generated(vec![curve.clone(), curve.clone()], "group".into()).unwrap();
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
        assert!(serde_json::from_value::<ProfileRef>(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
}
