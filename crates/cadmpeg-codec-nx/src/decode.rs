// SPDX-License-Identifier: Apache-2.0
//! Build IR and diagnostics from an NX SPLMSSTR container.
//!
//! [`scan`] parses the container and inflates its embedded streams. [`decode`]
//! converts supported analytic and NURBS carriers to millimetres, resolves
//! supported topology, preserves each Parasolid stream as an unknown record, and
//! returns a [`DecodeReport`] describing incomplete transfer. Partition and
//! deltas streams are both decoded; callers must use the report to account for
//! unresolved active-face selection and other loss.
//!
//! [`DecodeReport`]: cadmpeg_ir::report::DecodeReport

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::DecodeReport;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::{self, Container};
use crate::loss::NxLossCode;
use crate::parasolid::{self, Stream, StreamKind};

mod jpeg;
pub(crate) use jpeg::jpeg_dimensions;

mod report;
#[allow(unused_imports)]
pub(crate) use report::append_design_intent_losses;

mod feature_completeness;
#[allow(unused_imports)]
pub(crate) use feature_completeness::{
    active_configuration_state_is_incomplete, body_selection_is_incomplete,
    body_selections_overlap, chamfer_definition_is_incomplete, combine_definition_is_incomplete,
    datum_coordinate_system_is_incomplete, datum_plane_is_incomplete,
    delete_body_definition_is_incomplete, edge_selection_is_incomplete,
    extend_surface_definition_is_incomplete, extrude_definition_is_incomplete,
    extrude_extent_is_incomplete, extrude_start_is_incomplete, face_selection_is_incomplete,
    face_selections_overlap, hole_auxiliary_semantics_are_incomplete,
    hole_definition_is_incomplete, hole_feature_is_incomplete, incomplete_expression_parameters,
    loft_definition_is_incomplete, loft_section_is_incomplete, output_free_local_body_construction,
    output_free_native_snapshot, output_free_pattern_construction, path_ref_is_incomplete,
    pattern_feature_is_incomplete, pattern_is_incomplete, pattern_occurrence_count,
    profile_dependency_is_incomplete, profile_ref_is_incomplete,
    projected_curve_direction_is_incomplete, radius_spec_is_incomplete,
    replace_face_definition_is_incomplete, revolve_definition_is_incomplete,
    revolve_feature_is_incomplete, rib_definition_is_incomplete, rib_feature_is_incomplete,
    sew_bodies_definition_is_incomplete, sweep_definition_is_incomplete, sweep_mode_is_incomplete,
    sweep_orientation_is_incomplete, termination_dependency_is_incomplete,
    termination_is_incomplete, trim_bodies_definition_is_incomplete,
    trim_surface_definition_is_incomplete,
};

mod pcurves;
#[cfg(test)]
pub(crate) use pcurves::blend_boundary_parameter_from_support_spine;
#[allow(unused_imports)]
pub(crate) use pcurves::{
    attach_tolerant_edge_intersections, coincident_pcurve_pair,
    complete_exact_boundary_intersection_pcurves,
    complete_intersection_pcurves_from_coedge_incidence,
    complete_intersection_pcurves_from_opposite_charts,
    complete_intersection_supports_from_edge_incidence,
    complete_tolerant_intersection_pcurves_from_serialized_branches,
    exact_analytic_isocurve_pcurve, exact_boundary_pcurve, orient_tolerant_intersection_pcurve,
    pcurve_matches_edge, reverse_pcurve_over_range,
};

mod offset;
#[allow(unused_imports)]
pub(crate) use offset::{
    certified_offset_cache_fit, point_distance, solve_damped_least_squares_4x4,
    subdivide_offset_rectangle, surface_parameter_periods, translation_net_normal,
};
#[cfg(test)]
pub(crate) use offset::{
    continue_surface_intersection_parameters, offset_surface_parameters,
    offset_surface_parameters_with_tolerance,
};

mod build;
#[allow(unused_imports)]
pub(crate) use build::{
    ordered_curve_candidates, ordered_point_candidates, ordered_surface_candidates,
    select_active_body, try_decode_geometry,
};

