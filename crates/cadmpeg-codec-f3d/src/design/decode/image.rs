// SPDX-License-Identifier: Apache-2.0
//! Transfer uniquely named Design image resources into neutral assets.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::design::decode::sketch::native_scope_charged;
use cadmpeg_core::decode::DecodeContext;
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
    let data = ctx.copy_retained(
        scan.entry_bytes(&entry.name)?,
        "f3d embedded image data",
    )?;
    let name = String::from_utf8(ctx.copy_retained(
        asset_name.as_bytes(),
        "f3d embedded image name",
    )?)
    .map_err(|_| CodecError::Malformed("asset name must be UTF-8".into()))?;
    let native_ref = native_scope_charged(ctx, &entry.name)?;
    Ok(Some(
        Asset::try_new(
            crate::ids::neutral_asset_id(&entry.name),
            Some(name),
            media_type,
            AssetContent::Embedded {
                data: cadmpeg_ir::assets::AssetData::new(data)
                    .ok_or_else(|| {
                    CodecError::Malformed("asset data must not be empty".into())
                })?,
            },
            Some(native_ref),
        )
        .map_err(CodecError::Malformed)?,
    ))
}

/// Decode image scopes in their owning streams, ordered by native identity.
pub(super) fn decode_scoped_images<T>(
    scan: &ContainerScan,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    kind: &crate::records::feature::scope::DesignFeatureKind,
    mut parse: impl FnMut(
        &[u8],
        &str,
        &crate::records::feature::scope::DesignParameterScope,
    ) -> Option<T>,
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
        images.extend(
            scopes
                .iter()
                .filter(|scope| {
                    scope.kind().as_str() == kind.as_str()
                        && crate::ids::native_stream(&scope.id) == Some(stream.as_str())
                })
                .filter_map(|scope| parse(bytes, &entry.name, scope)),
        );
    }
    images.sort_by(|a, b| id(a).cmp(id(b)));
    images.dedup_by(|a, b| id(a) == id(b));
    Ok(images)
}

#[cfg(test)]
mod tests {
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
            for (limit, operation) in [
                (0, "f3d embedded image data"),
                (DATA.len() as u64, "f3d embedded image name"),
                (
                    (DATA.len() + NAME.len()) as u64,
                    "f3d native stream key",
                ),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                assert!(matches!(
                    super::embedded_image_asset(&ctx, scan, NAME),
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == ResourceDimension::RetainedBytes
                            && failure.operation == operation
                ));
            }
            crate::design::test_support::with_test_decode_context(|ctx| {
                assert!(super::embedded_image_asset(ctx, scan, NAME).unwrap().is_some());
            });
        });
    }
}
