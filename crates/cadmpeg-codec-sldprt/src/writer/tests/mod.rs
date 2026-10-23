// SPDX-License-Identifier: Apache-2.0
//! Semantic-writer unit tests.

mod brep_agreement;
pub(crate) mod configuration_carriers;
mod digests;
mod flex_history;
mod helix_surfaces;
mod homogeneous_poles;
mod native_properties;
mod parameters_extrude;
mod patterns_history;
mod record_losses;
mod round_trip;
mod sketch_tessellation;
mod swobjects;
mod targets;
mod thicken_reference;
mod transformed_surface;

mod parameter_conflicts;

/// One Info loss names the move of `parameter`, and `written` holds it under
/// the generated "Global parameters" owner as `name` with `expression`.
pub(super) fn assert_moved_to_global(
    report: &cadmpeg_ir::report::export::ExportReport,
    written: &cadmpeg_ir::CadIr,
    parameter: &str,
    name: &str,
    expression: &cadmpeg_ir::features::Expression,
) {
    let moves = report
        .losses
        .iter()
        .filter(|loss| {
            loss.code == crate::loss::SldprtLossCode::WriterParameterMovedToGlobal.kind()
                && loss.message.contains(&format!("parameter {parameter} "))
        })
        .collect::<Vec<_>>();
    assert_eq!(moves.len(), 1, "{parameter}");
    assert_eq!(moves[0].severity, cadmpeg_ir::report::Severity::Info);
    assert!(moves[0].message.contains("Global parameters"));
    assert!(!report.losses.iter().any(|loss| loss.code
        == crate::loss::SldprtLossCode::WriterEntityOmitted.kind()
        && loss.message.contains(&format!("parameter {parameter} "))));
    let owner = written
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Global parameters"))
        .expect("generated global owner")
        .id
        .clone();
    assert!(
        written
            .model
            .parameters
            .iter()
            .any(|held| held.owner.as_ref() == Some(&owner)
                && held.name == name
                && &held.expression == expression),
        "{parameter} as {name}"
    );
}
