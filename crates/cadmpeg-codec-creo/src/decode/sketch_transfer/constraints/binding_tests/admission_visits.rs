// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::{DefinitionIdentity, FeatureDefinition};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::SketchId;
use std::collections::BTreeSet;

#[test]
fn absent_emitted_radius_candidates_are_free_and_keep_original_refusal() {
    let definition = FeatureDefinition {
        identity: DefinitionIdentity::Parsed { schema_id: std::num::NonZeroU32::new(1), owner_feature_id: None },
        body: Vec::new(), parameter_frames: Vec::new(), outlines: Vec::new(), variables: None,
        segments: None, trim_entities: None, trim_vertices: None, order_table: None,
        section_3d: None, dimensions: None, relations: None, saved_section: None, offset: 0,
    };
    let sketch = SketchId::mint("creo:model:sketch#1").expect("valid sketch ID");
    for refused in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let original = refused.then(|| match ctx.charge_work(1, "seed radius refusal") {
            Err(CodecError::ResourceLimit(limit)) => limit,
            other => panic!("expected seed refusal: {other:?}"),
        });
        for _ in 0..2 {
            let result = super::super::section_segment_radius_constraints_for_emitted(
                &ctx, &definition, &sketch, &BTreeSet::new(), &BTreeSet::new(),
            );
            if let Some(original) = original {
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                assert!(result.expect("no radius bindings").is_empty());
            }
        }
        if let Some(original) = original {
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else { ctx.finish_session().expect("active session"); }
    }
}
