// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use crate::attributes::{AttributeTarget, SourceAttribute};
use crate::document::ArenaName;
use crate::report::check::Check;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;

#[test]
fn record_target_wire_names_only_registered_neutral_arenas() {
    let target = AttributeTarget::Record {
        arena: ArenaName::parse("bodies").unwrap(),
        id: "test:model:body#one".to_owned(),
    };
    let wire = json!({"kind":"record","id":{"arena":"bodies","id":"test:model:body#one"}});
    assert_eq!(serde_json::to_value(&target).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<AttributeTarget>(wire).unwrap(),
        target
    );
    for arena in [
        json!("fcstd.objects"),
        json!({"kind":"native","namespace":"fcstd","name":"objects"}),
    ] {
        assert!(serde_json::from_value::<AttributeTarget>(
            json!({"kind":"record","id":{"arena":arena,"id":"test:model:body#one"}})
        )
        .is_err());
    }
    assert!(serde_json::from_str::<AttributeTarget>(
        r#"{"kind":"record","id":{"arena":"bodies","arena":"faces","id":"test:model:body#one"}}"#
    )
    .is_err());
    assert!(serde_json::from_value::<AttributeTarget>(json!({"kind":"record","id":{"arena":"bodies","id":"test:model:body#one","alias":"fcstd:document:object#one"}})).is_err());
}

#[test]
fn record_target_validation_does_not_use_global_identity_lookup() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let body = ir.model.bodies[0].id.to_string();
    ir.model.attributes.push(SourceAttribute {
        id: "test:model:attribute#one".try_into().unwrap(),
        native_ref: None,
        target: AttributeTarget::Record {
            arena: ArenaName::parse("bodies").unwrap(),
            id: body.clone(),
        },
        name: "owner".to_owned(),
        values: vec![],
    });
    let report = crate::validate::validate_neutral(&ir, vec![]).unwrap();
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.check == Check::ReferentialIntegrity));
    ir.model.attributes[0].target = AttributeTarget::Record {
        arena: ArenaName::parse("faces").unwrap(),
        id: body,
    };
    let report = crate::validate::validate_neutral(&ir, vec![]).unwrap();
    let references = report
        .findings
        .iter()
        .filter(|finding| finding.check == Check::ReferentialIntegrity)
        .collect::<Vec<_>>();
    assert_eq!(references.len(), 1);
    assert_eq!(
        references[0].entity.as_deref(),
        Some("test:model:attribute#one")
    );
}

#[test]
fn actual_record_geometry_target_keeps_display_and_visibility_semantics() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let body = ir.model.bodies[0].id.clone();
    ir.model.bodies[0].color = None;
    let appearance: Appearance = serde_json::from_value(
        json!({"id":"test:model:appearance#one","base_color":{"r":0.25,"g":0.5,"b":0.75,"a":1.0}}),
    )
    .unwrap();
    let expected = appearance.base_color;
    ir.model.appearances.push(appearance);
    let target = AppearanceTarget::Record {
        arena: ArenaName::parse("bodies").unwrap(),
        id: body.to_string(),
    };
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:model:appearance-binding#one".try_into().unwrap(),
        native_ref: None,
        target: target.clone(),
        appearance: "test:model:appearance#one".try_into().unwrap(),
        source_entity_id: None,
        object_type: None,
        visible: Some(false),
        channels: std::collections::BTreeMap::default(),
    });
    assert_eq!(ir.effective_display_color(&target), expected);
    assert_eq!(
        ir.effective_display_color(&AppearanceTarget::Body(body.clone())),
        expected
    );
    assert_eq!(ir.effective_visibility(&target), Some(false));
    assert_eq!(
        ir.effective_visibility(&AppearanceTarget::Body(body.clone())),
        Some(false)
    );
    let wrong = AppearanceTarget::Record {
        arena: ArenaName::parse("faces").unwrap(),
        id: body.to_string(),
    };
    assert!(wrong.typed_record_target(&ir.model).is_none());
}

fn optional_link<T: DeserializeOwned + Serialize>(mut value: serde_json::Value) {
    let absent = serde_json::from_value::<T>(value.clone()).unwrap();
    assert!(serde_json::to_value(absent)
        .unwrap()
        .get("native_ref")
        .is_none());
    value["native_ref"] = json!("fcstd:document:object#one");
    let linked = serde_json::from_value::<T>(value.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(linked).unwrap()["native_ref"],
        value["native_ref"]
    );
    value["native_ref"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<T>(value).is_err());
}

#[test]
fn six_native_links_preserve_authored_absence_and_reject_explicit_null() {
    optional_link::<Appearance>(json!({"id":"test:model:appearance#one"}));
    optional_link::<AppearanceBinding>(
        json!({"id":"test:model:appearance-binding#one","target":{"kind":"body","id":"test:model:body#one"},"appearance":"test:model:appearance#one"}),
    );
    optional_link::<SourceAttribute>(
        json!({"id":"test:model:attribute#one","target":{"kind":"document"},"name":"producer","values":[]}),
    );
    optional_link::<crate::presentation::PresentationLayer>(
        json!({"id":"test:model:layer#one","name":"Layer"}),
    );
    optional_link::<crate::pmi::PmiAnnotation>(
        json!({"id":"test:model:pmi#one","definition":{"kind":"datum","identification":"A"}}),
    );
    let mesh = crate::tessellation::Tessellation::new(
        "test:model:mesh#one".try_into().unwrap(),
        crate::tessellation::TessellationMesh::List {
            vertices: vec![],
            triangles: vec![],
        },
        vec![],
    )
    .unwrap();
    optional_link::<crate::tessellation::Tessellation>(serde_json::to_value(mesh).unwrap());
}

#[test]
fn record_identity_rewrites_while_arena_registry_name_stays_fixed() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    let ctx = cadmpeg_test_support::service_decode_context();
    let target = AppearanceTarget::Record {
        arena: ArenaName::parse("bodies").unwrap(),
        id: "test:model:body#one".to_owned(),
    };
    let mut references = Vec::new();
    target
        .visit_identity_references(&ctx, &mut |id| {
            references.push(id.to_owned());
            Ok(())
        })
        .unwrap();
    assert_eq!(references, ["test:model:body#one"]);
    let mut map =
        IdentityMap::new(|source: &str| Ok(source.replace("test:model:", "test:instance:")));
    let mut wire = serde_json::to_value(&target).unwrap();
    <AppearanceTarget as RewriteIdentities>::rewrite_native_value(&ctx, &mut wire, &mut map)
        .unwrap();
    let rewritten = target.rewrite_identities(&ctx, &mut map).unwrap();
    assert_eq!(wire, serde_json::to_value(rewritten).unwrap());
    assert_eq!(wire["id"]["arena"], "bodies");
    assert_eq!(wire["id"]["id"], "test:instance:body#one");
}
