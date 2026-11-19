// SPDX-License-Identifier: Apache-2.0
//! Tests of the v11 design of a Fusion document.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureDefinition as LegacyDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation as LegacyOperation, UnresolvedFamily,
};
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition, Storage};

use crate::records::entity_header::{
    BaseTypeGuid, DesignFeatureTimeline, DesignTimelineFrame, SegmentType, SegmentTypeData,
};
use crate::records::identity::{Located, ReferenceRun};

const STREAM: &str = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";
const GROUND: &str = "21E03EC1-0E53-406D-A852-6D088D4D1E95";

fn timeline(stream: &str, source_ordinal: u32, items: &[u64]) -> DesignFeatureTimeline {
    let byte_offset = 100 * u64::from(source_ordinal);
    DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, byte_offset),
        DesignTimelineFrame::test_items(
            byte_offset,
            items
                .iter()
                .map(|&value| Located { value, offset: 0 })
                .collect(),
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        source_ordinal,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap()
}

fn record_type(guid: &str, entities: Vec<u64>) -> SegmentType {
    SegmentType::try_new(
        "f3d:FusionAssetName[Active]/Design1/MetaStream.dat:design-type#0".to_owned(),
        SegmentTypeData {
            byte_offset: 0,
            type_guid: guid.to_owned().try_into().unwrap(),
            type_guid_offset: 0,
            base_type_guid: BaseTypeGuid::Absent,
            version: 1,
            version_offset: 0,
            module: "Fusion".into(),
            entities: ReferenceRun::unlocated(entities),
        },
    )
    .unwrap()
}

fn legacy(
    key: &str,
    ordinal: u64,
    suppressed: Option<bool>,
    operation: LegacyOperation,
) -> LegacyFeature {
    LegacyFeature {
        product_definition: None,
        id: FeatureId::mint(format!("f3d:model:feature#{key}")).unwrap(),
        ordinal,
        name: Some(key.to_owned()),
        suppressed,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(LegacyDefinition::Operation(operation)),
        native_ref: Some(format!("{STREAM}:design-parameter-scope#{ordinal}")),
    }
}

fn members(container: &Container) -> &[Member] {
    let Container::Ordered { members, .. } = container else {
        panic!("ordered container")
    };
    members
}

#[test]
fn each_item_of_a_listing_timeline_is_one_feature_in_item_order() {
    let features = [
        legacy(
            "extrude:20",
            1,
            Some(true),
            LegacyOperation::Unresolved {
                family: UnresolvedFamily::Extrude,
            },
        ),
        legacy(
            "probe:10",
            0,
            Some(false),
            LegacyOperation::Native {
                kind: "Probe".into(),
                parameters: Default::default(),
            },
        ),
    ];
    let timelines = [timeline(STREAM, 1, &[]), timeline(STREAM, 0, &[10, 20, 30])];
    let types = [record_type(GROUND, vec![30])];
    let documents = crate::test_support::with_decode_context(|ctx| {
        super::timeline::project(ctx, &features, &timelines, &types)
    })
    .unwrap();
    assert_eq!(documents.keys().collect::<Vec<_>>(), [""]);
    let projected = &documents[""];
    assert_eq!(projected.containers.len(), 1);
    let container = &projected.containers[0];
    let Container::Ordered {
        kind,
        folders,
        rollback,
        ..
    } = container
    else {
        panic!("ordered container")
    };
    assert_eq!(*kind, OrderedKind::Timeline);
    assert!(folders.is_empty());
    assert_eq!(*rollback, Rollback::End);
    let ids = projected
        .features
        .iter()
        .map(|feature| Member::Feature(feature.id.clone()))
        .collect::<Vec<_>>();
    assert_eq!(members(container), ids.as_slice());
    let [probe, extrude, ground] = projected.features.as_slice() else {
        panic!("three features")
    };
    assert_eq!(probe.id.as_str(), "f3d:model:feature#probe:10");
    assert_eq!(probe.suppressed, Some(Dimension::Literal(false)));
    assert!(matches!(
        &probe.definition.operation,
        FeatureOperation::Native { kind, .. } if kind.as_str() == "f3d:Probe"
    ));
    assert_eq!(extrude.suppressed, Some(Dimension::Literal(true)));
    assert_eq!(
        extrude.definition.operation,
        FeatureOperation::Unresolved {
            family: OperationFamily::Extrude
        }
    );
    assert_eq!(extrude.native_ref, features[0].native_ref);
    assert!(matches!(
        &ground.definition.operation,
        FeatureOperation::Native { kind, .. } if kind.as_str() == "f3d:Ground"
    ));
    assert_eq!(ground.suppressed, None);
    assert_eq!(
        ground.native_ref.as_deref(),
        Some(timelines[1].id().as_str())
    );
}

