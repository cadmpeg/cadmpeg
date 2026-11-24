// SPDX-License-Identifier: Apache-2.0
//! Build IR and diagnostics from an NX native container.
//!
//! [`scan`] parses the container and inflates its embedded streams. [`decode`]
//! converts supported analytic and NURBS carriers to millimetres, resolves
//! supported topology, preserves each Parasolid stream as an unknown record, and
//! returns a decode body describing incomplete transfer; the sealed wrapper stamps
//! the classification onto the report. Partition and deltas streams are both
//! decoded; callers must use the report to account for unresolved active-face
//! selection and other loss.

use cadmpeg_core::decode::ArenaClass;
use cadmpeg_core::decode::{DecodeContext, DecodeDepth, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};
use std::cell::RefCell;
use std::collections::BTreeSet;

use crate::container::{self, Container, EntryContent};
use crate::loss::NxLossCode;
use crate::native::TypedNative;
use crate::parasolid::{self, Stream, StreamKind};

mod blend;
mod build;
mod design_v11;
pub(crate) mod emit;
pub(crate) mod feature_completeness;
mod geometry_work;
pub(crate) mod ids;
pub(crate) mod jpeg;
mod nurbs_fit;
mod offset;
pub(crate) mod pcurves;
pub(crate) mod report;
mod support_uv;

use build::try_decode_geometry;
use emit::{source_meta, unknown_stream};

const MISSING_TOLERANCE: f64 = -31_415_800_000_000.0;
/// Parsed container data shared by inspection and entity decoding.
pub(crate) struct Scan<'a> {
    /// Parsed SPLMSSTR container.
    pub(crate) container: Container<'a>,
    /// Located and inflated Parasolid or preview streams.
    pub(crate) streams: Vec<Stream>,
    /// Records kept without decoding, with the arena classes their bytes can
    /// hold, noted where each is retained.
    pub(crate) untyped: RefCell<Vec<(UnknownId, BTreeSet<ArenaClass>)>>,
}

impl Scan<'_> {
    /// Notes that the record `id` is retained without decoding.
    pub(crate) fn note_untyped(
        &self,
        id: &UnknownId,
        classes: impl IntoIterator<Item = ArenaClass>,
    ) {
        self.untyped
            .borrow_mut()
            .push((id.clone(), classes.into_iter().collect()));
    }

    /// Count streams with the requested classification.
    pub(super) fn count(&self, kind: StreamKind) -> usize {
        self.streams.iter().filter(|s| s.kind() == kind).count()
    }

    /// Return whether the file contains an inline Parasolid stream.
    ///
    /// NX assemblies may contain only references to external child parts.
    pub(super) fn has_parasolid(&self) -> bool {
        self.streams.iter().any(|s| s.kind().is_parasolid())
    }
}

/// Parse the SPLMSSTR container and inflate streams in its canonical part entry.
pub(crate) fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Scan<'a>, CodecError> {
    let (container, streams) = if container::looks_like_nx(root.window()) {
        let container = container::scan_bytes(root.window())?;
        let streams = parasolid::extract_streams(ctx, root, &container)?;
        (container, streams)
    } else {
        let (container, part) = container::scan_legacy(ctx, root)?;
        let streams = parasolid::extract_legacy_streams(ctx, part)?;
        (container, streams)
    };
    Ok(Scan {
        container,
        streams,
        untyped: RefCell::default(),
    })
}

/// Decode an NX `.prt` into IR and a loss report.
///
/// When [`DecodeContext::container_only`] is set, the returned IR contains source
/// metadata and preserved streams but no typed entities. Otherwise the decoder
/// emits supported geometry and resolvable topology. A valid container can
/// decode successfully with no geometry, including an assembly whose geometry
/// resides in external child parts.
pub(crate) fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Decoded, CodecError> {
    let decode_digest = Sha256Digest::digest(root.window());
    let scan = scan(ctx, root)?;
    let mut decoded = decode_scan(ctx, &scan, decode_digest)?;
    // A record an attachment rolled back is not retained, and its note goes
    // with it.
    for (id, classes) in scan.untyped.take() {
        if decoded
            .source_fidelity
            .retained_record(id.as_str())
            .is_some()
        {
            decoded.source_fidelity.mark_untyped(id.as_str(), classes)?;
        }
    }
    Ok(decoded)
}

