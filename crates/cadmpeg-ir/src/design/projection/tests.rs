// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;

use super::fields::{FeatureField, ModelFeatureField, ModelParameterField, ParameterField};
use super::project_layer;
use super::restated::Restater;
use crate::design::scope::DesignScope;
use crate::ids::ProductDefinitionId;
use crate::products::{ExternalDocument, ProductDefinition};
use crate::report::loss::LossNote;
use crate::report::Severity;
use crate::unknown::part::{record_undecoded_kind, record_unreadable_kind};
use crate::CadIr;

fn definition(key: &str) -> ProductDefinitionId {
    ProductDefinitionId::mint(format!("test:model:product-definition#{key}")).expect("valid id")
}

fn scope(definition: ProductDefinitionId) -> DesignScope {
    DesignScope {
        definition,
        roots: Vec::new(),
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::new(),
    }
}

fn kept_loss() -> LossNote {
    LossNote::new(record_unreadable_kind(), "an earlier loss")
}

#[test]
fn a_failed_projection_leaves_the_document_as_it_was_with_one_loss() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut ir = crate::examples::unit_cube().expect("cube fixture");
    let geometry = ir.geometry.clone();
    let model = ir.model.clone();
    let mut losses = vec![kept_loss()];
    project_layer(&ctx, "test", &mut ir, &mut losses, |ir, losses| {
        let root = definition("root");
        ir.model.product_definitions.push(ProductDefinition::root(
            root.clone(),
            ExternalDocument::default(),
        ));
        ir.design.scopes.push(scope(root));
        ir.model.restated_mut().record(
            "test:model:feature#body",
            Restater::Record("test:design:container#body".to_owned()),
            [],
        );
        losses.push(kept_loss());
        Err(CodecError::malformed("the third record has no frame"))
    })
    .expect("a record error does not end the session");
    assert_eq!(ir.geometry, geometry);
    assert_eq!(ir.model, model);
    assert_eq!(ir.model.restated().get("test:model:feature#body"), None);
    assert_eq!(ir.design.entity_count(), 0);
    assert_eq!(losses.len(), 2);
    assert_eq!(losses[0], kept_loss());
    assert_eq!(losses[1].code, record_undecoded_kind());
    assert_eq!(losses[1].severity, Severity::Warning);
    assert_eq!(
        losses[1].message,
        "test v11 design layer is not stated: malformed container: the third record has no frame"
    );
    ctx.finish_session()
        .expect("a record error does not fuse the session");
}

#[test]
fn a_projection_that_succeeds_keeps_its_records() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    project_layer(&ctx, "test", &mut ir, &mut losses, |ir, _| {
        ir.design.scopes.push(scope(definition("root")));
        Ok(())
    })
    .expect("projection");
    assert_eq!(ir.design.scopes.len(), 1);
    assert!(losses.is_empty());
}

#[test]
fn an_error_that_ends_the_session_ends_the_decode() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    let error = project_layer(&ctx, "test", &mut ir, &mut losses, |_, _| {
        Err(CodecError::Io(std::io::Error::other("source closed")))
    })
    .expect_err("I/O ends the session");
    assert!(matches!(error, CodecError::Io(_)));
}

/// The fixture document with a model feature of the id of its design fillet
/// and a design and a model parameter `width`, each with no exactness.
fn restated_document() -> CadIr {
    use crate::design::units::{Quantity, ValueKind};
    use crate::features::parameters::DesignParameter as ModelParameter;
    use crate::features::selections::DistinctMembers;
    use crate::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId};
    use crate::validate::design::fixture;

    let mut ir = fixture::document();
    ir.design.parameters.push(fixture::parameter(
        "width",
        ValueKind::Quantity(Quantity::LENGTH),
    ));
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("test:design:feature#fillet").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
        native_ref: None,
    });
    ir.model.parameters.push(ModelParameter {
        product_definition: None,
        id: ParameterId::mint("test:design:parameter#width").unwrap(),
        owner: None,
        ordinal: 0,
        name: "width".to_owned(),
        expression: "10 mm".to_owned(),
        display: None,
        value: None,
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::default(),
        pmi: None,
        native_ref: None,
    });
    ir
}

