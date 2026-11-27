// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::products::{ProductDefinition, ProductDefinitionKind};
use crate::report::check::Check;
use crate::validate::validate_neutral;
use crate::CadIr;

#[test]
fn typed_reference_walk_ignores_id_shaped_plain_strings() {
    let mut ir = crate::CadIr::empty();
    let owner =
        crate::ids::ProductDefinitionId::mint("test:model:product#owner").expect("valid identity");
    let target = crate::ids::BodyId::mint("test:model:body#missing").expect("valid identity");
    ir.model.product_definitions.push(ProductDefinition {
        sketches: Vec::default(),

        id: owner.clone(),
        kind: Some(ProductDefinitionKind::Part),
        source_name: Some("test:model:name#not-a-reference".into()),
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::new(),
        bodies: vec![target.clone()],
        native_ref: None,
        generated_from: None,
        storage: crate::products::Storage::Embedded,
        occurrences: Vec::new(),
        instance_bodies: Vec::new(),
        representations: Vec::new(),
        units: None,
        general_tolerance: None,
        active: None,
    });

    let mut references = Vec::new();
    crate::schema::EntitySchema::visit_references(
        &ir.model.product_definitions[0],
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &mut |reference| {
            references.push(reference.to_owned());
            Ok(())
        },
    )
    .expect("every entity states its typed references");
    assert_eq!(references, vec![target.as_str().to_owned()]);
    let mut borrowed_references = Vec::new();
    crate::schema::EntitySchema::visit_references(
        &ir.model.product_definitions[0],
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &mut |reference| {
            borrowed_references.push(reference.to_owned());
            Ok(())
        },
    )
    .expect("every entity states its typed references");
    assert_eq!(borrowed_references, references);

    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity
            && finding.entity.as_deref() == Some(owner.as_str())
            && finding.message.contains(target.as_str())
    }));
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.message.contains("not-a-reference")));
}

