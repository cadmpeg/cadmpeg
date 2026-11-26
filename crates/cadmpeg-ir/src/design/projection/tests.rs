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

/// A design record that restates a model feature or parameter under its id
/// takes the exactness of that model record, with each model field override
/// moved to the design field that restates it.
#[test]
fn restated_exactness_names_the_fields_of_the_design_record() {
    use crate::design::units::{Quantity, ValueKind};
    use crate::features::parameters::DesignParameter as ModelParameter;
    use crate::features::selections::DistinctMembers;
    use crate::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId};
    use crate::provenance::Exactness;
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
    let mut annotations = crate::AnnotationBuilder::new();
    annotations.exactness("test:design:feature#fillet", Exactness::Inferred);
    annotations.exactness("test:design:parameter#width", Exactness::Derived);
    for (id, field) in [
        ("test:design:feature#fillet", "source_content"),
        ("test:design:feature#fillet", "dependencies"),
        ("test:design:parameter#width", "owner"),
        ("test:design:parameter#width", "ordinal"),
        ("test:design:parameter#width", "properties"),
        ("test:design:parameter#width", "expression"),
        ("test:design:feature#extrude", "placement"),
    ] {
        annotations.derived(id, field).unwrap();
    }

    super::restate_exactness(&ir, &mut annotations).unwrap();

    let annotations = annotations.build();
    let stated = |id: &str| {
        let note = &annotations.exactness()[id];
        let fields = note
            .fields()
            .iter()
            .map(|(field, exactness)| (field.to_string(), *exactness))
            .collect::<Vec<_>>();
        (note.entity(), fields)
    };
    let derived = |fields: &[&str]| {
        fields
            .iter()
            .map(|field| ((*field).to_owned(), Exactness::Derived))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        stated("test:design:feature#fillet"),
        (Exactness::Inferred, derived(&["definition"]))
    );
    assert_eq!(
        stated("test:design:parameter#width"),
        (
            Exactness::Derived,
            derived(&["definition", "kind", "scope"])
        )
    );
    assert_eq!(
        stated("test:design:feature#extrude").1,
        derived(&["placement"])
    );
}