const FILLET: &str = "test:design:feature#fillet";

/// A projection that succeeds states the fields each design feature leaves
/// out of the model feature of its id: the operands of an operation it states
/// as `Unresolved`, and a name and a suppression state it does not state.
#[test]
fn a_projection_states_the_fields_a_design_feature_leaves_out_of_its_model_feature() {
    use crate::design::feature::{FeatureOperation, OperationFamily};
    use crate::features::evaluation::FeatureEvaluation;
    use crate::features::{
        FeatureDefinition, FeatureOperation as ModelOperation, NativeFeatureKind,
    };

    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut ir = restated_document();
    let model = ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id.as_str() == FILLET)
        .expect("model fillet");
    model.name = Some("Fillet1".to_owned());
    model.suppressed = Some(false);
    model.evaluation =
        FeatureEvaluation::from_definition(FeatureDefinition::Operation(ModelOperation::Native {
            kind: NativeFeatureKind::Fillet,
            parameters: BTreeMap::from([(
                cadmpeg_core::text::NonBlankString::new("radius".to_owned()).expect("name"),
                "2 mm".to_owned(),
            )]),
        }));
    let mut losses = Vec::new();
    project_layer(&ctx, "test", &mut ir, &mut losses, |ir, _| {
        let feature = ir
            .design
            .features
            .iter_mut()
            .find(|feature| feature.id.as_str() == FILLET)
            .expect("design fillet");
        feature.name = None;
        feature.suppressed = None;
        feature.definition.operation = FeatureOperation::Unresolved {
            family: OperationFamily::Fillet,
        };
        Ok(())
    })
    .expect("projection");
    assert_eq!(
        cut_by_itself(&ir, FILLET),
        Some(vec!["definition", "name", "suppressed"])
    );
}

/// The fields a design record of the same id leaves out of the model record
/// `id`, where it is the record's one restater.
fn cut_by_itself(ir: &CadIr, id: &str) -> Option<Vec<&'static str>> {
    let restating = ir.model.restated().get(id)?;
    let by = Restater::Record(id.to_owned());
    assert_eq!(
        restating
            .restaters()
            .map(|(restater, _)| restater)
            .collect::<Vec<_>>(),
        [&by]
    );
    restating
        .cut_by(|restater| restater == &by)
        .map(|cut| cut.into_iter().collect())
}

/// Runs a projection that states the design fillet's operation as
/// `operation`, over the model fillet whose definition is `definition` and
/// whose outputs and dependencies are `outputs` and `dependencies`.
fn project_fillet(
    definition: crate::features::FeatureDefinition,
    outputs: Vec<crate::ids::BodyId>,
    dependencies: Vec<crate::features::FeatureId>,
    operation: crate::design::feature::FeatureOperation,
) -> CadIr {
    use crate::features::evaluation::FeatureEvaluation;

    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut ir = restated_document();
    let model = ir
        .model
        .features
        .iter_mut()
        .find(|feature| feature.id.as_str() == FILLET)
        .expect("model fillet");
    model.evaluation = FeatureEvaluation::new(definition, outputs.into_iter().collect());
    model.dependencies = dependencies.into_iter().collect();
    let mut losses = Vec::new();
    project_layer(&ctx, "test", &mut ir, &mut losses, |ir, _| {
        let feature = ir
            .design
            .features
            .iter_mut()
            .find(|feature| feature.id.as_str() == FILLET)
            .expect("design fillet");
        feature.definition.operation = operation;
        Ok(())
    })
    .expect("projection");
    ir
}

fn body(key: &str) -> crate::ids::BodyId {
    crate::ids::BodyId::mint(format!("test:model:body#{key}")).expect("valid id")
}

