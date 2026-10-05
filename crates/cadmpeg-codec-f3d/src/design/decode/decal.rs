// SPDX-License-Identifier: Apache-2.0
//! Parse exact raster and face bindings owned by Design `Decal` scopes.

use crate::bytes::lp_utf16_bounded_charged;
use crate::container::ContainerScan;
use crate::design::decode::byte_fields::zeros_at;
use crate::design::decode::image::embedded_image_asset;
use crate::design::decode::record_streams::{has_stream, record_stream};
use crate::design::decode::scopes::shared_frames::marked_reference;
use crate::design::decode::sketch::{
    indexed_record_header_at, next_indexed_record_offset, IndexedRecordHeader, IndexedRecordOffsets,
};

use crate::ids;
use crate::layout::design_decal_image_asset_record as decal_asset;
use crate::layout::design_decal_image_name_prefix as decal_name;
use crate::layout::design_decal_scope_prefix as decal_scope;
use crate::records::{
    decal::{DesignDecalAsset, DesignDecalImage},
    feature::scope::DesignParameterScope,
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
    },
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::Asset;
use cadmpeg_ir::features::{
    DecalMapping, FaceSelection, Feature, FeatureDefinition, FeatureOperation,
};

const DECAL_TARGET_ROLE: crate::records::topology::extrude_selection::DesignOperandRole =
    crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A;

/// Decode every structurally complete Decal image record.
pub(crate) fn decode_decal_images(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignDecalImage>, CodecError> {
    super::image::decode_scoped_images(
        ctx,
        scan,
        scopes,
        &crate::records::feature::scope::DesignFeatureKind::Decal,
        parse_decal_image,
        |image| image.id.as_str(),
    )
}

/// Project exact Decal image and face bindings into neutral features.
pub(crate) fn project_decal_images(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    images: &[DesignDecalImage],
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignBodyRecipeOperand],
    features: &mut [Feature],
) -> Result<Vec<Asset>, CodecError> {
    let mut assets = Vec::new();
    for image in ctx.admit_iter(images, "scan F3D Decal images")? {
        if image.mapping_mode != crate::records::decal::DesignDecalMappingMode::FitToFaces {
            continue;
        }
        let image_stream = record_stream(ctx, &image.id)?;
        let Some(scope) = ctx.find_by(
            scopes,
            |scope| {
                Ok(scope.record_index == image.scope_record_index()
                    && has_stream(ctx, &scope.id, image_stream)?)
            },
            "find F3D Decal image scopes",
        )?
        else {
            continue;
        };
        let Some(group) = ctx.find_by(
            groups,
            |group| {
                Ok(group.scope_record_index == scope.record_index
                    && group.record_index == image.target_group_record_index
                    && group.role() == DECAL_TARGET_ROLE
                    && group.members().len() == 1
                    && has_stream(ctx, &group.id, image_stream)?)
            },
            "find F3D Decal operand groups",
        )?
        else {
            continue;
        };
        let Some(member) = group.members().first() else {
            continue;
        };
        let Some(operand) = ctx.find_by(
            operands,
            |operand| {
                Ok(operand.scope_record_index == scope.record_index
                    && operand.owner.group() == Some((group.record_index, 0))
                    && operand.record_index() == member.value
                    && has_stream(ctx, &operand.id, image_stream)?)
            },
            "find F3D Decal recipe operands",
        )?
        else {
            continue;
        };
        let mut faces = Vec::new();
        for reference in
            ctx.admit_iter(operand.references(), "scan F3D Decal operand references")?
        {
            for face in
                ctx.admit_iter(&reference.candidate_faces, "scan F3D Decal face candidates")?
            {
                let copied = face.try_clone_for_decode(ctx, "f3d Decal face identifier")?;
                ctx.push_vec(&mut faces, copied, "f3d Decal faces")?;
            }
        }
        ctx.stable_sort_by(
            &mut faces[..],
            |value| value.as_str(),
            Ord::cmp,
            "sort f3d design decal 1",
        )?;
        ctx.dedup_vec(&mut faces, "dedup f3d design decal faces")?;
        if faces.is_empty() {
            continue;
        }
        // The asset is retained only when it is the first with its ID.
        let (asset, asset_storage) = ctx.with_scoped_storage("f3d Decal assets", || {
            embedded_image_asset(ctx, scan, image.asset.name())
        })?;
        let Some(asset) = asset else {
            continue;
        };
        let feature_id = crate::design::identity::neutral_feature_id(ctx, scope)?;
        let Some(feature_index) = ctx.position_by(
            features,
            |feature| {
                ctx.equal_bytes(
                    feature.id.as_str().as_bytes(),
                    feature_id.as_str().as_bytes(),
                    "match F3D Decal neutral feature",
                )
            },
            "find F3D Decal neutral feature",
        )?
        else {
            continue;
        };
        let Some(feature) = features.get_mut(feature_index) else {
            continue;
        };
        let asset_id = asset
            .id
            .try_clone_for_decode(ctx, "f3d image feature asset identifier")?;
        let native_id =
            ctx.copy_retained_text(&operand.id, "f3d Decal native operand identifier")?;
        let known_asset = ctx.any_by(
            &assets,
            |candidate: &Asset| {
                ctx.equal_bytes(
                    candidate.id.as_str().as_bytes(),
                    asset_id.as_str().as_bytes(),
                    "match F3D Decal asset",
                )
            },
            "find F3D Decal asset",
        )?;
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Decal {
                asset: asset_id,
                faces: FaceSelection::Resolved {
                    faces,
                    native: native_id,
                },
                mapping: DecalMapping::FitToFaces,
                opacity: None,
            }));
        if !known_asset {
            asset_storage.commit()?;
            ctx.push_vec(&mut assets, asset, "f3d Decal assets")?;
        }
    }
    ctx.stable_sort_by(
        &mut assets[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design decal 2",
    )?;
    Ok(assets)
}

