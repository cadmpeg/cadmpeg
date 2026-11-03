// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic Inventor byte-fixture builders and diagnostic readers for
//! crate tests.

pub(crate) mod coil;
pub(crate) mod combine;
pub(crate) mod mirror;
pub(crate) mod rse_compression;
pub(crate) mod sheet_metal;
pub(crate) mod sketch_curves;
pub(crate) mod sketch_relations;
pub(crate) mod split;
pub(crate) mod surface_extrusion;
pub(crate) mod test_fixtures;
pub(crate) mod thicken;
pub(crate) mod thread;
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

/// An empty-carrier fixture stages one `inventor.kernel_bytes` give-up: a
/// carrier parse failure gives up that part (#227 §4.1/§4.5).
pub(crate) fn assert_kernel_bytes_give_up_charged(losses: &[cadmpeg_ir::report::loss::LossNote]) {
    assert_single_part_give_up(losses, "inventor.kernel_bytes");
}

/// Exactly one `record.unreadable` loss names the given-up `part`.
pub(crate) fn assert_single_part_give_up(
    losses: &[cadmpeg_ir::report::loss::LossNote],
    part: &str,
) {
    assert_eq!(losses.len(), 1, "one {part} give-up: {losses:#?}");
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert!(losses[0].message.contains(part), "{}", losses[0].message);
}