/// A typed design operation that holds fewer operands than its model
/// operation leaves out the model definition and the dependencies that
/// derive from it (D26); one that holds every operand states both.
#[test]
fn a_typed_operation_with_fewer_operands_leaves_out_its_definition_and_dependencies() {
    use crate::design::feature::{FeatureOperation, StoredOrigin};
    use crate::features::selections::BodySelection;
    use crate::features::{FeatureDefinition, FeatureId, FeatureOperation as ModelOperation};

    let base = || {
        FeatureDefinition::Operation(ModelOperation::BaseFeature {
            bodies: BodySelection::Bodies([body("a"), body("b")].into_iter().collect()),
        })
    };
    let stored = |bodies: &[&str]| FeatureOperation::Stored {
        bodies: Some(bodies.iter().map(|key| body(key)).collect()),
        tessellations: Vec::new(),
        origin: StoredOrigin::Base {},
    };
    let dependency = FeatureId::mint("test:design:feature#sketch").expect("valid id");
    let fewer = project_fillet(base(), Vec::new(), vec![dependency.clone()], stored(&["a"]));
    assert_eq!(
        cut_by_itself(&fewer, FILLET),
        Some(vec!["definition", "dependencies"])
    );
    let every = project_fillet(base(), Vec::new(), vec![dependency], stored(&["a", "b"]));
    assert_eq!(cut_by_itself(&every, FILLET), Some(Vec::new()));
}

/// A model feature's outputs are carried where a design record states that
/// the feature produced them, in its own stored operation or the current
/// state after the feature.
#[test]
fn the_outputs_of_a_feature_are_carried_by_its_stored_geometry_or_state() {
    use crate::design::feature::{FeatureOperation, OperationFamily, StoredOrigin};
    use crate::features::{FeatureDefinition, FeatureOperation as ModelOperation};

    let current = restated_document().design.scopes[0].current[0]
        .bodies
        .clone();
    let unresolved = || {
        FeatureDefinition::Operation(ModelOperation::Unresolved {
            family: crate::features::UnresolvedFamily::Fillet,
        })
    };
    let operation = || FeatureOperation::Unresolved {
        family: OperationFamily::Fillet,
    };
    let stated = project_fillet(unresolved(), current.clone(), Vec::new(), operation());
    assert_eq!(cut_by_itself(&stated, FILLET), Some(Vec::new()));
    let mut more = current;
    more.push(body("elsewhere"));
    let unstated = project_fillet(unresolved(), more.clone(), Vec::new(), operation());
    assert_eq!(cut_by_itself(&unstated, FILLET), Some(vec!["outputs"]));
    let stored = project_fillet(
        unresolved(),
        more,
        Vec::new(),
        FeatureOperation::Stored {
            bodies: Some(vec![body("elsewhere")]),
            tessellations: Vec::new(),
            origin: StoredOrigin::Base {},
        },
    );
    assert_eq!(cut_by_itself(&stored, FILLET), Some(Vec::new()));
}

/// A record restated by several restaters keeps each restater's cut apart:
/// a write charges the fields every restater it writes leaves out, and the
/// whole record when it writes none of them.
#[test]
fn a_record_with_several_restaters_is_cut_by_what_every_written_restater_leaves_out() {
    use super::restated::Restatements;

    let first = Restater::Record("test:design:container#first".to_owned());
    let mut restatements = Restatements::default();
    restatements.record(
        "test:model:feature#node",
        first.clone(),
        ["name", "outputs"],
    );
    restatements.record(
        "test:model:feature#node",
        Restater::Names,
        ["outputs", "suppressed"],
    );
    restatements.record("test:model:feature#node", first.clone(), ["definition"]);
    let restating = restatements
        .get("test:model:feature#node")
        .expect("restated");
    let cut = |writes: &dyn Fn(&Restater) -> bool| {
        restating
            .cut_by(writes)
            .map(|cut| cut.into_iter().collect::<Vec<_>>())
    };
    assert_eq!(cut(&|_| true), Some(vec!["outputs"]));
    assert_eq!(
        cut(&|by| by == &first),
        Some(vec!["definition", "name", "outputs"])
    );
    assert_eq!(
        cut(&|by| by == &Restater::Names),
        Some(vec!["outputs", "suppressed"])
    );
    assert_eq!(cut(&|_| false), None);
}

