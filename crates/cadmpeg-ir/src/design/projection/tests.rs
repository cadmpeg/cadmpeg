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
