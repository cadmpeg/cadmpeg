// SPDX-License-Identifier: Apache-2.0
//! Conversion from a PSB container to [`CadIr`].
//!
//! Decode transfers standard datum planes as derived plane surfaces and
//! preserves each geometry section as an [`UnknownRecord`]. Source metadata
//! records the namespace census, active units, and counts of decoded structural
//! rows. The typed dialect match owns layout identity.
//!
//! Surface and curve namespaces contain useful topology and prototype data, but
//! the placed body model is incomplete. The report therefore records blocking
//! geometry and topology losses instead of emitting a partial B-rep.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Decoded;

use crate::container;

mod analytic;
mod build;
mod coverage;
mod curve_expressions;
mod expanded;
mod feature_history;
mod holes;
mod native;
mod native_records;
mod records;
mod sketch;
mod sketch_ids;
mod sketch_transfer;
mod surfaces;
mod sweep;
pub(crate) mod uniqueness;

use build::{build_container_ir, build_ir, build_report, BuiltIr};

#[cfg(test)]
pub(crate) use sketch::{
    resolved_section_coordinates, resolved_section_points, resolved_section_radii,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod plane_reconciliation_tests;

#[cfg(test)]
mod topological_vertex_tests;

#[cfg(test)]
mod native_edge_parameter_tests;

#[cfg(test)]
mod native_pcurve_tests;

#[cfg(test)]
mod prototype_local_frame_tests;

#[cfg(test)]
mod prototype_association_tests;

/// Decode a `.prt` stream into an IR document and decode body; the sealed
/// wrapper stamps the report identity from `ir.source`.
///
/// The stream is read from its beginning. When `options.container_only` is set,
/// the returned IR contains source metadata and preserved geometry sections but
/// no transferred entities.
pub fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    let scan = container::scan_bytes(root.window());
    let classification = crate::dialect::classify(&scan);
    // Charge section cardinality before IR construction so max_entities can
    // refuse the build rather than only the finalizer.
    ctx.charge_entities(scan.framing.sections.len() as u64, "admit Creo sections")?;
    let mut admitted_entities = 0_u64;

    let BuiltIr {
        mut ir,
        annotations,
        unknowns,
        coverage,
        brep_diagnostics,
    } = if ctx.container_only() {
        build_container_ir(&scan, &classification)?
    } else {
        build_ir(ctx, &scan, &classification)?
    };
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        &mut admitted_entities,
        "admit Creo entities",
    )?;
    let body = build_report(
        &scan,
        &classification,
        &ir,
        coverage,
        &brep_diagnostics,
        ctx.container_only(),
    );
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations);
    source_fidelity.attach_native_unknown_records(&mut ir, "creo", unknowns)?;
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}