#[test]
fn typed_reference_walk_treats_historical_members_as_state_local() {
    use crate::features::edge_treatments::{FilletGroup, RadiusSpec};
    use crate::features::evaluation::FeatureInputTopology;
    use crate::features::selections::EdgeSelection;
    use crate::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use crate::ids::{FeatureInputTopologyId, HistoricalEdgeId};
    use crate::schema::EntitySchema;

    let feature_id = FeatureId::mint("test:model:feature#owner").expect("identity grammar");
    let state_id =
        FeatureInputTopologyId::mint("test:model:feature-input#owner").expect("valid identity");
    let historical_edge =
        HistoricalEdgeId::mint("test:model:historical-edge#local").expect("valid identity");
    let state = FeatureInputTopology {
        face_owners: Vec::new(),
        face_geometry: Vec::new(),
        body_writers: Vec::new(),
        id: state_id.clone(),
        input_of: feature_id.clone(),
        bodies: crate::features::selections::DistinctMembers::try_from(Vec::new()).unwrap(),
        faces: crate::features::selections::DistinctMembers::try_from(Vec::new()).unwrap(),
        edges: crate::features::selections::DistinctMembers::try_from(
            vec![historical_edge.clone()],
        )
        .unwrap(),
        vertices: crate::features::selections::DistinctMembers::try_from(Vec::new()).unwrap(),
        edge_geometry: Vec::new(),
        native_ref: None,
    };
    let feature = Feature {
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: crate::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Fillet {
                groups: crate::features::selections::NonEmptyMembers::one(FilletGroup {
                    edges: EdgeSelection::historical(
                        state_id.clone(),
                        vec![historical_edge],
                        "edge:local".into(),
                    )
                    .unwrap(),
                    radius: RadiusSpec::Constant {
                        radius: crate::scalar::PositiveLength::new(1.0).unwrap(),
                    },
                    tangency_weight: None,
                }),
            }),
        ),
        native_ref: None,
    };

    let mut state_references = Vec::new();
    state
        .visit_references(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &mut |reference| {
                state_references.push(reference.to_owned());
                Ok(())
            },
        )
        .expect("state states its typed references");
    assert_eq!(state_references, vec![feature_id.as_str().to_owned()]);
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut borrowed_state_references = Vec::new();
    state
        .visit_references(&ctx, &mut |target| {
            borrowed_state_references.push(target.to_owned());
            Ok(())
        })
        .unwrap();
    assert_eq!(borrowed_state_references, state_references);

    let mut feature_references = Vec::new();
    feature
        .visit_references(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &mut |reference| {
                feature_references.push(reference.to_owned());
                Ok(())
            },
        )
        .expect("feature states its typed references");
    assert_eq!(feature_references, vec![state_id.as_str()]);
    let mut borrowed_feature_references = Vec::new();
    feature
        .visit_references(&ctx, &mut |target| {
            borrowed_feature_references.push(target.to_owned());
            Ok(())
        })
        .unwrap();
    assert_eq!(borrowed_feature_references, feature_references);
    let mut local_members = Vec::new();
    crate::schema::rewrite::typed::RewriteIdentities::visit_local_members(
        feature.evaluation.definition(),
        &ctx,
        &mut |member| {
            local_members.push(member.to_owned());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(local_members, vec!["test:model:historical-edge#local"]);

    let mut ir = CadIr::empty();
    ir.model.feature_input_topologies.push(state);
    ir.model.features.push(feature);
    assert!(!validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model")
        .findings
        .iter()
        .any(|finding| finding.check == Check::ReferentialIntegrity));

    let missing = "test:model:historical-edge#missing";
    ir.model.features[0].evaluation.edit(|definition, _| {
        let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) = definition else {
            unreachable!("test feature is a fillet")
        };
        let EdgeSelection::Historical { edges, .. } = &mut groups[0].edges else {
            unreachable!("test fillet uses a historical selection")
        };
        *edges = vec![HistoricalEdgeId::mint(missing).expect("valid identity")]
            .try_into()
            .unwrap();
    });
    assert!(validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model")
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::ReferentialIntegrity
                && finding.entity.as_deref() == Some(feature_id.as_str())
                && finding.message == format!("references missing historical edge `{missing}`")
        }));
}

#[test]
fn borrowed_reference_walk_refuses_at_the_depth_limit() {
    use crate::schema::EntitySchema;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let entity = ProductDefinition {
        sketches: Vec::default(),

        id: "test:model:product#owner".try_into().unwrap(),
        kind: Some(ProductDefinitionKind::Part),
        source_name: Some("test:model:name#not-a-reference".into()),
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::new(),
        bodies: vec!["test:model:body#target".try_into().unwrap()],
        native_ref: None,
        generated_from: None,
        storage: crate::products::Storage::Embedded,
        occurrences: Vec::new(),
        instance_bodies: Vec::new(),
        representations: Vec::new(),
        units: None,
        general_tolerance: None,
        active: None,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = entity.visit_references(&ctx, &mut |_| Ok(())).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("borrowed walk refusal must remain a resource error");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut seen = 0;
    entity
        .visit_references(&ctx, &mut |target| {
            assert_eq!(target, "test:model:body#target");
            seen += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(seen, 1);
}

#[test]
fn borrowed_reference_walk_preserves_a_callback_resource_refusal() {
    use crate::schema::EntitySchema;
    use cadmpeg_core::decode::{ResourceDimension, ResourceLimit};
    use cadmpeg_core::CodecError;
    let entity = crate::topology::Vertex {
        id: "test:model:vertex#owner".try_into().unwrap(),
        point: "test:model:point#target".try_into().unwrap(),
        tolerance: None,
        native_ref: None,
    };
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let limit = ResourceLimit::allocation_failed(
        ResourceDimension::RetainedBytes,
        4096,
        64,
        "borrowed reference callback",
    );
    let error = entity
        .visit_references(&ctx, &mut |_| Err(CodecError::ResourceLimit(limit)))
        .unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(original) if original == limit));
}
