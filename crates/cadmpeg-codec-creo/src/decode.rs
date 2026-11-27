// SPDX-License-Identifier: Apache-2.0
//! Conversion from a PSB container to [`CadIr`].
//!
//! Decode transfers reference geometry, display meshes, features, sketches,
//! and supported native, extrusion, revolution, and circular B-rep routes.
//! Source metadata records the namespace census, units, and layout identity.
//! Preserved native records and geometry sections retain unresolved source
//! geometry. The report states geometry and topology losses for withheld data.

use cadmpeg_core::decode::{ArenaClass, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Decoded;
use cadmpeg_ir::unknown::part::{Cause, DecodeParts, GivenUpPart, Part};

use crate::container;

pub(crate) mod analytic;
mod assembly;
mod build;
mod coverage;
mod curve_expressions;
mod expanded;
mod feature_history;
mod holes;
mod native;
mod native_records;
mod pmi;
mod quadratic;
mod records;
mod sketch;
mod sketch_ids;
mod sketch_transfer;
mod source_carriers;
mod surfaces;
mod sweep;
pub(crate) mod uniqueness;

use crate::decode::build::ir::{build_container_ir, build_ir, BuiltIr};
use crate::decode::build::report::build_report;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn with_service_context<T>(run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test root is admitted");
    run(&ctx)
}

/// Decode a `.prt` stream into an IR document and decode body; the sealed
/// wrapper stamps the report identity from `ir.source`.
///
/// The stream is read from its beginning. When the context requests only the
/// container, the returned IR contains source metadata and preserved geometry
/// sections but no transferred entities.
pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    let mut scan = container::scan_bytes(ctx, root.window())?;
    surfaces::cap_envelopes::withhold_unresolved_cap_envelopes(ctx, &mut scan)?;
    surfaces::cone_origins::withhold_unresolved_cone_origins(ctx, &mut scan)?;
    scan.plane_candidates = analytic::planes::PlaneCandidateMemo::on();
    let classification = crate::dialect::classify(&scan)?;
    // Admit section identities before model construction.
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(scan.framing.sections.len()),
        "admit Creo sections",
    )?;
    let BuiltIr {
        mut ir,
        annotations,
        unknowns,
        coverage,
        brep_diagnostics,
        transfer_losses,
        given_up,
    } = if ctx.container_only() {
        build_container_ir(ctx, &scan, &classification)?
    } else {
        build_ir(ctx, &scan, &classification)?
    };
    let mut body = build_report(
        &scan,
        &classification,
        &ir,
        coverage,
        &brep_diagnostics,
        ctx.container_only(),
    )?;
    body.losses.extend(transfer_losses);
    if let Some(definition) = ir.state_root_definition(
        ctx,
        crate::identity::compose_checked::<cadmpeg_ir::ids::ProductDefinitionId>(
            &crate::identity::MODEL_PRODUCT_DEFINITION,
            "root",
        )?,
        cadmpeg_ir::products::ExternalDocument::default(),
    )? {
        definition.kind.get_or_insert(
            if crate::decode::assembly::declares_assembly(&scan.framing.version_line) {
                cadmpeg_ir::products::ProductDefinitionKind::Assembly
            } else {
                cadmpeg_ir::products::ProductDefinitionKind::Part
            },
        );
        let root = definition.id.clone();
        ir.display_unresolved_meshes(&root);
    }
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::for_root(root);
    source_fidelity.annotations = annotations;
    let (unknowns, classes): (Vec<_>, Vec<_>) = unknowns.into_iter().unzip();
    let ids = unknowns
        .iter()
        .map(|record| record.id().clone())
        .collect::<Vec<_>>();
    source_fidelity.attach_native_unknown_records(&mut ir, "creo", unknowns, ctx)?;
    for (id, classes) in ids.iter().zip(classes) {
        source_fidelity.mark_untyped(id.as_str(), classes)?;
    }
    for part in given_up {
        body.losses
            .push(part.commit(ctx, &mut ir, &mut source_fidelity)?);
    }
    for expansion in &scan.framing.expansion_losses {
        let part = give_up_unexpanded_section(ctx, &scan, expansion)?;
        body.losses
            .push(part.commit(ctx, &mut ir, &mut source_fidelity)?);
    }
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

/// Gives up the section whose compressed payload `expansion` reports as
/// unexpanded, as a `compressed-section` part. The passthrough record of the
/// section retains its bytes, so the part keeps only their length and digest
/// and its stub links to that record.
fn give_up_unexpanded_section(
    ctx: &DecodeContext<'_>,
    scan: &container::ContainerScan<'_>,
    expansion: &container::SectionExpansionLoss,
) -> Result<GivenUpPart, CodecError> {
    let bytes = scan
        .framing
        .sections
        .iter()
        .find(|section| section.offset() == expansion.section_offset)
        .and_then(|section| container::section_region(&scan.framing.data, section))
        .ok_or_else(|| CodecError::malformed("an unexpanded Creo section has no stored region"))?;
    let section = crate::identity::compose_checked::<cadmpeg_ir::ids::UnknownId>(
        &cadmpeg_ir::identity_namespace!("creo", "compressed", "section"),
        expansion.section_offset,
    )?;
    let part = Part::at(
        "creo",
        "compressed-section",
        cadmpeg_core::decode::u64_from_index(expansion.section_offset),
        bytes,
        [ArenaClass::Model, ArenaClass::Design],
    )
    .contained_in(cadmpeg_ir::ids::Identity::from(section))
    .digest_only();
    let why = format!(
        "{}: {}",
        crate::loss::CreoLossCode::CompressedSectionUnexpanded.kind(),
        expansion.message
    );
    ctx.give_up_part(part, expansion.cause, give_up_error(expansion.cause, why))
}

/// The error a give-up of `cause` states: `Malformed` for bytes that break
/// the format, `NotImplemented` for a form without a reading.
pub(super) fn give_up_error(cause: Cause, why: String) -> CodecError {
    match cause {
        Cause::Unreadable => CodecError::Malformed(why),
        Cause::Undecoded => CodecError::NotImplemented(why),
    }
}
