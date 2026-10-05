// SPDX-License-Identifier: Apache-2.0
//! Transfer uniquely named Design image resources into neutral assets.

use cadmpeg_core::container::{ContainerEntry, ContainerRole};

use crate::container::ContainerScan;
use crate::design::decode::record_streams::in_stream;
use crate::design::decode::sketch::{
    append_percent_encoded, native_scope_charged, native_scope_scoped, percent_encoded_len,
    IndexedRecordOffsets,
};
use crate::design::decode::text::rsplit_once_ascii;
use cadmpeg_core::decode::{index_from_u32, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent};

/// The neutral asset ID of a Design resource entry: the asset namespace, the
/// encoded name length, `:` and the percent-encoded name.
pub(super) fn neutral_asset_id_charged(
    ctx: &DecodeContext<'_>,
    entry_name: &str,
) -> Result<cadmpeg_ir::assets::AssetId, CodecError> {
    const PREFIX: &str = "f3d:model:asset#";
    const OPERATION: &str = "f3d asset identifier";
    let encoded_len = percent_encoded_len(ctx, entry_name, "measure F3D asset identifier")?;
    let digits = encoded_len
        .checked_ilog10()
        .map_or(1, |log| index_from_u32(log) + 1);
    let capacity = PREFIX
        .len()
        .checked_add(digits)
        .and_then(|length| length.checked_add(1))
        .and_then(|length| length.checked_add(encoded_len))
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, 0, 1))?;
    let mut id = ctx.retained_string(capacity, OPERATION)?;
    ctx.append_retained(&mut id, PREFIX, OPERATION)?;
    ctx.append_formatted_retained(&mut id, format_args!("{encoded_len}:"), OPERATION)?;
    append_percent_encoded(ctx, entry_name, &mut id, OPERATION)?;
    cadmpeg_ir::assets::AssetId::mint(id)
        .map_err(|error| crate::design::text::malformed_design(ctx, format_args!("{error}")))
}

/// The only Design image entry whose file name is `asset_name`.
pub(super) fn embedded_image_entry<'scan>(
    ctx: &DecodeContext<'_>,
    scan: &'scan ContainerScan,
    asset_name: &str,
) -> Result<Option<&'scan ContainerEntry>, CodecError> {
    const OPERATION: &str = "find F3D embedded image entry";
    let mut found = None;
    for entry in ctx.admit_iter(&scan.entries, OPERATION)? {
        if !scan.is_design_asset_entry(entry, ContainerRole::Image) {
            continue;
        }
        let file_name = rsplit_once_ascii(ctx, &entry.name, b'/', OPERATION)?
            .map_or(entry.name.as_str(), |(_, file_name)| file_name);
        if ctx.equal_bytes(file_name.as_bytes(), asset_name.as_bytes(), OPERATION)?
            && found.replace(entry).is_some()
        {
            return Ok(None);
        }
    }
    Ok(found)
}

/// The neutral asset `id` holding the bytes of the image `entry`, named
/// `asset_name`.
pub(super) fn embedded_image_asset(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    entry: &ContainerEntry,
    asset_name: &str,
    id: cadmpeg_ir::assets::AssetId,
) -> Result<Asset, CodecError> {
    let media_type = image_media_type(ctx, asset_name)?.map(str::to_owned);
    let data = ctx.copy_retained(scan.entry_bytes(&entry.name)?, "f3d embedded image data")?;
    let name = ctx.copy_retained_text(asset_name, "f3d embedded image name")?;
    let native_ref = native_scope_charged(ctx, &entry.name)?;
    Asset::try_new(
        ctx,
        id,
        Some(name),
        media_type,
        AssetContent::Embedded {
            data: cadmpeg_ir::assets::AssetData::new(data)
                .ok_or_else(|| CodecError::Malformed("asset data must not be empty".into()))?,
        },
        Some(native_ref),
    )
}

