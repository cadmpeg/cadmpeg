// SPDX-License-Identifier: Apache-2.0
//! Typed ownership of persisted `FreeCAD` object properties.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::source_fidelity::b1::provenance::{NativeOwnerResolver, NativeOwnership};
use cadmpeg_ir::CadIr;

struct PersistenceOwners;
impl NativeOwnerResolver for PersistenceOwners {
    fn visit_owners(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        visitor: &mut dyn FnMut(NativeOwnership) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let namespace = ir.native.namespace("fcstd").expect("FreeCAD namespace");
        for arena in namespace.arenas().keys() {
            match arena.as_str() {
                "objects" => {
                    for object in namespace
                        .arena_iter_as_for_decode::<crate::native::ObjectRecord>(ctx, arena)
                    {
                        let object = object?;
                        visitor(NativeOwnership::new(
                            Identity::new(object.id()).expect("object identity"),
                            None,
                        ))?;
                    }
                }
                "properties" => {
                    for property in namespace
                        .arena_iter_as_for_decode::<crate::native::PropertyRecord>(ctx, arena)
                    {
                        let property = property?;
                        let owner = Identity::new(property.owner).map_err(|error| {
                            CodecError::malformed(format_args!("property owner: {error}"))
                        })?;
                        visitor(NativeOwnership::new(
                            Identity::new(property.id).expect("property identity"),
                            Some(owner),
                        ))?;
                    }
                }
                _ => {
                    return Err(CodecError::malformed(
                        "persistence control has an unreviewed native family",
                    ))
                }
            }
        }
        Ok(())
    }
}

#[test]
fn persisted_property_owner_is_the_actual_object_identity_not_its_source_name() {
    let source = r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="Pad" type="PartDesign::Pad"/></Objects><ObjectData Count="1"><Object name="Pad"><Properties Count="1"><Property name="Length" type="App::PropertyLength"><Float value="5"/></Property></Properties></Object></ObjectData></Document>"#;
    crate::test_support::with_service_context(source.as_bytes(), |ctx| -> Result<(), CodecError> {
        let xml = crate::container::admit_document(ctx, source.as_bytes())?;
        let graph =
            crate::persistence::parse_document(&xml, crate::persistence::Vocabulary::Objects)?;
        assert_eq!(graph.objects[0].name(), "Pad");
        assert_eq!(graph.properties[0].owner, graph.objects[0].id().as_str());
        let object_id = graph.objects[0].id().clone();
        let property_id = graph.properties[0].id.clone();
        let mut ir = CadIr::empty();
        ir.native
            .namespace_mut("fcstd")
            .set_arena(ctx, "objects", &graph.objects)?;
        ir.native
            .namespace_mut("fcstd")
            .set_arena(ctx, "properties", &graph.properties)?;
        let mut seen = 0;
        ir.visit_native_provenance_with_owner_resolver(ctx, &PersistenceOwners, &mut |record| {
            if record.id() == property_id {
                assert_eq!(record.owner(), Some(object_id.as_str()));
                seen += 1;
            }
            Ok(())
        })?;
        assert_eq!(seen, 1);
        let mut properties = graph.properties;
        properties[0].owner = "Pad".into();
        ir.native
            .namespace_mut("fcstd")
            .set_arena(ctx, "properties", &properties)?;
        assert!(
            ir.visit_native_provenance_with_owner_resolver(
                ctx,
                &PersistenceOwners,
                &mut |_| Ok(())
            )
            .is_err(),
            "a raw name is not an admitted record identity"
        );
        Ok(())
    })
    .expect("typed persistence owner proof");
}
