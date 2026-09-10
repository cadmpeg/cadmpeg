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

use cadmpeg_core::bytes::assemble_u32_be;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::{self, Container, EntryContent};
use crate::loss::NxLossCode;
use crate::native::TypedNative;
use crate::parasolid::{self, Stream, StreamKind};

mod blend;
mod build;
pub(crate) mod emit;
pub(crate) mod feature_completeness;
mod geometry_work;
pub(crate) mod ids;
pub(crate) mod jpeg;
mod offset;
pub(crate) mod pcurves;
pub(crate) mod report;
mod support_uv;

use build::try_decode_geometry;
use emit::{source_meta, unknown_stream};

pub(crate) const MISSING_TOLERANCE: f64 = -31_415_800_000_000.0;
/// Parsed container data shared by inspection and entity decoding.
pub struct Scan<'a> {
    /// Parsed SPLMSSTR container.
    pub container: Container<'a>,
    /// Located and inflated Parasolid or preview streams.
    pub streams: Vec<Stream>,
}

impl Scan<'_> {
    /// Count streams with the requested classification.
    pub fn count(&self, kind: StreamKind) -> usize {
        self.streams.iter().filter(|s| s.kind() == kind).count()
    }

    /// Return whether the file contains an inline Parasolid stream.
    ///
    /// NX assemblies may contain only references to external child parts.
    pub fn has_parasolid(&self) -> bool {
        self.streams.iter().any(|s| s.kind().is_parasolid())
    }
}

/// Parse the SPLMSSTR container and inflate streams in its canonical part entry.
pub fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Scan<'a>, CodecError> {
    let (container, streams) = if container::looks_like_nx(root.window()) {
        let container = container::scan_bytes(root.window())?;
        let streams = parasolid::extract_streams(ctx, root, &container)?;
        (container, streams)
    } else {
        let (container, part) = container::scan_legacy(ctx, root)?;
        let streams = parasolid::extract_legacy_streams(ctx, part)?;
        (container, streams)
    };
    Ok(Scan { container, streams })
}

/// Decode an NX `.prt` into IR and a loss report.
///
/// When [`DecodeContext::container_only`] is set, the returned IR contains source
/// metadata and preserved streams but no typed entities. Otherwise the decoder
/// emits supported geometry and resolvable topology. A valid container can
/// decode successfully with no geometry, including an assembly whose geometry
/// resides in external child parts.
pub fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Decoded, CodecError> {
    let scan = scan(ctx, root)?;
    let (classification, notes) = summarize(&scan);
    let (dialects, dialect_losses) = classification.into_report_parts();

    let mut admitted_entities = 0_u64;
    ctx.charge_entities(scan.streams.len() as u64, "admit NX streams")?;
    if ctx.container_only() {
        let (ir, annotations, unknowns) = build_metadata_ir(ctx, root, &scan, &dialects)?;
        let mut body = build_container_body(&scan, dialect_losses, notes);
        report_untransferred_streams(&scan, &mut body, TypedNative::ContainerOnly);
        return decoded(ctx, ir, body, annotations, unknowns, &mut admitted_entities);
    }

    if let Some((ir, body, annotations, unknowns)) = try_decode_geometry(
        ctx,
        root,
        &scan,
        &dialects,
        &dialect_losses,
        &notes,
        &mut admitted_entities,
    )? {
        return decoded(ctx, ir, body, annotations, unknowns, &mut admitted_entities);
    }

    let (ir, annotations, unknowns) = build_metadata_ir(ctx, root, &scan, &dialects)?;
    let mut body = build_container_body(&scan, dialect_losses, notes);
    report_untransferred_streams(&scan, &mut body, TypedNative::Available);
    decoded(ctx, ir, body, annotations, unknowns, &mut admitted_entities)
}

fn decoded(
    ctx: &DecodeContext<'_>,
    mut ir: CadIr,
    body: DecodeBody,
    annotations: cadmpeg_ir::Annotations,
    unknowns: Vec<UnknownRecord>,
    admitted_entities: &mut u64,
) -> Result<Decoded, CodecError> {
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        admitted_entities,
        "admit NX entities",
    )?;
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations);
    source_fidelity.attach_native_unknown_records(&mut ir, "nx", unknowns)?;
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
                && crate::native::has_complete_saved_toggle_stream(&scan.container))
        {
            body.losses.push(NxLossCode::ContainerStreamOpaque.note(format!(
                "Named container stream {} is classified as {} and retained byte-exact; its field semantics are not completely typed.",
                entry.name,
                content.label()
            )));
        }
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

fn offset_store_control_counts(container: &Container) -> (usize, usize) {
    container
        .indexed_om_sections()
        .into_iter()
        .filter_map(|(_, section)| {
            section.as_offset_only().map(|(control, _, records)| {
                (control.clone(), records.first().map(|record| record.bytes))
            })
        })
        .fold((0, 0), |(total, classified), (control, first_record)| {
            (
                total + 1,
                classified
                    + usize::from(
                        crate::om::offset_store_control_form(control.bytes, first_record).is_some(),
                    ),
            )
        })
}

