// SPDX-License-Identifier: Apache-2.0
//! Synthetic Fusion manifest ZIP entries.
#![allow(clippy::unwrap_used)]

use std::io::{Seek, Write};

pub(crate) fn write_synthetic_manifests<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: zip::write::SimpleFileOptions,
) {
    zip.start_file("Manifest.dat", options).unwrap();
    zip.write_all(&crate::manifest::generated_top_level().unwrap())
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
