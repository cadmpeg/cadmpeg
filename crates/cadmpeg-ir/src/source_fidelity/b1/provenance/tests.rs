// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
use serde_json::{json, Value};

use super::*;
use crate::ids::{ProductDefinitionId, UnknownId};
use crate::native::NativeRecord;
use crate::products::{ProductDefinition, ProductDefinitionKind};
use crate::unknown::NativeUnknownRecord;

const SHARED: &str = "test:source:record#shared";

fn product(id: &str, native_ref: Option<&str>) -> ProductDefinition {
    ProductDefinition {
        id: ProductDefinitionId::mint(id).expect("valid product identity"),
        kind: ProductDefinitionKind::Part,
        source_name: Some(SHARED.into()),
        label: None,
        description: None,
        part_number: None,
        bom_properties: BTreeMap::new(),
        bodies: Vec::new(),
        native_ref: native_ref.map(str::to_owned),
    }
}

fn native(id: &str, fields: Value) -> NativeRecord {
    let Value::Object(fields) = fields else {
        panic!("test native fields must be an object");
    };
    NativeRecord::new(Identity::new(id).expect("valid native identity"), fields)
        .expect("valid native fields")
}

struct FixtureOwners;

impl NativeOwnerResolver for FixtureOwners {
    fn visit_owners(
        &self,
        _ctx: &DecodeContext<'_>,
        ir: &CadIr,
        visitor: &mut dyn FnMut(NativeOwnership) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        for namespace in ir.native.0.values() {
            for (arena, records) in namespace.arenas() {
                if arena == "unknowns" {
                    continue;
                }
                for record in records {
                    let owner = serde_json::from_value::<Option<Identity>>(
                        record.field("owner").unwrap_or(Value::Null),
                    )
                    .map_err(|error| {
                        CodecError::malformed(format_args!(
                            "fixture {} owner: {error}",
                            record.id()
                        ))
                    })?;
                    visitor(NativeOwnership::new(
                        Identity::new(record.id()).expect("fixture record identity"),
                        owner,
                    ))?;
                }
            }
        }
        Ok(())
    }
}

#[test]
fn owner_classes_use_the_neutral_registry_and_exact_reverse_links() {
    let mut ir = CadIr::empty();
    let product_id = "test:model:product#own";
    let config_id = "test:design:configuration#own";
    ir.model
        .product_definitions
        .push(product(product_id, Some(SHARED)));
    ir.model.configurations.push(
        serde_json::from_value(json!({"id":config_id,"native_ref":SHARED}))
            .expect("valid neutral configuration"),
    );

    assert_eq!(
        ir.provenance_classes(product_id),
        ArenaClasses::from([ArenaClass::Model])
    );
    assert_eq!(
        ir.provenance_classes(config_id),
        ArenaClasses::from([ArenaClass::Design])
    );
    assert_eq!(
        ir.provenance_classes(SHARED),
        ArenaClasses::from([ArenaClass::Model, ArenaClass::Design])
    );
    assert!(ir.provenance_classes("test:source:record#share").is_empty());

    let mut records = BTreeMap::new();
    ir.visit_neutral_provenance(&mut |record| {
        records.insert(
            record.id(),
            (record.kind(), record.class(), record.native_ref()),
        );
        Ok(())
    })
    .expect("visit borrowed neutral links");
    assert_eq!(
        records[product_id],
        (
            EntityKind::ProductDefinition,
            ArenaClass::Model,
            Some(SHARED)
        )
    );
    assert_eq!(
        records[config_id],
        (
            EntityKind::DesignConfiguration,
            ArenaClass::Design,
            Some(SHARED)
        )
    );

    ir.model.product_definitions[0].native_ref = None;
    ir.model.configurations[0].native_ref = None;
    assert!(
        ir.provenance_classes(SHARED).is_empty(),
        "an identity-shaped source name is not a native link"
    );
    assert_eq!(
        ir.provenance_classes(product_id),
        ArenaClasses::from([ArenaClass::Model])
    );
}

