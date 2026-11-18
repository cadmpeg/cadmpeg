// SPDX-License-Identifier: Apache-2.0
//! Part 10: exactly one root definition, which no occurrence places (§8.1).

use crate::document::CadIr;
use crate::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
use crate::report::check::{Check, Finding};
use crate::transform::Transform;

fn source() -> crate::document::SourceMeta {
    crate::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("test:root"),
        )),
        std::collections::BTreeMap::new(),
    )
}

/// A decoded document: one that a root file holds.
fn decoded() -> CadIr {
    CadIr::decoded(source())
}

fn root(id: &str) -> ProductDefinition {
    ProductDefinition::root(id.try_into().unwrap(), ExternalDocument::default())
}

fn root_findings(ir: &CadIr) -> Vec<Finding> {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::super::check_products(&ctx, ir, &mut findings).unwrap();
    findings
        .into_iter()
        .filter(|finding| {
            finding.message.contains("root definition")
                || finding.message.contains("no occurrence places")
        })
        .collect()
}

fn summary(ir: &CadIr) -> Vec<(Check, Option<String>, String)> {
    root_findings(ir)
        .into_iter()
        .map(|finding| (finding.check, finding.entity, finding.message))
        .collect()
}

#[test]
fn a_decoded_document_states_exactly_one_root_definition() {
    assert!(summary(&CadIr::empty()).is_empty());
    let mut ir = decoded();
    assert_eq!(
        summary(&ir),
        [(
            Check::Counts,
            None,
            "document states no root definition".to_owned()
        )]
    );
    ir.model
        .product_definitions
        .push(root("test:model:product-definition#root"));
    assert!(summary(&ir).is_empty());

    ir.model
        .product_definitions
        .push(root("test:model:product-definition#second"));
    let found = root_findings(&ir);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].check, Check::Counts);
    assert_eq!(
        found[0].entity.as_deref(),
        Some("test:model:product-definition#second")
    );
    assert_eq!(found[0].related, ["test:model:product-definition#root"]);
    assert_eq!(found[0].severity, crate::report::Severity::Error);
}

#[test]
fn a_component_file_is_not_a_root() {
    let mut ir = decoded();
    let mut component = ProductDefinition::new(
        "test:model:product-definition#pin".try_into().unwrap(),
        Storage::File {
            document: ExternalDocument::path("pin.f3d"),
            source: Some(source()),
        },
    );
    component
        .bodies
        .push("test:model:body#pin".try_into().unwrap());
    ir.model.product_definitions.push(component);
    assert_eq!(
        summary(&ir),
        [
            (
                Check::Counts,
                None,
                "document states no root definition".to_owned()
            ),
            (
                Check::Counts,
                Some("test:model:product-definition#pin".to_owned()),
                "a definition no occurrence places holds content but is not the root definition"
                    .to_owned()
            ),
        ]
    );
}

#[test]
fn no_occurrence_places_the_root_definition() {
    let mut ir = CadIr::empty();
    let mut top = root("test:model:product-definition#root");
    top.occurrences
        .push("test:model:occurrence#self".try_into().unwrap());
    ir.model.product_definitions.push(top);
    ir.model.occurrences.push(Occurrence::new(
        "test:model:occurrence#self".try_into().unwrap(),
        "test:model:product-definition#root".try_into().unwrap(),
        Some(Transform::identity()),
    ));
    let found = root_findings(&ir);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].check, Check::ReferentialIntegrity);
    assert_eq!(found[0].message, "an occurrence places the root definition");
    assert_eq!(
        found[0].entity.as_deref(),
        Some("test:model:product-definition#root")
    );
    assert_eq!(found[0].related, ["test:model:occurrence#self"]);
}

#[test]
fn only_the_root_heads_a_product_tree() {
    let mut ir = decoded();
    ir.model
        .product_definitions
        .push(root("test:model:product-definition#root"));
    let mut part = ProductDefinition::new(
        "test:model:product-definition#part".try_into().unwrap(),
        Storage::Embedded,
    );
    part.bodies.push("test:model:body#part".try_into().unwrap());
    ir.model.product_definitions.push(part);
    // A definition that holds nothing, and a document that was not read,
    // head no tree.
    ir.model.product_definitions.push(ProductDefinition::new(
        "test:model:product-definition#link".try_into().unwrap(),
        Storage::Embedded,
    ));
    ir.model.product_definitions.push(ProductDefinition::new(
        "test:model:product-definition#external".try_into().unwrap(),
        Storage::External(ExternalDocument::path("other.f3d")),
    ));
    assert_eq!(
        summary(&ir),
        [(
            Check::Counts,
            Some("test:model:product-definition#part".to_owned()),
            "a definition no occurrence places holds content but is not the root definition"
                .to_owned()
        )]
    );

    ir.model.product_definitions[0]
        .occurrences
        .push("test:model:occurrence#part".try_into().unwrap());
    ir.model.occurrences.push(Occurrence::new(
        "test:model:occurrence#part".try_into().unwrap(),
        "test:model:product-definition#part".try_into().unwrap(),
        Some(Transform::identity()),
    ));
    assert!(summary(&ir).is_empty());
}

#[test]
fn a_fragment_may_hold_no_root_and_unplaced_tops() {
    let mut ir = decoded();
    ir.fragment = true;
    let mut part = ProductDefinition::new(
        "test:model:product-definition#part".try_into().unwrap(),
        Storage::Embedded,
    );
    part.bodies.push("test:model:body#part".try_into().unwrap());
    ir.model.product_definitions.push(part);
    assert!(summary(&ir).is_empty());

    ir.model
        .product_definitions
        .push(root("test:model:product-definition#one"));
    ir.model
        .product_definitions
        .push(root("test:model:product-definition#two"));
    assert_eq!(root_findings(&ir).len(), 1);
}

#[test]
fn the_root_rule_rolls_back_no_decoder_candidate() {
    let ir = decoded();
    for allowed in [
        crate::validate::admit::DRAFT_CORE_CHECKS,
        crate::validate::admit::RHINO_DRAFT_CHECKS,
        crate::validate::admit::CATIA_ADMISSION_CHECKS,
    ] {
        let report = crate::validate::admit::admit(
            &cadmpeg_test_support::service_decode_context(),
            &ir,
            allowed,
            Vec::new(),
        )
        .unwrap();
        assert!(report.is_ok(), "{:?}", report.findings);
    }
}
