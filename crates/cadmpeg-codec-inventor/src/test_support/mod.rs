// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic Inventor byte-fixture builders and diagnostic readers for
//! crate tests.

pub(crate) mod combine;
pub(crate) mod mirror;
pub(crate) mod rse_compression;
pub(crate) mod sheet_metal;
pub(crate) mod sketch_curves;
pub(crate) mod surface_extrusion;
pub(crate) mod test_fixtures;
pub(crate) mod truncation;

/// The retained records of the parts a decode gave up.
pub(crate) fn given_up_records(
    fidelity: &cadmpeg_ir::SourceFidelity,
) -> Vec<&cadmpeg_ir::source_fidelity::RetainedSourceRecord> {
    fidelity
        .retained_records()
        .iter()
        .filter(|(id, _)| id.as_str().starts_with("inventor:source:part#"))
        .map(|(_, record)| record)
        .collect()
}
