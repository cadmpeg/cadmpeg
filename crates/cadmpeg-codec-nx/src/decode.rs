// SPDX-License-Identifier: Apache-2.0
//! Build IR and diagnostics from an NX native container.
//!
//! [`scan`] parses the container and inflates its embedded streams. [`decode`]
//! converts supported analytic and NURBS carriers to millimetres, resolves
//! supported topology, preserves each Parasolid stream as an unknown record, and
//! returns a [`DecodeReport`] describing incomplete transfer. Partition and
//! deltas streams are both decoded; callers must use the report to account for
//! unresolved active-face selection and other loss.
//!
//! [`DecodeReport`]: cadmpeg_ir::report::DecodeReport

use cadmpeg_core::bytes::assemble_u32_be;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::debug_assert_primary_layer;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::DecodeReport;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::container::{self, Container, EntryContent};
use crate::dialect::NxDialect;
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
    extrude_extent_is_incomplete, extrude_start_is_incomplete, face_blend_definition_is_incomplete,
    face_selection_is_incomplete, face_selections_overlap, fillet_definition_is_incomplete,
    finite_feature_point, hole_auxiliary_semantics_are_incomplete, hole_definition_is_incomplete,
    hole_feature_is_incomplete, incomplete_expression_parameters, loft_definition_is_incomplete,
    loft_section_is_incomplete, offset_surface_definition_is_incomplete,
    output_free_local_body_construction, output_free_native_snapshot,
    output_free_pattern_construction, output_free_trim_surface_construction,
    path_ref_is_incomplete, pattern_feature_is_incomplete, pattern_is_incomplete,
    pattern_occurrence_count, positive_feature_length, profile_dependency_is_incomplete,
    profile_ref_is_incomplete, projected_curve_direction_is_incomplete, radius_spec_is_incomplete,
    replace_face_definition_is_incomplete, resolved_body_selection_len,
    revolve_definition_is_incomplete, revolve_feature_is_incomplete, rib_definition_is_incomplete,
    rib_feature_is_incomplete, sew_bodies_definition_is_incomplete, shell_definition_is_incomplete,
    sphere_definition_is_incomplete, sweep_definition_is_incomplete, sweep_mode_is_incomplete,
    sweep_orientation_is_incomplete, termination_dependency_is_incomplete,
    termination_is_incomplete, thicken_definition_is_incomplete,
    trim_bodies_definition_is_incomplete, trim_surface_definition_is_incomplete,
    unit_feature_direction, valid_draft_angle, valid_feature_direction,
};

mod geometry_work;

mod pcurves;
#[cfg(test)]
pub(crate) use pcurves::blend_boundary_parameter_from_support_spine;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use pcurves::complete_tolerant_intersection_pcurves_from_serialized_branches;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use pcurves::{
    attach_tolerant_edge_intersections, blend_boundary_parameter_from_support_spine_with_index,
    coincident_pcurve_pair, exact_boundary_pcurve, orient_tolerant_intersection_pcurve,
    pcurve_matches_edge, pcurve_matches_edge_range,
};
#[allow(unused_imports)]
pub(crate) use pcurves::{
    attach_tolerant_edge_intersections_with_budget, exact_boundary_curve_breaks,
    ordered_parameter_range, pcurve_matches_edge_range_with_index_and_budget,
    pcurve_parameter_range, reverse_pcurve_over_range,
};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use pcurves::{
    complete_intersection_pcurves_from_coedge_incidence,
    complete_intersection_supports_from_edge_incidence,
    complete_tolerant_intersection_pcurves_from_serialized_branches_with_budget,
    exact_analytic_isocurve_pcurve,
};

