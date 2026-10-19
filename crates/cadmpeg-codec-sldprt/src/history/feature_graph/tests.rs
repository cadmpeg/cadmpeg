// SPDX-License-Identifier: Apache-2.0
use super::{validate_feature_graph, AncestryWalk};
use crate::records::{Feature, FeatureSource, TreeParent};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn feature(id: &str, source: u32, parent: Option<(&str, u32)>) -> Feature {
    Feature {
        id: id.into(),
        parent: "test:history#graph".into(),
        xml_tag: "Feature".into(),
        tree_parent: parent.map(|(record_id, source)| TreeParent::Record {
            record_id: record_id.into(),
            source_id: Some(FeatureSource::from_value(source).expect("native source id")),
        }),
        source_id: Some(FeatureSource::from_value(source).expect("native source id")),
        ordinal: source,
        name: id.into(),
        kind: "Native".into(),
        input_class: None,
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    }
}

fn features() -> [Feature; 3] {
    // Both ancestry relations visit a two-link path before reaching the root.
    [
        feature("test:feature#leaf", 3, Some(("test:feature#middle", 2))),
        feature("test:feature#middle", 2, Some(("test:feature#root", 1))),
        feature("test:feature#root", 1, None),
    ]
}

#[test]
fn feature_graph_validates_forward_ancestry_and_rejects_cycles() {
    let mut rows = features();
    validate_feature_graph(&rows).unwrap();
    rows[2].tree_parent = Some(TreeParent::Record {
        record_id: rows[0].id.clone(),
        source_id: rows[0].source_id,
    });
    assert!(
        matches!(validate_feature_graph(&rows), Err(CodecError::Malformed(message)) if message == "feature parent cycle")
    );
    rows[2].tree_parent = Some(TreeParent::Record {
        record_id: rows[0].id.clone(),
        source_id: None,
    });
    assert!(
        matches!(validate_feature_graph(&rows), Err(CodecError::Malformed(message)) if message == "feature tree cycle")
    );
}

#[test]
fn feature_graph_keeps_missing_and_duplicate_identity_diagnostics() {
    let mut rows = features();
    rows[1].source_id = rows[0].source_id;
    assert!(
        matches!(validate_feature_graph(&rows), Err(CodecError::Malformed(message)) if message == "duplicate feature source id")
    );
    rows = features();
    rows[1].id = rows[0].id.clone();
    assert!(
        matches!(validate_feature_graph(&rows), Err(CodecError::Malformed(message)) if message == "duplicate feature record id")
    );
    rows = features();
    assert!(
        matches!(validate_feature_graph(&rows[..2]), Err(CodecError::Malformed(message)) if message == "feature references missing parent")
    );
    rows[1].tree_parent = Some(TreeParent::Record {
        record_id: "absent".into(),
        source_id: None,
    });
    assert!(
        matches!(validate_feature_graph(&rows), Err(CodecError::Malformed(message)) if message == "feature references missing tree parent")
    );
}

#[test]
fn ancestry_walk_visits_a_shared_chain_once() {
    const COUNT: usize = 4096;
    let mut walk = AncestryWalk::new(COUNT);
    let mut visits = 0;
    for start in 0..COUNT {
        walk.walk(
            start,
            |slot| {
                visits += 1;
                Ok((slot + 1 < COUNT).then_some(slot + 1))
            },
            "cycle",
        )
        .unwrap();
    }
    assert_eq!(visits, COUNT);
}
