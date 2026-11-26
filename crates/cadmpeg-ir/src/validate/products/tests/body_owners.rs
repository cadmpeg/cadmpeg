// SPDX-License-Identifier: Apache-2.0
//! Part 10: every body of the geometry is listed by exactly one definition's
//! `bodies` and by no `instance_bodies` entry, or by `instance_bodies` entries
//! and by no definition's `bodies` (§8.1).

use crate::design::selection::OccurrencePath;
use crate::document::CadIr;
use crate::ids::BodyId;
use crate::products::{ExternalDocument, InstanceBodies, ProductDefinition, Storage};
use crate::report::check::Finding;
use crate::report::Severity;

fn source() -> crate::document::SourceMeta {
    crate::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("test:root"),
        )),
        std::collections::BTreeMap::new(),
    )
}

/// The unit cube, decoded, with a root definition that lists no body.
fn cube() -> (CadIr, BodyId) {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.source = Some(source());
    ir.model
        .product_definitions
        .retain(|definition| !definition.is_root());
    ir.model.product_definitions.push(ProductDefinition::root(
        "test:model:product-definition#root".try_into().unwrap(),
        ExternalDocument::default(),
    ));
    let body = ir.geometry.bodies[0].id.clone();
    (ir, body)
}

fn part(key: &str) -> ProductDefinition {
    ProductDefinition::new(
        format!("test:model:product-definition#{key}")
            .as_str()
            .try_into()
            .unwrap(),
        Storage::Embedded,
    )
}

fn owner_findings(ir: &CadIr) -> Vec<Finding> {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut findings = Vec::new();
    super::super::check_products(&ctx, ir, &mut findings).unwrap();
    findings
        .into_iter()
        .filter(|finding| finding.message.starts_with("body is listed by"))
        .collect()
}

#[test]
fn a_body_is_listed_by_exactly_one_definition() {
    let (ir, body) = cube();
    let found = owner_findings(&ir);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].entity.as_deref(), Some(body.as_str()));
    assert_eq!(found[0].severity, Severity::Error);
    assert!(found[0].message.contains("listed by no definition"));

    let (mut ir, body) = cube();
    ir.model.product_definitions[0].bodies.push(body.clone());
    let mut other = part("other");
    other.bodies.push(body.clone());
    ir.model.product_definitions.push(other);
    let found = owner_findings(&ir);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].entity.as_deref(), Some(body.as_str()));
    assert_eq!(found[0].severity, Severity::Error);
    assert_eq!(
        found[0].related,
        [
            "test:model:product-definition#other",
            "test:model:product-definition#root"
        ]
    );
}

#[test]
fn a_body_listed_once_or_by_instance_bodies_breaks_no_rule() {
    let (mut ir, body) = cube();
    ir.model.product_definitions[0].bodies.push(body.clone());
    assert!(owner_findings(&ir).is_empty());

    let (mut ir, body) = cube();
    ir.model.product_definitions[0]
        .instance_bodies
        .push(InstanceBodies {
            occurrence: OccurrencePath(Vec::new()),
            bodies: vec![body],
        });
    assert!(owner_findings(&ir).is_empty());

    let (mut ir, _) = cube();
    ir.fragment = true;
    assert!(owner_findings(&ir).is_empty());
}

#[test]
fn a_body_an_instance_bodies_entry_shows_is_listed_by_no_definition() {
    let (mut ir, body) = cube();
    let mut other = part("other");
    other.bodies.push(body.clone());
    ir.model.product_definitions.push(other);
    ir.model.product_definitions[0]
        .instance_bodies
        .push(InstanceBodies {
            occurrence: OccurrencePath(Vec::new()),
            bodies: vec![body.clone()],
        });
    let found = owner_findings(&ir);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].entity.as_deref(), Some(body.as_str()));
    assert_eq!(found[0].severity, Severity::Error);
    assert_eq!(found[0].related, ["test:model:product-definition#other"]);
    assert!(found[0].message.contains("an `instance_bodies` entry"));
}

#[test]
fn a_document_with_no_source_meets_the_rule_too() {
    let (mut ir, body) = cube();
    ir.source = None;
    let found = owner_findings(&ir);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].entity.as_deref(), Some(body.as_str()));
    assert!(found[0].message.contains("listed by no definition"));
}
