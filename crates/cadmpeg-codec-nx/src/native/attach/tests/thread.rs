// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::feature_projection::body_writing_unresolved_feature_definition;
use std::collections::BTreeMap;

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, UnresolvedFamily};

#[test]
fn nx_body_writing_thread_labels_retain_distinct_unresolved_families() {
    let mut source_properties = BTreeMap::new();
    source_properties.insert("body_write.0".to_string(), "witness".to_string());

    let threads = body_writing_unresolved_feature_definition(
        &cadmpeg_test_support::service_decode_context(),
        "THREADS",
        &source_properties,
    )
    .expect("body-writing projection admission");
    let detailed = body_writing_unresolved_feature_definition(
        &cadmpeg_test_support::service_decode_context(),
        "DETAILED_THREAD",
        &source_properties,
    )
    .expect("body-writing projection admission");

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
        body_writing_unresolved_feature_definition(
            &cadmpeg_test_support::service_decode_context(),
            "THREADS",
            &source_properties
        )
        .expect("body-writing projection admission"),
        None
    );
    assert_eq!(
        body_writing_unresolved_feature_definition(
            &cadmpeg_test_support::service_decode_context(),
            "DETAILED_THREAD",
            &source_properties
        )
        .expect("body-writing projection admission"),
        None
    );
}
