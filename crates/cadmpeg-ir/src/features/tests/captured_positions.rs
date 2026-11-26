use crate::features::tree::{CapturedComponentPosition, CapturedPositionTransform};
use crate::features::FeatureOperation;
use crate::ids::{NativeId, OccurrenceId};
use crate::transform::Transform;

#[test]
fn captured_positions_keep_repeated_targets_and_order_on_the_wire() {
    let mut first = CapturedComponentPosition::new(
        NativeId::mint("synthetic:test:selector#one").unwrap(),
        Transform::identity(),
    );
    first.occurrence = Some(OccurrenceId::mint("synthetic:test:occurrence#one").unwrap());
    let second = CapturedComponentPosition::new(
        first.selector.clone(),
        Transform::affine([
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap(),
    );
    let operation = FeatureOperation::CapturedComponentPositions {
        positions: vec![first, second],
    };
    let wire = cadmpeg_core::serde_erase::json::to_value(&operation).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"kind":"captured_component_positions", "value":{"positions":[
            {"selector":"synthetic:test:selector#one", "occurrence":"synthetic:test:occurrence#one", "transform":{"kind":"affine", "transform":[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0]]}},
            {"selector":"synthetic:test:selector#one", "transform":{"kind":"affine", "transform":[[1.0,0.0,0.0,20.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0]]}}
        ]}})
    );
    assert_eq!(
        cadmpeg_core::serde_erase::json::from_value::<FeatureOperation>(wire.clone()).unwrap(),
        operation
    );
    let mut null = wire.clone();
    null["value"]["positions"][1]["occurrence"] = serde_json::Value::Null;
    assert!(
        cadmpeg_core::serde_erase::json::from_value::<FeatureOperation>(null)
            .unwrap_err()
            .to_string()
            .contains("occurrence")
    );
    let mut invalid = wire;
    invalid["value"]["positions"][0]["selector"] = serde_json::json!("");
    assert!(cadmpeg_core::serde_erase::json::from_value::<FeatureOperation>(invalid).is_err());
    let empty =
        serde_json::json!({"kind":"captured_component_positions", "value":{"positions":[]}});
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(
            cadmpeg_core::serde_erase::json::from_value::<FeatureOperation>(empty.clone()).unwrap()
        )
        .unwrap(),
        empty
    );
}

#[test]
fn captured_position_identity_rewrite_maps_selector_and_occurrence() {
    use crate::schema::rewrite::typed::RewriteIdentities;
    let source = FeatureOperation::CapturedComponentPositions {
        positions: vec![CapturedComponentPosition {
            selector: NativeId::mint("synthetic:source:selector#one").unwrap(),
            occurrence: Some(OccurrenceId::mint("synthetic:source:occurrence#one").unwrap()),
            transform: CapturedPositionTransform::Native {
                scope: NativeId::mint("synthetic:source:scope#capture").unwrap(),
                row: 3,
            },
        }],
    };
    let mut references = Vec::new();
    let wire = cadmpeg_core::serde_erase::json::to_value(&source).unwrap();
    assert_eq!(
        wire["value"]["positions"][0]["transform"],
        serde_json::json!({
            "kind": "native", "scope": "synthetic:source:scope#capture", "row": 3
        })
    );
    assert_eq!(
        cadmpeg_core::serde_erase::json::from_value::<FeatureOperation>(wire).unwrap(),
        source
    );
    source
        .visit_identity_references(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &mut |id| {
                references.push(id.to_owned());
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(references, ["synthetic:source:occurrence#one"]);
    let rewritten = crate::schema::rewrite::identities(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        source.clone(),
        |id| Ok(id.replace(":source:", ":copy:")),
    )
    .unwrap();
    let FeatureOperation::CapturedComponentPositions { positions } = rewritten else {
        panic!("captured positions")
    };
    assert_eq!(
        positions[0].selector.as_str(),
        "synthetic:copy:selector#one"
    );
    assert_eq!(
        positions[0].occurrence.as_ref().unwrap().as_str(),
        "synthetic:copy:occurrence#one"
    );
    assert_eq!(
        positions[0].transform,
        CapturedPositionTransform::Native {
            scope: NativeId::mint("synthetic:copy:scope#capture").unwrap(),
            row: 3,
        }
    );
    let FeatureOperation::CapturedComponentPositions { positions } = source else {
        panic!("captured positions")
    };
    assert_eq!(
        positions[0].selector.as_str(),
        "synthetic:source:selector#one"
    );
}
