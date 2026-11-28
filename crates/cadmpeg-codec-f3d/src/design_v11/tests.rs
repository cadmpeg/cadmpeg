// SPDX-License-Identifier: Apache-2.0
//! Tests of the v11 design of a Fusion document.
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::design::container::{Container, OrderedKind, Rollback};
use cadmpeg_ir::design::dimension::{Dimension, ParameterRef, RefTarget};
use cadmpeg_ir::design::expr::{Expression, ExpressionDialect, ExpressionToken, Neutral, Node};
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
use cadmpeg_ir::design::ids::ContainerId;
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

mod history_states;

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

fn tail_flags(suppression: [bool; 2], no_result: bool, rolled_back: bool) -> DesignScopeTailFlags {
    DesignScopeTailFlags {
        suppression,
        unevaluated: false,
        wide_state: None,
        state_offset: 0,
        no_result,
        rolled_back,
        no_result_offset: 3,
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
        .map(|&(record, rolled_back)| {
            ((STREAM, record), tail_flags([false; 2], false, rolled_back))
        })
        .collect();
    let mut losses = Vec::new();
    let mut documents = crate::test_support::with_decode_context(|ctx| {
        super::timeline::project(
            ctx,
            &[],
            &[timeline(STREAM, 0, items)],
            &[],
            &super::timeline::ScopeInputs {
                records: &[],
                tail_flags: &flags,
            },
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
        rollback.expect("the flags state the rollback"),
        members,
        losses.into_iter().map(|loss| loss.code).collect(),
    )
}

#[test]
fn a_referenced_design_is_a_document_of_its_own() {
    const XREF: &str = "role-1/reference-0/occurrence-0@";
    assert_eq!(super::timeline::document_prefix(STREAM), "");
    let nested = format!("f3d:{XREF}FusionAssetName[Active]/Design1/BulkStream.dat");
    assert_eq!(super::timeline::document_prefix(&nested), XREF);
    let deeper =
        "f3d:role-1/reference-0/occurrence-0/xref/role-2/reference-1/occurrence-3@Design1/BulkStream.dat";
    assert_eq!(
        super::timeline::document_prefix(deeper),
        "role-1/reference-0/occurrence-0/xref/role-2/reference-1/occurrence-3@"
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
    const XREF: &str = "role-1/reference-0/occurrence-0@";
    let root = crate::tree::file_root_id();
    let member = "f3d:xref:product-definition#role-1/reference-0/occurrence-0".to_owned();
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
    let spans = super::span(&definitions, &super::Placers::new(&definitions), &scopes);
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

/// A scope over the file-stored definition `scope`, holding one feature per
/// `(key, owner)`, each naming its owner as its component.
fn component_document(
    scope: &str,
    definitions: Vec<ProductDefinition>,
    features: &[(&str, &str)],
) -> (
    cadmpeg_ir::CadIr,
    BTreeSet<FeatureId>,
    HashMap<FeatureId, ProductDefinitionId>,
) {
    use cadmpeg_ir::design::feature::{Feature, FeatureDefinition};
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.product_definitions = definitions;
    ir.design
        .scopes
        .push(cadmpeg_ir::design::scope::DesignScope {
            definition: ProductDefinitionId::mint(scope).unwrap(),
            roots: vec![ContainerId::mint("f3d:design:container#timeline").unwrap()],
            active_configuration: None,
            active_arrangements: BTreeSet::new(),
            occurrences: BTreeMap::new(),
            current: Vec::new(),
            unproduced: BTreeSet::default(),
        });
    let mut ids = BTreeSet::new();
    let mut owners = HashMap::new();
    let mut members = Vec::new();
    for (key, owner) in features {
        let id = FeatureId::mint(format!("f3d:design:feature#{key}")).unwrap();
        let owner = ProductDefinitionId::mint(*owner).unwrap();
        members.push(Member::Feature(id.clone()));
        ir.design.features.push(Feature {
            id: id.clone(),
            name: None,
            suppressed: None,
            definition: FeatureDefinition {
                operation: FeatureOperation::Unresolved {
                    family: OperationFamily::Extrude,
                },
                assembly: None,
                refine: None,
                fuzzy_tolerance: None,
            },
            placement: None,
            component: Some(owner.clone()),
            driven_by: None,
            native_ref: None,
        });
        ids.insert(id.clone());
        owners.insert(id, owner);
    }
    ir.design.containers.push(Container::Ordered {
        id: ContainerId::mint("f3d:design:container#timeline").unwrap(),
        kind: OrderedKind::Timeline,
        members,
        folders: Vec::new(),
        rollback: None,
        placement: None,
    });
    (ir, ids, owners)
}

fn file_definition(id: &str) -> ProductDefinition {
    ProductDefinition::new(
        ProductDefinitionId::mint(id).unwrap(),
        Storage::File {
            document: ExternalDocument::path("part.f3d"),
            source: Some(cadmpeg_ir::SourceMeta::classified(
                cadmpeg_core::dialect::DialectLayers::of(
                    cadmpeg_core::dialect::DialectMatch::admitted(
                        cadmpeg_core::dialect::DialectId::parse("f3d:manifest-3-2-0-0").unwrap(),
                    ),
                ),
                BTreeMap::new(),
            )),
        },
    )
}

#[test]
fn projected_components_are_settled_before_operation_projection() {
    let scope = "f3d:archive:product-definition#0";
    let owner = "f3d:model:component#part";
    let (mut ir, _, owners) = component_document(
        scope,
        vec![
            file_definition(scope),
            definition(owner, Storage::Embedded, &[]),
        ],
        &[("feature", owner)],
    );
    let occurrence = cadmpeg_ir::ids::OccurrenceId::mint("f3d:model:occurrence#part").unwrap();
    ir.model.product_definitions[0]
        .occurrences
        .push(occurrence.clone());
    ir.model
        .occurrences
        .push(cadmpeg_ir::products::Occurrence::new(
            occurrence,
            ProductDefinitionId::mint(owner).unwrap(),
            None,
        ));
    let mut projected = crate::design_v11::timeline::Projected::default();
    projected.features = std::mem::take(&mut ir.design.features);
    projected.containers = std::mem::take(&mut ir.design.containers);
    for feature in &mut projected.features {
        feature.component = None;
    }
    ir.design.scopes.clear();
    let mut scopes = BTreeMap::from([(
        String::new(),
        (ProductDefinitionId::mint(scope).unwrap(), projected),
    )]);
    super::settle_projected_components(
        &mut ir,
        &mut scopes,
        &owners,
        &BTreeSet::new(),
        &mut Vec::new(),
    );
    assert!(ir.design.scopes.is_empty());
    assert!(ir.design.features.is_empty());
    assert!(ir.design.containers.is_empty());
    assert_eq!(scopes[""].1.features.len(), 1);
    assert_eq!(scopes[""].1.containers.len(), 1);
    assert_eq!(
        scopes[""].1.features[0]
            .component
            .as_ref()
            .map(ProductDefinitionId::as_str),
        Some(owner)
    );
}

#[test]
fn a_feature_names_a_component_the_scope_reaches_through_an_occurrence() {
    const XREF: &str = "f3d:xref/role-1/reference-0/occurrence-0/";
    let scope = format!("{XREF}archive:product-definition#0");
    let placed = format!("{XREF}model:component#placed");
    let unplaced = format!("{XREF}model:component#unplaced");
    let mut member = file_definition(&scope);
    member.occurrences =
        vec![cadmpeg_ir::ids::OccurrenceId::mint(format!("{XREF}model:occurrence#0")).unwrap()];
    let (mut ir, ids, owners) = component_document(
        &scope,
        vec![
            member,
            definition(&placed, Storage::Embedded, &[]),
            definition(&unplaced, Storage::Embedded, &[]),
        ],
        &[("a", &scope), ("b", &placed), ("c", &unplaced)],
    );
    ir.model
        .occurrences
        .push(cadmpeg_ir::products::Occurrence::new(
            cadmpeg_ir::ids::OccurrenceId::mint(format!("{XREF}model:occurrence#0")).unwrap(),
            ProductDefinitionId::mint(&placed).unwrap(),
            None,
        ));
    super::settle_components(
        &mut ir,
        &[(ProductDefinitionId::mint(&scope).unwrap(), ids)],
        &owners,
        &BTreeSet::new(),
        &mut Vec::new(),
    );
    let components = ir
        .design
        .features
        .iter()
        .map(|feature| feature.component.as_ref().map(ProductDefinitionId::as_str))
        .collect::<Vec<_>>();
    assert_eq!(
        components,
        [Some(scope.as_str()), Some(placed.as_str()), None]
    );
}

#[test]
fn uncertain_item_kinds_in_a_spanned_component_keep_none_with_one_loss() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let scope = "f3d:xref/role-1/reference-0/occurrence-0/archive:product-definition#0";
    let owner = "f3d:xref/role-1/reference-0/occurrence-0/model:component#embedded";
    let occurrence = cadmpeg_ir::ids::OccurrenceId::mint("f3d:model:occurrence#embedded").unwrap();
    let mut member = file_definition(scope);
    member.occurrences.push(occurrence.clone());
    let (mut ir, ids, owners) = component_document(
        scope,
        vec![member, definition(owner, Storage::Embedded, &[])],
        &[
            ("base", owner),
            ("cut", owner),
            ("insert", owner),
            ("extrude", owner),
        ],
    );
    ir.model
        .occurrences
        .push(cadmpeg_ir::products::Occurrence::new(
            occurrence,
            ProductDefinitionId::mint(owner).unwrap(),
            None,
        ));
    let records = ["Base Feature", "CutPaste", "Component Insert", "Extrude"]
        .into_iter()
        .enumerate()
        .map(|(index, kind)| {
            let id = format!("{STREAM}:design-parameter-scope#{}", 100 + index);
            ir.design.features[index].native_ref = Some(id.clone());
            DesignParameterScope::empty(
                &id,
                DesignFeatureKind::try_from(kind.to_owned()).unwrap(),
                100 + u32::try_from(index).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let mut losses = Vec::new();
    super::settle_components(
        &mut ir,
        &[(ProductDefinitionId::mint(scope).unwrap(), ids)],
        &owners,
        &super::unstated_components(&records),
        &mut losses,
    );
    assert!(ir.design.features[..3]
        .iter()
        .all(|feature| feature.component.is_none()));
    assert_eq!(
        ir.design.features[3]
            .component
            .as_ref()
            .map(ProductDefinitionId::as_str),
        Some(owner)
    );
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::HistoryMemberComponentUnstated.kind()
    );
    assert!(loss.message.contains(owner));
    for key in ["base", "cut", "insert"] {
        assert!(loss.message.contains(&format!("f3d:design:feature#{key}")));
    }
}

#[test]
fn a_component_named_in_one_round_widens_the_span_of_the_next() {
    const XREF: &str = "f3d:xref/role-1/reference-0/occurrence-0/";
    let scope = format!("{XREF}archive:product-definition#0");
    let embedded = format!("{XREF}model:component#embedded");
    let child = format!("{XREF}archive:product-definition#1");
    let occurrence = |key: &str| {
        cadmpeg_ir::ids::OccurrenceId::mint(format!("{XREF}model:occurrence#{key}")).unwrap()
    };
    let mut member = file_definition(&scope);
    member.occurrences = vec![occurrence("embedded"), occurrence("child")];
    let (mut ir, ids, owners) = component_document(
        &scope,
        vec![
            member,
            definition(&embedded, Storage::Embedded, &[]),
            file_definition(&child),
        ],
        &[("a", &scope), ("b", &embedded), ("c", &child)],
    );
    for (key, prototype) in [("embedded", &embedded), ("child", &child)] {
        ir.model
            .occurrences
            .push(cadmpeg_ir::products::Occurrence::new(
                occurrence(key),
                ProductDefinitionId::mint(prototype).unwrap(),
                None,
            ));
    }
    for feature in &mut ir.design.features {
        feature.component = None;
    }
    super::settle_components(
        &mut ir,
        &[(ProductDefinitionId::mint(&scope).unwrap(), ids)],
        &owners,
        &BTreeSet::new(),
        &mut Vec::new(),
    );
    let components = ir
        .design
        .features
        .iter()
        .map(|feature| feature.component.as_ref().map(ProductDefinitionId::as_str))
        .collect::<Vec<_>>();
    assert_eq!(
        components,
        [
            Some(scope.as_str()),
            Some(embedded.as_str()),
            Some(child.as_str())
        ]
    );
}

#[test]
fn a_hosted_owner_outside_the_span_charges_one_loss_for_its_features() {
    const XREF: &str = "f3d:xref/role-1/reference-0/occurrence-0/";
    let scope = format!("{XREF}archive:product-definition#0");
    let unplaced = "f3d:hosted@model:component#unplaced".to_owned();
    let (mut ir, ids, owners) = component_document(
        &scope,
        vec![
            file_definition(&scope),
            definition(&unplaced, Storage::Embedded, &[]),
        ],
        &[("a", &scope), ("b", &unplaced), ("c", &unplaced)],
    );
    let mut losses = Vec::new();
    super::settle_components(
        &mut ir,
        &[(ProductDefinitionId::mint(&scope).unwrap(), ids)],
        &owners,
        &BTreeSet::new(),
        &mut losses,
    );
    assert!(ir
        .design
        .features
        .iter()
        .all(|feature| feature.component.is_none()));
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}")
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::HistoryMemberComponentUnstated.kind()
    );
    assert!(loss.message.contains(&unplaced));
    assert!(loss.message.contains("f3d:design:feature#b"));
    assert!(loss.message.contains("f3d:design:feature#c"));
}

#[test]
fn the_scope_owner_needs_no_component_loss() {
    let scope = "f3d:archive:product-definition#0";
    let (mut ir, ids, owners) = component_document(
        scope,
        vec![file_definition(scope)],
        &[("a", scope), ("b", scope)],
    );
    let mut losses = Vec::new();
    super::settle_components(
        &mut ir,
        &[(ProductDefinitionId::mint(scope).unwrap(), ids)],
        &owners,
        &BTreeSet::new(),
        &mut losses,
    );
    assert!(ir
        .design
        .features
        .iter()
        .all(|feature| feature.component.is_none()));
    assert!(losses.is_empty());
}

#[test]
fn the_root_scope_is_the_root_component_where_its_record_stands_for_the_root() {
    let root = "f3d:model:component#root";
    let definitions = [
        definition(
            root,
            ProductDefinition::root(crate::tree::file_root_id(), ExternalDocument::default())
                .storage,
            &["f3d:brep:entity#1"],
        ),
        definition(
            "f3d:model:component#a",
            Storage::Embedded,
            &["f3d:brep:entity#2"],
        ),
    ];
    let scope = super::scope_definition(&definitions, "").unwrap();
    assert_eq!(scope.as_str(), root);
    let scopes = BTreeMap::from([(String::new(), (scope, ()))]);
    let span = super::span(&definitions, &super::Placers::new(&definitions), &scopes)
        .remove("")
        .unwrap();
    assert_eq!(
        span.bodies.iter().map(BodyId::as_str).collect::<Vec<_>>(),
        ["f3d:brep:entity#1", "f3d:brep:entity#2"]
    );
    let stated = [definition(
        crate::tree::file_root_id().as_str(),
        ProductDefinition::root(crate::tree::file_root_id(), ExternalDocument::default()).storage,
        &[],
    )];
    assert_eq!(
        super::scope_definition(&stated, ""),
        Some(crate::tree::file_root_id())
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
                &super::timeline::ScopeInputs {
                    records: &[],
                    tail_flags: &super::timeline::TailFlags::new(),
                },
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
    const XREF: &str = "role-1/reference-0/occurrence-0@";
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
        ProductDefinitionId::mint("f3d:xref:product-definition#role-1/reference-0/occurrence-0")
            .unwrap();
    let scopes = BTreeMap::from([
        (String::new(), root.clone()),
        (XREF.to_owned(), referenced.clone()),
    ]);
    let features = BTreeSet::from([OWNER]);
    let mut losses = Vec::new();
    let projected = super::parameters::project(
        &legacy,
        &HashMap::new(),
        &scopes,
        &BTreeSet::new(),
        &features,
        &mut losses,
    );
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
            cadmpeg_ir::codec_name!("f3d:role"),
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

/// The parameter of a driven (reference) dimension is `Driven` (§5.12, D11).
/// Its value is the measured value its frame stores, not the value of the
/// literal its record stores, and the source setting `f3d:literal` keeps
/// that literal; with no measured value it has no value. A driving dimension's
/// parameter keeps its literal expression.
#[test]
fn the_parameter_of_a_driven_dimension_is_driven() {
    use cadmpeg_ir::design::dimension::Dimension;
    use cadmpeg_ir::design::options::NativeOption;
    use cadmpeg_ir::design::parameter::{ParameterDefinition, ParameterValue};
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::{FiniteReal, Length};
    const OWNER: &str = "f3d:model:feature#sketch:20";
    let legacy = [
        parameter(
            "reference",
            Some(OWNER),
            "12.5 mm",
            Some(V10::Length(Length::new(12.5).unwrap())),
            &[],
            &[
                ("source_kind", "Linear Dimension-2"),
                ("driven", "true"),
                ("measured_value", "12.25"),
            ],
        ),
        parameter(
            "unmeasured",
            Some(OWNER),
            "3 mm",
            Some(V10::Length(Length::new(3.0).unwrap())),
            &[],
            &[("source_kind", "Linear Dimension-4"), ("driven", "true")],
        ),
        parameter(
            "driving",
            Some(OWNER),
            "4 mm",
            Some(V10::Length(Length::new(4.0).unwrap())),
            &[],
            &[("source_kind", "Linear Dimension-3")],
        ),
    ];
    let scopes = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
    let mut losses = Vec::new();
    let projected = super::parameters::project(
        &legacy,
        &HashMap::new(),
        &scopes,
        &BTreeSet::new(),
        &BTreeSet::from([OWNER]),
        &mut losses,
    );
    assert!(losses.is_empty(), "{losses:?}");
    let [reference, unmeasured, driving] = projected.as_slice() else {
        panic!("three parameters: {projected:?}")
    };
    let literal = |text: &str| {
        Some(NativeOption::Text(Dimension::Literal(
            cadmpeg_core::text::NonBlankString::new(text.to_owned()).unwrap(),
        )))
    };
    assert!(matches!(reference.definition, ParameterDefinition::Driven));
    assert_eq!(
        reference.value,
        Some(ParameterValue::Number(FiniteReal::new(12.25).unwrap()))
    );
    assert_eq!(
        reference
            .source_settings
            .get(super::parameters::LITERAL)
            .cloned(),
        literal("12.5 mm")
    );
    assert!(matches!(unmeasured.definition, ParameterDefinition::Driven));
    assert_eq!(unmeasured.value, None);
    assert_eq!(
        unmeasured
            .source_settings
            .get(super::parameters::LITERAL)
            .cloned(),
        literal("3 mm")
    );
    assert!(matches!(
        &driving.definition,
        ParameterDefinition::Expression(expression)
            if expression == &number(4.0, Some(UnitSymbol::Mm))
    ));
    assert_eq!(
        driving.value,
        Some(ParameterValue::Number(FiniteReal::new(4.0).unwrap()))
    );
    assert_eq!(
        driving.source_settings.get(super::parameters::LITERAL),
        None
    );
    assert!(reference.native_options.is_empty());
}

#[test]
fn a_hosted_referenced_designs_user_parameters_stay_off_the_scope_that_hosts_it() {
    use cadmpeg_ir::features::parameters::ParameterValue as V10;
    use cadmpeg_ir::scalar::Length;
    const FIRST: &str = "role-1/reference-0/occurrence-0@";
    const SECOND: &str = "role-1/reference-1/occurrence-0@";
    const UNLISTED: &str = "role-1/reference-2/occurrence-0@";
    const OWNER: &str = "f3d:role-1/reference-0/occurrence-0@model:feature#extrude:5";
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
        &HashMap::new(),
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
        &HashMap::new(),
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
        &HashMap::new(),
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
        &[
            row("wide", true),
            LegacyConfiguration {
                ordinal: 1,
                ..row("narrow", false)
            },
        ],
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
    assert_eq!((wide.position, narrow.position), (Some(0), Some(1)));
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
    const HOSTED: &str = "role-1/reference-0/occurrence-0@";
    const UNKEYED: &str = "role-1/reference-1/occurrence-0@";
    const INACTIVE: &str = "role-1/reference-2/occurrence-0@";
    let root = crate::tree::file_root_id();
    let inactive_scope =
        ProductDefinitionId::mint("f3d:xref:product-definition#role-1/reference-2/occurrence-0")
            .unwrap();
    let scopes = BTreeMap::from([
        (String::new(), root.clone()),
        (HOSTED.to_owned(), root.clone()),
        (INACTIVE.to_owned(), inactive_scope),
    ]);
    let hosted = BTreeSet::from([HOSTED.to_owned()]);
    let features = BTreeSet::from([OWNER]);
    let parameters = super::parameters::project(
        &[parameter("d1", Some(OWNER), "10 mm", None, &[], &[])],
        &HashMap::new(),
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

/// A feature's parameter takes the local ordinal of its owner record, where
/// the feature's owner records state distinct ordinals of one parameter
/// scope.
#[test]
fn an_owned_parameter_is_placed_by_its_owner_record() {
    use super::parameters::OwnerPlace;
    const OWNER: &str = "f3d:model:feature#extrude:20";
    const SHARED: &str = "f3d:model:feature#fillet:30";
    let legacy = [
        parameter("d1", Some(OWNER), "10 mm", None, &[], &[]),
        parameter("d2", Some(OWNER), "20 mm", None, &[], &[]),
        parameter("r1", Some(SHARED), "1 mm", None, &[], &[]),
        parameter("r2", Some(SHARED), "2 mm", None, &[], &[]),
    ];
    let place = |key: &str, ordinal: u32| {
        (
            format!("{STREAM}:design-parameter#{key}"),
            OwnerPlace {
                list: (STREAM.to_owned(), 7),
                ordinal,
            },
        )
    };
    let places = HashMap::from([
        place("d1", 1),
        place("d2", 0),
        place("r1", 0),
        place("r2", 0),
    ]);
    let scopes = BTreeMap::from([(String::new(), crate::tree::file_root_id())]);
    let projected = super::parameters::project(
        &legacy,
        &places,
        &scopes,
        &BTreeSet::new(),
        &BTreeSet::from([OWNER, SHARED]),
        &mut Vec::new(),
    );
    let positions = projected
        .iter()
        .map(|parameter| {
            (
                parameter.name.as_ref().unwrap().as_str(),
                parameter.position,
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        positions,
        BTreeMap::from([("d1", Some(1)), ("d2", Some(0)), ("r1", None), ("r2", None)])
    );
}

/// The fields a CADIR write of `ir` names for the model parameter `id`:
/// none where it carries the parameter whole.
fn parameter_charge(ir: &cadmpeg_ir::document::CadIr, id: &str) -> Vec<String> {
    use cadmpeg_ir::codec::write::{CadirEncoder, EncodeInput, Encoder};

    let plan = CadirEncoder
        .plan(EncodeInput::new(ir, None))
        .expect("the CADIR writer drops what it cannot write");
    let Some(loss) = plan.report().losses.iter().find(|loss| {
        loss.code.local_code() == "arena.omitted"
            && loss.message.contains("model arena `parameters`")
    }) else {
        return Vec::new();
    };
    let (omitted, part) = loss
        .message
        .split_once(" stated in part: ")
        .unwrap_or((loss.message.as_str(), ""));
    assert!(!omitted.contains(id), "{}", loss.message);
    part.split_once(&format!("{id} ("))
        .and_then(|(_, fields)| fields.split_once(')'))
        .map(|(fields, _)| fields.split(", ").map(str::to_owned).collect())
        .unwrap_or_default()
}

mod member_component;
mod parameter_display;
mod timeline_items;
mod timeline_match;
