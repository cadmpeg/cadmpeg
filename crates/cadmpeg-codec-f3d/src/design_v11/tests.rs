// SPDX-License-Identifier: Apache-2.0
//! Tests of the v11 design of a Fusion document.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::{Dimension, ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{Expression, ExpressionDialect, ExpressionToken, Neutral, Node};
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::units::UnitSymbol;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{
    Feature as LegacyFeature, FeatureContent, FeatureDefinition as LegacyDefinition, FeatureId,
    FeatureOperation as LegacyOperation, UnresolvedFamily,
};
use cadmpeg_ir::ids::{BodyId, ProductDefinitionId};
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition, Storage};

use crate::records::entity_header::{
    BaseTypeGuid, DesignFeatureTimeline, DesignTimelineFrame, SegmentType, SegmentTypeData,
};
use crate::records::feature::scope::DesignScopeTailFlags;
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
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
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
fn an_unresolved_coordinate_frame_keeps_the_next_timeline_item() {
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#200"),
        crate::records::feature::scope::DesignFeatureKind::JointOrigin,
        20,
    );
    scope.with_joint_origin_transform(
        [
            [0.0, -1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, -2.0],
            [0.0, 0.0, 1.0, 3.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap(),
    );
    let frame = crate::design::feature_project::joint_origin_frame(&scope).unwrap();
    let features = [
        legacy(
            "joint-origin:10",
            0,
            None,
            LegacyOperation::Native {
                kind: "JointOrigin".into(),
                parameters: BTreeMap::default(),
            },
        ),
        legacy(
            "joint-origin:20",
            1,
            Some(false),
            LegacyOperation::DatumCoordinateSystem { frame },
        ),
    ];
    let mut losses = Vec::new();
    let documents = crate::test_support::with_decode_context(|ctx| {
        super::timeline::project(
            ctx,
            &features,
            &[timeline(STREAM, 0, &[10, 20])],
            &[],
            &super::timeline::TailFlags::new(),
            &BTreeSet::new(),
            &mut losses,
        )
    })
    .unwrap();
    let projected = &documents[""];
    assert_eq!(projected.features.len(), 2);
    assert!(matches!(
        projected.features[0].definition.operation,
        FeatureOperation::Native { .. }
    ));
    let FeatureOperation::Datum(datum) = &projected.features[1].definition.operation else {
        panic!("datum frame")
    };
    let cadmpeg_ir::design::ops::datum::DatumElement::Frame(construction) = &datum.element else {
        panic!("coordinate frame")
    };
    let resolved = construction.resolved.as_ref().unwrap();
    assert_eq!(
        resolved.origin.get(),
        cadmpeg_ir::math::Point3::new(10.0, -20.0, 30.0)
    );
    assert_eq!(
        *resolved.x_axis.as_raw(),
        cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
    );
    assert_eq!(
        *resolved.y_axis.as_raw(),
        cadmpeg_ir::math::Vector3::new(-1.0, 0.0, 0.0)
    );
    assert_eq!(
        *resolved.z_axis.as_raw(),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    );
    assert_eq!(projected.features[1].native_ref, features[1].native_ref);
    assert_eq!(
        projected.features[1].suppressed,
        Some(Dimension::Literal(false))
    );
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.local_code())
            .collect::<Vec<_>>(),
        [
            "feature.coordinate-system-frame-unresolved",
            "feature.coordinate-system-construction-dropped",
            "history.rollback-unread",
        ]
    );
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
                parameters: BTreeMap::default(),
            },
        ),
    ];
    let timelines = [timeline(STREAM, 1, &[]), timeline(STREAM, 0, &[10, 20, 30])];
    let types = [record_type(GROUND, vec![30])];
    let mut losses = Vec::new();
    let documents = crate::test_support::with_decode_context(|ctx| {
        super::timeline::project(
            ctx,
            &features,
            &timelines,
            &types,
            &super::timeline::TailFlags::new(),
            &BTreeSet::new(),
            &mut losses,
        )
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
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::HistoryRollbackUnread.kind()
    );
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Warning);
    assert!(
        loss.message.contains(container.id().as_str()),
        "{}",
        loss.message
    );
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
    let mut losses = Vec::new();
    let documents = crate::test_support::with_decode_context(|ctx| {
        super::timeline::project(
            ctx,
            &[],
            &[timeline(STREAM, 0, &[])],
            &[],
            &super::timeline::TailFlags::new(),
            &BTreeSet::new(),
            &mut losses,
        )
    })
    .unwrap();
    assert!(documents.is_empty());
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn an_item_of_an_unregistered_type_is_a_native_timeline_item() {
    let documents = crate::test_support::with_decode_context(|ctx| {
        super::timeline::project(
            ctx,
            &[],
            &[timeline(STREAM, 0, &[7])],
            &[],
            &super::timeline::TailFlags::new(),
            &BTreeSet::new(),
            &mut Vec::new(),
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

fn tail_flags(suppressed: bool, rolled_back: bool) -> DesignScopeTailFlags {
    DesignScopeTailFlags {
        suppressed,
        rolled_back,
        suppressed_offset: 0,
        envelope_offset: None,
    }
}

/// The rollback marker, the members and the losses of one timeline of
/// `items`, whose scope tails carry the rolled-back flags in `flags`.
fn rollback_of(
    items: &[u64],
    flags: &[(u64, bool)],
) -> (
    Rollback,
    Vec<Member>,
    Vec<cadmpeg_ir::report::loss::LossKind>,
) {
    let flags: super::timeline::TailFlags<'_> = flags
        .iter()
        .map(|&(record, rolled_back)| ((STREAM, record), tail_flags(false, rolled_back)))
        .collect();
    let mut losses = Vec::new();
    let mut documents = crate::test_support::with_decode_context(|ctx| {
        super::timeline::project(
            ctx,
            &[],
            &[timeline(STREAM, 0, items)],
            &[],
            &flags,
            &BTreeSet::new(),
            &mut losses,
        )
    })
    .unwrap();
    let Container::Ordered {
        members, rollback, ..
    } = documents.remove("").unwrap().containers.remove(0)
    else {
        panic!("ordered container")
    };
    (
        rollback,
        members,
        losses.into_iter().map(|loss| loss.code).collect(),
    )
}

#[test]
fn the_rollback_marker_stands_before_the_first_of_the_trailing_rolled_back_items() {
    let (rollback, members, losses) = rollback_of(
        &[10, 20, 30, 40],
        &[(10, false), (20, false), (30, true), (40, true)],
    );
    assert_eq!(rollback, Rollback::Before(members[2].clone()));
    assert!(losses.is_empty(), "{losses:?}");
    let (rollback, members, losses) = rollback_of(&[10, 20], &[(10, true), (20, true)]);
    assert_eq!(rollback, Rollback::Before(members[0].clone()));
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn an_unread_item_before_the_rolled_back_items_charges_the_marker_bound_loss() {
    let (rollback, members, losses) = rollback_of(&[10, 20, 30], &[(10, false), (30, true)]);
    assert_eq!(rollback, Rollback::Before(members[2].clone()));
    assert_eq!(
        losses,
        [crate::loss::F3dLossCode::HistoryRollbackBoundUnread.kind()]
    );
}

#[test]
fn an_unread_item_after_the_first_rolled_back_item_charges_the_rollback_loss() {
    let (rollback, members, losses) = rollback_of(&[10, 20], &[(10, true)]);
    assert_eq!(rollback, Rollback::Before(members[0].clone()));
    assert_eq!(
        losses,
        [crate::loss::F3dLossCode::HistoryRollbackUnread.kind()]
    );
}

#[test]
fn a_timeline_whose_items_are_all_read_and_built_ends_after_its_last_item() {
    let (rollback, _, losses) = rollback_of(&[10, 20], &[(10, false), (20, false)]);
    assert_eq!(rollback, Rollback::End);
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn a_built_item_after_a_rolled_back_item_leaves_the_marker_unread() {
    let (rollback, _, losses) = rollback_of(&[10, 20], &[(10, true), (20, false)]);
    assert_eq!(rollback, Rollback::End);
    assert_eq!(
        losses,
        [crate::loss::F3dLossCode::HistoryRollbackUnread.kind()]
    );
}

#[test]
fn the_current_state_stands_at_the_rollback_marker() {
    let id =
        cadmpeg_ir::design::ids::ContainerId::mint("f3d:design:timeline#0".to_owned()).unwrap();
    let members = [10, 20, 30]
        .map(|key| Member::Feature(FeatureId::mint(format!("f3d:model:feature#{key}")).unwrap()));
    assert_eq!(
        super::rollback_position(&id, &members, &Rollback::End),
        Position::After(members[2].clone())
    );
    assert_eq!(
        super::rollback_position(&id, &members, &Rollback::Before(members[1].clone())),
        Position::After(members[0].clone())
    );
    assert_eq!(
        super::rollback_position(&id, &members, &Rollback::Before(members[0].clone())),
        Position::Start(id.clone())
    );
}

#[test]
fn an_unprojected_item_takes_its_suppression_from_its_scope_tail() {
    let flags: super::timeline::TailFlags<'_> = [((STREAM, 7), tail_flags(true, false))]
        .into_iter()
        .collect();
    let documents = crate::test_support::with_decode_context(|ctx| {
        super::timeline::project(
            ctx,
            &[],
            &[timeline(STREAM, 0, &[7, 8])],
            &[],
            &flags,
            &BTreeSet::new(),
            &mut Vec::new(),
        )
    })
    .unwrap();
    let [flagged, unflagged] = documents[""].features.as_slice() else {
        panic!("two features")
    };
    assert_eq!(flagged.suppressed, Some(Dimension::Literal(true)));
    assert_eq!(unflagged.suppressed, None);
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

#[test]
fn a_referenced_design_without_a_definition_is_hosted_by_its_enclosing_scope() {
    const OUTER: &str = "xref/role-1/reference-0/occurrence-0/";
    let scopes = BTreeMap::from([(String::new(), 0_u8), (OUTER.to_owned(), 0)]);
    let inner = format!("{OUTER}xref/role-2/reference-0/occurrence-1/");
    assert_eq!(super::host(&scopes, &inner), OUTER);
    assert_eq!(
        super::host(&scopes, "xref/role-3/reference-0/occurrence-0/"),
        ""
    );
    assert_eq!(super::host(&BTreeMap::<String, u8>::new(), OUTER), "");
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

#[test]
fn a_base_feature_whose_bodies_are_current_is_stored_geometry() {
    let body = BodyId::mint("f3d:brep:entity#1").unwrap();
    let base = |bodies: Vec<BodyId>| {
        legacy(
            "base:10",
            0,
            Some(false),
            LegacyOperation::BaseFeature {
                bodies: cadmpeg_ir::features::selections::BodySelection::Bodies(
                    bodies.into_iter().collect(),
                ),
            },
        )
    };
    let project = |feature: LegacyFeature, current: &[&str]| {
        let current = current.iter().copied().collect::<BTreeSet<_>>();
        let timelines = [timeline(STREAM, 0, &[10])];
        crate::test_support::with_decode_context(|ctx| {
            super::timeline::project(
                ctx,
                &[feature],
                &timelines,
                &[],
                &super::timeline::TailFlags::new(),
                &current,
                &mut Vec::new(),
            )
        })
        .unwrap()
        .remove("")
        .unwrap()
        .features
        .remove(0)
        .definition
        .operation
    };
    assert_eq!(
        project(base(vec![body.clone()]), &[body.as_str()]),
        FeatureOperation::Stored {
            bodies: Some(vec![body.clone()]),
            tessellations: Vec::new(),
            origin: cadmpeg_ir::design::feature::StoredOrigin::Base {},
        }
    );
    for (bodies, current) in [
        (vec![body.clone()], &[][..]),
        (Vec::new(), &[body.as_str()][..]),
    ] {
        assert!(matches!(
            project(base(bodies), current),
            FeatureOperation::Native { kind, .. } if kind.as_str() == "f3d:base_feature"
        ));
    }
}

fn parameter(
    key: &str,
    owner: Option<&str>,
    expression: &str,
    value: Option<cadmpeg_ir::features::parameters::ParameterValue>,
    dependencies: &[&str],
    properties: &[(&str, &str)],
) -> cadmpeg_ir::features::parameters::DesignParameter {
    cadmpeg_ir::features::parameters::DesignParameter {
        product_definition: None,
        id: cadmpeg_ir::features::ParameterId::mint(format!("f3d:model:parameter#{key}")).unwrap(),
        owner: owner.map(|owner| FeatureId::mint(owner).unwrap()),
        ordinal: 0,
        name: key.to_owned(),
        expression: expression.to_owned(),
        display: None,
        value,
        dependencies: dependencies
            .iter()
            .map(|key| {
                cadmpeg_ir::features::ParameterId::mint(format!("f3d:model:parameter#{key}"))
                    .unwrap()
            })
            .collect(),
        properties: properties
            .iter()
            .map(|(key, value)| {
                (
                    cadmpeg_core::text::NonBlankString::new((*key).to_owned()).unwrap(),
                    (*value).to_owned(),
                )
            })
            .collect(),
        pmi: None,
        native_ref: Some(format!("{STREAM}:design-parameter#{key}")),
    }
}

fn number(value: f64, unit: Option<cadmpeg_ir::design::units::UnitSymbol>) -> Expression {
    use cadmpeg_ir::design::units::{Unit, UnitFactor};
    Expression::Neutral(Neutral(Node::Number {
        value: cadmpeg_ir::scalar::NonNegativeReal::new(value).unwrap(),
        unit: Unit(
            unit.into_iter()
                .map(|symbol| UnitFactor {
                    symbol,
                    power: std::num::NonZeroI8::new(1).unwrap(),
                })
                .collect(),
        ),
    }))
}

fn source(tokens: Vec<ExpressionToken<ParameterRef>>) -> Expression {
    Expression::Source {
        dialect: ExpressionDialect::Fusion,
        tokens,
    }
}

#[test]
fn parameters_state_literals_and_source_text_in_their_scope() {
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};
    const OWNER: &str = "f3d:model:feature#extrude:20";
    const XREF: &str = "xref/role-1/reference-0/occurrence-0/";
    let mut shared = parameter(
        "shared",
        Some(format!("f3d:{XREF}model:document-parameters#0").as_str()),
        "2 mm",
        Some(V10::Length(Length::new(2.0).unwrap())),
        &[],
        &[],
    );
    shared.native_ref = Some(format!(
        "f3d:{XREF}FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter#shared"
    ));
    let legacy = [
        parameter(
            "d1",
            Some(OWNER),
            "10 mm",
            Some(V10::Length(Length::new(10.0).unwrap())),
            &[],
            &[("source_kind", "Linear Dimension-1"), ("comment", "depth")],
        ),
        parameter(
            "d2",
            Some(OWNER),
            "d1 * 2",
            Some(V10::Length(Length::new(20.0).unwrap())),
            &["d1"],
            &[],
        ),
        parameter(
            "turn",
            None,
            "45 deg",
            Some(V10::Angle(Angle::new(std::f64::consts::FRAC_PI_4).unwrap())),
            &[],
            &[],
        ),
        parameter(
            "alias",
            None,
            "d1",
            Some(V10::Length(Length::new(10.0).unwrap())),
            &["d1"],
            &[],
        ),
        parameter(
            "count",
            None,
            "3",
            Some(V10::Real(FiniteReal::new(3.0).unwrap())),
            &[],
            &[],
        ),
        parameter(
            "bare",
            None,
            "3",
            Some(V10::Length(Length::new(3.0).unwrap())),
            &[],
            &[],
        ),
        parameter(
            "lost",
            None,
            "1 mm",
            None,
            &[],
            &[("owner_record_index", "77")],
        ),
        shared,
        parameter(
            "other",
            Some("f3d:model:feature#other:9"),
            "1 mm",
            None,
            &[],
            &[],
        ),
    ];
    let root = crate::tree::file_root_id();
    let referenced =
        ProductDefinitionId::mint(format!("f3d:{XREF}archive:product-definition#0")).unwrap();
    let scopes = BTreeMap::from([
        (String::new(), root.clone()),
        (XREF.to_owned(), referenced.clone()),
    ]);
    let features = BTreeSet::from([OWNER]);
    let mut losses = Vec::new();
    let projected =
        super::parameters::project(&legacy, &scopes, &BTreeSet::new(), &features, &mut losses);
    let [outside, unresolved] = losses.as_slice() else {
        panic!("two losses: {losses:?}")
    };
    assert_eq!(
        outside.code,
        crate::loss::F3dLossCode::ParameterOwnerOutsideTimeline.kind()
    );
    assert!(
        outside.message.ends_with(legacy[8].id.as_str()),
        "{}",
        outside.message
    );
    assert_eq!(
        unresolved.code,
        crate::loss::F3dLossCode::ParameterOwnerUnresolvedOmitted.kind()
    );
    assert!(
        unresolved.message.ends_with(legacy[6].id.as_str()),
        "{}",
        unresolved.message
    );
    let by_name = projected
        .iter()
        .map(|parameter| (parameter.name.as_ref().unwrap().as_str(), parameter))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        by_name.keys().copied().collect::<Vec<_>>(),
        ["alias", "bare", "count", "d1", "d2", "shared", "turn"]
    );
    assert_eq!(by_name["shared"].owner, None);
    assert_eq!(by_name["shared"].scope, referenced);
    let d1 = by_name["d1"];
    assert_eq!(d1.scope, root);
    assert_eq!(
        d1.owner,
        Some(cadmpeg_ir::design::parameter::ParameterOwner::Feature(
            FeatureId::mint(OWNER).unwrap()
        ))
    );
    assert_eq!(
        d1.value,
        Some(cadmpeg_ir::design::parameter::ParameterValue::Number(
            FiniteReal::new(10.0).unwrap()
        ))
    );
    assert_eq!(d1.comment.as_deref(), Some("depth"));
    assert_eq!(
        d1.source_settings,
        BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("f3d:role"),
            cadmpeg_ir::design::options::NativeOption::Text(Dimension::Literal(
                cadmpeg_core::nonblank_literal!("Linear Dimension-1")
            )),
        )])
    );
    let definition = |name: &str| {
        let cadmpeg_ir::design::parameter::ParameterDefinition::Expression(expression) =
            &by_name[name].definition
        else {
            panic!("expression")
        };
        expression.clone()
    };
    assert_eq!(definition("d1"), number(10.0, Some(UnitSymbol::Mm)));
    assert_eq!(
        definition("d2"),
        source(vec![
            ExpressionToken::Reference(ParameterRef {
                target: RefTarget::Parameter(by_name["d1"].id.clone()),
                through: None,
            }),
            ExpressionToken::Text(" * 2".to_owned()),
        ])
    );
    assert_eq!(
        definition("alias"),
        Expression::Neutral(Neutral(Node::Reference(ParameterRef {
            target: RefTarget::Parameter(by_name["d1"].id.clone()),
            through: None,
        })))
    );
    assert_eq!(by_name["turn"].owner, None);
    assert_eq!(definition("turn"), number(45.0, Some(UnitSymbol::Deg)));
    assert_eq!(definition("count"), number(3.0, None));
    assert_eq!(
        definition("bare"),
        source(vec![ExpressionToken::Text("3".to_owned())])
    );
}

#[test]
fn a_hosted_referenced_designs_user_parameters_stay_off_the_scope_that_hosts_it() {
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::Length;
    const FIRST: &str = "xref/role-1/reference-0/occurrence-0/";
    const SECOND: &str = "xref/role-1/reference-1/occurrence-0/";
    const UNLISTED: &str = "xref/role-1/reference-2/occurrence-0/";
    const OWNER: &str = "f3d:xref/role-1/reference-0/occurrence-0/model:feature#extrude:5";
    let in_document = |prefix: &str, key: &str, name: &str, owner: &str| {
        let mut parameter = parameter(
            key,
            Some(owner),
            "4 mm",
            Some(V10::Length(Length::new(4.0).unwrap())),
            &[],
            &[],
        );
        parameter.name = name.to_owned();
        parameter.native_ref = Some(format!(
            "f3d:{prefix}FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter#{key}"
        ));
        parameter
    };
    let legacy = [
        in_document(
            FIRST,
            "first_width",
            "width",
            format!("f3d:{FIRST}model:document-parameters#0").as_str(),
        ),
        in_document(
            SECOND,
            "second_width",
            "width",
            format!("f3d:{SECOND}model:document-parameters#0").as_str(),
        ),
        in_document(
            UNLISTED,
            "unlisted_height",
            "height",
            format!("f3d:{UNLISTED}model:document-parameters#0").as_str(),
        ),
        in_document(FIRST, "depth", "depth", OWNER),
        parameter(
            "length",
            None,
            "8 mm",
            Some(V10::Length(Length::new(8.0).unwrap())),
            &[],
            &[],
        ),
    ];
    let root = crate::tree::file_root_id();
    let scopes = BTreeMap::from([
        (String::new(), root.clone()),
        (FIRST.to_owned(), root.clone()),
        (SECOND.to_owned(), root.clone()),
    ]);
    let hosted = BTreeSet::from([FIRST.to_owned(), SECOND.to_owned()]);
    let mut losses = Vec::new();
    let projected = super::parameters::project(
        &legacy,
        &scopes,
        &hosted,
        &BTreeSet::from([OWNER]),
        &mut losses,
    );
    let stated = projected
        .iter()
        .map(|parameter| {
            (
                parameter.name.as_ref().unwrap().as_str(),
                parameter.scope.clone(),
                parameter.owner.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        stated,
        [
            (
                "depth",
                root.clone(),
                Some(cadmpeg_ir::design::parameter::ParameterOwner::Feature(
                    FeatureId::mint(OWNER).unwrap()
                ))
            ),
            ("length", root, None),
        ]
    );
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::ParameterReferencedDesignUnscoped.kind()
    );
    let named = loss.message.split_whitespace().collect::<BTreeSet<_>>();
    for unscoped in &legacy[..3] {
        assert!(named.contains(unscoped.id.as_str()), "{}", loss.message);
    }
}

#[test]
fn each_class_of_parameter_the_design_omits_charges_one_loss_naming_them() {
    use crate::loss::F3dLossCode;
    const OWNER: &str = "f3d:model:feature#extrude:20";
    const OUTSIDE: &str = "f3d:model:feature#unlisted:9";
    let mut blank = parameter("blank", None, "1 mm", None, &[], &[]);
    blank.name = "  ".to_owned();
    let mut loose = parameter("loose", None, "1 mm", None, &[], &[]);
    loose.native_ref = None;
    let legacy = [
        parameter("kept", Some(OWNER), "1 mm", None, &[], &[]),
        parameter("first", Some(OUTSIDE), "1 mm", None, &[], &[]),
        parameter("second", Some(OUTSIDE), "1 mm", None, &[], &[]),
        parameter(
            "unowned",
            None,
            "1 mm",
            None,
            &[],
            &[("owner_record_index", "77")],
        ),
        loose,
        blank,
    ];
    let scopes = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
    let mut losses = Vec::new();
    let projected = super::parameters::project(
        &legacy,
        &scopes,
        &BTreeSet::new(),
        &BTreeSet::from([OWNER]),
        &mut losses,
    );
    let [kept] = projected.as_slice() else {
        panic!("one parameter: {projected:?}")
    };
    assert_eq!(kept.id, legacy[0].id);
    let expected = [
        (F3dLossCode::ParameterOwnerOutsideTimeline, &legacy[1..3]),
        (F3dLossCode::ParameterOwnerUnresolvedOmitted, &legacy[3..4]),
        (
            F3dLossCode::ParameterReferencedDesignUnscoped,
            &legacy[4..5],
        ),
        (F3dLossCode::ParameterNameBlank, &legacy[5..6]),
    ];
    assert_eq!(losses.len(), expected.len(), "{losses:?}");
    for (loss, (code, omitted)) in losses.iter().zip(expected) {
        assert_eq!(loss.code, code.kind());
        let words = loss.message.split_whitespace().collect::<Vec<_>>();
        assert_eq!(words[0], omitted.len().to_string(), "{}", loss.message);
        for parameter in omitted {
            assert!(words.contains(&parameter.id.as_str()), "{}", loss.message);
        }
    }
}

#[test]
fn the_active_row_is_the_base_and_other_rows_state_their_cells() {
    use cadmpeg_ir::features::parameters::{
        ConfigurationEvaluation, ConfigurationFeatureState,
        DesignConfiguration as LegacyConfiguration, ParameterValue as V10,
    };
    use cadmpeg_ir::features::ConfigurationId;
    use cadmpeg_ir::scalar::Length;
    const OWNER: &str = "f3d:model:feature#extrude:20";
    let root = crate::tree::file_root_id();
    let scopes = BTreeMap::from([(String::new(), root.clone())]);
    let features = BTreeSet::from([OWNER]);
    let parameters = super::parameters::project(
        &[parameter(
            "d1",
            Some(OWNER),
            "10 mm",
            Some(V10::Length(Length::new(10.0).unwrap())),
            &[],
            &[],
        )],
        &scopes,
        &BTreeSet::new(),
        &features,
        &mut Vec::new(),
    );
    let d1 = parameters[0].id.clone();
    let row = |key: &str, active: bool| LegacyConfiguration {
        id: ConfigurationId::mint(format!("f3d:configuration:variant#{key}")).unwrap(),
        ordinal: 0,
        active,
        source_index: None,
        name: Some(key.to_owned()),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::from([(d1.clone(), "12 mm".to_owned())]),
        bodies: None,
        parameter_values: BTreeMap::from([(d1.clone(), V10::Length(Length::new(12.0).unwrap()))]),
        feature_states: BTreeMap::from([(
            FeatureId::mint(OWNER).unwrap(),
            ConfigurationFeatureState {
                evaluation: ConfigurationEvaluation::Suppressed {},
                dependencies: DistinctMembers::default(),
                definition: LegacyDefinition::Operation(LegacyOperation::Unresolved {
                    family: UnresolvedFamily::Extrude,
                }),
            },
        )]),
        native_ref: Some("f3d:configuration:entry#FusionAssetName[Active]/table".to_owned()),
    };
    let mut losses = Vec::new();
    let projected = super::configurations::project(
        &[row("wide", true), row("narrow", false)],
        &scopes,
        &BTreeSet::new(),
        &features,
        &parameters,
        &mut losses,
    );
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(
        projected.active[&root].as_str(),
        "f3d:configuration:variant#wide"
    );
    let [wide, narrow] = projected.records.as_slice() else {
        panic!("two configurations")
    };
    assert_eq!(wide.name, "wide");
    assert_eq!(
        wide.overrides,
        cadmpeg_ir::design::configuration::Overrides::default()
    );
    assert_eq!(narrow.scope, root);
    let parameter = &narrow.overrides.parameters[&d1];
    assert_eq!(
        parameter.expression,
        Some(number(12.0, Some(UnitSymbol::Mm)))
    );
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::design::parameter::ParameterValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(12.0).unwrap()
        ))
    );
    assert_eq!(
        narrow.overrides.features[&FeatureId::mint(OWNER).unwrap()].suppressed,
        Some(true)
    );
}

#[test]
fn rows_and_cells_the_design_omits_are_named_by_one_loss_per_class() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::features::parameters::{
        ConfigurationEvaluation, ConfigurationFeatureState,
        DesignConfiguration as LegacyConfiguration,
    };
    use cadmpeg_ir::features::{ConfigurationId, ParameterId};
    const OWNER: &str = "f3d:model:feature#extrude:20";
    const UNLISTED: &str = "f3d:model:feature#unlisted:9";
    const HOSTED: &str = "xref/role-1/reference-0/occurrence-0/";
    const UNKEYED: &str = "xref/role-1/reference-1/occurrence-0/";
    const INACTIVE: &str = "xref/role-1/reference-2/occurrence-0/";
    let root = crate::tree::file_root_id();
    let inactive_scope =
        ProductDefinitionId::mint(format!("f3d:{INACTIVE}archive:product-definition#0")).unwrap();
    let scopes = BTreeMap::from([
        (String::new(), root.clone()),
        (HOSTED.to_owned(), root.clone()),
        (INACTIVE.to_owned(), inactive_scope),
    ]);
    let hosted = BTreeSet::from([HOSTED.to_owned()]);
    let features = BTreeSet::from([OWNER]);
    let parameters = super::parameters::project(
        &[parameter("d1", Some(OWNER), "10 mm", None, &[], &[])],
        &scopes,
        &hosted,
        &features,
        &mut Vec::new(),
    );
    let d1 = parameters[0].id.clone();
    let ghost = ParameterId::mint("f3d:model:parameter#ghost").unwrap();
    let suppressed = |id: &str| {
        (
            FeatureId::mint(id).unwrap(),
            ConfigurationFeatureState {
                evaluation: ConfigurationEvaluation::Suppressed {},
                dependencies: DistinctMembers::default(),
                definition: LegacyDefinition::Operation(LegacyOperation::Unresolved {
                    family: UnresolvedFamily::Extrude,
                }),
            },
        )
    };
    let row = |prefix: &str, key: &str, active: bool| LegacyConfiguration {
        id: ConfigurationId::mint(format!("f3d:{prefix}configuration:variant#{key}")).unwrap(),
        ordinal: 0,
        active,
        source_index: None,
        name: Some(key.to_owned()),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::from([
            (d1.clone(), "12 mm".to_owned()),
            (ghost.clone(), "3 mm".to_owned()),
        ]),
        bodies: None,
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::from([suppressed(OWNER), suppressed(UNLISTED)]),
        native_ref: Some(format!("f3d:{prefix}configuration:entry#table")),
    };
    let legacy = [
        row("", "base", true),
        row("", "variant", false),
        row(HOSTED, "hosted_base", true),
        row(HOSTED, "hosted_variant", false),
        row(UNKEYED, "unkeyed", true),
        row(INACTIVE, "orphan", false),
    ];
    let mut losses = Vec::new();
    let projected = super::configurations::project(
        &legacy,
        &scopes,
        &hosted,
        &features,
        &parameters,
        &mut losses,
    );
    assert_eq!(
        projected.active,
        BTreeMap::from([(root, legacy[0].id.clone())])
    );
    let [base, variant] = projected.records.as_slice() else {
        panic!("two configurations: {:?}", projected.records)
    };
    assert_eq!((&base.id, &variant.id), (&legacy[0].id, &legacy[1].id));
    assert_eq!(
        variant.overrides.parameters.keys().collect::<Vec<_>>(),
        [&d1]
    );
    assert_eq!(
        variant.overrides.features.keys().collect::<Vec<_>>(),
        [&FeatureId::mint(OWNER).unwrap()]
    );
    let [unscoped, inactive, unstated, uncached] = losses.as_slice() else {
        panic!("four losses: {losses:?}")
    };
    assert_eq!(
        unscoped.code,
        F3dLossCode::ConfigurationReferencedDesignUnscoped.kind()
    );
    let named = unscoped.message.split_whitespace().collect::<Vec<_>>();
    assert_eq!(named[0], "3", "{}", unscoped.message);
    for row in &legacy[2..5] {
        assert!(named.contains(&row.id.as_str()), "{}", unscoped.message);
    }
    assert_eq!(
        inactive.code,
        F3dLossCode::ConfigurationScopeWithoutActiveRow.kind()
    );
    assert!(
        inactive.message.ends_with(legacy[5].id.as_str()),
        "{}",
        inactive.message
    );
    assert_eq!(
        unstated.code,
        F3dLossCode::ConfigurationOverrideUnstated.kind()
    );
    assert!(unstated.message.starts_with("2 "), "{}", unstated.message);
    for cell in [ghost.as_str(), UNLISTED] {
        assert!(
            unstated
                .message
                .contains(&format!("{cell} in {}", legacy[1].id)),
            "{}",
            unstated.message
        );
    }
    assert_eq!(variant.overrides.parameters[&d1].value, None);
    assert_eq!(
        uncached.code,
        F3dLossCode::ConfigurationOverrideValueUnprojected.kind()
    );
    assert!(
        uncached
            .message
            .ends_with(&format!(": {d1} in {}", legacy[1].id)),
        "{}",
        uncached.message
    );
}

