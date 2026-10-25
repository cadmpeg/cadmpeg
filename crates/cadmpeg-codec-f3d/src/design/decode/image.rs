// SPDX-License-Identifier: Apache-2.0
//! Transfer uniquely named Design image resources into neutral assets.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent};

pub(super) fn embedded_image_asset(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    asset_name: &str,
) -> Result<Option<Asset>, CodecError> {
    let mut entries = scan.entries.iter().filter(|entry| {
        scan.is_design_asset_entry(entry, ContainerRole::Image)
            && entry.name.rsplit('/').next() == Some(asset_name)
    });
    let (Some(entry), None) = (entries.next(), entries.next()) else {
        return Ok(None);
    };
    let media_type = std::path::Path::new(asset_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| {
            if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
                Some("image/jpeg")
            } else if extension.eq_ignore_ascii_case("png") {
                Some("image/png")
            } else {
                None
            }
        })
        .map(str::to_owned);
    if entry.attributes.contains_key("payload_error") {
        return Ok(None);
    }
    let bytes = scan.entry_bytes(&entry.name)?;
    // The asset keeps a decoded copy of the source image bytes.
    ctx.retain_bytes(u64_from_index(bytes.len()), "retain F3D embedded image")?;
    let data = bytes.to_vec();
    let name = asset_name.to_owned();
    let native_ref = crate::ids::native_scope(&entry.name);
    Ok(Some(
        Asset::try_new(
            crate::ids::neutral_asset_id(&entry.name),
            Some(name),
            media_type,
            AssetContent::Embedded {
                data: cadmpeg_ir::assets::AssetData::new(data)
                    .ok_or_else(|| CodecError::Malformed("asset data must not be empty".into()))?,
            },
            Some(native_ref),
        )
        .map_err(CodecError::Malformed)?,
    ))
}

/// One located image record of a scope.
pub(super) struct ImageRecord {
    /// The image's native identity.
    pub(super) id: String,
    /// The stream offset of the indexed record whose primary-index frame is
    /// the image's extent.
    pub(super) start: usize,
    /// The extent's end in a stream with no primary index.
    pub(super) end: usize,
}

/// The stream bytes of a range, read through the stream's source view.
pub(super) type ImageRead<'s, 'r> = dyn Fn(std::ops::Range<usize>) -> Option<&'s [u8]> + 'r;

/// Decode image records of scopes in their owning streams, ordered by native
/// identity. Each image is parsed inside its record scope: `locate` finds its record,
/// and `parse` reads its grammar through the read it is given. A scope with
/// no complete image rolls back and leaves no capture. The image's source
/// offsets are `record_layout` pointers in `pointers`.
pub(super) fn decode_scoped_image_records<'s, T>(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'s>,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    kind: &crate::records::feature::scope::DesignFeatureKind,
    pointers: &mut crate::design::decode::sketch::SourcePointers,
    locate: impl Fn(
        &[u8],
        &str,
        &crate::records::feature::scope::DesignParameterScope,
    ) -> Option<ImageRecord>,
    parse: impl Fn(
        &[u8],
        &str,
        &crate::records::feature::scope::DesignParameterScope,
        &ImageRead<'s, '_>,
    ) -> Result<Option<T>, CodecError>,
    source_offsets: impl Fn(&T) -> Vec<(&'static str, Option<u64>)>,
) -> Result<Vec<T>, CodecError> {
    let mut images = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let source = scan
            .entry_view(&entry.name)
            .ok_or_else(|| CodecError::malformed(format_args!("entry {} not found", entry.name)))?;
        let bytes = source.window();
        let stream = crate::ids::native_scope(&entry.name);
        let mut frame_ends = std::collections::BTreeMap::new();
        if let Some(meta) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        {
            for frame in crate::metastream::primary_record_frames(&meta, bytes.len())? {
                frame_ends.insert(frame.start, frame.end);
            }
        }
        let child = |range: std::ops::Range<usize>| {
            source
                .start()
                .checked_add(range.start)
                .zip(source.start().checked_add(range.end))
                .and_then(|(start, end)| source.child(start, end))
        };
        let read = |range: std::ops::Range<usize>| {
            let view = child(range)?;
            view.read_slice(view.start()..view.end())
        };
        for scope in scopes.iter().filter(|scope| {
            scope.kind().as_str() == kind.as_str()
                && crate::ids::native_stream(&scope.id) == Some(stream.as_str())
        }) {
            let Some(record) = locate(bytes, &entry.name, scope) else {
                continue;
            };
            // An indexed record's extent is its primary-index frame.
            let end = frame_ends.get(&record.start).copied().unwrap_or(record.end);
            let Some(extent) = child(record.start..end) else {
                continue;
            };
            if pointers.contains_key(&record.id)
                || images.iter().any(|(id, _): &(String, T)| *id == record.id)
            {
                continue;
            }
            let mut skipped = false;
            let parsed = ctx.record(&record.id, extent, || {
                match parse(bytes, &entry.name, scope, &read)? {
                    Some(image) => Ok(image),
                    None => {
                        skipped = true;
                        Err(CodecError::malformed("no complete scoped image"))
                    }
                }
            });
            let image = match parsed {
                Ok(image) => image,
                Err(_) if skipped => continue,
                Err(error) => return Err(error),
            };
            let fields = crate::design::decode::parameters::source_layout::pointer_fields(
                ctx,
                source_offsets(&image),
            )?;
            pointers.insert(record.id.clone(), fields);
            images.push((record.id, image));
        }
    }
    images.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(images.into_iter().map(|(_, image)| image).collect())
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use zip::CompressionMethod;

    use crate::test_support::manifest_test::write_synthetic_manifests;
    use crate::test_support::zip_test::{with_context_scan, with_scan};

    #[test]
    fn embedded_image_asset_reads_unique_entry() {
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
            let ctx = cadmpeg_test_support::service_decode_context();
            assert!(super::embedded_image_asset(&ctx, scan, NAME)
                .unwrap()
                .is_some());

            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = u64::try_from(DATA.len()).unwrap() - 1;
            let (limited, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                super::embedded_image_asset(&limited, scan, NAME),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                        && limit.operation == "retain F3D embedded image"
            ));
        });
    }

    #[test]
    fn scoped_images_collect_parsed_records() {
        const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(b"scope").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let stream = crate::ids::native_scope(ENTRY);
        let scope: crate::records::feature::scope::DesignParameterScope =
            crate::records::feature::test_support::located_scope_from_fixture(serde_json::json!({
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
            }));
        let kind = crate::records::feature::scope::DesignFeatureKind::Fillet;
        with_context_scan(&archive, |ctx, scan| {
            let mut pointers = crate::design::decode::sketch::SourcePointers::new();
            let images = super::decode_scoped_image_records(
                ctx,
                scan,
                std::slice::from_ref(&scope),
                &kind,
                &mut pointers,
                |_, _, _| {
                    Some(super::ImageRecord {
                        id: "image".to_owned(),
                        start: 0,
                        end: 5,
                    })
                },
                |_, _, _, _| Ok(Some(17_u32)),
                |_| Vec::new(),
            )
            .unwrap();
            assert_eq!(images, [17]);
        });
    }
}
