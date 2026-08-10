//! Tests for the `component_paths` module.

use super::*;
use crate::records::{Feature, FeatureInputComponentPathEntry};
use std::collections::BTreeMap;

#[test]
fn component_path_type_identities_name_ordered_features() {
    let feature = |id: &str, source_id: &str| Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: Some(source_id.into()),
        parent_source_id: None,
        ordinal: 0,
        name: String::new(),
        kind: String::new(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::default(),
        dimension_properties: BTreeMap::default(),
        properties: BTreeMap::default(),
        text: None,
        content: Vec::new(),
    };
    let mut signature = [0u8; 12];
    signature[4..8].copy_from_slice(&42u32.to_le_bytes());
    let components = vec![
        FeatureInputComponentPathEntry {
            instance: Some(0x8032),
            type_signature: signature,
            local_id: Some(7),
        },
        FeatureInputComponentPathEntry {
            instance: Some(0x803b),
            type_signature: signature,
            local_id: Some(1),
        },
    ];
    assert_eq!(
        component_path_features(&components, &[feature("producer", "42")]),
        vec!["producer"]
    );
    assert_eq!(
        component_path_features(
            &components,
            &[feature("first", "42"), feature("second", "42")]
        ),
        Vec::<String>::new()
    );
    let mut mixed = components;
    mixed[1].type_signature[4..8].copy_from_slice(&43u32.to_le_bytes());
    assert_eq!(
        component_path_features(&mixed, &[feature("producer", "42"), feature("other", "43")]),
        vec!["producer", "other"]
    );
    assert_eq!(
        component_path_terminal_feature(
            &mixed,
            &[feature("producer", "42"), feature("other", "43")]
        ),
        Some("other".into())
    );
    assert_eq!(
        surface_selection_producer_features(
            &mixed,
            Some("explicit"),
            &[feature("producer", "42"), feature("other", "43")]
        ),
        ["producer", "other", "explicit"]
    );
    mixed.push(FeatureInputComponentPathEntry {
        instance: Some(0x8040),
        type_signature: {
            let mut signature = [0; 12];
            signature[4..8].copy_from_slice(&99u32.to_le_bytes());
            signature
        },
        local_id: Some(5),
    });
    assert_eq!(
        component_path_terminal_feature(
            &mixed,
            &[feature("producer", "42"), feature("other", "43")]
        ),
        Some("other".into())
    );

    let owner = feature("mirror", "44");
    mixed.push(FeatureInputComponentPathEntry {
        instance: None,
        type_signature: {
            let mut signature = [0; 12];
            signature[4..8].copy_from_slice(&44u32.to_le_bytes());
            signature
        },
        local_id: Some(9),
    });
    let producer = feature("producer", "42");
    let other = feature("other", "43");
    let history = [&producer, &other, &owner];
    let (component, preceding) =
        component_path_feature(&mixed, &history, "mirror", ComponentPathEnd::Trailing)
            .expect("required invariant");
    assert_eq!(preceding.id, "other");
    assert_eq!(component.local_id, Some(1));

    let mut prior = feature("prior", "42");
    prior.ordinal = 3;
    let mut consumer = feature("consumer", "88");
    consumer.ordinal = 2;
    let mut future = feature("future", "99");
    future.ordinal = 1;
    let path = [88_u32, 42, 99, 88]
        .into_iter()
        .map(|source| FeatureInputComponentPathEntry {
            instance: Some(0x8180),
            type_signature: {
                let mut signature = [0; 12];
                signature[4..8].copy_from_slice(&source.to_le_bytes());
                signature
            },
            local_id: Some(1),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        component_path_input_features(&path, &[prior, consumer, future], "consumer"),
        ["prior"]
    );
}
