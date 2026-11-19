// SPDX-License-Identifier: Apache-2.0
//! Tests of the v11 design of a Fusion document.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::{Dimension, ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{Expression, ExpressionDialect, ExpressionToken, Neutral, Node};
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::units::UnitSymbol;
use cadmpeg_ir::features::{
    DistinctMembers, Feature as LegacyFeature, FeatureContent,
    FeatureDefinition as LegacyDefinition, FeatureEvaluation, FeatureId,
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
    let documents = crate::test_support::with_decode_context(|ctx| {
        super::timeline::project(ctx, &features, &timelines, &types, &BTreeSet::new())
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
        super::timeline::project(ctx, &[], &[timeline(STREAM, 0, &[])], &[], &BTreeSet::new())
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
                bodies: cadmpeg_ir::features::BodySelection::Bodies(bodies.into_iter().collect()),
            },
        )
    };
    let project = |feature: LegacyFeature, current: &[&str]| {
        let current = current.iter().copied().collect::<BTreeSet<_>>();
        let timelines = [timeline(STREAM, 0, &[10])];
        crate::test_support::with_decode_context(|ctx| {
            super::timeline::project(ctx, &[feature], &timelines, &[], &current)
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
    value: Option<cadmpeg_ir::features::ParameterValue>,
    dependencies: &[&str],
    properties: &[(&str, &str)],
) -> cadmpeg_ir::features::DesignParameter {
    cadmpeg_ir::features::DesignParameter {
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
    use cadmpeg_ir::features::ParameterValue as V10;
    use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};
    const OWNER: &str = "f3d:model:feature#extrude:20";
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
        parameter(
            "shared",
            Some("f3d:xref/role-1/reference-0/occurrence-0/model:document-parameters#0"),
            "2 mm",
            Some(V10::Length(Length::new(2.0).unwrap())),
            &[],
            &[],
        ),
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
    let scopes = BTreeMap::from([(String::new(), root.clone())]);
    let features = BTreeSet::from([OWNER]);
    let projected = super::parameters::project(&legacy, &scopes, &features);
    let by_name = projected
        .iter()
        .map(|parameter| (parameter.name.as_ref().unwrap().as_str(), parameter))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        by_name.keys().copied().collect::<Vec<_>>(),
        ["alias", "bare", "count", "d1", "d2", "shared", "turn"]
    );
    assert_eq!(by_name["shared"].owner, None);
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
fn the_active_row_is_the_base_and_other_rows_state_their_cells() {
    use cadmpeg_ir::features::{
        ConfigurationEvaluation, ConfigurationFeatureState, ConfigurationId,
        DesignConfiguration as LegacyConfiguration, ParameterValue as V10,
    };
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
        &features,
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
    let projected = super::configurations::project(
        &[row("wide", true), row("narrow", false)],
        &scopes,
        &features,
        &parameters,
    );
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
