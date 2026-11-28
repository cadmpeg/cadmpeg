// SPDX-License-Identifier: Apache-2.0
//! Select synthetic fixtures or optional external inputs.

fn external_path(relative: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("CADMPEG_F3D_VERIFICATION_FIXTURES")
        .map(|root| std::path::PathBuf::from(root).join(relative))
}

pub(crate) fn text(relative: &str, synthetic: &str) -> String {
    external_path(relative).map_or_else(
        || synthetic.to_owned(),
        |path| std::fs::read_to_string(path).expect("external verification fixture"),
    )
}

pub(crate) fn bytes(relative: &str, synthetic: &[u8]) -> Vec<u8> {
    external_path(relative).map_or_else(
        || synthetic.to_vec(),
        |path| std::fs::read(path).expect("external verification fixture"),
    )
}
