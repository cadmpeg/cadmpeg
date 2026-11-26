// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;

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
    ir
}

const FILLET: &str = "test:design:feature#fillet";
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
    let ir = restated_document();
    let mut annotations = crate::AnnotationBuilder::new();
    for (id, exactness) in entities {
        annotations.exactness(id, *exactness);
    }
    for (id, field, exactness) in marks {
        annotations.field_exactness(id, *field, *exactness).unwrap();
    }
    super::restate_exactness(&ir, &mut annotations).unwrap();
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

/// A mark already on a design field is kept: on a field that restates no
/// model field as it is, and on one that does at the least exact of the mark
/// and the model fields. A design record with no model record keeps its note.
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
            (extrude, "placement", Inferred),
        ],
        &[FILLET, WIDTH, extrude],
    ))
    .unwrap();
    assert_eq!(
        fillet.1,
        fields(&[("definition", Derived), ("placement", Inferred)])
    );
    assert_eq!(
        width.1,
        fields(&[
            ("definition", Derived),
            ("kind", Inferred),
            ("scope", Inferred)
        ])
    );
    assert_eq!(extrude.1, fields(&[("placement", Inferred)]));
}