/// Aggregate carrier counts across the decoded streams, for reporting.
#[derive(Debug, Default)]
pub(crate) struct Counts {
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
    root: View<'_>,
    scan: &Scan,
    dialects: &DialectLayers,
) -> Result<(CadIr, cadmpeg_ir::Annotations, Vec<UnknownRecord>), CodecError> {
    let mut ir = CadIr::decoded(source_meta(scan, dialects));
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    for (si, stream) in scan.streams.iter().enumerate() {
        if stream.kind().is_parasolid() {
            let unknown = unknown_stream(ctx, si, stream)?;
            let source_stream = annotations.stream("nx:container");
            annotations
                .note(unknown.id(), &source_stream, stream.file_offset as u64)
                .tag(stream.kind().label());
            annotations.exactness(unknown.id(), Exactness::Derived);
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
        let mut parsed = crate::native::ParsedStreams::parse(scan);
        let model = crate::native::NativeModel::extract(
            ctx,
            root,
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
        )?;
    }
    Ok((ir, annotations.build(), unknowns))
}

fn build_container_body(
    scan: &Scan,
    dialect_losses: Vec<LossNote>,
    notes: Vec<String>,
) -> DecodeBody {
    let mut losses = Vec::new();

    let assembly = scan
        .container
        .entries
        .iter()
        .any(|e| e.name.contains("ExternalReferences"))
        && !scan.has_parasolid();

    if assembly {
        losses.push(NxLossCode::AssemblyComponentsExternal.note(
            "No inline Parasolid geometry: this is an assembly .prt. Component geometry \
                      lives in external child .prt files named in EXTREFSTREAM, and the assembled \
                      solid's inputs (child partitions + constraint solve) are absent from this \
                      file. This is an external-dependency boundary, not a decode gap.",
        ));
    } else {
        losses.push(NxLossCode::GeometryNotTransferred.note(
            "No B-rep geometry was transferred: no gate-passing analytic carrier was found \
                      in the embedded Parasolid streams (they may hold only B-spline/procedural \
                      geometry this codec does not yet type). The streams are preserved verbatim as \
                      unknown passthrough records.",
        ));
    }

    losses.extend(dialect_losses);
    DecodeBody {
        transfer: cadmpeg_ir::report::DecodeTransfer::full(false),
        coverage: cadmpeg_ir::Coverage::default(),
        losses,
        notes,
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
    }
}

/// Classify a scan and build its inspection and decode notes.
pub fn summarize(scan: &Scan) -> (crate::dialect::LayerClassification, Vec<String>) {
    let c = &scan.container;
    let (control_count, classified_control_count) = offset_store_control_counts(c);
    let header_entry_count = c.entry_count(crate::container::Region::Header);
    let footer_entry_count = c.entry_count(crate::container::Region::Footer);
    let mut notes = match c.layout {
        crate::container::ContainerLayout::LegacyCfb { .. } => vec![format!(
            "legacy CFB container: {} directory entr{}",
            header_entry_count,
            if header_entry_count == 1 {
                "y"
            } else {
                "ies"
            },
        )],
        crate::container::ContainerLayout::Modern {
            file_tag,
            footer_offset,
            footer_fingerprint,
            ..
        } => vec![format!(
            "SPLMSSTR container: file tag {}, footer offset {}, {} HEADER and {} FOOTER directory entry/ies, fingerprint {:08x}",
            file_tag,
            footer_offset,
            header_entry_count,
            footer_entry_count,
            assemble_u32_be(footer_fingerprint),
        )],
    };
    notes.push(format!(
        "embedded streams: {} partition, {} deltas, {} plain (cached body), {} preview/non-Parasolid",
        scan.count(StreamKind::Partition),
        scan.count(StreamKind::Deltas),
        scan.count(StreamKind::Plain),
        scan.count(StreamKind::Preview),
    ));
    if control_count != 0 {
        notes.push(format!(
            "NX object model: {classified_control_count} of {control_count} bounded offset-store control block(s) have an admitted complete grammar"
        ));
    }
    let framed_om_sections = c.om_sections();
    if !framed_om_sections.is_empty() {
        let declarations = framed_om_sections
            .iter()
            .map(|(_, section)| section.types.len())
            .sum::<usize>();
        let fields = framed_om_sections
            .iter()
            .map(|(_, section)| section.fields.len())
            .sum::<usize>();
        notes.push(format!(
            "NX object model: {} size-framed section(s), {} class declaration(s), {} field declaration(s)",
            framed_om_sections.len(),
            declarations,
            fields
        ));
    }
    let om_sections = c.indexed_om_sections();
    if !om_sections.is_empty() {
        let entities = om_sections
            .iter()
            .filter_map(|(_, section)| section.as_fixed())
            .map(<[crate::om::FixedEntityRecord<'_>]>::len)
            .sum::<usize>();
        let blocks = om_sections
            .iter()
            .filter_map(|(_, section)| section.as_offset_only())
            .map(|(_control, _, records)| records.len() + 1)
            .sum::<usize>();
        if blocks == 0 {
            notes.push(format!(
                "NX object model: {} indexed section(s), {} bounded entity record(s)",
                om_sections.len(),
                entities
            ));
        } else {
            notes.push(format!(
                "NX object model: {} indexed section(s), {} ID-bounded entity record(s), {} offset-only data block(s)",
                om_sections.len(),
                entities,
                blocks
            ));
        }
    }
    if !scan.has_parasolid()
        && c.entries
            .iter()
            .any(|e| e.name.contains("ExternalReferences"))
    {
        notes.push(
            "no inline Parasolid geometry (assembly .prt: geometry in external child parts)"
                .to_string(),
        );
    }
    (crate::dialect::classify_layers(scan), notes)
}

#[cfg(test)]
mod feature_completeness_tests;
#[cfg(test)]
mod tests;