fn parse_decal_image(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    stream: &str,
    scope: &DesignParameterScope,
) -> Result<Option<DesignDecalImage>, CodecError> {
    let Ok(scope_at) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    parse_decal_image_frame(ctx, bytes, records, stream, scope.record_index, scope_at)
}

/// The fixed members of a Decal scope prefix at `scope_at`: the asset record
/// index, the mapping-mode code and the target operand-group record index.
fn decal_scope_prefix(bytes: &[u8], scope_at: usize) -> Option<(u32, u8, u32)> {
    if !zeros_at::<10>(bytes, scope_at + decal_scope::ZERO_RUN_10)
        || !zeros_at::<6>(bytes, scope_at + decal_scope::ASSET_REFERENCE_ZERO_RUN)
        || !zeros_at::<6>(bytes, scope_at + decal_scope::TARGET_REFERENCE_ZERO_RUN)
    {
        return None;
    }
    let asset_record_index = marked_reference(bytes, scope_at + decal_scope::ASSET_REFERENCE)?;
    let mapping_mode = *bytes.get(scope_at + decal_scope::MAPPING_MODE)?;
    let target_group_record_index =
        marked_reference(bytes, scope_at + decal_scope::TARGET_GROUP_REFERENCE)?;
    Some((asset_record_index, mapping_mode, target_group_record_index))
}

