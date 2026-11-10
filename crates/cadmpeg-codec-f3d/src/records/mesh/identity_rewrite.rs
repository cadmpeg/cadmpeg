// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignGuidText, DesignMeshBody, DesignMeshBodyWords, DesignMeshCollection,
    DesignMeshCollectionBacklink, DesignMeshCollectionOwner, DesignMeshEntryName,
    DesignMeshFaceGroup, DesignMeshFaceGroups, DesignMeshFeature, DesignMeshFixedRecord,
    DesignMeshFlaggedCollection, DesignMeshFlaggedEntry, DesignMeshGuid, DesignMeshHistoryStream,
    DesignMeshOperationBody, DesignMeshOperationInput, DesignMeshPlacement,
    DesignMeshRecordIdentity, DesignMeshSceneBounds, DesignMeshSceneNode, DesignMeshSceneNodeForm,
    DesignMeshSceneState, DesignMeshScope, DesignMeshSourceRecord, DesignMeshSplitStream,
    DesignMeshTextureFile, DesignMeshTextureResource, DesignMeshTextureTable, DesignMeshUuid,
    DesignOccurrenceKey, DesignRelaxedGuidText, MeshAffineTransform,
};

rewrite_native_record!(DesignGuidText, []; (field0));
rewrite_native_record!(DesignOccurrenceKey, []; (field0));
rewrite_native_record!(DesignMeshFaceGroup, []; {guid});
rewrite_native_record!(DesignMeshFaceGroups, []; {entries});
rewrite_native_record!(DesignMeshSplitStream, []; {name, descriptor});
rewrite_native_record!(DesignMeshHistoryStream, []; {name, encoding, decoded_length});
rewrite_native_record!(DesignMeshSourceRecord, []; {record, type_guid, bytes});
rewrite_native_record!(DesignMeshOperationInput, []; {guid, entry});
rewrite_native_record!(DesignMeshOperationBody, []; {scope, group, member, inputs, body});
rewrite_native_scalar!(DesignMeshBodyWords);
rewrite_native_record!(DesignMeshBody, []; {words, split_streams, history_streams, face_group_metadata, placement, entry, guid, wrapper_record, scene_state, scene_node, scene_auxiliary_record, owner_record, container_mesh_uuid, tessellation_id});
rewrite_native_scalar!(DesignMeshFlaggedEntry);
rewrite_native_record!(DesignMeshFlaggedCollection, []; {entries});
rewrite_native_record!(DesignMeshCollection, []; {record, base_class_tag, flagged_data});
rewrite_native_scalar!(DesignMeshCollectionBacklink);
rewrite_native_record!(DesignMeshCollectionOwner, []; {record, backlink});
rewrite_native_record!(DesignMeshEntryName, []; {record, name});
rewrite_native_record!(DesignMeshFeature, []; {id, scopes, collection, texture_table, collection_owner, bodies, operation_bodies});
impl<const LENGTH: u64> cadmpeg_ir::schema::rewrite::typed::RewriteIdentities
    for DesignMeshFixedRecord<LENGTH>
{
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _value: &mut serde_json::Value,
        _map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        Ok(())
    }

    fn visit_identity_references(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        Ok(())
    }
    fn rewrite_identities<RewriteMapFn: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _map: &mut cadmpeg_ir::schema::rewrite::typed::IdentityMap<'_, RewriteMapFn>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(self)
    }
}

rewrite_native_record!(DesignMeshGuid, []; {record, value});
rewrite_native_record!(DesignMeshPlacement, []; {record, transform});
rewrite_native_record!(DesignMeshRecordIdentity, []; {class_tag, record_index, byte_offset, frame_length});
rewrite_native_scalar!(DesignMeshSceneBounds);
rewrite_native_record!(DesignMeshSceneNode, []; {form, bounds});
rewrite_native_enum!(DesignMeshSceneNodeForm, []; {
    Compact(field0),
    Placed {record, transform},
});
rewrite_native_record!(DesignMeshSceneState, []; {record, bounds});
rewrite_native_record!(DesignMeshScope, []; {record, base_class_tag, owner_record_index});
rewrite_native_record!(DesignMeshTextureFile, []; {record, archive_entry_name});
rewrite_native_record!(DesignMeshTextureResource, []; {ordinal, resource_guid, flags, filename_ordinal, file, asset});
rewrite_native_record!(DesignMeshTextureTable, []; {record, resources});
rewrite_native_record!(DesignMeshUuid, []; (field0));
rewrite_native_record!(DesignRelaxedGuidText, []; (field0));
rewrite_native_scalar!(MeshAffineTransform);