#[test]
fn configuration_properties_are_named_by_a_loss() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::features::parameters::DesignConfiguration as LegacyConfiguration;
    use cadmpeg_ir::features::ConfigurationId;
    let root = crate::tree::file_root_id();
    let scopes = BTreeMap::from([(String::new(), root)]);
    let row = |key: &str, active: bool, properties: &[(&str, &str)]| LegacyConfiguration {
        id: ConfigurationId::mint(format!("f3d:configuration:variant#{key}")).unwrap(),
        ordinal: 0,
        active,
        source_index: None,
        name: Some(key.to_owned()),
        material: None,
        properties: properties
            .iter()
            .map(|(key, value)| {
                (
                    cadmpeg_core::text::NonBlankString::new((*key).to_owned()).unwrap(),
                    (*value).to_owned(),
                )
            })
            .collect(),
        parameter_overrides: BTreeMap::new(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some("f3d:configuration:entry#table".to_owned()),
    };
    let legacy = [
        row("wide", true, &[("property:Part Number", "W-1")]),
        row(
            "narrow",
            false,
            &[
                ("property:Part Number", "N-1"),
                ("suppressed:column-3", "true"),
                ("parameter:column-4", "4 mm"),
            ],
        ),
        row("plain", false, &[]),
    ];
    let mut losses = Vec::new();
    let projected = super::configurations::project(
        &legacy,
        &scopes,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &[],
        &mut losses,
    );
    assert_eq!(projected.records.len(), 3);
    let [unstated, properties] = losses.as_slice() else {
        panic!("two losses: {losses:?}")
    };
    assert_eq!(
        unstated.code,
        F3dLossCode::ConfigurationOverrideUnstated.kind()
    );
    assert!(unstated.message.starts_with("2 "), "{}", unstated.message);
    for key in ["suppressed:column-3", "parameter:column-4"] {
        assert!(
            unstated
                .message
                .contains(&format!("{key} in {}", legacy[1].id)),
            "{}",
            unstated.message
        );
    }
    assert_eq!(
        properties.code,
        F3dLossCode::ConfigurationPropertyUnprojected.kind()
    );
    assert!(
        properties
            .message
            .ends_with(&format!(": property:Part Number in {}", legacy[0].id)),
        "{}",
        properties.message
    );
    assert_eq!(
        projected.records[1].overrides.definitions[&crate::tree::file_root_id()].part_number,
        Some(cadmpeg_ir::design::configuration::PartNumber::Text(
            "N-1".to_owned()
        ))
    );
}

