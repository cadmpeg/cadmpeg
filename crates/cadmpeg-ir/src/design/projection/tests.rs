// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;

use super::fields::{
    ArrangementField, FeatureField, ModelConfigurationField, ModelFeatureField,
    ModelParameterField, ParameterField,
};
use super::project_layer;
use crate::design::scope::DesignScope;
use crate::ids::ProductDefinitionId;
use crate::products::{ExternalDocument, ProductDefinition};
use crate::report::loss::LossNote;
use crate::report::Severity;
use crate::unknown::part::record_unreadable_kind;
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
        losses.push(kept_loss());
        Err(CodecError::malformed("the third record has no frame"))
    })
    .expect("a record error does not end the session");
    assert_eq!(ir.geometry, geometry);
    assert_eq!(ir.model, model);
    assert_eq!(ir.design.entity_count(), 0);
    assert_eq!(losses.len(), 2);
    assert_eq!(losses[0], kept_loss());
    assert_eq!(losses[1].code, record_unreadable_kind());
    assert_eq!(losses[1].severity, Severity::Error);
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
    ir.design
        .arrangements
        .push(crate::design::configuration::Arrangement {
            id: crate::design::ids::ArrangementId::mint(OPEN).unwrap(),
            scope: ir.design.scopes[0].definition.clone(),
            name: cadmpeg_core::nonblank_literal!("Open"),
            kind: crate::design::configuration::ArrangementKind::Positional,
            configuration: None,
            overrides: crate::design::configuration::Overrides::default(),
            appearances: Vec::new(),
            moves: Vec::new(),
            nested: Vec::new(),
            native_ref: None,
        });
    ir.model
        .configurations
        .push(crate::features::parameters::DesignConfiguration {
            id: crate::features::ConfigurationId::mint(OPEN).unwrap(),
            ordinal: 0,
            active: true,
            source_index: Some(0),
            name: Some("Open".to_owned()),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            bodies: None,
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        });
    ir
}

const OPEN: &str = "test:design:arrangement#open";

const FILLET: &str = "test:design:feature#fillet";

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

fn arrangement_inputs(field: ArrangementField) -> &'static [ModelConfigurationField] {
    match field {
        ArrangementField::Name => &[ModelConfigurationField::Name],
        ArrangementField::NativeRef => &[ModelConfigurationField::NativeRef],
        ArrangementField::Scope
        | ArrangementField::Kind
        | ArrangementField::Configuration
        | ArrangementField::Overrides
        | ArrangementField::Appearances
        | ArrangementField::Moves
        | ArrangementField::Nested => &[],
    }
}

const RESTATEMENT: super::Restatement = super::Restatement {
    feature: feature_inputs,
    parameter: parameter_inputs,
    arrangement: arrangement_inputs,
    active_arrangements: &[ModelConfigurationField::Active],
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
        arrangement: arrangement_inputs,
        active_arrangements: &[],
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

/// An arrangement that takes the id of a model configuration restates it: a
/// configuration mark moves to the arrangement field built from it, and a
/// mark no arrangement field restates leaves. The arrangement states no
/// native record, so the `native_ref` mark leaves too.
#[test]
fn an_arrangement_takes_the_marks_of_the_configuration_it_restates() {
    use crate::provenance::Exactness::{ByteExact, Derived, Inferred};
    let [open] = <[_; 1]>::try_from(restated(
        &[],
        &[
            (OPEN, "name", Inferred),
            (OPEN, "ordinal", Derived),
            (OPEN, "active", Derived),
            (OPEN, "source_index", Derived),
            (OPEN, "native_ref", Derived),
        ],
        &[OPEN],
    ))
    .unwrap();
    assert_eq!(open, (ByteExact, fields(&[("name", Inferred)])));
}

/// A scope's `active_arrangements` is built from the `active` field of each
/// configuration its arrangements restate. Its mark is stated at the scope's
/// key, the id of its definition, where that key claims more, and with a
/// restatement that reads no configuration field there is none.
#[test]
fn a_scope_takes_the_active_marks_of_the_configurations_its_arrangements_restate() {
    use crate::provenance::Exactness::{ByteExact, Derived, Inferred};
    let scope = restated_document().design.scopes[0]
        .definition
        .as_str()
        .to_owned();
    let active = [(OPEN, "active", Derived)];
    let [stated, covered, unmarked] = [
        restated(&[], &active, &[&scope]),
        restated(&[(&scope, Inferred)], &active, &[&scope]),
        restated(&[], &[(OPEN, "name", Derived)], &[&scope]),
    ]
    .map(|marks| <[_; 1]>::try_from(marks).unwrap()[0].clone());
    assert_eq!(
        stated,
        (ByteExact, fields(&[("active_arrangements", Derived)]))
    );
    assert_eq!(covered, (Inferred, Vec::new()));
    assert_eq!(unmarked, (ByteExact, Vec::new()));
    let unread = super::Restatement {
        active_arrangements: &[],
        ..RESTATEMENT
    };
    assert_eq!(
        restated_with(unread, &[], &active, &[&scope]),
        [(ByteExact, Vec::new())]
    );
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
        arrangement: arrangement_inputs,
        active_arrangements: &[],
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