fn decode_scan(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
    decode_digest: Sha256Digest,
) -> Result<Decoded, CodecError> {
    let (classification, notes) = crate::scan_notes::summarize(scan)?;
    let (dialects, dialect_losses) = classification.into_report_parts();

    let mut admitted_entities = 0_u64;
    if ctx.container_only() {
        let (ir, annotations, unknowns, native_losses) = build_metadata_ir(ctx, scan, &dialects)?;
        let mut body = build_container_body(scan, dialect_losses, notes);
        body.transfer = cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {};
        body.losses.extend(native_losses);
        report_untransferred_streams(scan, &mut body, TypedNative::ContainerOnly);
        return decoded(
            ctx,
            decode_digest,
            ir,
            body,
            annotations,
            unknowns,
            &mut admitted_entities,
        );
    }

    if let Some((ir, body, annotations, unknowns)) = try_decode_geometry(
        ctx,
        scan,
        &dialects,
        &dialect_losses,
        &notes,
        &mut admitted_entities,
    )? {
        return decoded(
            ctx,
            decode_digest,
            ir,
            body,
            annotations,
            unknowns,
            &mut admitted_entities,
        );
    }

    let (ir, annotations, unknowns, native_losses) = build_metadata_ir(ctx, scan, &dialects)?;
    let mut body = build_container_body(scan, dialect_losses, notes);
    body.losses.extend(native_losses);
    report_untransferred_streams(scan, &mut body, TypedNative::Available);
    decoded(
        ctx,
        decode_digest,
        ir,
        body,
        annotations,
        unknowns,
        &mut admitted_entities,
    )
}

