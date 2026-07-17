// SPDX-License-Identifier: Apache-2.0
//! Record-ticket issuance for the CATIA decode (doc §6.2, §10 Phase 3D).
//!
//! Phase 3D instruments the §3.3 commit boundary the codec already crosses:
//! for every record-shaped unit the codec walks it issues a [`RecordTicket`]
//! and resolves it with the disposition the outcome decides, so no record is
//! silently lost. `Check::TransferAccounting` (run at
//! [`DecodeContext::finish`](cadmpeg_ir::decode::DecodeContext::finish))
//! validates the resolved table against the ledger and the report's losses.
//!
//! Issuance rides the codec's L1 coarse ledger (§6.1, [`crate::ledger`]), so
//! the record units are the units that ledger tiles: the container framing —
//! `Structural`, no semantic content — and the reconstructed record stream the
//! decode commits to interpreting. When that stream reaches typed IR the ticket
//! resolves [`RecordDisposition::Typed`] naming the entities emitted; when a
//! salvage path yields no typed entity — the metadata and container-only
//! fallbacks preserve the payload as a native unknown but transfer no record —
//! it resolves [`RecordDisposition::Dropped`] with an accountable loss note that
//! also rides the report. Finer per-record dispositions inside the stream await
//! the L2 refined ledger; at L1 the stream is one opaque span and one ticket.

use cadmpeg_ir::decode::{DecodeContext, RecordDisposition, RecordKind, View};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};

use crate::container::ContainerScan;

/// Issues and resolves the decode's record tickets against `ctx`.
///
/// Called once per decode path at the commit boundary, after the IR and report
/// are built. Every ticket it commits it also resolves, so the `finish`-time
/// unresolved-ticket check never fires for the CATIA codec and the disposition
/// table it leaves validates in both strict and salvage modes.
pub(crate) fn account_records(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    _scan: &ContainerScan<'_>,
    ir: &CadIr,
    report: &mut DecodeReport,
) {
    let location = root.location();

    // Container framing carries no semantic content: outer/inner headers and the
    // stream directory the ledger classes `Structural`.
    let framing = ctx.commit_record(location, RecordKind("container_framing"));
    ctx.resolve(framing, RecordDisposition::Structural);

    // The reconstructed record stream the decode committed to interpreting. At
    // L1 it is one opaque ledger span, so it is one ticket resolved by outcome.
    let outputs = collect_entity_ids(ir);
    let stream = ctx.commit_record(location, RecordKind("catia_record_stream"));
    if outputs.is_empty() {
        // A salvage path that transferred no record to typed IR. The payload is
        // preserved byte-addressable as a native unknown, but from the transfer
        // ledger's view the record is dropped: record it as such with a loss the
        // report carries, so the omission is accountable rather than silent.
        let loss = LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message:
                "The CATIA record stream was preserved as a native unknown but transferred no \
                 record to typed IR; its disposition is recorded as dropped."
                    .to_string(),
            provenance: None,
        };
        report.losses.push(loss.clone());
        ctx.resolve(stream, RecordDisposition::Dropped { loss });
    } else {
        ctx.resolve(stream, RecordDisposition::Typed { outputs });
    }
}

/// Collects the ids of every model entity the decode emitted, in sorted order
/// for a deterministic disposition table. `Check::TransferAccounting` requires
/// each named `Typed` output to resolve in the IR model, which these do by
/// construction.
fn collect_entity_ids(ir: &CadIr) -> Vec<String> {
    let model = &ir.model;
    let mut ids = Vec::new();
    ids.extend(model.bodies.iter().map(|entity| entity.id.0.clone()));
    ids.extend(model.regions.iter().map(|entity| entity.id.0.clone()));
    ids.extend(model.shells.iter().map(|entity| entity.id.0.clone()));
    ids.extend(model.faces.iter().map(|entity| entity.id.0.clone()));
    ids.extend(model.loops.iter().map(|entity| entity.id.0.clone()));
    ids.extend(model.coedges.iter().map(|entity| entity.id.0.clone()));
    ids.extend(model.edges.iter().map(|entity| entity.id.0.clone()));
    ids.extend(model.vertices.iter().map(|entity| entity.id.0.clone()));
    ids.extend(model.points.iter().map(|entity| entity.id.0.clone()));
    ids.extend(model.surfaces.iter().map(|entity| entity.id.0.clone()));
    ids.extend(model.curves.iter().map(|entity| entity.id.0.clone()));
    ids.extend(model.pcurves.iter().map(|entity| entity.id.0.clone()));
    ids.sort();
    ids
}