/// The Decal image of the scope prefix at `scope_at`. The asset record index
/// it names must carry exactly one complete asset record in the stream, so
/// every header of that index is tested.
fn parse_decal_image_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    stream: &str,
    scope_record_index: u32,
    scope_at: usize,
) -> Result<Option<DesignDecalImage>, CodecError> {
    let Some((asset_record_index, mapping_mode, target_group_record_index)) =
        decal_scope_prefix(bytes, scope_at)
    else {
        return Ok(None);
    };
    let Ok(scope_offset) = u64::try_from(scope_at) else {
        return Ok(None);
    };

    let mut asset_record = None;
    for &asset_at in records.offsets(asset_record_index) {
        ctx.charge_work(1, "scan F3D Decal asset records")?;
        let Some(asset) = indexed_record_header_at(bytes, asset_at) else {
            continue;
        };
        let Some(candidate) = parse_decal_asset_record(ctx, bytes, &asset)? else {
            continue;
        };
        if asset_record.replace(candidate).is_some() {
            return Ok(None);
        }
    }
    let Some(asset_record) = asset_record else {
        return Ok(None);
    };
    let id = ids::native_scoped_id_charged(ctx, stream, "design-decal-image", scope_at)?;
    Ok(DesignDecalImage::new(
        id,
        crate::records::identity::Located {
            value: scope_record_index,
            offset: scope_offset,
        },
        crate::records::decal::DesignDecalMappingMode::from_code(mapping_mode),
        target_group_record_index,
        asset_record,
    )
    .ok())
}

