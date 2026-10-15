//! Edge identity indexing begins at a terminal or producer lookup.

use crate::records::{FeatureInputEdgeSelection, FeatureInputLane};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::edge_treatments::{FilletGroup, RadiusSpec};
use cadmpeg_ir::features::{
    EdgeSelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    NonEmptyMembers, UnresolvedFamily,
};
use std::collections::BTreeMap;

const IDENTITIES: &str = "index SLDPRT compact edge feature identities";

fn inputs() -> ([Feature; 2], FeatureInputLane) {
    let features = ["consumer", "producer"].map(|name| Feature {
        id: FeatureId::mint(format!("synthetic:test:id#{name}")).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Unresolved {
                family: UnresolvedFamily::Draft,
            },
        )),
        native_ref: Some(name.into()),
    });
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: vec![FeatureInputEdgeSelection {
            id: "edge".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_name_ref: "name".into(),
            feature_ref: "consumer".into(),
            local_edge_ids: vec![7],
            components: Vec::new(),
            references: Vec::new(),
            producer_feature_refs: Vec::new(),
            terminal_feature_ref: None,
        }],
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    (features, lane)
}

fn fillet(edges: EdgeSelection) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Fillet {
        groups: NonEmptyMembers::one(FilletGroup {
            edges,
            radius: RadiusSpec::Constant {
                radius: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
            },
            tangency_weight: None,
        }),
    })
}

#[test]
fn edge_identity_index_waits_for_a_lookup() {
    for route in [0, 1, 2] {
        let (mut features, mut lane) = inputs();
        if route != 0 {
            features[0].evaluation.set_definition(fillet(if route == 1 {
                EdgeSelection::Unresolved
            } else {
                EdgeSelection::Native("existing".into())
            }));
        }
        if route == 2 {
            lane.edge_selections[0].terminal_feature_ref = Some("producer".into());
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, IDENTITIES, None);
        super::super::project_compact_edge_selections(&ctx, &mut features, &[], &[lane]).unwrap();
        if route == 1 {
            assert!(matches!(features[0].evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
                    if matches!(&groups.as_slice()[0].edges, EdgeSelection::Native(value)
                        if value == "sldprt:feature-input:edge-ids:7")));
        } else if route == 2 {
            assert_eq!(
                features[0].evaluation.definition(),
                &fillet(EdgeSelection::Native("existing".into()))
            );
        }
        assert!(features[0].dependencies.is_empty());
        let CodecError::ResourceLimit(limit) = ctx.charge_work(1, IDENTITIES).unwrap_err() else {
            panic!("an absent identity lookup must leave the index probe armed");
        };
        assert_eq!(limit.operation, IDENTITIES);
        drop(probe);
    }
}

#[test]
fn edge_identity_index_has_terminal_and_producer_reader_boundaries() {
    for terminal in [false, true] {
        let (mut features, mut lane) = inputs();
        features[0]
            .evaluation
            .set_definition(fillet(EdgeSelection::Unresolved));
        if terminal {
            lane.edge_selections[0].terminal_feature_ref = Some("producer".into());
        } else {
            lane.edge_selections[0]
                .producer_feature_refs
                .push("producer".into());
        }
        crate::test_support::work_refusal_at(IDENTITIES, |ctx| {
            super::super::project_compact_edge_selections(
                ctx,
                &mut features.clone(),
                &[],
                std::slice::from_ref(&lane),
            )
        });
        let ctx = cadmpeg_test_support::service_decode_context();
        super::super::project_compact_edge_selections(&ctx, &mut features, &[], &[lane]).unwrap();
        if terminal {
            assert!(matches!(features[0].evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Fillet { groups })
                    if matches!(&groups.as_slice()[0].edges, EdgeSelection::Generated { edges, .. }
                        if edges.as_slice()[0].feature == features[1].id)));
        } else {
            assert_eq!(
                features[0].dependencies.as_slice(),
                std::slice::from_ref(&features[1].id)
            );
        }
    }
}