mod support_uv;
#[allow(unused_imports)]
pub(crate) use support_uv::{
    assign_ext11_support_uv_to_surfaces, attach_completed_intersection_pcurves,
    blend_spine_cache_fit_tolerance, complete_ext11_support_uv,
    complete_parameterization_equivalent_support_uv, complete_support_uv,
    invalidate_inconsistent_support_uv, parameterization_equivalent_surfaces,
};

mod blend;
#[allow(unused_imports)]
pub(crate) use blend::{
    analytic_surface_offset, bezier_spans, blend_contact_offset_matches,
    closest_nurbs_curve_parameter, closest_pcurve_parameters, closest_spine_parameter,
    constant_surface_offset_between, homogeneous_residual_distance, real_polynomial_roots,
    surface_offset_lineage,
};
#[cfg(test)]
pub(crate) use blend::{
    blend_surface_parameters, blend_surface_parameters_for_fit, blend_surface_point,
    blend_surface_u_derivative, coarse_blend_surface_parameters, refine_blend_surface_parameters,
    surface_contact_direction,
};

mod emit;
#[allow(unused_imports)]
pub(crate) use emit::{decoded_tolerance, orient_edge_range, source_meta, unknown_stream};

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
        self.streams.iter().filter(|s| s.kind == kind).count()
    }

    /// Return whether the file contains an inline Parasolid stream.
    ///
    /// NX assemblies may contain only references to external child parts.
    pub fn has_parasolid(&self) -> bool {
        self.streams.iter().any(|s| s.kind.is_parasolid())
    }
}

/// Parse the SPLMSSTR container and inflate streams in its canonical part entry.
pub fn scan<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Scan<'a>, CodecError> {
    let container = container::scan_bytes(root.window())?;
    let streams = parasolid::extract_streams(ctx, root, &container)?;
    Ok(Scan { container, streams })
}

/// Decode an NX `.prt` into IR and a loss report.
///
/// When [`DecodeContext::container_only`] is set, the returned IR contains source
/// metadata and preserved streams but no typed entities. Otherwise the decoder
/// emits supported geometry and resolvable topology. A valid container can
/// decode successfully with no geometry, including an assembly whose geometry
/// resides in external child parts.
pub fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<DecodeResult, CodecError> {
    let scan = scan(ctx, root)?;

    let mut admitted_entities = 0_u64;
    if ctx.container_only() {
        let (ir, annotations, unknowns) = build_metadata_ir(ctx, root, &scan)?;
        let mut report = build_container_report(&scan, true);
        report_untransferred_streams(&scan, &mut report);
        return decode_result(
            ctx,
            ir,
            report,
            annotations,
            unknowns,
            &mut admitted_entities,
        );
    }

    // Charge stream cardinality before geometry construction.
    ctx.charge_entities(scan.streams.len() as u64, "admit NX streams")?;

    if let Some((ir, report, annotations, unknowns)) =
        try_decode_geometry(ctx, root, &scan, &mut admitted_entities)?
    {
        return decode_result(
            ctx,
            ir,
            report,
            annotations,
            unknowns,
            &mut admitted_entities,
        );
    }

    let (ir, annotations, unknowns) = build_metadata_ir(ctx, root, &scan)?;
    let mut report = build_container_report(&scan, false);
    report_untransferred_streams(&scan, &mut report);
    decode_result(
        ctx,
        ir,
        report,
        annotations,
        unknowns,
        &mut admitted_entities,
    )
}

fn decode_result(
    ctx: &DecodeContext<'_>,
    mut ir: CadIr,
    report: DecodeReport,
    annotations: cadmpeg_ir::Annotations,
    unknowns: Vec<UnknownRecord>,
    admitted_entities: &mut u64,
) -> Result<DecodeResult, CodecError> {
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        admitted_entities,
        "admit NX entities",
    )?;
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations);
    source_fidelity.attach_native_unknown_records(&mut ir, "nx", unknowns)?;
    Ok(DecodeResult::new(ir, report, source_fidelity))
}

