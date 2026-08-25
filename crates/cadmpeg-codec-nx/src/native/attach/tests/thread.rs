// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::features::FeatureDefinition;

use super::*;

#[test]
fn nx_body_writing_thread_labels_retain_distinct_unresolved_families() {
    let mut source_properties = BTreeMap::new();
    source_properties.insert("body_write.0".to_string(), "witness".to_string());

    let threads = super::body_writing_thread_definition("THREADS", &source_properties);
    let detailed = super::body_writing_thread_definition("DETAILED_THREAD", &source_properties);

    assert_eq!(threads, Some(FeatureDefinition::ThreadUnresolved));
    assert_eq!(detailed, Some(FeatureDefinition::DetailedThreadUnresolved));
}

#[test]
fn nx_non_body_writing_thread_labels_remain_unresolved_for_semantic_review() {
    let source_properties = BTreeMap::new();

    assert_eq!(
        super::body_writing_thread_definition("THREADS", &source_properties),
        None
    );
    assert_eq!(
        super::body_writing_thread_definition("DETAILED_THREAD", &source_properties),
        None
    );
}