#[test]
fn every_neutral_arena_has_a_required_typed_provenance_accessor() {
    fn arena_kind<T: EntitySchema>(_: &[T]) -> EntityKind {
        T::KIND
    }
    let model = crate::document::Model::default();
    macro_rules! kinds {
        ($($field:ident: $ty:ty, $class:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
            [$(arena_kind(&model.$field)),*]
        };
    }
    assert_eq!(
        crate::document::classified_arena_registry!(kinds),
        EntityKind::ALL
    );
    let ir = crate::examples::unit_cube().expect("valid cube");
    let mut ids = Vec::new();
    ir.visit_neutral_provenance(&mut |record| {
        assert_eq!(record.class(), ArenaClass::Model);
        assert_eq!(record.native_ref(), None);
        ids.push(record.id());
        Ok(())
    })
    .expect("visit all populated cube arenas");
    assert_eq!(
        ids.len(),
        ir.model.bodies.len()
            + ir.model.regions.len()
            + ir.model.shells.len()
            + ir.model.faces.len()
            + ir.model.loops.len()
            + ir.model.coedges.len()
            + ir.model.edges.len()
            + ir.model.vertices.len()
            + ir.model.points.len()
            + ir.model.surfaces.len()
            + ir.model.curves.len()
    );
}

#[test]
fn native_owners_are_direct_and_do_not_infer_classes_from_other_fields() {
    let mut ir = CadIr::empty();
    let parent = "test:source:record#parent";
    let child = "test:source:record#child";
    let grandparent = "test:source:record#grandparent";
    ir.model
        .product_definitions
        .push(product("test:model:product#p", Some(grandparent)));
    ir.model.configurations.push(
        serde_json::from_value(json!({
            "id":"test:design:configuration#c", "native_ref":parent
        }))
        .expect("valid configuration"),
    );
    ir.native.namespace_mut("test").arenas_mut().insert(
        "objects".into(),
        vec![
            native(
                child,
                json!({"owner":parent,"label":grandparent,"properties":{"owner":grandparent}}),
            ),
            native(parent, json!({"owner":grandparent,"geometry":[1,2,3]})),
            native(grandparent, json!({"owner":null,"name":child})),
        ],
    );
    ir.native.namespace_mut("foreign").arenas_mut().insert(
        "other".into(),
        vec![native(
            "foreign:source:record#independent",
            json!({"name":parent}),
        )],
    );
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), DecodeDepth::Full);
    let mut owners = BTreeMap::new();
    ir.visit_native_provenance_with_owner_resolver(&ctx, &FixtureOwners, &mut |record| {
        assert!(record.unknown_links().is_empty());
        assert_eq!(record.identity_only(), None);
        owners.insert(
            record.id(),
            (
                record.namespace(),
                record.arena(),
                record.owner().map(str::to_owned),
            ),
        );
        Ok(())
    })
    .expect("visit every native arena");
    assert_eq!(owners.len(), 4);
    assert_eq!(owners[child], ("test", "objects", Some(parent.into())));
    assert_eq!(owners[parent].2.as_deref(), Some(grandparent));
    assert_eq!(owners[grandparent].2, None);
    assert!(ir.provenance_classes(child).is_empty());
    assert_eq!(
        ir.provenance_classes(owners[child].2.as_deref().expect("direct owner")),
        ArenaClasses::from([ArenaClass::Design])
    );
    assert_eq!(
        ir.provenance_classes(grandparent),
        ArenaClasses::from([ArenaClass::Model])
    );
}

#[test]
fn native_owner_shape_and_identity_are_validated() {
    for owner in [
        json!(3),
        json!([SHARED]),
        json!({"id":SHARED}),
        json!("bad identity"),
    ] {
        let mut ir = CadIr::empty();
        ir.native.namespace_mut("test").arenas_mut().insert(
            "objects".into(),
            vec![native(SHARED, json!({"owner":owner}))],
        );
        let arena = DecodeArena::new();
        let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), DecodeDepth::Full);
        let error = ir
            .visit_native_provenance_with_owner_resolver(&ctx, &FixtureOwners, &mut |_| Ok(()))
            .expect_err("invalid owner must refuse");
        assert!(error.to_string().contains(SHARED));
        assert!(error.to_string().contains("owner"));
    }
}

#[test]
fn unknown_stubs_expose_only_their_admitted_links() {
    let mut ir = CadIr::empty();
    let unknown = NativeUnknownRecord {
        id: UnknownId::mint("test:source:unknown#0").expect("valid unknown identity"),
        links: vec![Identity::new(SHARED).expect("valid linked identity")],
    };
    ir.native
        .namespace_mut("test")
        .arenas_mut()
        .insert("unknowns".into(), vec![NativeRecord::from(&unknown)]);
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), DecodeDepth::Full);
    ir.visit_native_provenance(&ctx, &mut |record| {
        assert_eq!(record.id(), unknown.id.as_str());
        assert_eq!(record.unknown_links(), unknown.links);
        assert_eq!(record.owner(), None);
        assert_eq!(record.identity_only(), Some(true));
        Ok(())
    })
    .expect("read typed unknown stub");
    ir.native
        .namespace_mut("test")
        .arenas_mut()
        .get_mut("unknowns")
        .expect("unknown arena")[0] = native(
        unknown.id.as_str(),
        json!({"links":[SHARED],"payload":"unclassified"}),
    );
    assert!(
        ir.visit_native_provenance(&ctx, &mut |_| Ok(())).is_err(),
        "a raw payload must not become an identity-only typed stub"
    );
}

