// SPDX-License-Identifier: Apache-2.0
//! Tagged owner roles resolve through typed design-object associations.

use crate::native::{CatiaDesignObject, CatiaObjectOwner, CatiaObjectRecord};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::source_fidelity::b1::provenance::{NativeOwnerResolver, NativeOwnership};
use cadmpeg_ir::CadIr;

struct ObjectOwners;
impl NativeOwnerResolver for ObjectOwners {
    fn visit_owners(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        visitor: &mut dyn FnMut(NativeOwnership) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let namespace = ir.native.namespace("catia").expect("CATIA namespace");
        for arena in namespace.arenas().keys() {
            match arena.as_str() {
                "design_objects" => {
                    for object in
                        namespace.arena_iter_as_for_decode::<CatiaDesignObject>(ctx, arena)
                    {
                        let object = object?;
                        visitor(NativeOwnership::new(
                            Identity::new(object.id).expect("object identity"),
                            None,
                        ))?;
                    }
                }
                "object_graph_records" => {
                    for record in
                        namespace.arena_iter_as_for_decode::<CatiaObjectRecord>(ctx, arena)
                    {
                        let record = record?;
                        let owner = match record.owner {
                            Some(CatiaObjectOwner::Entity(entity)) => {
                                let mut selected = None;
                                for object in namespace
                                    .arena_iter_as_for_decode::<CatiaDesignObject>(
                                        ctx,
                                        "design_objects",
                                    )
                                {
                                    let object = object?;
                                    if object.owner_entity_id == entity
                                        && object.parent == record.parent
                                    {
                                        if selected.replace(object.id).is_some() {
                                            return Err(CodecError::malformed(
                                                "ambiguous typed object owner",
                                            ));
                                        }
                                    }
                                }
                                if selected != record.design_object {
                                    return Err(CodecError::malformed(
                                        "typed object owner disagrees with design-object link",
                                    ));
                                }
                                selected
                            }
                            Some(CatiaObjectOwner::UnassignedLiteral(_)) | None => None,
                        }
                        .map(Identity::new)
                        .transpose()
                        .map_err(|error| {
                            CodecError::malformed(format_args!("object owner: {error}"))
                        })?;
                        visitor(NativeOwnership::new(
                            Identity::new(record.id).expect("record identity"),
                            owner,
                        ))?;
                    }
                }
                _ => return Err(CodecError::malformed("unreviewed CATIA control family")),
            }
        }
        Ok(())
    }
}

#[test]
fn tagged_object_owner_uses_its_actual_design_object_link() {
    let owner_id = "catia:native:design-object#7";
    let object = CatiaDesignObject {
        id: owner_id.into(),
        parent: "catia:native:graph#0".into(),
        ordinal: 0,
        first_field_byte_offset: 0,
        owner_entity_id: 7,
        owner_record: None,
        owner_design_object: None,
        owner_class: None,
        owner_storage_ref: None,
        fields: Vec::new(),
        field_classes: Vec::new(),
        definition_values: Vec::new(),
        definition_chain_values: Vec::new(),
        relations: Vec::new(),
        parallel_reference_table: None,
    };
    let record = |id: &str, owner, design_object| CatiaObjectRecord {
        id: id.into(),
        parent: "catia:native:graph#0".into(),
        design_object,
        entity: None,
        ordinal: 0,
        byte_offset: 0,
        byte_len: 0,
        lead: 0x14,
        head: Vec::new(),
        inline_body: None,
        owner,
        class: None,
        storage: None,
        payload: crate::object_graph::ObjectPayload {
            size: 0,
            fields: Vec::new(),
        },
        repeated_reference_schema_selection: None,
        references: Vec::new(),
    };
    let records = [
        record(
            "catia:native:object#0",
            Some(CatiaObjectOwner::Entity(7)),
            Some(owner_id.into()),
        ),
        record(
            "catia:native:object#1",
            Some(CatiaObjectOwner::UnassignedLiteral(7)),
            None,
        ),
    ];
    crate::test_support::with_service_context(|ctx| -> Result<(), CodecError> {
        let mut ir = CadIr::empty();
        ir.native
            .namespace_mut("catia")
            .set_arena(ctx, "design_objects", &[object])?;
        ir.native
            .namespace_mut("catia")
            .set_arena(ctx, "object_graph_records", &records)?;
        let mut seen = 0;
        ir.visit_native_provenance_with_owner_resolver(ctx, &ObjectOwners, &mut |record| {
            if record.id() == "catia:native:object#0" {
                assert_eq!(record.owner(), Some(owner_id));
                seen += 1;
            }
            if record.id() == "catia:native:object#1" {
                assert_eq!(record.owner(), None);
                seen += 1;
            }
            Ok(())
        })?;
        assert_eq!(seen, 2);
        Ok(())
    })
    .expect("typed tagged owners");
}
