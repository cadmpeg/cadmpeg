// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::body_writing_unresolved_feature_definition;
use std::collections::BTreeMap;

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, UnresolvedFamily};

#[test]
fn nx_body_writing_thread_labels_retain_distinct_unresolved_families() {
    let mut source_properties = BTreeMap::new();
    source_properties.insert("body_write.0".to_string(), "witness".to_string());

    let threads = body_writing_unresolved_feature_definition("THREADS", &source_properties);
    let detailed =
        body_writing_unresolved_feature_definition("DETAILED_THREAD", &source_properties);

    assert_eq!(
        threads,
        Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Thread
        }))
    );
    assert_eq!(
        detailed,
        Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DetailedThread
        }))
    );
}

#[test]
fn nx_non_body_writing_thread_labels_remain_unresolved_for_semantic_review() {
    let source_properties = BTreeMap::new();

    assert_eq!(
        body_writing_unresolved_feature_definition("THREADS", &source_properties),
        None
    );
    assert_eq!(
        body_writing_unresolved_feature_definition("DETAILED_THREAD", &source_properties),
        None
    );
}