#[test]
fn visitor_refusal_stops_before_later_records() {
    let mut ir = CadIr::empty();
    ir.model
        .product_definitions
        .push(product("test:model:product#0", None));
    ir.model
        .product_definitions
        .push(product("test:model:product#1", None));
    let mut visits = 0;
    let error = ir
        .visit_neutral_provenance(&mut |_| {
            visits += 1;
            Err(CodecError::malformed("visitor stopped"))
        })
        .expect_err("propagate callback refusal");
    assert_eq!(visits, 1);
    assert_eq!(
        error.to_string(),
        CodecError::malformed("visitor stopped").to_string()
    );
}

#[test]
fn native_rows_cannot_silently_skip_owner_resolution() {
    let mut ir = CadIr::empty();
    ir.native.namespace_mut("test").arenas_mut().insert(
        "objects".into(),
        vec![native(SHARED, json!({"owner":null}))],
    );
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), DecodeDepth::Full);
    let error = ir
        .visit_native_provenance(&ctx, &mut |_| Ok(()))
        .expect_err("non-unknown rows need their codec's typed resolver");
    assert!(error.to_string().contains(SHARED));
    assert!(error
        .to_string()
        .contains("requires one typed owner resolution"));
}

#[test]
fn owner_resolution_refuses_missing_targets_and_duplicate_or_missing_rows() {
    struct Rows(Vec<NativeOwnership>);
    impl NativeOwnerResolver for Rows {
        fn visit_owners(
            &self,
            _: &DecodeContext<'_>,
            _: &CadIr,
            visitor: &mut dyn FnMut(NativeOwnership) -> Result<(), CodecError>,
        ) -> Result<(), CodecError> {
            for row in &self.0 {
                visitor(row.clone())?;
            }
            Ok(())
        }
    }
    let mut ir = CadIr::empty();
    let other = "test:source:record#other";
    ir.native.namespace_mut("test").arenas_mut().insert(
        "objects".into(),
        vec![
            native(SHARED, json!({"owner":null})),
            native(other, json!({"owner":null})),
        ],
    );
    let id = |text| Identity::new(text).expect("control identity");
    for rows in [
        vec![],
        vec![NativeOwnership::new(id(SHARED), None)],
        vec![
            NativeOwnership::new(id(SHARED), None),
            NativeOwnership::new(id(SHARED), None),
        ],
        vec![
            NativeOwnership::new(id(SHARED), Some(id("test:source:record#missing"))),
            NativeOwnership::new(id(other), None),
        ],
        vec![
            NativeOwnership::new(id(SHARED), None),
            NativeOwnership::new(id("foreign:source:record#extra"), None),
        ],
    ] {
        let arena = DecodeArena::new();
        let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), DecodeDepth::Full);
        let mut visits = 0;
        assert!(ir
            .visit_native_provenance_with_owner_resolver(&ctx, &Rows(rows), &mut |_| {
                visits += 1;
                Ok(())
            })
            .is_err());
        assert_eq!(
            visits, 0,
            "validate complete ownership before exposing any fact"
        );
    }
}

#[test]
fn resolved_owner_can_be_an_actual_neutral_record() {
    struct NeutralOwner;
    impl NativeOwnerResolver for NeutralOwner {
        fn visit_owners(
            &self,
            _: &DecodeContext<'_>,
            ir: &CadIr,
            visitor: &mut dyn FnMut(NativeOwnership) -> Result<(), CodecError>,
        ) -> Result<(), CodecError> {
            visitor(NativeOwnership::new(
                Identity::new(SHARED).expect("native identity"),
                Some(
                    Identity::new(ir.model.product_definitions[0].id.as_str())
                        .expect("neutral identity"),
                ),
            ))
        }
    }
    let mut ir = CadIr::empty();
    ir.model
        .product_definitions
        .push(product("test:model:product#owner", None));
    ir.native
        .namespace_mut("test")
        .arenas_mut()
        .insert("objects".into(), vec![native(SHARED, json!({}))]);
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), DecodeDepth::Full);
    ir.visit_native_provenance_with_owner_resolver(&ctx, &NeutralOwner, &mut |record| {
        assert_eq!(record.owner(), Some("test:model:product#owner"));
        assert_eq!(
            ir.provenance_classes(record.owner().expect("resolved owner")),
            ArenaClasses::from([ArenaClass::Model])
        );
        Ok(())
    })
    .expect("actual neutral owner exists in the same document");
}
