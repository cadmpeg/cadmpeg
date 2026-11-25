use crate::features::geometry::FinitePoint3;
use crate::features::{
    Feature, FeatureContent, FeatureDefinition, FeatureId, FeatureOperation, FeatureSourceContent,
    ParameterId,
};
use crate::math::Point3;

#[test]
fn source_content_rejects_repeated_references_and_preserves_repeated_text() {
    let parameter =
        FeatureSourceContent::Parameter(ParameterId::mint("test:test:reference#one").unwrap());
    let child = FeatureSourceContent::Feature(FeatureId::mint("test:test:reference#one").unwrap());
    for reference in [parameter.clone(), child.clone()] {
        assert!(FeatureContent::try_from(vec![reference.clone(), reference.clone()]).is_err());
        let mut content = FeatureContent::try_from(vec![reference.clone()]).unwrap();
        let original = content.clone();
        assert!((content
            .push(reference)
            .map_err(cadmpeg_core::CodecError::from))
        .is_err());
        assert_eq!(content, original);
    }
    let mut content = FeatureContent::text(["text".to_owned(), "text".to_owned()]);
    content
        .push(parameter.clone())
        .map_err(cadmpeg_core::CodecError::from)
        .unwrap();
    content
        .push(child.clone())
        .map_err(cadmpeg_core::CodecError::from)
        .unwrap();
    content
        .push(FeatureSourceContent::Text("text".into()))
        .map_err(cadmpeg_core::CodecError::from)
        .unwrap();
    assert_eq!(
        (&*content),
        &[
            FeatureSourceContent::Text("text".into()),
            FeatureSourceContent::Text("text".into()),
            parameter,
            child,
            FeatureSourceContent::Text("text".into()),
        ]
    );
    let wire = serde_json::to_value(&content).unwrap();
    assert_eq!(
        serde_json::from_value::<FeatureContent>(wire).unwrap(),
        content
    );
}

#[test]
fn feature_membership_is_checked_on_the_feature_wire() {
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("test:test:feature#owner").unwrap(),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: crate::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                position: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                construction: None,
            }),
        ),
        native_ref: None,
    };
    let dependency = FeatureId::mint("test:test:feature#dependency").unwrap();
    feature.dependencies.insert(dependency.clone());
    let parameter =
        FeatureSourceContent::Parameter(ParameterId::mint("test:test:parameter#one").unwrap());
    feature
        .source_content
        .push(parameter.clone())
        .map_err(cadmpeg_core::CodecError::from)
        .unwrap();
    let original = serde_json::to_value(&feature).unwrap();
    assert_eq!(
        serde_json::from_value::<Feature>(original.clone()).unwrap(),
        feature
    );
    for (field, bad) in [
        ("dependencies", serde_json::json!([dependency, dependency])),
        ("source_content", serde_json::json!([parameter, parameter])),
    ] {
        let mut wire = original.clone();
        wire[field] = bad;
        assert!(serde_json::from_value::<Feature>(wire)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
    feature.dependencies.clear();
    feature.source_content = FeatureContent::default();
    let wire = serde_json::to_value(&feature).unwrap();
    assert_eq!(wire["dependencies"], serde_json::json!([]));
    assert_eq!(wire["source_content"], serde_json::json!([]));
    assert_eq!(serde_json::from_value::<Feature>(wire).unwrap(), feature);
}