mod offset;
#[allow(unused_imports)]
pub(crate) use offset::{
    active_spline_controls, clamp_intersection_parameters, clamp_surface_parameters,
    clamp_surface_parameters_with_periods, coarse_model_surface_parameters,
    correct_intersection_parameters, determinant_3x3, intersection_parameter_jacobian,
    intersection_parameter_tangent, intersection_side, least_squares_step, lift_periodic_parameter,
    model_surface_derivative, normalize_pcurve_parameters, null_vector_3x4, nurbs_active_domain,
    parameter_derivative_step, point_distance, positive_weights, saved_offset_carriers, solve_4x4,
    solve_damped_least_squares_4x4, subdivide_offset_rectangle, surface_parameters,
    translation_net_normal, HomogeneousControlBounds, HomogeneousSurfaceNet,
    IntersectionParameterSpace,
};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use offset::{
    certified_offset_cache_fit, continue_surface_intersection_parameters,
    continue_surface_intersection_parameters_with_seeds, offset_surface_parameters,
    offset_surface_parameters_with_tolerance, offset_surface_parameters_with_tolerance_with_index,
};

mod build;
#[allow(unused_imports)]
pub(crate) use build::{
    classify_body_kinds, finalize_point_topology, ordered_curve_candidates,
    ordered_fixed_candidates, ordered_point_candidates, ordered_surface_candidates,
    prune_inactive_geometry, prune_inactive_topology, prune_unreferenced_unknown_carriers,
    retain_live_annotations, retain_live_unknown_links, rmfastload_allows_terminal_lineage,
    rmfastload_selected_bodies, rmfastload_stream_indices, select_active_body,
    select_terminal_feature_bodies, topology_body_node_ids, try_decode_geometry, GeometryDecode,
};

mod support_uv;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use support_uv::{complete_ext11_support_uv, invalidate_inconsistent_support_uv};
#[allow(unused_imports)]
pub(crate) use support_uv::{
    complete_ext11_support_uv_with_budget, complete_parameterization_equivalent_support_uv,
    linear_knots, missing_support_parameter, pcurve_control_point_seed, pcurve_requires_completion,
    pending_support_lanes_requiring_completion, support_uv_lane_matches_surface_with_budget,
    PendingExt11SupportUv, SerializedSupportUv,
};

#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use support_uv::parameterization_equivalent_surfaces;

mod blend;
#[allow(unused_imports)]
pub(crate) use blend::analytic_surface_offset;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use blend::closest_pcurve_parameters;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use blend::closest_spine_parameter;
#[allow(unused_imports)]
pub(crate) use blend::{
    add_bernstein_polynomials, bernstein_product, bezier_spans, binomial_coefficient,
    blend_boundary_parameter_from_support_pcurve_with_budget,
    blend_boundary_point_with_index_and_budget, blend_contact_offset_matches,
    blend_surface_point_from_frame, blend_surface_point_inner_with_index_and_budget,
    blend_surface_u_derivative_with_index_and_budget, closest_blend_surface_grid_parameters,
    closest_parameter_candidates, closest_periodic_analytic_curve_parameter_with_budget,
    decoded_surface_point_inner_with_budget, homogeneous_residual_distance,
    insert_homogeneous_curve_knot, lift_periodic_parameters, polynomial_roots_in_unit_interval,
    polynomial_value, rational_squared_distance_derivative, real_polynomial_roots,
    rodrigues_rotate, scalar_bernstein_sign_variations, scalar_bezier_value, signed_angle,
    spine_contact_point_with_index_and_budget, stationary_rational_distance_candidates,
    subdivide_scalar_bezier_span, subtract_bernstein_polynomials, sum_bernstein_polynomials,
    BezierSpan, BlendContactDerivativeContext, BlendParameterGrid, BlendSurfaceFrame,
    BoundaryInverseTarget, HomogeneousCurveSpans, ScalarBezierRoots, ScalarBezierSpan,
};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use blend::{
    blend_surface_parameters, blend_surface_parameters_for_fit,
    blend_surface_parameters_for_fit_with_grid, blend_surface_point, blend_surface_point_inner,
    blend_surface_u_derivative, closest_nurbs_curve_parameter, coarse_blend_surface_parameters,
    refine_blend_surface_parameters, surface_contact_direction,
    surface_contact_direction_with_index,
};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use blend::{constant_surface_offset_between, surface_offset_lineage};

