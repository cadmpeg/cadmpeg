use super::{parse_pattern_feature, PmDcPatternFamily, EXTRUSION_CLASS_ID, MIRROR_FEATURE_TYPE};
use super::{pattern_feature_bytes, segment, test_feature, test_label, test_type_id};
use crate::record_identity::Located;
use crate::test_support::test_fixtures::parse;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn feature_token_check_stops_at_first_distinct_token() {
    let bytes = pattern_feature_bytes(21, PmDcPatternFamily::Mirror);
    let pattern = parse(&bytes, |ctx, source| {
        parse_pattern_feature(ctx, source, 21, PmDcPatternFamily::Mirror).expect("pattern fixture")
    });
    for count in [2_u32, 256] {
        // Ordinary-only, pattern-only and ordinary followed by patterns.
        for mode in 0..3 {
            for with_label in [false, true] {
                let mut features = Vec::new();
                let mut pattern_features = Vec::new();
                for ordinal in 0..count {
                    let token = if ordinal == 1 {
                        cadmpeg_ir::identity_key!("other")
                    } else {
                        segment()
                    };
                    if mode == 0 || (mode == 2 && ordinal == 0) {
                        let mut feature = test_feature(ordinal, 0, &[]);
                        feature.identity.segment_token = token;
                        features.push(feature);
                    } else {
                        pattern_features.push(Located::new(
                            pattern.clone(),
                            test_type_id(MIRROR_FEATURE_TYPE),
                            token,
                            ordinal,
                        ));
                    }
                }
                let inventory = super::FeatureInventory {
                    features,
                    pattern_features,
                    terminators: Vec::new(),
                    properties: Vec::new(),
                    labels: if with_label {
                        vec![test_label(0, 1, EXTRUSION_CLASS_ID, &[])]
                    } else {
                        Vec::new()
                    },
                    entity_style_links: Vec::new(),
                    issues: Vec::new(),
                };
                let design = crate::design::DesignInventory {
                    parameters: Vec::new(),
                    expressions: Vec::new(),
                    units: Vec::new(),
                    issues: Vec::new(),
                };
                let sketch = crate::sketch::SketchInventory {
                    sketches: Vec::new(),
                    entities: Vec::new(),
                    transforms: Vec::new(),
                    directions: Vec::new(),
                    constraints: Vec::new(),
                    issues: Vec::new(),
                };
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = 0;
                policy.limits.max_materialized_bytes = 0;
                // One source visit compares the first token and the second token.
                let work = 1 + cadmpeg_core::decode::u64_from_index(
                    segment().as_str().len() + "other".len(),
                );
                policy.limits.max_work_units = work;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
                let projection = super::project(&ctx, &inventory, &design, &sketch, &[], &[])
                    .expect("second token ends the check");
                assert_eq!(
                    projection.unresolved_features,
                    usize::try_from(count).expect("count")
                );
                assert!(projection.features.is_empty());
                assert!(projection.result_topologies.is_empty());
                if with_label && mode != 1 {
                    assert!(matches!(ctx.charge_work(1, "probe"),
                    Err(CodecError::ResourceLimit(limit))
                        if limit.dimension == ResourceDimension::WorkUnits && limit.used == work));
                } else {
                    // No labels or no ordinary features means zero projection work.
                    ctx.charge_work(1, "probe")
                        .expect("no token search is needed");
                    assert!(matches!(ctx.charge_work(work, "probe"),
                    Err(CodecError::ResourceLimit(limit))
                        if limit.dimension == ResourceDimension::WorkUnits && limit.used == 1));
                }
            }
        }
    }
}
