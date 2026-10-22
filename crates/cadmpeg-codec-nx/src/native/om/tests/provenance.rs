// SPDX-License-Identifier: Apache-2.0
//! Source owner tokens do not become document identities.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::source_fidelity::b1::provenance::{NativeOwnerResolver, NativeOwnership};
use cadmpeg_ir::CadIr;

struct AttributeOwners;
impl NativeOwnerResolver for AttributeOwners {
    fn visit_owners(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        visitor: &mut dyn FnMut(NativeOwnership) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let namespace = ir.native.namespace("nx").expect("NX namespace");
        for arena in namespace.arenas().keys() {
            if arena != "part_attributes" {
                return Err(CodecError::malformed("unreviewed NX control family"));
            }
            for attribute in
                namespace.arena_iter_as_for_decode::<super::super::PartAttribute>(ctx, arena)
            {
                let attribute = attribute?;
                visitor(NativeOwnership::new(
                    Identity::new(attribute.id).expect("attribute identity"),
                    None,
                ))?;
            }
        }
        Ok(())
    }
}

#[test]
fn native_part_attribute_owner_token_is_not_parsed_as_an_identity() {
    let source = br#"<UgAttributes version="4"><Attribute owner="00000001" title="Title" value="Value" type="string" pdmBased="false" version="1"/></UgAttributes>"#;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).expect("bounded source");
    let run = |ctx: &DecodeContext<'_>| -> Result<(), CodecError> {
        let mut attributes =
            super::super::parse_part_attributes(ctx, source, 0, "/Root/part/attrs", 0)?
                .expect("actual NX attribute grammar");
        assert_eq!(attributes.len(), 1);
        assert_eq!(attributes[0].owner, "00000001");
        let mut ir = CadIr::empty();
        ir.native
            .namespace_mut("nx")
            .set_arena(ctx, "part_attributes", &attributes)?;
        ir.visit_native_provenance_with_owner_resolver(ctx, &AttributeOwners, &mut |record| {
            assert_eq!(record.owner(), None);
            assert_eq!(record.id(), attributes[0].id);
            Ok(())
        })?;
        attributes[0].owner = attributes[0].id.clone();
        ir.native
            .namespace_mut("nx")
            .set_arena(ctx, "part_attributes", &attributes)?;
        ir.visit_native_provenance_with_owner_resolver(ctx, &AttributeOwners, &mut |record| {
            assert_eq!(
                record.owner(),
                None,
                "an identity-shaped source owner token is still a token"
            );
            Ok(())
        })
    };
    run(&ctx).expect("typed native attribute ownership");
}