mod emit;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use emit::orient_edge_range;
#[allow(unused_imports)]
pub(crate) use emit::{
    annotate_node, canonical_trim_range, curve_tag, decoded_tolerance,
    retain_unresolved_topology_carriers, sense, source_meta, surface_tag, unknown_stream,
};

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
pub fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<DecodeResult, CodecError> {
    let scan = scan(ctx, root)?;

    let mut admitted_entities = 0_u64;
    if ctx.container_only() {
        ctx.charge_entities(scan.streams.len() as u64, "admit NX streams")?;
        let (ir, annotations, unknowns) = build_container_only_ir(ctx, &scan)?;
        let mut report = build_container_report(&scan, true);
        report_untransferred_streams(&scan, &mut report, false);
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
    report_untransferred_streams(&scan, &mut report, true);
    decode_result(
        ctx,
        ir,
        report,
        annotations,
        unknowns,
        &mut admitted_entities,
    )
}

fn build_container_only_ir(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
) -> Result<(CadIr, cadmpeg_ir::Annotations, Vec<UnknownRecord>), CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    for (si, stream) in scan.streams.iter().enumerate() {
        if stream.kind.is_parasolid() {
            let unknown = unknown_stream(ctx, si, stream)?;
            let source_stream = annotations.stream("nx:container");
            annotations
                .note(&unknown.id, source_stream, stream.file_offset as u64)
                .tag(stream.kind.label());
            annotations.exactness(&unknown.id, Exactness::Derived);
            unknowns.push(unknown);
        }
    }
    crate::native::attach_container_layer(
        ctx,
        &mut ir,
        scan,
        &mut annotations,
        &mut unknowns,
        false,
    )?;
    Ok((ir, annotations.build(), unknowns))
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

fn report_untransferred_streams(
    scan: &Scan,
    report: &mut DecodeReport,
    typed_native_available: bool,
) {
    let (control_count, classified_control_count) = offset_store_control_counts(&scan.container);
    if classified_control_count != control_count {
        report.losses.push(NxLossCode::OffsetStoreControlUntyped.note(format!(
            "{} of {control_count} bounded offset-store control block(s) have no admitted complete grammar.",
            control_count - classified_control_count
        )));
    }
    for entry in &scan.container.entries {
        let content = entry.content();
        if content.retains_opaque_payload()
            && !(typed_native_available
                && content == EntryContent::SaveToggleInfo
                && crate::native::has_complete_saved_toggle_stream(&scan.container))
        {
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
        .filter_map(|(_, section)| {
            section
                .control
                .map(|control| (control, section.records.first().map(|record| record.bytes)))
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
) -> Result<(CadIr, cadmpeg_ir::Annotations, Vec<UnknownRecord>), CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    for (si, stream) in scan.streams.iter().enumerate() {
        if stream.kind.is_parasolid() {
            let unknown = unknown_stream(ctx, si, stream)?;
            let source_stream = annotations.stream("nx:container");
            annotations
                .note(&unknown.id, source_stream, stream.file_offset as u64)
                .tag(stream.kind.label());
            annotations.exactness(&unknown.id, Exactness::Derived);
            unknowns.push(unknown);
        }
    }
    let mut parsed = crate::native::ParsedStreams::parse(scan);
    let model = crate::native::NativeModel::extract(
        ctx,
        root,
        &scan.container,
        &scan.streams,
        &mut parsed,
        None,
    );
    crate::native::attach_annotations(ctx, &mut ir, &model, scan, &mut annotations, &mut unknowns)?;
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

    let dialects = vec![NxDialect::classify(&scan.container)];
    debug_assert_primary_layer(&dialects, crate::dialect::FORMAT);
    DecodeReport {
        dialects,
        format: crate::dialect::FORMAT.to_string(),
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
    let mut notes = if c.is_legacy_cfb() {
        vec![format!(
            "legacy CFB container: UGII payload version {:#04x}, {} directory entr{}",
            c.version,
            c.header_entry_count,
            if c.header_entry_count == 1 {
                "y"
            } else {
                "ies"
            },
        )]
    } else {
        vec![format!(
            "SPLMSSTR container: version {:#04x}, file tag {}, footer offset {}, {} HEADER and {} FOOTER directory entry/ies, fingerprint {:08x}",
            c.version,
            c.file_tag,
            c.footer_offset,
            c.header_entry_count,
            c.footer_entry_count,
            assemble_u32_be(c.footer_fingerprint),
        )]
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
