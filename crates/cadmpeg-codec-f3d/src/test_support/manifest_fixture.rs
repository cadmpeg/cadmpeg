// SPDX-License-Identifier: Apache-2.0
//! Synthetic Fusion manifest ZIP entries.

use std::io::{Seek, Write};

pub(crate) fn write_synthetic_manifests<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: zip::write::SimpleFileOptions,
) {
    write_synthetic_manifests_with_version(
        zip,
        options,
        crate::manifest::TOP_LEVEL_MANIFEST_VERSION,
    );
}

/// The synthetic manifest pair, with the top-level manifest declaring
/// `version`. Every field after the version field is unchanged.
pub(super) fn write_synthetic_manifests_with_version<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: zip::write::SimpleFileOptions,
    version: &str,
) {
    zip.start_file("Manifest.dat", options).unwrap();
    zip.write_all(&crate::manifest::generated_top_level_with_version(version))
        .unwrap();
    zip.start_file(
        format!(
            "{}/Manifest.dat",
            crate::manifest::GENERATED_DESIGN_ASSET_FOLDER
        ),
        options,
    )
    .unwrap();
    zip.write_all(&crate::manifest::generated_design_asset().unwrap())
        .unwrap();
}

pub(crate) fn api_reference_extension(targets: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut out = vec![0, 0, 0, 0, u8::from(!targets.is_empty())];
    if targets.is_empty() {
        return out;
    }
    out.extend_from_slice(&u32::try_from(targets.len()).unwrap().to_le_bytes());
    for (role, key, path) in targets {
        for text in [*role, "root.f3d", *path, &format!("cache/{path}")] {
            super::lp_utf16(&mut out, text);
        }
        for value in [2u32, 4, 1, 0, 6] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        let lineage = format!("urn:adsk.wipprod:dm.lineage:{key}");
        let version = format!("urn:adsk.wipprod:fs.file:vf.{key}?version=7");
        for (name, value) in [
            ("LineageId", lineage.as_str()),
            ("LineageUrn", lineage.as_str()),
            ("toDocDisplayName", *path),
            ("URL", version.as_str()),
            ("linkedAssetGUID", "11111111-2222-4333-8444-555555555555"),
            ("linkClassification", "OriginalDesign"),
        ] {
            super::lp_utf16(&mut out, name);
            super::lp_utf16(&mut out, value);
        }
    }
    out
}