/// A container restates a model tree node without the children it does not
/// hold as members, the active child it does not state, and the name and
/// suppression it does not hold; one that holds them all restates it whole.
#[test]
fn a_container_restates_a_tree_node_without_what_it_does_not_hold() {
    use super::restated::{restate_tree_nodes, NodeStatement};
    use crate::features::evaluation::FeatureEvaluation;
    use crate::features::tree::{FeatureTreeNodeRole, TreeChildren};
    use crate::features::{FeatureDefinition, FeatureId, FeatureOperation as ModelOperation};

    let child = |key: &str| FeatureId::mint(format!("test:model:feature#{key}")).expect("id");
    let node = || {
        let mut ir = restated_document();
        let model = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.id.as_str() == FILLET)
            .expect("model node");
        model.name = Some("Folder1".to_owned());
        model.suppressed = Some(false);
        model.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            ModelOperation::TreeNode {
                role: FeatureTreeNodeRole::FeatureFolder,
                children: TreeChildren::new(vec![child("a"), child("b")], Some(child("b")))
                    .expect("children"),
            },
        ));
        ir
    };
    let container = Restater::Record("test:design:container#folder".to_owned());
    let cut = |ir: &CadIr| {
        ir.model
            .restated()
            .get(FILLET)
            .and_then(|restating| restating.cut_by(|by| by == &container))
            .map(|cut| cut.into_iter().collect::<Vec<_>>())
    };
    let mut part = node();
    restate_tree_nodes(
        &mut part,
        [(
            FeatureId::mint(FILLET).expect("id"),
            container.clone(),
            NodeStatement {
                members: vec![child("a")],
                ..NodeStatement::default()
            },
        )],
    );
    assert_eq!(cut(&part), Some(vec!["definition", "name", "suppressed"]));
    let mut whole = node();
    restate_tree_nodes(
        &mut whole,
        [(
            FeatureId::mint(FILLET).expect("id"),
            container.clone(),
            NodeStatement {
                members: vec![child("a"), child("b")],
                active: Some(child("b")),
                name: Some("Folder1".to_owned()),
                suppressed: true,
                bodies: Vec::new(),
            },
        )],
    );
    assert_eq!(cut(&whole), Some(Vec::new()));
}

fn feature_inputs(field: FeatureField) -> &'static [ModelFeatureField] {
    match field {
        FeatureField::Name => &[ModelFeatureField::Name],
        FeatureField::Suppressed => &[ModelFeatureField::Suppressed],
        FeatureField::Definition => &[
            ModelFeatureField::Definition,
            ModelFeatureField::Dependencies,
        ],
        FeatureField::NativeRef => &[ModelFeatureField::NativeRef],
        FeatureField::Placement | FeatureField::Component | FeatureField::DrivenBy => &[],
    }
}

fn parameter_inputs(field: ParameterField) -> &'static [ModelParameterField] {
    match field {
        ParameterField::Owner => &[ModelParameterField::Owner],
        ParameterField::Name => &[ModelParameterField::Name],
        ParameterField::Kind | ParameterField::Unit => &[ModelParameterField::Properties],
        ParameterField::Definition => &[
            ModelParameterField::ExpressionText,
            ModelParameterField::Dependencies,
            ModelParameterField::Properties,
        ],
        ParameterField::Value => &[ModelParameterField::Value],
        ParameterField::Display => &[ModelParameterField::Display],
        ParameterField::NativeRef => &[ModelParameterField::NativeRef],
        ParameterField::Scope
        | ParameterField::Tolerance
        | ParameterField::Decimals
        | ParameterField::Comment
        | ParameterField::NativeOptions
        | ParameterField::SourceSettings => &[],
    }
}

const RESTATEMENT: super::Restatement = super::Restatement {
    feature: feature_inputs,
    parameter: parameter_inputs,
};
const WIDTH: &str = "test:design:parameter#width";

/// Restates `marks` (id, field, exactness) over entity exactness `entities`
/// and returns, for each of `ids`, its entity exactness and field overrides.
fn restated(
    entities: &[(&str, crate::provenance::Exactness)],
    marks: &[(&str, &str, crate::provenance::Exactness)],
    ids: &[&str],
) -> Vec<(
    crate::provenance::Exactness,
    Vec<(String, crate::provenance::Exactness)>,
)> {
    restated_with(RESTATEMENT, entities, marks, ids)
}

