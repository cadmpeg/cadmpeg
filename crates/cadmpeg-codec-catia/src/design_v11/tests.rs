// SPDX-License-Identifier: Apache-2.0
//! The v11 design layer of decoded CATIA parts.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::check::Check;
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::test_container::outer_container_catpart;
use crate::test_support::test_formula::standard_catpart_with_formula_relation;
use crate::test_support::test_object_graph::{
    catalog_stream, entity_backed_object_graph, object_graph_record,
};
use crate::CatiaCodec;

fn decode(bytes: Vec<u8>, options: &DecodeOptions) -> EditableDecodeResult {
    EditableDecodeResult::from(CatiaCodec.decode(&mut Cursor::new(bytes), options).unwrap())
}

fn principal_plane_part(class: &str) -> Vec<u8> {
    let records = [
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
        object_graph_record(&[0x12, 0x82, 0x84], &[0xfe]),
    ];
    let mut stream = entity_backed_object_graph(&records, &[2, 3]);
    stream.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        class,
    ]));
    outer_container_catpart(&stream).0
}

fn design_findings(result: &EditableDecodeResult) -> Vec<String> {
    cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .unwrap()
        .findings
        .iter()
        .filter(|finding| finding.check == Check::Design)
        .map(|finding| format!("{finding:?}"))
        .collect()
}

/// The one root definition of an unscoped part owns every body (§8.1).
fn assert_root_owns_every_body(ir: &cadmpeg_ir::CadIr) {
    let [root] = ir.model.product_definitions.as_slice() else {
        panic!("one root definition");
    };
    assert!(root.is_root());
    assert_eq!(root.id.as_str(), "catia:model:product-definition#root");
    assert_eq!(
        root.bodies,
        ir.geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_part_with_parameters_states_one_scope_over_its_root_definition() {
    let result = decode(
        standard_catpart_with_formula_relation(4, false),
        &DecodeOptions::default(),
    );
    let ir = result.ir();
    assert_eq!(ir.model.parameters.len(), 2);
    let [root] = ir.model.product_definitions.as_slice() else {
        panic!("one root definition");
    };
    assert!(root.is_root());
    assert_eq!(root.id.as_str(), "catia:model:product-definition#root");
    assert!(root.bodies.is_empty());
    assert!(!ir.geometry.bodies.is_empty());
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one design scope");
    };
    assert_eq!(scope.definition, root.id);
    assert!(scope.roots.is_empty());
    assert!(scope.current.is_empty());
    assert_eq!(
        ir.design
            .parameters
            .iter()
            .map(|parameter| (&parameter.id, &parameter.scope))
            .collect::<Vec<_>>(),
        ir.model
            .parameters
            .iter()
            .map(|parameter| (&parameter.id, &root.id))
            .collect::<Vec<_>>()
    );
    assert!(ir.design.features.is_empty());
    assert!(ir.design.containers.is_empty());
    assert_eq!(design_findings(&result), Vec::<String>::new());
}

#[test]
fn a_feature_whose_container_is_unread_states_no_design_record() {
    let result = decode(principal_plane_part("xy-plane"), &DecodeOptions::default());
    let ir = result.ir();
    assert_eq!(ir.model.features.len(), 1);
    assert_root_owns_every_body(ir);
    assert!(ir.design.scopes.is_empty());
    assert!(ir.design.containers.is_empty());
    assert!(ir.design.features.is_empty());
}

#[test]
fn a_part_with_no_history_states_no_design_record() {
    let result = decode(
        crate::test_support::test_container::standard_catpart(),
        &DecodeOptions::default(),
    );
    let ir = result.ir();
    assert!(ir.model.features.is_empty());
    assert!(ir.model.parameters.is_empty());
    assert!(ir.design.scopes.is_empty());
    assert!(ir.design.parameters.is_empty());
    assert_root_owns_every_body(ir);
}

#[test]
fn a_geometry_decode_states_no_design_record() {
    let options = DecodeOptions {
        depth: cadmpeg_core::decode::DecodeDepth::Geometry,
        ..DecodeOptions::default()
    };
    let result = decode(standard_catpart_with_formula_relation(4, false), &options);
    let ir = result.ir();
    assert!(ir.design.scopes.is_empty());
    assert!(ir.design.parameters.is_empty());
    assert_root_owns_every_body(ir);
}
