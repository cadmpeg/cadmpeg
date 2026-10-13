//! Pattern source, thread, generated-identity and line views wait for their readers.

use super::*;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const SOURCES: &str = "index SLDPRT component path producers";
const THREADS: &str = "index SLDPRT cosmetic thread seeds";

fn inputs(class: &str) -> (FeatureHistory, FeatureInputLane, [Feature; 1]) {
    let native = |source, ordinal, name: &str, class: &str| NativeFeature {
        id: name.into(),
        parent: "history".into(),
        xml_tag: "Feature".into(),
        tree_parent: None,
        source_id: FeatureSource::from_value(source),
        ordinal,
        name: name.into(),
        kind: "Custom".into(),
        input_class: Some(class.into()),
        suppressed: false,
        parameters: BTreeMap::new(),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: Vec::new(),
    };
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native(7, 1, "pattern", class),
            native(8, 2, "following", "moDerivedCosmeticThread_c"),
            native(9, 0, "thread", "moCosmeticThread_c"),
        ],
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: vec![0; 128],
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "pattern-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                value: "pattern".into(),
                object_id: ObjectId::from_value(7),
            },
            FeatureInputName {
                id: "following-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 64,
                value: "following".into(),
                object_id: ObjectId::from_value(8),
            },
        ],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let model = [Feature {
        id: FeatureId::mint("synthetic:test:id#pattern").unwrap(),
        ordinal: 0,
        name: Some("pattern".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::UNRESOLVED,
            }),
        ),
        native_ref: Some("pattern".into()),
    }];
    (history, lane, model)
}

#[test]
fn pattern_preparation_requires_both_input_sides() {
    let (history, lane, model) = inputs("moCirPattern_c");
    for absent_model in [true, false] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut candidates = if absent_model {
            Vec::new()
        } else {
            model.to_vec()
        };
        let before = candidates.clone();
        let lanes = if absent_model {
            std::slice::from_ref(&lane)
        } else {
            &[]
        };
        bind_pattern_inputs(&ctx, &mut candidates, std::slice::from_ref(&history), lanes).unwrap();
        assert_eq!(candidates, before);
        assert!(ctx.resource_refusal().is_none());
    }
}

#[test]
fn pattern_indexes_wait_for_a_model_consumer() {
    let (history, lane, mut model) = inputs("moMirrorPattern_c");
    model[0].native_ref = Some("unselected-pattern".into());
    for operation in [SOURCES, THREADS] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let mut candidates = model.clone();
        bind_pattern_inputs(
            &ctx,
            &mut candidates,
            std::slice::from_ref(&history),
            std::slice::from_ref(&lane),
        )
        .unwrap();
        assert_eq!(candidates, model);
        let CodecError::ResourceLimit(limit) = ctx.charge_work(1, operation).unwrap_err() else {
            panic!("an absent reader must leave the construction probe armed");
        };
        assert_eq!(limit.operation, operation);
    }
}

#[test]
fn derived_thread_seed_index_has_a_first_reader_boundary() {
    let (history, lane, model) = inputs("moMirrorPattern_c");
    let solve = |ctx: &DecodeContext<'_>| {
        let mut candidates = model.clone();
        bind_pattern_inputs(
            ctx,
            &mut candidates,
            std::slice::from_ref(&history),
            std::slice::from_ref(&lane),
        )
    };
    solve(&cadmpeg_test_support::service_decode_context()).unwrap();
    crate::test_support::work_refusal_at(THREADS, solve);
}

#[test]
fn pattern_source_index_has_a_first_reader_boundary() {
    let (history, mut lane, model) = inputs("moCirPattern_c");
    let signature = |source: u32| {
        let mut bytes = [0; 12];
        bytes[4..8].copy_from_slice(&source.to_le_bytes());
        bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        bytes
    };
    lane.generated_surface_identities
        .push(FeatureInputGeneratedSurfaceIdentity {
            id: "identity".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 32,
            type_prefix: [0; 4],
            feature_source_id: 9_u32.try_into().unwrap(),
            local_identity: 1,
            components: vec![
                FeatureInputComponentPathEntry {
                    instance: None,
                    type_signature: signature(7),
                    local_id: None,
                },
                FeatureInputComponentPathEntry {
                    instance: None,
                    type_signature: signature(9),
                    local_id: Some(1),
                },
            ],
        });
    let solve = |ctx: &DecodeContext<'_>| {
        let mut candidates = model.clone();
        bind_pattern_inputs(
            ctx,
            &mut candidates,
            std::slice::from_ref(&history),
            std::slice::from_ref(&lane),
        )
    };
    solve(&cadmpeg_test_support::service_decode_context()).unwrap();
    crate::test_support::work_refusal_at(SOURCES, solve);
}

#[test]
fn pattern_lane_views_wait_for_their_specific_readers() {
    for class in ["moCirPattern_c", "moLPattern_c", "moMirrorPattern_c"] {
        let (history, mut lane, mut model) = inputs(class);
        lane.classes.push(FeatureInputClass {
            id: "line-class".into(), parent: "lane".into(), ordinal: 0,
            offset: 16, name: "moLineRef_w".into(),
        });
        for selected in [false, true] {
            model[0].native_ref = Some(if selected { "pattern" } else { "unselected-pattern" }.into());
            // Only circular seeds consume identities. The unresolved linear form
            // has no admitted direction, and mirrors consume neither lane view.
            let operations = if selected && class == "moCirPattern_c" {
                &["scan SLDPRT pattern line declarations"][..]
            } else {
                &["decode SLDPRT generated surface identities", "scan SLDPRT pattern line declarations"][..]
            };
            for operation in operations {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = u64::MAX;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
                let mut candidates = model.clone();
                bind_pattern_inputs(&ctx, &mut candidates, std::slice::from_ref(&history), std::slice::from_ref(&lane)).unwrap();
                assert_eq!(candidates, model);
                let CodecError::ResourceLimit(limit) = ctx.charge_work(1, operation).unwrap_err() else {
                    panic!("unused lane preparation must leave the probe armed");
                };
                assert_eq!(limit.operation, *operation);
                drop(probe);
            }
        }
    }
}

#[test]
fn pattern_lane_views_have_first_reader_boundaries() {
    for (class, operation) in [
        ("moCirPattern_c", "decode SLDPRT generated surface identities"),
        ("moLPattern_c", "scan SLDPRT pattern line declarations"),
    ] {
        let (history, mut lane, mut model) = inputs(class);
        lane.classes.push(FeatureInputClass {
            id: "line-class".into(), parent: "lane".into(), ordinal: 0,
            offset: 16, name: "moLineRef_w".into(),
        });
        if class == "moLPattern_c" {
            model[0].evaluation.set_definition(FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::new(PatternTransform::Linear {
                    direction: None,
                    spacing: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
                    count: 2, second: None,
                }).unwrap(),
            }));
        }
        crate::test_support::work_refusal_at(operation, |ctx| {
            let mut candidates = model.clone();
            bind_pattern_inputs(ctx, &mut candidates, std::slice::from_ref(&history), std::slice::from_ref(&lane))
        });
    }
}

mod scratch_lifetimes;
