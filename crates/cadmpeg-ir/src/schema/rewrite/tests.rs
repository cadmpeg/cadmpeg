// SPDX-License-Identifier: Apache-2.0
use super::identities;
use super::typed::{IdentityMap, RewriteIdentities};
use crate::ids::PointId;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;

#[derive(Clone, Serialize)]
struct Text(String);

#[derive(Clone, Serialize)]
enum Variant {
    Unit,
    Newtype(PointId),
    Tuple(PointId, String),
    Struct { reference: PointId, text: String },
}

#[derive(Clone, Serialize)]
struct Entity {
    id: PointId,
    label: String,
    optional: Option<PointId>,
    absent: Option<PointId>,
    tuple: (PointId, Text),
    variants: Vec<Variant>,
    references: BTreeMap<PointId, PointId>,
    extensions: BTreeMap<String, String>,
}

rewrite_record!(Text, []; (text));
rewrite_enum!(Variant, []; {
    Unit,
    Newtype(identity),
    Tuple(identity, text),
    Struct {reference, text},
});
rewrite_record!(Entity, []; {id, label, optional, absent, tuple, variants, references, extensions});

fn id(key: &str) -> PointId {
    PointId::mint(format!("test:model:point#{key}")).unwrap()
}

#[test]
fn generic_unknown_links_share_the_typed_identity_rewrite_boundary() {
    let record = crate::NativeUnknownRecord {
        id: crate::ids::UnknownId::mint("test:model:unknown#source").unwrap(),
        links: vec![crate::ids::Identity::new("test:model:point#a").unwrap()],
    };
    let rewritten = serde_json::to_value(
        identities(
            &cadmpeg_test_support::service_decode_context(),
            "test identity rewrite",
            record.clone(),
            |source| Ok(source.replace("test:model:", "test:occurrence:")),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        rewritten,
        serde_json::json!({
            "id": "test:occurrence:unknown#source",
            "links": ["test:occurrence:point#a"]
        })
    );
    assert_eq!(record.links[0].as_str(), "test:model:point#a");
}

#[test]
fn rewriting_distinguishes_identities_from_identical_text_in_every_container() {
    let source = id("a").to_string();
    let entity = Entity {
        id: id("a"),
        label: source.clone(),
        optional: Some(id("a")),
        absent: None,
        tuple: (id("a"), Text(source.clone())),
        variants: vec![
            Variant::Unit,
            Variant::Newtype(id("a")),
            Variant::Tuple(id("a"), source.clone()),
            Variant::Struct {
                reference: id("a"),
                text: source.clone(),
            },
        ],
        references: BTreeMap::from([(id("a"), id("b"))]),
        extensions: BTreeMap::from([(source.clone(), source.clone())]),
    };
    let original = serde_json::to_value(&entity).unwrap();
    let calls = std::cell::Cell::new(0);
    let rewritten = serde_json::to_value(
        identities(
            &cadmpeg_test_support::service_decode_context(),
            "test identity rewrite",
            entity.clone(),
            |source| {
                calls.set(calls.get() + 1);
                Ok(source.replace("test:model:", "test:occurrence:"))
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(
        rewritten,
        serde_json::json!({
            "id": "test:occurrence:point#a", "label": source,
            "optional": "test:occurrence:point#a", "absent": null,
            "tuple": ["test:occurrence:point#a", source],
            "variants": ["Unit", {"Newtype": "test:occurrence:point#a"},
                {"Tuple": ["test:occurrence:point#a", source]},
                {"Struct": {"reference": "test:occurrence:point#a", "text": source}}],
            "references": {"test:occurrence:point#a": "test:occurrence:point#b"},
            "extensions": {"test:model:point#a": "test:model:point#a"}
        })
    );
    assert_eq!(serde_json::to_value(&entity).unwrap(), original);
}

#[test]
fn colliding_map_keys_and_invalid_targets_fail_without_changing_the_source() {
    let source = BTreeMap::from([(id("a"), 1), (id("b"), 2)]);
    let before = serde_json::to_value(&source).unwrap();
    let error = identities(
        &cadmpeg_test_support::service_decode_context(),
        "test identity rewrite",
        source.clone(),
        |_| Ok("test:occurrence:point#same".into()),
    )
    .unwrap_err();
    assert!(error.to_string().contains("test:model:point#b"), "{error}");
    assert!(error.to_string().contains("collides"), "{error}");
    let error = identities(
        &cadmpeg_test_support::service_decode_context(),
        "test identity rewrite",
        source.clone(),
        |_| Ok(String::new()),
    )
    .unwrap_err();
    assert!(error.to_string().contains("invalid identity"), "{error}");
    assert_eq!(serde_json::to_value(&source).unwrap(), before);
}

#[derive(Debug, Clone)]
struct SwallowsElementErrors([PointId; 2]);

impl Serialize for SwallowsElementErrors {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(2))?;
        for id in &self.0 {
            sequence.serialize_element(id).ok();
        }
        sequence.end()
    }
}

impl RewriteIdentities for SwallowsElementErrors {
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        self.0.visit_identity_references(ctx, visitor)
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        for identity in &self.0 {
            identity.clone().rewrite_identities(ctx, map).ok();
        }
        Ok(self)
    }
}

#[test]
fn a_serializer_cannot_swallow_a_rewrite_collision() {
    let source = SwallowsElementErrors([id("a"), id("b")]);
    let error = identities(
        &cadmpeg_test_support::service_decode_context(),
        "test identity rewrite",
        source.clone(),
        |_| Ok("test:occurrence:point#same".into()),
    )
    .unwrap_err();
    assert!(error.to_string().contains("collides"), "{error}");
    assert_eq!(
        serde_json::to_value(&source).unwrap(),
        serde_json::json!(["test:model:point#a", "test:model:point#b"])
    );
}

#[test]
fn a_field_walk_cannot_swallow_a_callback_resource_refusal() {
    let original = cadmpeg_core::decode::ResourceLimit {
        dimension: cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        reason: cadmpeg_core::decode::ResourceFailure::AllocationFailed,
        limit: 4096,
        used: 128,
        additional: 256,
        operation: "test identity callback allocation",
    };
    let source = SwallowsElementErrors([id("a"), id("b")]);
    let calls = std::cell::Cell::new(0);
    let error = identities(
        &cadmpeg_test_support::service_decode_context(),
        "test identity rewrite",
        source,
        |_| {
            calls.set(calls.get() + 1);
            Err(CodecError::ResourceLimit(original))
        },
    )
    .unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit == original));
    assert_eq!(calls.get(), 1);
}

