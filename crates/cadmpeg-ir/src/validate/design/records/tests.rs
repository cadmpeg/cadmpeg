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
        *DesignRecords::new(&ir).spanned_definitions("test:design:definition#part"),
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
        *DesignRecords::new(&ir).spanned_definitions("test:design:definition#part"),
        BTreeSet::from(["test:design:definition#part"])
    );
}

#[test]
fn a_single_definition_scope_stops_at_file_and_external_children() {
    for storage in [
        Storage::File {
            document: ExternalDocument::document_id("child"),
            source: None,
        },
        Storage::External(ExternalDocument::path("child.f3d")),
    ] {
        let mut ir = document();
        ir.model.product_definitions[0]
            .occurrences
            .push(id("occurrence", "child"));
        ir.model.product_definitions.push(ProductDefinition {
            occurrences: vec![id("occurrence", "leaf")],
            ..ProductDefinition::new(id("definition", "child"), storage)
        });
        ir.model.product_definitions.push(ProductDefinition::new(
            id("definition", "leaf"),
            Storage::Embedded,
        ));
        for key in ["child", "leaf"] {
            ir.model.occurrences.push(Occurrence::new(
                id("occurrence", key),
                id("definition", key),
                None,
            ));
        }
        assert_eq!(
            *DesignRecords::new(&ir).spanned_definitions("test:design:definition#part"),
            BTreeSet::from(["test:design:definition#part"])
        );
    }
}

#[test]
fn a_multi_definition_scope_spans_file_children_and_their_embedded_children() {
    let mut ir = document();
    ir.design.features[0].component = Some(id("definition", "child"));
    ir.model.product_definitions[0]
        .occurrences
        .push(id("occurrence", "child"));
    ir.model.product_definitions.push(ProductDefinition {
        occurrences: vec![id("occurrence", "leaf")],
        ..ProductDefinition::new(
            id("definition", "child"),
            Storage::File {
                document: ExternalDocument::document_id("child"),
                source: None,
            },
        )
    });
    ir.model.product_definitions.push(ProductDefinition::new(
        id("definition", "leaf"),
        Storage::Embedded,
    ));
    for key in ["child", "leaf"] {
        ir.model.occurrences.push(Occurrence::new(
            id("occurrence", key),
            id("definition", key),
            None,
        ));
    }
    assert_eq!(
        *DesignRecords::new(&ir).spanned_definitions("test:design:definition#part"),
        BTreeSet::from([
            "test:design:definition#part",
            "test:design:definition#child",
            "test:design:definition#leaf",
        ])
    );
}

#[test]
fn the_span_of_every_scope_comes_from_one_index() {
    let mut ir = document();
    ir.model.product_definitions[0].storage = Storage::File {
        document: ExternalDocument::document_id("part"),
        source: None,
    };
    ir.model.product_definitions.push(ProductDefinition::new(
        id("definition", "unplaced"),
        Storage::Embedded,
    ));
    let mut other = ir.design.scopes[0].clone();
    other.definition = id("definition", "other");
    other.roots.clear();
    ir.design.scopes.push(other);
    ir.model.product_definitions.push(ProductDefinition::new(
        id("definition", "other"),
        Storage::Embedded,
    ));
    let spans = ir.spanned_definitions_of_scopes();
    assert_eq!(spans.len(), 2);
    for scope in &ir.design.scopes {
        let expected = ir
            .spanned_definitions(&scope.definition)
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        assert_eq!(spans[scope.definition.as_str()], expected);
    }
    assert_eq!(
        spans["test:design:definition#part"],
        BTreeSet::from([
            "test:design:definition#part".to_owned(),
            "test:design:definition#unplaced".to_owned(),
        ])
    );
}
