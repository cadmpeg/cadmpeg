// SPDX-License-Identifier: Apache-2.0
//! Projection lookup construction follows its first consumer.

use crate::history::tests::feature;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn sketch_history_does_not_build_solid_or_construction_indexes() {
    let mut first = feature("first", Some("1"), 0);
    first.kind = "Sketch".into();
    first.xml_tag = "Sketch".into();
    let mut second = first.clone();
    second.id = "second".into();
    second.source_id = Some(crate::records::FeatureSource::from_value(2).unwrap());
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: std::collections::BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![first, second],
    };
    for operation in [
        "index SLDPRT source profiles",
        "index SLDPRT history records by identity",
        "index SLDPRT native construction sources",
        "index SLDPRT native construction features",
    ] {
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::WorkUnits,
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = u64::MAX;
            policy.limits.max_work_units = u64::MAX;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let _probe =
                cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(dimension, operation, None);
            let features = super::project_features(&ctx, std::slice::from_ref(&history)).unwrap();
            assert_eq!(features.len(), 2);
            assert!(features.iter().all(|feature| matches!(
                feature.evaluation.definition(),
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Sketch { .. }
                )
            )));
            ctx.finish_session().unwrap();
        }
    }
}
