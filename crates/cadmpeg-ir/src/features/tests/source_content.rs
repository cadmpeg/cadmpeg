use crate::features::{
    Feature, FeatureContent, FeatureDefinition, FeatureId, FeatureSourceContent, FinitePoint3,
    ParameterId,
};
use crate::math::Point3;

#[test]
fn source_content_rejects_repeated_references_and_preserves_repeated_text() {
    let parameter =
        FeatureSourceContent::Parameter(ParameterId::mint("test:reference#one").unwrap());
    let child = FeatureSourceContent::Feature(FeatureId::mint("test:reference#one").unwrap());
    for reference in [parameter.clone(), child.clone()] {
        assert!(FeatureContent::try_from(vec![reference.clone(), reference.clone()]).is_err());
        let mut content = FeatureContent::try_from(vec![reference.clone()]).unwrap();
        let original = content.clone();
        assert!(content.push(reference).is_err());
        assert_eq!(content, original);
    }
    let mut content = FeatureContent::text(["text".to_owned(), "text".to_owned()]);
    content.push(parameter.clone()).unwrap();
    content.push(child.clone()).unwrap();
    content
        .push(FeatureSourceContent::Text("text".into()))
        .unwrap();
    assert_eq!(
        content.as_slice(),
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
fn feature_membership_is_checked_on_standalone_and_model_wire_routes() {
    let mut feature = Feature::new(
        FeatureId::mint("test:feature#owner").unwrap(),
        1,
        FeatureDefinition::DatumPoint {
            position: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            construction: None,
        },
    );
    let dependency = FeatureId::mint("test:feature#dependency").unwrap();
    feature.dependencies.insert(dependency.clone());
    let parameter =
        FeatureSourceContent::Parameter(ParameterId::mint("test:parameter#one").unwrap());
    feature.source_content.push(parameter.clone()).unwrap();
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
        assert!(serde_json::from_value::<Feature>(wire.clone())
            .unwrap_err()
            .to_string()
            .contains(field));
        let mut model = serde_json::to_value(crate::document::Model::default()).unwrap();
        model["features"] = serde_json::json!([wire]);
        assert!(serde_json::from_value::<crate::document::Model>(model)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
    feature.dependencies.clear();
    feature.source_content = Default::default();
    let wire = serde_json::to_value(&feature).unwrap();
    assert!(wire.get("dependencies").is_none());
    assert!(wire.get("source_content").is_none());
    assert_eq!(serde_json::from_value::<Feature>(wire).unwrap(), feature);
}
