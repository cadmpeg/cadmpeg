// SPDX-License-Identifier: Apache-2.0
//! Transfer uniquely named Design image resources into neutral assets.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent};

/// Mint the neutral asset ID of a Design image entry: the encoded entry name
/// after its encoded byte length.
pub(super) fn neutral_asset_id(
    entry_name: &str,
) -> Result<cadmpeg_ir::assets::AssetId, CodecError> {
    let encoded = crate::ids::identity_key_component(entry_name);
    cadmpeg_ir::assets::AssetId::mint(format!("f3d:model:asset#{}:{encoded}", encoded.len()))
        .map_err(|error| crate::design::text::malformed_design(format_args!("{error}")))
}

pub(super) fn embedded_image_asset(
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
    let data = scan.entry_bytes(&entry.name)?.to_vec();
    let name = String::from_utf8(asset_name.as_bytes().to_vec())
        .map_err(|_| CodecError::Malformed("asset name must be UTF-8".into()))?;
    let native_ref = crate::ids::native_scope(&entry.name);
    Ok(Some(
        Asset::try_new(
            neutral_asset_id(&entry.name)?,
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

/// Decode image scopes in their owning streams, ordered by native identity.
pub(super) fn decode_scoped_images<T>(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    kind: &crate::records::feature::scope::DesignFeatureKind,
    mut parse: impl FnMut(
        &DecodeContext<'_>,
        &[u8],
        &str,
        &crate::records::feature::scope::DesignParameterScope,
    ) -> Result<Option<T>, CodecError>,
    id: impl Fn(&T) -> &str,
) -> Result<Vec<T>, CodecError> {
    let mut images = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = crate::ids::native_scope(&entry.name);
        for scope in scopes.iter().filter(|scope| {
            scope.kind().as_str() == kind.as_str()
                && crate::ids::native_stream(&scope.id) == Some(stream.as_str())
        }) {
            if let Some(image) = parse(ctx, bytes, &entry.name, scope)? {
                images.push(image);
            }
        }
    }
    images.sort_by(|a, b| id(a).cmp(id(b)));
    images.dedup_by(|a, b| id(a) == id(b));
    Ok(images)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use zip::CompressionMethod;

    use crate::test_support::manifest_test::write_synthetic_manifests;
    use crate::test_support::zip_test::with_scan;

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
            assert!(super::embedded_image_asset(scan, NAME).unwrap().is_some());
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
            crate::design::test_support::with_test_decode_context(|ctx| {
                let images = super::decode_scoped_images(
                    ctx,
                    scan,
                    std::slice::from_ref(&scope),
                    &kind,
                    |_, _, _, _| Ok(Some(17_u32)),
                    |_| "image",
                )
                .unwrap();
                assert_eq!(images, [17]);
            });
        });
    }
}
