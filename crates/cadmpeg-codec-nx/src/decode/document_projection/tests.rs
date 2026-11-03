// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::project;
use cadmpeg_ir::CadIr;
use serde_json::json;

#[test]
fn native_document_projections_keep_authored_fields_and_use_inward_links() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let source = [
        (
            "fast_load_component_prototypes",
            json!({"id":"nx:document:prototype#one","name":"Part A","ordinal":0}),
        ),
        (
            "display_jt_string_property_atoms",
            json!({"id":"nx:display:atom#one","value":"authored label","code_units":[97,98],"ordinal":1}),
        ),
        (
            "display_jt_material_attributes",
            json!({"id":"nx:display:material#one","diffuse":[0.25,0.5,0.75,1.0],"ambient":[0.1,0.2,0.3],"shininess":0.6}),
        ),
    ];
    for (arena, record) in &source {
        ir.native
            .namespace_mut("nx")
            .set_arena(&ctx, *arena, std::slice::from_ref(record))
            .unwrap();
    }
    let before = ir.native.clone();
    let mut losses = Vec::new();
    project(&ctx, &mut ir, &mut losses).unwrap();
    assert!(losses.is_empty());
    assert_eq!(ir.native, before);
    assert_eq!(ir.model.product_definitions.len(), 1);
    assert_eq!(
        ir.model.product_definitions[0].source_name.as_deref(),
        Some("Part A")
    );
    assert_eq!(
        ir.model.product_definitions[0].native_ref.as_deref(),
        Some("nx:document:prototype#one")
    );
    assert!(ir.model.attributes.is_empty());
    assert_eq!(ir.model.appearances.len(), 1);
    assert_eq!(
        ir.model.appearances[0].native_ref.as_deref(),
        Some("nx:display:material#one")
    );
    assert_eq!(
        ir.model.appearances[0].base_color,
        cadmpeg_ir::topology::Color::new(0.25, 0.5, 0.75, 1.0)
    );
    for (arena, _) in &source {
        let record = &ir.native.namespace("nx").unwrap().arenas()[*arena][0];
        for backlink in ["neutral_attribute", "neutral_product", "neutral_appearance"] {
            assert!(record.field(backlink).is_none());
        }
    }
}

#[test]
fn fast_load_occurrences_project_without_placements() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let prototypes = [
        json!({"id":"nx:fast-load:prototype#0","name":"Bracket","ordinal":0}),
        json!({"id":"nx:fast-load:prototype#1","name":"","ordinal":1}),
    ];
    let occurrences = [
        json!({"id":"nx:fast-load:occurrence#7","ordinal":7,"prototype":"nx:fast-load:prototype#0"}),
        json!({"id":"nx:fast-load:occurrence#8","ordinal":8,"prototype":"nx:fast-load:prototype#9"}),
    ];
    ir.native
        .namespace_mut("nx")
        .set_arena(&ctx, "fast_load_component_prototypes", &prototypes)
        .unwrap();
    ir.native
        .namespace_mut("nx")
        .set_arena(&ctx, "fast_load_component_occurrences", &occurrences)
        .unwrap();
    let mut losses = Vec::new();
    project(&ctx, &mut ir, &mut losses).unwrap();
    assert!(losses.is_empty());
    assert_eq!(ir.model.product_definitions.len(), 2);
    assert_eq!(
        ir.model.product_definitions[0].source_name.as_deref(),
        Some("Bracket")
    );
    assert_eq!(ir.model.product_definitions[1].source_name, None);
    assert_eq!(ir.model.occurrences.len(), 2);
    let first = &ir.model.occurrences[0];
    assert!(matches!(
        first.prototype,
        cadmpeg_ir::products::PrototypeReference::Local { .. }
    ));
    assert_eq!(first.ordinal, 7);
    assert_eq!(first.transform, None);
    assert_eq!(
        first.native_ref.as_deref(),
        Some("nx:fast-load:occurrence#7")
    );
    let second = &ir.model.occurrences[1];
    assert!(matches!(
        second.prototype,
        cadmpeg_ir::products::PrototypeReference::Unresolved {}
    ));
    assert_eq!(
        second.native_ref.as_deref(),
        Some("nx:fast-load:occurrence#8")
    );
}

#[test]
fn external_strings_without_lanes_project_as_child_paths() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let strings = [
        json!({"id":"nx:external-reference:entry#0","ordinal":0,"path":"child.prt","source_entry":"entry"}),
        json!({"id":"nx:external-reference:entry#1","ordinal":1,"path":"nested/b.prt","source_entry":"entry"}),
    ];
    ir.native
        .namespace_mut("nx")
        .set_arena(&ctx, "external_references", &strings)
        .unwrap();
    let mut losses = Vec::new();
    project(&ctx, &mut ir, &mut losses).unwrap();
    assert!(losses.is_empty());
    assert_eq!(ir.model.occurrences.len(), 2);
    for (occurrence, path) in ir
        .model
        .occurrences
        .iter()
        .zip(["child.prt", "nested/b.prt"])
    {
        assert!(matches!(
            occurrence.prototype,
            cadmpeg_ir::products::PrototypeReference::External { .. }
        ));
        assert_eq!(occurrence.transform, None);
        assert_eq!(occurrence.name.as_deref(), Some(path));
    }
}

#[test]
fn external_child_lanes_join_directory_and_name() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let strings = [
        json!({"id":"nx:external-reference:entry#0","ordinal":0,"path":"child.prt","source_entry":"entry"}),
        json!({"id":"nx:external-reference:entry#2","ordinal":2,"path":"dirB","source_entry":"entry"}),
        json!({"id":"nx:external-reference:entry#3","ordinal":3,"path":"extra","source_entry":"entry"}),
    ];
    let records = [json!({"id":"nx:external-reference-record:entry#6","source_entry":"entry"})];
    let children = [
        json!({"id":"nx:external-reference-record:entry#6:child","external_record":"nx:external-reference-record:entry#6","name_reference":"nx:external-reference:entry#0","directory_reference":"nx:external-reference:entry#2"}),
    ];
    ir.native
        .namespace_mut("nx")
        .set_arena(&ctx, "external_references", &strings)
        .unwrap();
    ir.native
        .namespace_mut("nx")
        .set_arena(&ctx, "external_reference_records", &records)
        .unwrap();
    ir.native
        .namespace_mut("nx")
        .set_arena(&ctx, "external_reference_record_children", &children)
        .unwrap();
    let mut losses = Vec::new();
    project(&ctx, &mut ir, &mut losses).unwrap();
    assert!(losses.is_empty());
    assert_eq!(ir.model.occurrences.len(), 1);
    let occurrence = &ir.model.occurrences[0];
    match &occurrence.prototype {
        cadmpeg_ir::products::PrototypeReference::External { document, object } => {
            assert_eq!(
                document,
                &cadmpeg_ir::products::ExternalDocument::path("dirB/child.prt")
            );
            assert_eq!(object, &None);
        }
        other => panic!("expected an external prototype, found {other:?}"),
    }
    assert_eq!(occurrence.name.as_deref(), Some("dirB/child.prt"));
    assert_eq!(
        occurrence.native_ref.as_deref(),
        Some("nx:external-reference-record:entry#6:child")
    );
}