/// The Decal asset record whose indexed header is `asset`, with the name
/// record that directly follows it.
fn parse_decal_asset_record(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    asset: &IndexedRecordHeader<'_>,
) -> Result<Option<DesignDecalAsset>, CodecError> {
    let asset_at = asset.offset;
    if !zeros_at::<8>(bytes, asset_at + decal_asset::ZERO_RUN_8)
        || !zeros_at::<6>(bytes, asset_at + decal_asset::ZERO_RUN_6)
    {
        return Ok(None);
    }
    let Some(asset_entity_suffix) = marked_reference(
        bytes,
        asset_at + decal_asset::DESIGN_ENTITY_SUFFIX_REFERENCE,
    ) else {
        return Ok(None);
    };
    // With both zero runs in place, no indexed header can start inside the
    // fixed asset record, so the next header follows it exactly when one
    // opens at its end.
    let name_at = asset_at + decal_asset::LEN;
    let Some(name) = indexed_record_header_at(bytes, name_at) else {
        return Ok(None);
    };
    if !zeros_at::<10>(bytes, name_at + decal_name::ZERO_RUN_10) {
        return Ok(None);
    }
    let Ok(byte_offset) = u64::try_from(asset_at) else {
        return Ok(None);
    };
    let Some((asset_name, after_asset_name)) = lp_utf16_bounded_charged(
        ctx,
        bytes,
        name_at + decal_name::ASSET_NAME_CODE_UNIT_COUNT,
        1..=1024,
        "f3d Design UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    // The first header after the name prefix must open where the name ends.
    // A header at that offset ends eleven bytes later, so the search reads
    // only the name bytes the decode already paid for and that header.
    let Some(window) = after_asset_name
        .checked_add(11)
        .and_then(|window_end| bytes.get(..window_end))
    else {
        return Ok(None);
    };
    if next_indexed_record_offset(ctx, window, name_at + decal_name::ZERO_RUN_10)?
        != Some(after_asset_name)
    {
        return Ok(None);
    }
    let asset_class_tag = asset.retain_class_tag(ctx, "f3d Decal asset class tag")?;
    let name_class_tag = name.retain_class_tag(ctx, "f3d Decal name class tag")?;

    Ok(DesignDecalAsset::new(
        [String::from(asset_class_tag), String::from(name_class_tag)],
        [asset.record_index, name.record_index],
        byte_offset,
        asset_entity_suffix,
        asset_name,
    )
    .ok())
}

#[cfg(test)]
mod tests {
    use crate::records::decal::{DesignDecalImage, DesignDecalMappingMode};
    use crate::test_support::write_marked_reference;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    /// Parse the scope prefix at `scope_at` against a stream index built
    /// outside `ctx`.
    fn parse_decal_image_frame(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        stream: &str,
        scope_record_index: u32,
        scope_at: usize,
    ) -> Result<Option<DesignDecalImage>, cadmpeg_core::CodecError> {
        let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
        super::parse_decal_image_frame(ctx, bytes, &records, stream, scope_record_index, scope_at)
    }

    fn header(bytes: &mut [u8], at: usize, tag: [u8; 3], index: u32) {
        bytes[at..at + 4].copy_from_slice(&3u32.to_le_bytes());
        bytes[at + 4..at + 7].copy_from_slice(&tag);
        bytes[at + 7..at + 11].copy_from_slice(&index.to_le_bytes());
    }

    fn fixture() -> (Vec<u8>, usize) {
        let mut bytes = vec![0; 240];
        let asset_at = 0;
        let name_at = 30;
        let scope_at = 71;
        let end_at = 200;
        header(&mut bytes, asset_at, *b"258", 17);
        write_marked_reference(&mut bytes, asset_at + 19, 50);
        header(&mut bytes, name_at, *b"279", 18);
        let name = "mark.png".encode_utf16().collect::<Vec<_>>();
        bytes[name_at + 21..name_at + 25]
            .copy_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
        for (ordinal, unit) in name.into_iter().enumerate() {
            let at = name_at + 25 + ordinal * 2;
            bytes[at..at + 2].copy_from_slice(&unit.to_le_bytes());
        }
        header(&mut bytes, scope_at, *b"301", 23);
        write_marked_reference(&mut bytes, scope_at + 21, 17);
        bytes[scope_at + 32] = DesignDecalMappingMode::FitToFaces.code();
        write_marked_reference(&mut bytes, scope_at + 33, 24);
        header(&mut bytes, 150, *b"440", 17);
        header(&mut bytes, end_at, *b"302", 23);
        (bytes, scope_at)
    }

    fn assert_class_tag_retained_refusal(limit: u64, operation: &'static str) {
        let (bytes, scope_at) = fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = parse_decal_image_frame(&ctx, &bytes, "Design/BulkStream.dat", 23, scope_at)
            .err()
            .unwrap();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == operation)
        );
    }

    #[test]
    fn decal_asset_class_tag_refuses_retained_limit() {
        assert_class_tag_retained_refusal(10, "f3d Decal asset class tag");
    }

    #[test]
    fn decal_name_class_tag_refuses_retained_limit() {
        assert_class_tag_retained_refusal(13, "f3d Decal name class tag");
    }

    #[test]
    fn decal_frame_decodes_image_mode_and_target() {
        let (bytes, scope_at) = fixture();
        let image = parse_decal_image_frame(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            "Design/BulkStream.dat",
            23,
            scope_at,
        )
        .unwrap()
        .expect("complete synthetic Decal frame");
        assert_eq!(image.asset.record_index(), 17);
        assert_eq!(image.asset.entity_suffix(), 50);
        assert_eq!(image.asset.name(), "mark.png");
        assert_eq!(
            image.mapping_mode,
            crate::records::decal::DesignDecalMappingMode::FitToFaces
        );
        assert_eq!(image.target_group_record_index, 24);
        assert_eq!(
            crate::records::decal::DesignDecalAsset::primary_frame_length(),
            30
        );
        assert_eq!(image.asset.name_frame_length(), 41);
    }

    #[test]
    fn decal_frame_rejects_an_unframed_name() {
        let (mut bytes, scope_at) = fixture();
        bytes[scope_at..scope_at + 4].fill(0);
        assert!(parse_decal_image_frame(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            "Design/BulkStream.dat",
            23,
            scope_at
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn decal_asset_name_refuses_retained_limit() {
        let (bytes, scope_at) = fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 7;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = parse_decal_image_frame(&ctx, &bytes, "Design/BulkStream.dat", 23, scope_at)
            .err()
            .unwrap();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "f3d Design UTF-16 text")
        );
    }

    #[test]
    fn decal_asset_and_name_searches_refuse_work_limits_through_optional_parse() {
        let (bytes, scope_at) = fixture();
        // Both asset-index headers are tested; the name search reads from the
        // name prefix to the header that ends the name.
        for (operation, skip) in [
            ("scan F3D Decal asset records", 1),
            ("find F3D indexed record header", 0),
        ] {
            let error = crate::test_support::resource_refusal_at(
                ResourceDimension::WorkUnits,
                operation,
                skip,
                |ctx| {
                    parse_decal_image_frame(ctx, &bytes, "Design/BulkStream.dat", 23, scope_at)
                        .map(|_| ())
                },
            );
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(refusal)
                    if refusal.dimension == ResourceDimension::WorkUnits
                        && refusal.operation == operation
            ));
        }
    }

    #[test]
    fn decal_projection_refuses_face_asset_native_and_output_limits() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_ir::features::{
            Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation, SketchFeatureBinding,
        };
        use std::collections::BTreeMap;
        use std::io::{Cursor, Write};
        use zip::CompressionMethod;

        const STREAM: &str = "Design/BulkStream.dat";
        const ENTRY: &str = "FusionAssetName[Active]/Design1/Images.BlobParts/mark.png";
        let (bytes, scope_at) = fixture();
        let image = parse_decal_image_frame(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            STREAM,
            23,
            scope_at,
        )
        .unwrap()
        .unwrap();
        let scope = crate::records::feature::scope::DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#23",
            crate::records::feature::scope::DesignFeatureKind::Decal,
            23,
        );
        let group = crate::records::topology::construction::DesignConstructionOperandGroup::try_from(
            crate::records::topology::construction::DesignConstructionOperandGroupDraft {
                id: "f3d:Design/BulkStream.dat:operand-group#24".into(),
                scope_record_index: 23,
                scope_reference_ordinal: 0,
                record_index: 24,
                byte_offset: 900,
                class_tag: crate::records::references::DesignClassTag::try_from("269".to_owned()).unwrap(),
                members: vec![crate::records::identity::Located { value: 100, offset: 926 }],
                lost_edge_references: Vec::new(),
                frame: crate::records::topology::construction::DesignConstructionOperandGroupFrame::try_from(
                    crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                        member_count_offset: 921,
                        auxiliary_records: Vec::new(),
                        auxiliary_paths: Vec::new(),
                        trailing_records: vec![crate::records::identity::Located { value: 200, offset: 943 }],
                        trailing_transforms: Vec::new(),
                        trailing_dual_transforms: Vec::new(),
                        trailing_flags: Vec::new(),
                        opaque_index: 1,
                        opaque_index_offset: 971,
                        opaque_scalar: 0.0,
                        opaque_scalar_offset: 975,
                        variant: false,
                    },
                ).unwrap(),
                operand_role: crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A,
                ),
                role_offset: 953,
                paired_class_tag: crate::records::references::DesignClassTag::try_from("265".to_owned()).unwrap(),
                paired_byte_offset: 1024,
            },
        ).unwrap();
        let face = cadmpeg_ir::ids::FaceId::mint("test:model:face#1").unwrap();
        let operand = crate::records::topology::body_recipe::DesignBodyRecipeOperand::try_new(
            crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
                id: "f3d:Design/BulkStream.dat:body-operand#100".into(),
                scope_record_index: 23,
                owner: crate::records::topology::body_recipe::DesignOperandOwner::Group {
                    group_record_index: 24,
                    group_member_ordinal: 0,
                },
                record_index: 100,
                byte_offset: 1000,
                class_tag: crate::records::references::DesignClassTag::try_from("365".to_owned())
                    .unwrap(),
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 1056,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "11111111-2222-4333-8444-555555555555".to_owned(),
                )
                .unwrap(),
                context_id_offset: 1090,
                selector_tail: None,
                references: vec![
                    crate::records::topology::body_recipe::DesignBodyRecipeReference {
                        design_reference: 1,
                        design_reference_offset: 1025,
                        form: 1,
                        form_offset: 1033,
                        candidate_faces: vec![face.clone()],
                        preceding_candidate_faces: Vec::new(),
                        preceding_body_slots: Vec::new(),
                    },
                ],
                nested_record_index: 103,
                nested_record_index_offset: 1038,
                recipe_id: "f3d:Design/BulkStream.dat:recipe#1".into(),
                resolved_face_slot: None,
                resolved_body_state_id: None,
                resolved_body_slot: None,
                resolved_body_face_slots: Vec::new(),
                next_record_index: 104,
                next_byte_offset: 1200,
            },
        )
        .unwrap();
        let feature = || Feature {
            id: crate::ids::neutral_feature_id(&scope),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Sketch {
                    sketch: SketchFeatureBinding::Unresolved,
                },
            )),
            native_ref: Some(scope.id.clone()),
        };
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(b"PNG").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let asset_id_len = crate::ids::neutral_asset_id(ENTRY).as_str().len();
            let after_embedded = face.as_str().len()
                + 3
                + "mark.png".len()
                + crate::ids::native_scope(ENTRY).len()
                + asset_id_len
                + crate::ids::neutral_feature_id(&scope).as_str().len();
            for (retained, items, dimension, operation) in [
                (
                    u64::MAX,
                    u64::MAX,
                    ResourceDimension::WorkUnits,
                    "find F3D Decal neutral feature",
                ),
                (
                    u64::MAX,
                    0,
                    ResourceDimension::CollectionItems,
                    "f3d Decal faces",
                ),
                (
                    u64::MAX,
                    1,
                    ResourceDimension::CollectionItems,
                    "f3d Decal assets",
                ),
                (
                    u64::try_from(face.as_str().len() - 1).unwrap(),
                    u64::MAX,
                    ResourceDimension::RetainedBytes,
                    "f3d Decal face identifier",
                ),
                (
                    u64::try_from(after_embedded + asset_id_len - 1).unwrap(),
                    u64::MAX,
                    ResourceDimension::RetainedBytes,
                    "f3d image feature asset identifier",
                ),
                (
                    u64::try_from(after_embedded + asset_id_len + operand.id.len() - 1).unwrap(),
                    u64::MAX,
                    ResourceDimension::RetainedBytes,
                    "f3d Decal native operand identifier",
                ),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = retained;
                policy.limits.max_collection_items = items;
                let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
                    dimension,
                    operation,
                    |cap| {
                        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                        match dimension {
                            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                                policy.limits.max_retained_bytes = cap;
                            }
                            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                                policy.limits.max_collection_items = cap;
                            }
                            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                                policy.limits.max_materialized_bytes = cap;
                            }
                            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                                policy.limits.max_work_units = cap;
                            }
                            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                        }
                        let (ctx, _) =
                            DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                        (super::project_decal_images(
                            &ctx,
                            scan,
                            std::slice::from_ref(&scope),
                            std::slice::from_ref(&image),
                            std::slice::from_ref(&group),
                            std::slice::from_ref(&operand),
                            &mut [feature()],
                        ))
                        .map(|_| ())
                    },
                ) {
                    cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                    error => panic!("unexpected refusal: {error:?}"),
                };
                policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = refusal_cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = refusal_cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = refusal_cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = refusal_cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = super::project_decal_images(
                    &ctx,
                    scan,
                    std::slice::from_ref(&scope),
                    std::slice::from_ref(&image),
                    std::slice::from_ref(&group),
                    std::slice::from_ref(&operand),
                    &mut [feature()],
                );
                assert!(
                    matches!(result,
                        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                            if failure.dimension == dimension && failure.operation == operation
                    ),
                    "operation {operation}: {result:?}"
                );
            }
            let assets = super::project_decal_images(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                std::slice::from_ref(&scope),
                std::slice::from_ref(&image),
                std::slice::from_ref(&group),
                std::slice::from_ref(&operand),
                &mut [feature()],
            )
            .unwrap();
            assert_eq!(assets.len(), 1);
            assert_eq!(assets[0].id, crate::ids::neutral_asset_id(ENTRY));
        });
    }
}