#[derive(Clone, Serialize)]
struct NativeLink {
    id: PointId,
    label: String,
    native_ref: Option<String>,
    geometry_ref: Option<String>,
    endpoint_refs: Vec<String>,
}
rewrite_record!(NativeLink, []; {id, label, native_ref, geometry_ref, endpoint_refs});

#[test]
fn full_fidelity_reference_rewrites_without_rewriting_identical_display_text() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let source = NativeLink {
        id: id("owner"),
        label: "test:native:record#1".into(),
        native_ref: Some("test:native:record#1".into()),
        geometry_ref: Some("test:native:curve#1".into()),
        endpoint_refs: vec!["test:native:point#1".into()],
    };
    let rewritten = identities(&ctx, "native link scope", source.clone(), |source| {
        Ok(source.replace("test:", "scoped:"))
    })
    .unwrap();
    assert_eq!(rewritten.label, "test:native:record#1");
    assert_eq!(
        rewritten.native_ref.as_deref(),
        Some("scoped:native:record#1")
    );
    assert_eq!(
        rewritten.geometry_ref.as_deref(),
        Some("scoped:native:curve#1")
    );
    assert_eq!(rewritten.endpoint_refs, ["scoped:native:point#1"]);
    let mut wire = serde_json::to_value(source).unwrap();
    let mut map = IdentityMap::new(&ctx, "native wire link scope", |source: &str| {
        Ok(source.replace("test:", "scoped:"))
    })
    .unwrap();
    NativeLink::rewrite_native_value(&ctx, &mut wire, &mut map).unwrap();
    map.finish(&ctx).unwrap();
    assert_eq!(wire["native_ref"], "scoped:native:record#1");
    assert_eq!(wire["label"], "test:native:record#1");
    assert_eq!(wire["geometry_ref"], "scoped:native:curve#1");
    assert_eq!(
        wire["endpoint_refs"],
        serde_json::json!(["scoped:native:point#1"])
    );
}