/// The media type a file name's extension states. A name that starts with its
/// only `.` has no extension. Each extension test reads at most the four
/// bytes of a candidate.
fn image_media_type(
    ctx: &DecodeContext<'_>,
    file_name: &str,
) -> Result<Option<&'static str>, CodecError> {
    const OPERATION: &str = "classify F3D embedded image extension";
    let Some((stem, extension)) = rsplit_once_ascii(ctx, file_name, b'.', OPERATION)? else {
        return Ok(None);
    };
    if stem.is_empty() {
        return Ok(None);
    }
    for (candidate, media_type) in [
        ("jpg", "image/jpeg"),
        ("jpeg", "image/jpeg"),
        ("png", "image/png"),
    ] {
        if extension.eq_ignore_ascii_case(candidate) {
            return Ok(Some(media_type));
        }
    }
    Ok(None)
}

/// Decode image scopes in their owning streams, ordered by native identity.
/// A stream with an owner scope is indexed once, under a scoped reservation,
/// and every scope of the stream is parsed against that index. Each parsed
/// image is held under its own scoped reservation until the duplicates of its
/// identity are dropped; the kept images become retained.
pub(super) fn decode_scoped_images<T>(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    kind: &crate::records::feature::scope::DesignFeatureKind,
    mut parse: impl FnMut(
        &DecodeContext<'_>,
        &[u8],
        &IndexedRecordOffsets,
        &str,
        &crate::records::feature::scope::DesignParameterScope,
    ) -> Result<Option<T>, CodecError>,
    id: impl Fn(&T) -> &str,
) -> Result<Vec<T>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "f3d scoped image candidates")?;
    let mut parsed = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D image stream entries")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let (_stream_storage, stream) = native_scope_scoped(ctx, &entry.name)?;
        let mut stream_records = None;
        for scope in ctx.admit_iter(scopes, "scan F3D image owner scopes")? {
            // The kind is a literal name, so the comparison is constant.
            if scope.kind_name() != kind.as_str() || !in_stream(ctx, &scope.id, &stream)? {
                continue;
            }
            let (records, _records_storage) = match &mut stream_records {
                Some(records) => records,
                slot @ None => slot.insert(IndexedRecordOffsets::build_scoped(ctx, bytes)?),
            };
            let (image, storage) = ctx.with_scoped_storage("f3d scoped image records", || {
                parse(ctx, bytes, records, &entry.name, scope)
            })?;
            if let Some(image) = image {
                ctx.push_scoped_vec(
                    &mut scratch,
                    &mut parsed,
                    (image, storage),
                    "f3d scoped image candidates",
                )?;
            }
        }
    }
    ctx.stable_sort_by(
        &mut parsed[..],
        |(image, _)| id(image),
        Ord::cmp,
        "sort f3d design image 1",
    )?;
    let mut images = Vec::new();
    for (image, storage) in parsed {
        ctx.charge_work(1, "dedup f3d design images")?;
        if let Some(kept) = images.last() {
            if ctx.equal_bytes(
                id(kept).as_bytes(),
                id(&image).as_bytes(),
                "dedup f3d design images",
            )? {
                continue;
            }
        }
        storage.commit()?;
        ctx.push_vec(&mut images, image, "f3d scoped image records")?;
    }
    Ok(images)
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::u64_from_index;

    use std::io::{Cursor, Write};

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use zip::CompressionMethod;

    use crate::test_support::manifest_test::write_synthetic_manifests;
    use crate::test_support::zip_test::with_scan;

    #[test]
    fn embedded_image_asset_refuses_each_retained_copy_limit() {
        const ENTRY: &str = "FusionAssetName[Active]/Design1/Images.BlobParts/mark.png";
        const NAME: &str = "mark.png";
        const DATA: &[u8] = b"PNG";
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(DATA).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        with_scan(&archive, |scan| {
            let asset = |ctx: &DecodeContext<'_>| {
                let entry = super::embedded_image_entry(ctx, scan, NAME)?.expect("image entry");
                super::embedded_image_asset(
                    ctx,
                    scan,
                    entry,
                    NAME,
                    crate::ids::neutral_asset_id(ENTRY),
                )
            };
            for (limit, operation) in [
                (0, "f3d embedded image data"),
                (u64_from_index(DATA.len()), "f3d embedded image name"),
                (
                    u64_from_index(DATA.len() + NAME.len()),
                    "f3d native stream key",
                ),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = limit;

                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                assert!(matches!(
                    asset(&ctx),
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == ResourceDimension::RetainedBytes
                            && failure.operation == operation
                ));
            }
            let extension_error = crate::test_support::resource_refusal_at(
                ResourceDimension::WorkUnits,
                "classify F3D embedded image extension",
                0,
                |ctx| asset(ctx).map(|_| ()),
            );
            assert!(matches!(
                extension_error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::WorkUnits
                        && limit.operation == "classify F3D embedded image extension"
                        // The reverse search admits the name and the pattern.
                        && limit.additional == u64_from_index(NAME.len() + 1)
            ));
            crate::design::test_support::with_test_decode_context(|ctx| {
                assert_eq!(asset(ctx).unwrap().id, crate::ids::neutral_asset_id(ENTRY));
            });
        });
    }

    #[test]
    fn scoped_image_collection_refuses_before_growth() {
        const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(b"scope").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let stream = crate::ids::native_scope(ENTRY);
        let scope: crate::records::feature::scope::DesignParameterScope =
            serde_json::from_value(serde_json::json!({
                "id": format!("{stream}:scope#1"),
                "byte_offset": 0,
                "class_tag": "300",
                "record_index": 1,
                "frame_length": 200,
                "kind": "Fillet",
                "kind_offset": 32,
                "feature_ordinal": 1,
                "feature_ordinal_offset": 128,
                "history_state_id": 8,
                "history_state_id_offset": 24,
                "previous_history_state_id": 7,
                "previous_history_state_id_offset": 158,
                "reference_count_offset": 9,
                "reference_members": [2],
                "reference_member_offsets": [14],
                "fixed_fillet_parameters": {
                    "groups": [{
                        "tangency_weight": {"value": 1.0, "record_index": 4, "value_offset": 0},
                        "radii": [0.3],
                        "radius_record_indexes": [5],
                        "radius_offsets": [0],
                        "intermediate_parameters": [],
                        "intermediate_parameter_record_indexes": [],
                        "intermediate_parameter_offsets": []
                    }]
                },
                "paired_class_tag": "261",
                "paired_byte_offset": 200
            }))
            .unwrap();
        let kind = crate::records::feature::scope::DesignFeatureKind::Fillet;
        with_scan(&archive, |scan| {
            for (dimension, operation) in [
                (
                    ResourceDimension::MaterializedBytes,
                    "f3d scoped native stream key",
                ),
                (
                    ResourceDimension::CollectionItems,
                    "f3d scoped image candidates",
                ),
                (
                    ResourceDimension::CollectionItems,
                    "f3d scoped image records",
                ),
            ] {
                let error =
                    crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
                        super::decode_scoped_images(
                            ctx,
                            scan,
                            std::slice::from_ref(&scope),
                            &kind,
                            |_, _, _, _, _| Ok(Some(17_u32)),
                            |_| "image",
                        )
                    });
                assert!(matches!(
                    error,
                    cadmpeg_core::CodecError::ResourceLimit(failure)
                        if failure.dimension == dimension && failure.operation == operation
                ));
            }
            crate::design::test_support::with_test_decode_context(|ctx| {
                let images = super::decode_scoped_images(
                    ctx,
                    scan,
                    std::slice::from_ref(&scope),
                    &kind,
                    |_, _, _, _, _| Ok(Some(17_u32)),
                    |_| "image",
                )
                .unwrap();
                assert_eq!(images, [17]);
            });
        });
    }

    #[test]
    fn embedded_asset_identifier_refuses_retained_limit() {
        for entry in ["plain.png", "a:b#c%d.png", "spaced\tname é.png"] {
            let expected = crate::ids::neutral_asset_id(entry);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = u64::try_from(expected.as_str().len() - 1).unwrap();
            let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let refusal = super::neutral_asset_id_charged(&limited, entry);
            assert!(matches!(
                refusal,
                Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::RetainedBytes
                        && failure.operation == "f3d asset identifier"
            ));
            let admitted = super::neutral_asset_id_charged(
                &cadmpeg_test_support::service_decode_context(),
                entry,
            )
            .unwrap();
            assert_eq!(admitted, expected);
        }
    }
}
