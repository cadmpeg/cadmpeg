// SPDX-License-Identifier: Apache-2.0
//! Conversion from a PSB container to [`CadIr`].
//!
//! Decode transfers reference geometry, display meshes, features, sketches,
//! and supported native, extrusion, revolution, and circular B-rep routes.
//! Source metadata records the namespace census, units, and layout identity.
//! Preserved native records and geometry sections retain unresolved source
//! geometry. The report states geometry and topology losses for withheld data.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Decoded;

use crate::container;

mod analytic;
mod build;
mod coverage;
mod curve_expressions;
mod document_projection;
mod expanded;
mod feature_history;
mod holes;
mod native;
mod native_records;
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
    let scan = container::scan_bytes(ctx, root.window())?;
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
    } = if ctx.container_only() {
        build_container_ir(ctx, &scan, &classification)?
    } else {
        build_ir(ctx, &scan, &classification)?
    };
    ir.scope = if ctx.container_only() {
        cadmpeg_ir::ContentScope::container()
    } else {
        cadmpeg_ir::ContentScope::full()
    };
    let mut untyped = cadmpeg_ir::ArenaClasses::default();
    let mut body = build_report(
        &mut untyped,
        &scan,
        &classification,
        &ir,
        coverage,
        &brep_diagnostics,
        ctx.container_only(),
    )?;
    body.losses.extend(transfer_losses);
    for state in &scan.features.operation_states {
        if state
            .name
            .stored_name_bytes()
            .is_some_and(|bytes| std::str::from_utf8(bytes).is_err())
        {
            untyped.insert(cadmpeg_ir::ArenaClass::Design);
            body.losses.push(crate::loss::CreoLossCode::NeutralRecordUnprojected.note(format!(
                "Feature operation {} state at {} has a stored name whose text encoding is not decoded; source bytes remain in fidelity.", state.feature_id, state.state_offset
            )));
        }
    }
    if !ctx.container_only() {
        document_projection::project(ctx, &scan, &mut ir, &mut body.losses)?;
    }
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations);
    source_fidelity.attach_native_unknown_records(&mut ir, "creo", unknowns, ctx)?;
    if ctx.container_only() {
        // Container mode retains expanded section bytes, not the typed modern
        // geometry and history payloads emitted by build_ir.
        if scan
            .framing
            .sections
            .iter()
            .any(|section| section.role() == crate::container::SectionRole::PsbGeometry)
            || !scan.surfaces.rows.is_empty()
            || !scan.curves.prototypes.is_empty()
        {
            untyped.insert(cadmpeg_ir::ArenaClass::Model);
        }
        if !scan.features.rows.is_empty()
            || !scan.features.definitions.is_empty()
            || !scan.features.depdb_recipe_rows.is_empty()
        {
            untyped.insert(cadmpeg_ir::ArenaClass::Design);
        }
    }
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(root.window().len()),
        "retain Creo source image outside CADIR",
    )?;
    source_fidelity.insert_retained_record(
        cadmpeg_ir::ids::UnknownId::mint("creo:source:image#whole")
            .map_err(CodecError::malformed)?,
        cadmpeg_ir::source_fidelity::RetainedSourceRecord::whole(
            "creo:source",
            root.window().to_vec(),
        )
        .with_untyped(untyped),
    )?;
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}