/// [`restated`] through `restatement`.
fn restated_with(
    restatement: super::Restatement,
    entities: &[(&str, crate::provenance::Exactness)],
    marks: &[(&str, &str, crate::provenance::Exactness)],
    ids: &[&str],
) -> Vec<(
    crate::provenance::Exactness,
    Vec<(String, crate::provenance::Exactness)>,
)> {
    let ir = restated_document();
    let mut annotations = crate::AnnotationBuilder::new();
    for (id, exactness) in entities {
        annotations.exactness(*id, *exactness);
    }
    for (id, field, exactness) in marks {
        annotations
            .field_exactness(*id, *field, *exactness)
            .unwrap();
    }
    super::restate_exactness(&ir, &mut annotations, restatement).unwrap();
    let annotations = annotations.build();
    ids.iter()
        .map(|id| {
            annotations.exactness().get(*id).map_or_else(
                || (crate::provenance::Exactness::ByteExact, Vec::new()),
                |note| {
                    let fields = note
                        .fields()
                        .iter()
                        .map(|(field, exactness)| (field.to_string(), *exactness))
                        .collect();
                    (note.entity(), fields)
                },
            )
        })
        .collect()
}

fn fields(
    marks: &[(&str, crate::provenance::Exactness)],
) -> Vec<(String, crate::provenance::Exactness)> {
    marks
        .iter()
        .map(|(field, exactness)| ((*field).to_owned(), *exactness))
        .collect()
}

/// Each model mark moves to the stated design field that restates it. A
/// model field that no stated design field restates leaves: `owner` (the
/// design parameter states no owner), `ordinal` and `source_content`.
#[test]
fn restated_exactness_names_the_fields_of_the_design_record() {
    use crate::provenance::Exactness::{ByteExact, Derived, Inferred};
    let [fillet, width] = <[_; 2]>::try_from(restated(
        &[(FILLET, Inferred)],
        &[
            (FILLET, "source_content", Derived),
            (WIDTH, "owner", Derived),
            (WIDTH, "ordinal", Derived),
            (WIDTH, "properties", Derived),
            (WIDTH, "value", Derived),
        ],
        &[FILLET, WIDTH],
    ))
    .unwrap();
    assert_eq!(fillet, (Inferred, Vec::new()));
    assert_eq!(
        width,
        (
            ByteExact,
            fields(&[("definition", Derived), ("kind", Derived)])
        )
    );
}

/// A design field restating several model fields takes the least exact of
/// them, and a model field with no mark of its own counts at the entity
/// exactness.
#[test]
fn a_design_field_takes_the_least_exact_of_the_model_fields_it_restates() {
    use crate::provenance::Exactness::{ByteExact, Derived, Inferred};
    let [fillet, width] = <[_; 2]>::try_from(restated(
        &[(FILLET, Inferred)],
        &[
            (FILLET, "dependencies", Derived),
            (WIDTH, "expression", Derived),
            (WIDTH, "dependencies", Inferred),
            (WIDTH, "properties", ByteExact),
        ],
        &[FILLET, WIDTH],
    ))
    .unwrap();
    assert_eq!(fillet, (Inferred, Vec::new()), "definition stays inferred");
    assert_eq!(width, (ByteExact, fields(&[("definition", Inferred)])));
}

/// A mark already on a stated design field is kept: on a field with no model
/// field as it is, and on one built from model fields at the least exact of
/// the mark and those fields. A mark on a field the design record does not
/// state leaves: the fillet states no placement. A design record with no
/// model record keeps its note.
#[test]
fn restated_exactness_keeps_the_marks_of_design_fields() {
    use crate::provenance::Exactness::{Derived, Inferred};
    let extrude = "test:design:feature#extrude";
    let [fillet, width, extrude] = <[_; 3]>::try_from(restated(
        &[],
        &[
            (FILLET, "placement", Inferred),
            (FILLET, "dependencies", Derived),
            (WIDTH, "scope", Inferred),
            (WIDTH, "kind", Inferred),
            (WIDTH, "properties", Derived),
            (extrude, "definition", Inferred),
        ],
        &[FILLET, WIDTH, extrude],
    ))
    .unwrap();
    assert_eq!(fillet.1, fields(&[("definition", Derived)]));
    assert_eq!(
        width.1,
        fields(&[
            ("definition", Derived),
            ("kind", Inferred),
            ("scope", Inferred)
        ])
    );
    assert_eq!(extrude.1, fields(&[("definition", Inferred)]));
}