#[test]
fn a_stream_whose_timelines_list_no_item_states_no_container() {
    let documents = crate::test_support::with_decode_context(|ctx| {
        super::timeline::project(ctx, &[], &[timeline(STREAM, 0, &[])], &[])
    })
    .unwrap();
    assert!(documents.is_empty());
}

#[test]
fn an_item_of_an_unregistered_type_is_a_native_timeline_item() {
    let documents = crate::test_support::with_decode_context(|ctx| {
        super::timeline::project(
            ctx,
            &[],
            &[timeline(STREAM, 0, &[7])],
            &[],
            &BTreeSet::new(),
        )
    })
    .unwrap();
    let [feature] = documents[""].features.as_slice() else {
        panic!("one feature")
    };
    assert!(matches!(
        &feature.definition.operation,
        FeatureOperation::Native { kind, .. } if kind.as_str() == "f3d:timeline-item"
    ));
}

#[test]
fn a_referenced_design_is_a_document_of_its_own() {
    const XREF: &str = "xref/role-1/reference-0/occurrence-0/";
    assert_eq!(super::timeline::document_prefix(STREAM), "");
    let nested = format!("f3d:{XREF}FusionAssetName[Active]/Design1/BulkStream.dat");
    assert_eq!(super::timeline::document_prefix(&nested), XREF);
    let deeper = format!("f3d:{XREF}xref/role-2/reference-1/occurrence-3/Design1/BulkStream.dat");
    assert_eq!(
        super::timeline::document_prefix(&deeper),
        format!("{XREF}xref/role-2/reference-1/occurrence-3/")
    );
}

fn definition(id: &str, storage: Storage, bodies: &[&str]) -> ProductDefinition {
    let mut definition = ProductDefinition::root(
        ProductDefinitionId::mint(id).unwrap(),
        ExternalDocument::default(),
    );
    definition.storage = storage;
    definition.bodies = bodies
        .iter()
        .map(|body| BodyId::mint(*body).unwrap())
        .collect();
    definition
}

#[test]
fn a_definition_and_its_bodies_belong_to_the_scope_that_spans_it() {
    const XREF: &str = "xref/role-1/reference-0/occurrence-0/";
    let root = crate::tree::file_root_id();
    let member = format!("f3d:{XREF}archive:product-definition#0");
    let definitions = [
        definition(
            root.as_str(),
            ProductDefinition::root(root.clone(), ExternalDocument::default()).storage,
            &["f3d:brep:entity#1"],
        ),
        definition(
            "f3d:model:component#a",
            Storage::Embedded,
            &["f3d:brep:entity#2"],
        ),
        definition(&member, Storage::Embedded, &["f3d:brep:entity#3"]),
        definition(
            &format!("f3d:{XREF}model:component#b"),
            Storage::Embedded,
            &["f3d:brep:entity#4"],
        ),
        definition(
            "f3d:model:product-definition#external",
            Storage::External(ExternalDocument::default()),
            &[],
        ),
    ];
    assert_eq!(
        super::scope_definition(&definitions, XREF)
            .unwrap()
            .as_str(),
        member
    );
    let scopes = BTreeMap::from([
        (String::new(), (root, ())),
        (
            XREF.to_owned(),
            (ProductDefinitionId::mint(&member).unwrap(), ()),
        ),
    ]);
    let spans = super::span(&definitions, &scopes);
    let spanned = spans
        .iter()
        .map(|(prefix, span)| (prefix.clone(), span.definitions.len()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        spanned,
        BTreeMap::from([(String::new(), 2), (XREF.to_owned(), 2)])
    );
    let bodies = spans
        .into_iter()
        .map(|(prefix, span)| {
            (
                prefix,
                span.bodies
                    .iter()
                    .map(|body| body.as_str().to_owned())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        bodies,
        BTreeMap::from([
            (
                String::new(),
                vec![
                    "f3d:brep:entity#1".to_owned(),
                    "f3d:brep:entity#2".to_owned()
                ]
            ),
            (
                XREF.to_owned(),
                vec![
                    "f3d:brep:entity#3".to_owned(),
                    "f3d:brep:entity#4".to_owned()
                ]
            ),
        ])
    );
}
