// SPDX-License-Identifier: Apache-2.0
//! Byte and projection fixtures shared by the userdata integration suites.

use crate::chunks::ArchiveVersion;

/// The world plane an annotation userdata payload carries.
pub(super) fn plane() -> Vec<u8> {
    [
        0.0, 0.0, 0.0, // origin
        1.0, 0.0, 0.0, // x axis
        0.0, 1.0, 0.0, // y axis
        0.0, 0.0, 1.0, // z axis
        0.0, 0.0, 1.0, 0.0, // equation
    ]
    .into_iter()
    .flat_map(f64::to_le_bytes)
    .collect()
}

/// Wrap `body` in an anonymous chunk carrying a major and a minor version.
pub(super) fn anonymous_major(
    archive: ArchiveVersion,
    major: i32,
    minor: i32,
    body: &[u8],
) -> Vec<u8> {
    let mut payload = major.to_le_bytes().to_vec();
    payload.extend(minor.to_le_bytes());
    payload.extend(body);
    crate::test_support::test_dump::crc_chunk(archive, 0x4000_8000, &payload)
}

/// The native parameters of the one typed hatch feature in `result`.
pub(super) fn hatch_parameters(
    result: &cadmpeg_ir::codec::DecodeResult,
) -> &std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String> {
    result
        .ir()
        .model
        .features
        .iter()
        .find_map(|feature| match feature.evaluation.definition() {
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Native {
                    kind, parameters, ..
                },
            ) if kind.as_str() == "hatch" => Some(parameters),
            _ => None,
        })
        .expect("typed hatch feature")
}