fn decoded(
    ctx: &DecodeContext<'_>,
    decode_digest: Sha256Digest,
    mut ir: CadIr,
    mut body: DecodeBody,
    annotations: cadmpeg_ir::Annotations,
    unknowns: Vec<UnknownRecord>,
    admitted_entities: &mut u64,
) -> Result<Decoded, CodecError> {
    let annotations = if ctx.depth() >= DecodeDepth::Geometry {
        let (definition, minted) = design_v11::root_definition(&mut ir)?;
        if ctx.depth() >= DecodeDepth::Design {
            cadmpeg_ir::design::projection::project_layer(
                ctx,
                "nx",
                &mut ir,
                &mut body.losses,
                |ir, _| design_v11::project(ir, definition.clone()),
            )?;
        }
        let mut builder = AnnotationBuilder::resume(annotations);
        if minted {
            builder.exactness(definition, Exactness::Derived);
        }
        builder.build()
    } else {
        annotations
    };
    ctx.admit_entities(
        cadmpeg_core::decode::u64_from_index(ir.entity_count()),
        admitted_entities,
        "admit NX entities",
    )?;
    if let Some(root) = ir.state_root_definition(
        ctx,
        cadmpeg_ir::ids::ProductDefinitionId::mint(design_v11::PART_ROOT)
            .map_err(CodecError::malformed)?,
        cadmpeg_ir::products::ExternalDocument::default(),
    )? {
        let root = root.id.clone();
        ir.display_unresolved_meshes(&root);
    }
    let mut source_fidelity =
        cadmpeg_ir::SourceFidelity::with_annotations(annotations, decode_digest);
    source_fidelity.attach_native_unknown_records(&mut ir, "nx", unknowns, ctx)?;
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

fn report_untransferred_streams(scan: &Scan, body: &mut DecodeBody, typed_native: TypedNative) {
    let (control_count, classified_control_count) = offset_store_control_counts(&scan.container);
    if classified_control_count != control_count {
        body.losses.push(NxLossCode::OffsetStoreControlUntyped.note(format!(
                "{} of {control_count} bounded offset-store control block(s) have no admitted complete grammar.",
                control_count - classified_control_count
            )));
    }
    for entry in &scan.container.entries {
        let content = entry.content();
        if content.retains_opaque_payload()
            && !(typed_native == TypedNative::Available
                && content == EntryContent::SaveToggleInfo
                && crate::native::toggle::has_complete_saved_toggle_stream(&scan.container))
        {
            body.losses.push(NxLossCode::ContainerStreamOpaque.note(format!(
                    "Named container stream {} is classified as {} and retained byte-exact; its field semantics are not completely typed.",
                    entry.name,
                    content.label()
                )));
        }
    }
    for entry in scan.container.bounded_indexed_om_entries() {
        body.losses.push(NxLossCode::SourceScanBounded.note(format!(
            "The indexed object-model section search in {} reached its work bound of {}; \
             its remaining sections were skipped.",
            entry.name,
            crate::om::MAX_INDEXED_SECTION_REGISTRY_WORK,
        )));
    }
    for (index, stream) in scan.streams.iter().enumerate() {
        if !stream.kind().is_parasolid() {
            body.losses
                .push(NxLossCode::NonParasolidStreamOmitted.note(format!(
                    "Non-Parasolid {} stream #{index} was classified but not transferred.",
                    stream.kind().label()
                )));
        }
    }
}

pub(super) fn offset_store_control_counts(container: &Container) -> (usize, usize) {
    let mut total = 0;
    let mut classified = 0;
    for (_, section) in container.indexed_om_sections() {
        let Some((control, _, records)) = section.as_offset_only() else {
            continue;
        };
        total += 1;
        if crate::om::offset_store_control_form(
            control.bytes,
            records.first().map(|record| record.bytes),
        )
        .is_some()
        {
            classified += 1;
        }
    }
    (total, classified)
}

/// Aggregate carrier counts across the decoded streams, for reporting.
#[derive(Debug, Default)]
struct Counts {
    points: usize,
    planes: usize,
    cylinders: usize,
    cones: usize,
    spheres: usize,
    tori: usize,
    nurbs_surfaces: usize,
    offset_surfaces: usize,
    blend_surfaces: usize,
    lines: usize,
    circles: usize,
    ellipses: usize,
    nurbs_curves: usize,
    intersection_curves: usize,
    intersection_rejections: crate::intersection::RejectionCounts,
}

impl Counts {
    fn surfaces(&self) -> usize {
        self.planes
            + self.cylinders
            + self.cones
            + self.spheres
            + self.tori
            + self.nurbs_surfaces
            + self.offset_surfaces
            + self.blend_surfaces
    }
    fn curves(&self) -> usize {
        self.lines + self.circles + self.ellipses + self.nurbs_curves + self.intersection_curves
    }
}

fn build_metadata_ir(
    ctx: &DecodeContext<'_>,
    scan: &Scan,
    dialects: &DialectLayers,
) -> Result<
    (
        CadIr,
        cadmpeg_ir::Annotations,
        Vec<UnknownRecord>,
        Vec<LossNote>,
    ),
    CodecError,
> {
    let mut unknowns = Vec::new();
    let mut ir = CadIr::decoded(source_meta(scan, dialects, None)?);
    let mut annotations = AnnotationBuilder::new();
    let mut losses = Vec::new();
    let source_stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
    for (si, stream) in scan.streams.iter().enumerate() {
        if stream.kind().is_parasolid() {
            let unknown = unknown_stream(ctx, si, stream)?;
            annotations.note(
                unknown.id().as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(stream.file_offset),
                Some(stream.kind().label()),
            );
            annotations.exactness(unknown.id().as_str(), Exactness::Derived);
            scan.note_untyped(unknown.id(), [ArenaClass::Model]);
            unknowns.push(unknown);
        }
    }
    if ctx.container_only() {
        crate::native::attach_container_layer(
            ctx,
            &mut ir,
            scan,
            &mut annotations,
            &mut unknowns,
            TypedNative::ContainerOnly,
        )?;
    } else {
        let mut parsed = crate::native::substrate::ParsedStreams::parse(scan)?;
        losses.extend(parsed.primary_losses());
        let model = crate::native::model::NativeModel::extract(
            ctx,
            &scan.container,
            &scan.streams,
            &mut parsed,
            None,
        )?;
        crate::native::attach_annotations(
            ctx,
            &mut ir,
            &model,
            scan,
            &mut annotations,
            &mut unknowns,
            &mut losses,
        )?;
    }
    Ok((ir, annotations.build(), unknowns, losses))
}

fn build_container_body(
    scan: &Scan,
    dialect_losses: Vec<LossNote>,
    notes: Vec<String>,
) -> cadmpeg_ir::codec::DecodeBody {
    let assembly = scan.container.has_external_references() && !scan.has_parasolid();

    let loss = if assembly {
        NxLossCode::AssemblyComponentsExternal.note(
            "No inline Parasolid geometry: this is an assembly .prt. Component geometry \
             lives in external child .prt files named in EXTREFSTREAM.",
        )
    } else {
        NxLossCode::GeometryNotTransferred.note(
            "No B-rep geometry was transferred: no gate-passing analytic carrier was found \
                      in the embedded Parasolid streams (they may hold only B-spline/procedural \
                      geometry this codec does not yet type). The streams are preserved verbatim as \
                      unknown passthrough records.",
        )
    };

    let mut body = DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(false),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes,
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    };
    body.losses.push(loss);
    body.losses.extend(dialect_losses);
    body
}

#[cfg(test)]
mod tests;
