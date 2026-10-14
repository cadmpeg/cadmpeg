// SPDX-License-Identifier: Apache-2.0
//! Collection charges for shape bindings and body placements.
//!
//! These scoped collections are released before a later, larger charge, so no
//! collection ceiling makes them the refusing charge.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use crate::reader::{decode_exchange, Packaging};

const SHEET_SOURCE: &str = include_str!("../../../../tests/fixtures/ap214_sheet.p21");

fn charged(source: &str, operation: &str) {
    let source = source.as_bytes();
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid product exchange");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("root fits service policy");
    cadmpeg_test_support::refusal::assert_charged(
        ResourceDimension::CollectionItems,
        operation,
        || decode_exchange(source, exchange, &diagnostics, &ctx, Packaging::Bare),
    );
}

fn shape_binding_source() -> String {
    SHEET_SOURCE.replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "#80=APPLICATION_CONTEXT('mechanical design');\n#81=PRODUCT_CONTEXT('',#80,'mechanical');\n#82=PRODUCT('P','Shape part','',(#81));\n#83=PRODUCT_DEFINITION_FORMATION('','',#82);\n#84=PRODUCT_DEFINITION_CONTEXT('part definition',#80,'design');\n#85=PRODUCT_DEFINITION('part','',#83,#84);\n#86=PRODUCT_DEFINITION_SHAPE('','',#85);\n#87=SHAPE_DEFINITION_REPRESENTATION(#86,#32);\nENDSEC;\nEND-ISO-10303-21;",
    )
}

#[test]
fn body_placement_indices_charge_collection_items() {
    charged(SHEET_SOURCE, "step_body_placement_indices");
}

#[test]
fn shape_binding_groups_charge_collection_items() {
    charged(&shape_binding_source(), "step_shape_binding_groups");
}

#[test]
fn shape_binding_bodies_charge_collection_items() {
    charged(&shape_binding_source(), "step_shape_binding_bodies");
}

#[test]
fn body_placement_groups_charge_collection_items() {
    charged(
        &super::mapped_body_placement_source(),
        "step_body_placement_groups",
    );
}

#[test]
fn body_placement_group_members_charge_collection_items() {
    charged(
        &super::mapped_body_placement_source(),
        "step_body_placement_group_members",
    );
}

#[test]
fn unique_body_placements_charge_collection_items() {
    charged(
        &super::mapped_body_placement_source(),
        "step_unique_body_placements",
    );
}
