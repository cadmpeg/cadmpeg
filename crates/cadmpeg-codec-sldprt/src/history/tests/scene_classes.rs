//! Deferred scene-class lookup tests.

use super::feature;
use crate::history::enrich_scene_classes;
use crate::records::FeatureHistory;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::{BTreeMap, HashMap};

fn history(features: Vec<crate::records::Feature>) -> FeatureHistory {
    FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features,
    }
}

#[test]
fn scene_class_lookup_skips_histories_without_consumers() {
    let mut classified = feature("classified", Some("153"), 0);
    classified.input_class = Some("moDirectionLight_c".into());
    let mut payload = feature("payload", Some("155"), 1);
    payload.text = Some("retained feature payload".into());
    let mut histories = vec![
        history(Vec::new()),
        history(vec![classified.clone(), payload.clone(), feature("unbound", None, 2)]),
    ];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    enrich_scene_classes(&ctx, &mut histories, || {
        panic!("scene index has no consumer")
    })
    .expect("no scene lookup");
    assert_eq!(histories[1].features[0].input_class, classified.input_class);
    assert_eq!(histories[1].features[1].text, payload.text);
    assert_eq!(histories[1].features[2].input_class, None);
    ctx.finish_session().expect("accepted session");
}

#[test]
fn scene_class_lookup_builds_once_for_multiple_consumers() {
    let mut histories = vec![history(vec![
        feature("first", Some("153"), 0),
        feature("second", Some("155"), 1),
    ])];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let mut builds = 0;
    enrich_scene_classes(&ctx, &mut histories, || {
        builds += 1;
        Ok(HashMap::from([
            (153, "moDirectionLight_c".into()),
            (155, "moDirectionLight_c".into()),
        ]))
    })
    .expect("scene classes");
    assert_eq!(builds, 1);
    for feature in &histories[0].features {
        assert_eq!(feature.input_class.as_deref(), Some("moDirectionLight_c"));
    }
    ctx.finish_session().expect("accepted session");
}