fn report_untransferred_streams(scan: &Scan, report: &mut DecodeReport) {
    let (control_count, classified_control_count) = offset_store_control_counts(&scan.container);
    if classified_control_count != control_count {
        report.losses.push(NxLossCode::OffsetStoreControlUntyped.note(format!(
            "{} of {control_count} bounded offset-store control block(s) have no admitted complete grammar.",
            control_count - classified_control_count
        )));
    }
    for entry in &scan.container.entries {
        let content = entry.content();
        if content.retains_opaque_payload() {
            report.losses.push(NxLossCode::ContainerStreamOpaque.note(format!(
                "Named container stream {} is classified as {} and retained byte-exact; its field semantics are not completely typed.",
                entry.name,
                content.label()
            )));
        }
    }
    for (index, stream) in scan.streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            report
                .losses
                .push(NxLossCode::NonParasolidStreamOmitted.note(format!(
                    "Non-Parasolid {} stream #{index} was classified but not transferred.",
                    stream.kind.label()
                )));
        }
    }
}

fn offset_store_control_counts(container: &Container) -> (usize, usize) {
    container
        .indexed_om_sections()
        .into_iter()
        .filter_map(|(_, section)| section.control)
        .fold((0, 0), |(total, classified), control| {
            (
                total + 1,
                classified
                    + usize::from(crate::om::offset_store_control_form(control.bytes).is_some()),
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
) -> Result<(CadIr, cadmpeg_ir::Annotations, Vec<UnknownRecord>), CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    for (si, stream) in scan.streams.iter().enumerate() {
        if stream.kind.is_parasolid() {
            let unknown = unknown_stream(si, stream);
            let source_stream = annotations.stream("nx:container");
            annotations
                .note(&unknown.id, source_stream, stream.file_offset as u64)
                .tag(stream.kind.label());
            annotations.exactness(&unknown.id, Exactness::Derived);
            unknowns.push(unknown);
        }
    }
    let parsed = crate::native::ParsedStreams::parse(scan);
    let model =
        crate::native::NativeModel::extract(ctx, root, &scan.container, &scan.streams, &parsed);
    crate::native::attach_annotations(&mut ir, &model, scan, &mut annotations, &mut unknowns)
        .map_err(|error| CodecError::Malformed(error.to_string()))?;
    Ok((ir, annotations.build(), unknowns))
}

fn build_container_report(scan: &Scan, container_only: bool) -> DecodeReport {
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

    if container_only {
        losses.push(
            NxLossCode::ContainerOnly
                .note("Container-only decode requested; entity decode was not attempted."),
        );
    }

    DecodeReport {
        format: "nx".to_string(),
        container_only,
        geometry_transferred: false,
        coverage: std::collections::BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses,
        notes: summary_notes(scan),
    }
}

/// Build container and embedded-stream notes for inspection and decode reports.
pub fn summary_notes(scan: &Scan) -> Vec<String> {
    let c = &scan.container;
    let (control_count, classified_control_count) = offset_store_control_counts(c);
    let mut notes = vec![format!(
        "SPLMSSTR container: version {:#04x}, file tag {}, footer offset {}, {} HEADER and {} FOOTER directory entry/ies, fingerprint {:08x}",
        c.version,
        c.file_tag,
        c.footer_offset,
        c.header_entry_count,
        c.footer_entry_count,
        u32::from_be_bytes(c.footer_fingerprint),
    )];
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
    if let Some(schema) = scan.streams.iter().find_map(|s| s.schema.as_deref()) {
        notes.push(format!("Parasolid schema: {schema}"));
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
            .filter(|(_, section)| {
                section
                    .records
                    .first()
                    .is_some_and(|record| record.object_id.is_some())
            })
            .map(|(_, section)| section.records.len())
            .sum::<usize>();
        let blocks = om_sections
            .iter()
            .filter(|(_, section)| {
                section
                    .records
                    .first()
                    .is_some_and(|record| record.object_id.is_none())
            })
            .map(|(_, section)| section.records.len() + usize::from(section.control.is_some()))
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
    notes
}

#[cfg(test)]
mod feature_completeness_tests;
#[cfg(test)]
mod tests;