/// A mark on a sub-path of a model field counts toward that field. A model
/// mark on a field whose design field the record does not state is written
/// nowhere, and the design record's own mark on that field leaves.
#[test]
fn a_model_sub_path_mark_counts_toward_its_field_and_unstated_fields_take_no_mark() {
    use crate::provenance::Exactness::{ByteExact, Derived, Inferred};
    let [width] = <[_; 1]>::try_from(restated(
        &[],
        &[
            (WIDTH, "properties", Derived),
            (WIDTH, "properties.unit", Inferred),
            (WIDTH, "display", Derived),
            (WIDTH, "unit", Derived),
            (WIDTH, "tolerance.upper", Inferred),
        ],
        &[WIDTH],
    ))
    .unwrap();
    assert_eq!(
        width,
        (
            ByteExact,
            fields(&[("definition", Inferred), ("kind", Inferred)])
        )
    );
}

/// A design record's own mark on a sub-path of a stated field is kept, at no
/// more exactness than its field.
#[test]
fn a_design_sub_path_mark_is_kept_at_no_more_than_its_field() {
    use crate::provenance::Exactness::{Derived, Inferred};
    let [width] = <[_; 1]>::try_from(restated(
        &[],
        &[
            (WIDTH, "properties", Inferred),
            (WIDTH, "definition.value", Derived),
            (WIDTH, "native_options.test", Derived),
        ],
        &[WIDTH],
    ))
    .unwrap();
    assert_eq!(
        width.1,
        fields(&[
            ("definition", Inferred),
            ("definition.value", Inferred),
            ("kind", Inferred),
            ("native_options.test", Derived),
        ])
    );
}

/// A mark under a name both records share counts toward the design field of
/// that name, at field level, also where the codec builds the field from no
/// model field of that name. A mark stated on the design record before the
/// restatement is not lost.
#[test]
fn a_mark_under_a_shared_name_folds_into_the_design_field_of_that_name() {
    use crate::provenance::Exactness::{ByteExact, Derived, Inferred};
    fn without_definition(field: FeatureField) -> &'static [ModelFeatureField] {
        match field {
            FeatureField::Definition => &[],
            other => feature_inputs(other),
        }
    }
    let restatement = super::Restatement {
        feature: without_definition,
        parameter: parameter_inputs,
    };
    let [fillet] = <[_; 1]>::try_from(restated_with(
        restatement,
        &[],
        &[
            (FILLET, "definition.operation.value", Inferred),
            (FILLET, "dependencies", Derived),
        ],
        &[FILLET],
    ))
    .unwrap();
    assert_eq!(fillet, (ByteExact, fields(&[("definition", Inferred)])));
    let [fillet] = <[_; 1]>::try_from(restated_with(
        restatement,
        &[],
        &[(FILLET, "definition", Inferred)],
        &[FILLET],
    ))
    .unwrap();
    assert_eq!(fillet, (ByteExact, fields(&[("definition", Inferred)])));
}

/// A mark under a shared name that the codec builds from no model field of
/// that name may describe the model field, so the design field claims no
/// more than its entity: a derived `definition` on an inferred fillet leaves,
/// and an unknown one stays.
#[test]
fn an_unlisted_shared_name_mark_claims_no_more_than_its_entity() {
    use crate::provenance::Exactness::{Derived, Inferred, Unknown};
    fn without_definition(field: FeatureField) -> &'static [ModelFeatureField] {
        match field {
            FeatureField::Definition => &[],
            other => feature_inputs(other),
        }
    }
    let restatement = super::Restatement {
        feature: without_definition,
        parameter: parameter_inputs,
    };
    for (mark, expected) in [
        (Derived, vec![]),
        (Unknown, fields(&[("definition", Unknown)])),
    ] {
        let [fillet] = <[_; 1]>::try_from(restated_with(
            restatement,
            &[(FILLET, Inferred)],
            &[
                (FILLET, "definition", mark),
                (FILLET, "definition.operation", Derived),
            ],
            &[FILLET],
        ))
        .unwrap();
        assert_eq!(fillet, (Inferred, expected), "{mark:?}");
    }
}