#[test]
fn part_number_and_description_cells_of_other_rows_override_the_scope_definition() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::design::configuration::{DefinitionOverride, PartNumber};
    use cadmpeg_ir::features::parameters::DesignConfiguration as LegacyConfiguration;
    use cadmpeg_ir::features::ConfigurationId;
    let root = crate::tree::file_root_id();
    let scopes = BTreeMap::from([(String::new(), root.clone())]);
    let row = |key: &str, active: bool, properties: &[(&str, &str)]| LegacyConfiguration {
        id: ConfigurationId::mint(format!("f3d:configuration:variant#{key}")).unwrap(),
        ordinal: 0,
        active,
        source_index: None,
        name: Some(key.to_owned()),
        material: None,
        properties: properties
            .iter()
            .map(|(key, value)| {
                (
                    cadmpeg_core::text::NonBlankString::new((*key).to_owned()).unwrap(),
                    (*value).to_owned(),
                )
            })
            .collect(),
        parameter_overrides: BTreeMap::new(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: Some("f3d:configuration:entry#table".to_owned()),
    };
    let legacy = [
        row(
            "base",
            true,
            &[
                ("property:Part Number", "B-1"),
                ("property:Description", "Base"),
            ],
        ),
        row(
            "typed",
            false,
            &[
                ("property:Part Number", "W-1"),
                ("property:Description", "Wide plate"),
                ("property:Finish", "Anodized"),
            ],
        ),
        row(
            "blank",
            false,
            &[("property:Part Number", ""), ("property:Description", "")],
        ),
    ];
    let mut losses = Vec::new();
    let projected = super::configurations::project(
        &legacy,
        &scopes,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &[],
        &mut losses,
    );
    let [base, typed, blank] = projected.records.as_slice() else {
        panic!("three configurations: {:?}", projected.records)
    };
    assert!(base.overrides.definitions.is_empty());
    assert_eq!(
        typed.overrides.definitions,
        BTreeMap::from([(
            root.clone(),
            DefinitionOverride {
                description: Some("Wide plate".to_owned()),
                part_number: Some(PartNumber::Text("W-1".to_owned())),
                ..DefinitionOverride::default()
            }
        )])
    );
    assert_eq!(
        blank.overrides.definitions,
        BTreeMap::from([(
            root,
            DefinitionOverride {
                description: Some(String::new()),
                part_number: Some(PartNumber::ConfigurationName),
                ..DefinitionOverride::default()
            }
        )])
    );
    let [properties] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(
        properties.code,
        F3dLossCode::ConfigurationPropertyUnprojected.kind()
    );
    assert!(
        properties.message.starts_with("3 "),
        "{}",
        properties.message
    );
    for named in [
        format!("property:Part Number in {}", legacy[0].id),
        format!("property:Description in {}", legacy[0].id),
        format!("property:Finish in {}", legacy[1].id),
    ] {
        assert!(
            properties.message.contains(&named),
            "{}",
            properties.message
        );
    }
}
