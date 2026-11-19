// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::DesignRecords;
use crate::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
use crate::validate::design::fixture::{document, id};

#[test]
fn a_file_spans_its_unplaced_embedded_definitions_by_file_key() {
    let mut ir = document();
    ir.model.product_definitions[0].storage = Storage::File {
        document: ExternalDocument::document_id("part"),
        source: None,
    };
    for definition in [
        "test:design:definition#unplaced",
        "test:part@stream:definition#local",
        "test:other@stream:definition#foreign",
    ] {
        ir.model.product_definitions.push(ProductDefinition::new(
            definition.try_into().unwrap(),
            Storage::Embedded,
        ));
    }
    assert_eq!(
        DesignRecords::new(&ir).spanned_definitions("test:design:definition#part"),
        BTreeSet::from([
            "test:design:definition#part",
            "test:design:definition#unplaced",
            "test:part@stream:definition#local",
        ])
    );
}

#[test]
fn spanning_stops_at_another_scope_and_its_placed_definitions() {
    let mut ir = document();
    ir.model.product_definitions[0].storage = Storage::File {
        document: ExternalDocument::document_id("part"),
        source: None,
    };
    let mut other = ir.design.scopes[0].clone();
    other.definition = id("definition", "other");
    other.roots.clear();
    ir.design.scopes.push(other);
    ir.model.product_definitions.push(ProductDefinition {
        occurrences: vec![id("occurrence", "leaf")],
        ..ProductDefinition::new(id("definition", "other"), Storage::Embedded)
    });
    ir.model.product_definitions.push(ProductDefinition::new(
        id("definition", "leaf"),
        Storage::Embedded,
    ));
    ir.model.occurrences.push(Occurrence::new(
        id("occurrence", "leaf"),
        id("definition", "leaf"),
        None,
    ));
    assert_eq!(
        DesignRecords::new(&ir).spanned_definitions("test:design:definition#part"),
        BTreeSet::from(["test:design:definition#part"])
    );
}
