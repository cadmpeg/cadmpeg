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
    assert_eq!(ir.model.attributes.len(), 1);
    assert_eq!(
        ir.model.attributes[0].native_ref.as_deref(),
        Some("nx:display:atom#one")
    );
    assert_eq!(
        ir.model.attributes[0].values,
        [cadmpeg_ir::attributes::AttributeValue::String(
            "authored label".to_owned()
        )]
    );
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
