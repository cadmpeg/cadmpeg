// SPDX-License-Identifier: Apache-2.0
//! Loss notes derived from coverage counters and undecoded PSB layers.

use crate::container::ContainerScan;
use crate::decode::surfaces::brep::{
    BrepTransferDiagnostics, FaceAdmissionDetail, FaceAdmissionRejection,
    FACE_REJECTION_SAMPLE_LIMIT,
};
use crate::loss::CreoLossCode;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::coverage::torus_parameter_coverage;
use cadmpeg_ir::report::loss::LossNote;

pub(super) fn push_report_loss(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    code: CreoLossCode,
    message: impl std::fmt::Display,
) -> Result<(), CodecError> {
    let message = ctx.format_retained(format_args!("{message}"), "creo report loss text")?;
    ctx.reserve_vec(losses, 1, "creo report losses")?;
    losses.push(code.note(message));
    Ok(())
}

struct BrepRejectionDetails<'a> {
    reasons: [(
        FaceAdmissionRejection,
        usize,
        [Option<&'a FaceAdmissionDetail>; FACE_REJECTION_SAMPLE_LIMIT],
    ); FaceAdmissionRejection::ALL.len()],
}

impl<'a> BrepRejectionDetails<'a> {
    fn new(
        ctx: &DecodeContext<'_>,
        diagnostics: &'a BrepTransferDiagnostics,
    ) -> Result<Self, CodecError> {
        let mut reasons = FaceAdmissionRejection::ALL
            .map(|reason| (reason, 0, [None; FACE_REJECTION_SAMPLE_LIMIT]));
        for (reason, count, samples) in &mut reasons {
            let (total, evidence) = diagnostics.evidence(ctx, *reason)?;
            *count = total;
            for (index, detail) in evidence.enumerate() {
                samples[index] = Some(detail);
            }
        }
        Ok(Self { reasons })
    }
}

impl std::fmt::Display for BrepRejectionDetails<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first_reason = true;
        for (reason, count, samples) in self.reasons {
            if count == 0 {
                continue;
            }
            if !first_reason {
                f.write_str(", ")?;
            }
            write!(f, "{}={} (sample faces: ", reason.label(), count)?;
            for (index, detail) in samples.into_iter().flatten().enumerate() {
                if index != 0 {
                    f.write_str(",")?;
                }
                write!(f, "{}", detail.face_id)?;
                if !detail.boundary_half_edges.is_empty() || !detail.vertex_ids.is_empty() {
                    f.write_str("[")?;
                    if !detail.boundary_half_edges.is_empty() {
                        f.write_str("edges:")?;
                        for (index, half_edge) in detail.boundary_half_edges.iter().enumerate() {
                            if index != 0 {
                                f.write_str("|")?;
                            }
                            write!(f, "{}:{}", half_edge.curve_id, half_edge.side)?;
                        }
                    }
                    if !detail.vertex_ids.is_empty() {
                        if !detail.boundary_half_edges.is_empty() {
                            f.write_str(";")?;
                        }
                        f.write_str("vertices:")?;
                        for (index, vertex) in detail.vertex_ids.iter().enumerate() {
                            if index != 0 {
                                f.write_str("|")?;
                            }
                            write!(f, "{vertex}")?;
                        }
                    }
                    f.write_str("]")?;
                }
            }
            f.write_str(")")?;
            first_reason = false;
        }
        if first_reason {
            f.write_str("none")?;
        }
        Ok(())
    }
}

struct ComponentGate<'a>(&'a BrepTransferDiagnostics);

impl std::fmt::Display for ComponentGate<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} admitted component(s), selected body count ",
            self.0.admitted_component_count
        )?;
        match self.0.selected_body_count {
            Some(count) => write!(f, "{count}")?,
            None => f.write_str("unresolved")?,
        }
        f.write_str("; ")?;
        let mut has_reason = false;
        if self.0.body_count_mismatch {
            f.write_str("selected body count mismatch")?;
            has_reason = true;
        }
        if self.0.legacy_body_ownership_ambiguous {
            if has_reason {
                f.write_str(", ")?;
            }
            f.write_str("legacy body ownership ambiguous")?;
            has_reason = true;
        }
        if self.0.empty_component_count != 0 {
            if has_reason {
                f.write_str(", ")?;
            }
            write!(
                f,
                "{} empty admitted component(s)",
                self.0.empty_component_count
            )?;
            has_reason = true;
        }
        if !has_reason {
            f.write_str("passed")?;
        }
        Ok(())
    }
}

struct PcurveMismatchEvidence<'a>(&'a BrepTransferDiagnostics);

impl std::fmt::Display for PcurveMismatchEvidence<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let samples = &self.0.vertex_solve.pcurve.mismatch_samples;
        if samples.is_empty() {
            return Ok(());
        }
        f.write_str(" Pcurve mismatch samples: ")?;
        for (index, detail) in samples.iter().enumerate() {
            if index != 0 {
                f.write_str(",")?;
            }
            write!(
                f,
                "{}[faces:{}|{};same:{:.3e};reverse:{:.3e}]",
                detail.curve_id,
                detail.faces[0],
                detail.faces[1],
                detail.same_order_error,
                detail.reverse_order_error,
            )?;
        }
        f.write_str(".")
    }
}

struct PcurveActivityEvidence<'a>(&'a BrepTransferDiagnostics);

impl std::fmt::Display for PcurveActivityEvidence<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pcurve = &self.0.vertex_solve.pcurve;
        if pcurve.inactive_paths == 0
            && pcurve.inactive_records == 0
            && pcurve.partial_records == 0
            && pcurve.topology_mismatch_records == 0
        {
            return Ok(());
        }
        write!(
            f,
            " Pcurve path activity: inactive paths={}, inactive records={}, partial records={}, topology mismatches={}.",
            pcurve.inactive_paths,
            pcurve.inactive_records,
            pcurve.partial_records,
            pcurve.topology_mismatch_records,
        )
    }
}

struct PcurveCarrierEvidence<'a>(&'a BrepTransferDiagnostics);

impl std::fmt::Display for PcurveCarrierEvidence<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pcurve = &self.0.vertex_solve.pcurve;
        let fixed_endpoint_conflicts = self.0.vertex_solve.pcurve_fixed_endpoint_conflicts;
        let ambiguous_endpoint_vertices = self.0.vertex_solve.pcurve_ambiguous_endpoint_vertices;
        if pcurve.carrier_validated_paths == 0
            && pcurve.carrier_rejected_paths == 0
            && pcurve.carrier_unknown_paths() == 0
            && pcurve.carrier_rejected_records == 0
            && fixed_endpoint_conflicts == 0
            && ambiguous_endpoint_vertices == 0
        {
            return Ok(());
        }
        write!(
            f,
            " Pcurve carrier join: validated paths={}, rejected paths={}, unknown paths={} \
             (missing surface={}, missing carrier={}, unsupported pair={}, parallel plane={}, \
             unsupported path={}), rejected records={}, fixed endpoint conflicts={}, ambiguous \
             endpoint vertices={}.",
            pcurve.carrier_validated_paths,
            pcurve.carrier_rejected_paths,
            pcurve.carrier_unknown_paths(),
            pcurve.carrier_unknown_missing_surface_paths,
            pcurve.carrier_unknown_missing_carrier_paths,
            pcurve.carrier_unknown_unsupported_pair_paths,
            pcurve.carrier_unknown_parallel_plane_paths,
            pcurve.carrier_unknown_unsupported_path_paths,
            pcurve.carrier_rejected_records,
            fixed_endpoint_conflicts,
            ambiguous_endpoint_vertices,
        )
    }
}

struct TwoChartMappingEvidence<'a>(&'a BrepTransferDiagnostics);

impl std::fmt::Display for TwoChartMappingEvidence<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pcurve = &self.0.vertex_solve.pcurve;
        if pcurve.two_chart_records == 0 {
            return Ok(());
        }
        write!(
            f,
            " Two-chart mapping: {} record(s), {} mapped ({} complete, {} partial), {} unmapped; {} missing surface path(s), {} unevaluable path(s), {} surface disagreement(s), {} empty sample record(s).",
            pcurve.two_chart_records,
            pcurve.two_chart_mapped_records(),
            pcurve.two_chart_complete_records,
            pcurve.two_chart_partial_records,
            pcurve.two_chart_unmapped_records,
            pcurve.two_chart_missing_surface_paths,
            pcurve.two_chart_unevaluable_paths,
            pcurve.two_chart_surface_mismatch_records,
            pcurve.two_chart_no_sample_records,
        )
    }
}

struct CarrierRejectionEvidence<'a>(&'a BrepTransferDiagnostics);

impl std::fmt::Display for CarrierRejectionEvidence<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let solve = &self.0.vertex_solve;
        if solve.carrier_no_geometric_candidate_vertices == 0
            && solve.carrier_no_valid_candidate_vertices == 0
            && solve.carrier_rejection_samples.is_empty()
        {
            return Ok(());
        }
        write!(
            f,
            " Carrier solver classification: no geometric candidate={}, no valid candidate={}; rejection samples: ",
            solve.carrier_no_geometric_candidate_vertices,
            solve.carrier_no_valid_candidate_vertices,
        )?;
        for (index, sample) in solve.carrier_rejection_samples.iter().enumerate() {
            if index != 0 {
                f.write_str(",")?;
            }
            write!(f, "{}[faces:", sample.vertex_id)?;
            for (index, face_id) in sample.incident_face_ids.iter().enumerate() {
                if index != 0 {
                    f.write_str("|")?;
                }
                write!(f, "{face_id}")?;
            }
            f.write_str(";carriers:")?;
            for (index, kind) in sample.carrier_kinds.iter().enumerate() {
                if index != 0 {
                    f.write_str("|")?;
                }
                f.write_str(kind)?;
            }
            write!(
                f,
                ";pair:{};triple:{};valid:{};unique:0]",
                sample.pair_intersections, sample.triple_intersections, sample.valid_candidates,
            )?;
        }
        f.write_str(".")
    }
}

pub(super) fn coverage_count(coverage: &cadmpeg_ir::report::decode::Coverage, key: &str) -> usize {
    coverage.get(key).copied().unwrap_or(0)
}

fn legacy_type_count(
    ctx: &DecodeContext<'_>,
    coverage: &cadmpeg_ir::report::decode::Coverage,
    prefix: &str,
    type_code: u8,
    suffix: &str,
) -> Result<usize, CodecError> {
    Ok(ctx
        .find_map(
            &**coverage,
            |(key, count)| {
                // The prefix and suffix are code constants and the type code
                // fits in a byte, so the comparison and parse are bounded.
                let stored_type = key
                    .strip_prefix(prefix)
                    .and_then(|rest| rest.strip_suffix(suffix))
                    .and_then(|digits| digits.parse::<u8>().ok());
                Ok((stored_type == Some(type_code)).then_some(*count))
            },
            "creo legacy type coverage search",
        )?
        .unwrap_or(0))
}

pub(super) fn push_legacy_value_losses(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    coverage: &cadmpeg_ir::report::decode::Coverage,
) -> Result<(), CodecError> {
    let unresolved_legacy_reals = coverage_count(coverage, "unresolved_legacy_real_value_count");
    if unresolved_legacy_reals != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::LegacyRealValueUnresolved,
            format_args!(
                "{unresolved_legacy_reals} legacy type-2 value row(s) did not form a complete \
             finite scalar or dimension-complete real array."
            ),
        )?;
    }
    let unresolved_legacy_integers =
        coverage_count(coverage, "unresolved_legacy_integer_value_count");
    if unresolved_legacy_integers != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::LegacyIntegerValueUnresolved,
            format_args!(
                "{unresolved_legacy_integers} legacy type-1 value row(s) did not form a signed \
             32-bit scalar or dimension-complete integer array."
            ),
        )?;
    }
    for type_code in [3u8, 4] {
        let unresolved = legacy_type_count(
            ctx,
            coverage,
            "unresolved_legacy_type_",
            type_code,
            "_value_count",
        )?;
        if unresolved != 0 {
            push_report_loss(
                ctx,
                losses,
                CreoLossCode::LegacyContinuationFormUndefined,
                format_args!(
                    "{unresolved} legacy type-{type_code} value row(s) use an undefined \
                 continuation form."
                ),
            )?;
        }
        let undecoded = legacy_type_count(
            ctx,
            coverage,
            "undecoded_legacy_type_",
            type_code,
            "_encoding_count",
        )?;
        if undecoded != 0 {
            push_report_loss(
                ctx,
                losses,
                CreoLossCode::LegacyByteStringEncodingRetained,
                format_args!(
                    "{undecoded} legacy type-{type_code} byte-string value(s) retain exact \
                 source bytes because their character encoding is not UTF-8."
                ),
            )?;
        }
    }
    for type_code in [5u8, 7, 9, 11] {
        let unresolved = legacy_type_count(
            ctx,
            coverage,
            "unresolved_legacy_type_",
            type_code,
            "_value_count",
        )?;
        if unresolved != 0 {
            push_report_loss(
                ctx,
                losses,
                CreoLossCode::LegacyUnsignedValueUnresolved,
                format_args!(
                    "{unresolved} legacy type-{type_code} value row(s) did not form an unsigned \
                 32-bit scalar or dimension-complete unsigned array."
                ),
            )?;
        }
    }
    let unresolved_legacy_type_6 = coverage_count(coverage, "unresolved_legacy_type_6_value_count");
    if unresolved_legacy_type_6 != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::LegacyCompactRealUnresolved,
            format_args!(
                "{unresolved_legacy_type_6} legacy type-6 value row(s) did not form a complete \
             finite compact-real scalar or dimension-complete real array."
            ),
        )?;
    }
    let incomplete_legacy_object_arrays =
        coverage_count(coverage, "incomplete_legacy_object_array_count");
    if incomplete_legacy_object_arrays != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::LegacyObjectArrayIncomplete,
            format_args!(
                "{incomplete_legacy_object_arrays} legacy type-0 object array(s) have a direct \
             element count that differs from their declared extents."
            ),
        )?;
    }
    let unresolved_legacy_objects =
        coverage_count(coverage, "unresolved_legacy_object_value_count");
    if unresolved_legacy_objects != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::LegacyObjectPayloadUndefined,
            format_args!(
                "{unresolved_legacy_objects} legacy type-0 value row(s) use an undefined object \
             payload form."
            ),
        )?;
    }
    let incomplete_legacy_string_arrays =
        coverage_count(coverage, "incomplete_legacy_string_array_count");
    if incomplete_legacy_string_arrays != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::LegacyStringArrayIncomplete,
            format_args!(
                "{incomplete_legacy_string_arrays} legacy type-10 string array(s) have a direct \
             element count that differs from their first extent."
            ),
        )?;
    }
    let unresolved_legacy_strings =
        coverage_count(coverage, "unresolved_legacy_string_value_count");
    if unresolved_legacy_strings != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::LegacyStringContinuationUndefined,
            format_args!(
                "{unresolved_legacy_strings} legacy type-10 value row(s) use an undefined \
             continuation form."
            ),
        )?;
    }
    let undecoded_legacy_string_encodings =
        coverage_count(coverage, "undecoded_legacy_string_encoding_count");
    if undecoded_legacy_string_encodings != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::LegacyStringEncodingRetained,
            format_args!(
                "{undecoded_legacy_string_encodings} legacy type-10 string element(s) retain \
             exact source bytes because their character encoding is not UTF-8."
            ),
        )?;
    }

    let conflicting_triangle_strip_representations = coverage_count(
        coverage,
        "conflicting_primitive_triangle_strip_representation_count",
    );
    if conflicting_triangle_strip_representations != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::TriangleStripRepresentationConflict,
            format_args!(
                "{conflicting_triangle_strip_representations} primitive triangle-strip record(s) \
             contain complete position representations that disagree."
            ),
        )?;
    }
    Ok(())
}

pub(super) fn push_brep_transfer_note(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    diagnostics: &BrepTransferDiagnostics,
    geometry_section_count: usize,
) -> Result<(), CodecError> {
    for (body_id, reason) in ctx.admit_iter(
        &diagnostics.rejected_extrusion_bodies,
        "creo rejected extrusion body loss traversal",
    )? {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::ExtrusionBodyRejected,
            format_args!("Extrusion body {body_id} was not transferred: {reason}"),
        )?;
    }
    let rejected_face_count = diagnostics.face_rejection_diagnostics.len();
    let rejection_details = BrepRejectionDetails::new(ctx, diagnostics)?;
    let component_gate = ComponentGate(diagnostics);
    let pcurve_mismatch_evidence = PcurveMismatchEvidence(diagnostics);
    let pcurve_activity_evidence = PcurveActivityEvidence(diagnostics);
    let pcurve_carrier_evidence = PcurveCarrierEvidence(diagnostics);
    let two_chart_mapping_evidence = TwoChartMappingEvidence(diagnostics);
    let carrier_rejection_evidence = CarrierRejectionEvidence(diagnostics);
    let (vertex_evidence, _vertex_evidence_reservation) = ctx.format_scoped(
        format_args!(
            "Boundary evidence: {} curve(s), {} without a unique incidence pair, {} with an \
         unsolved endpoint vertex. Vertex solver: {} topological, {} carrier intersections, \
         {} carrier-bearing vertices, {} pair-intersection candidate(s), {} triple-intersection \
         candidate(s), {} validated carrier candidate(s), {} carrier vertices with no candidate, \
         {} ambiguous carrier vertices, {} pcurve record(s), {} pcurve path(s), {} path(s) without \
         a unique surface, {} unevaluable path(s), {} mapped path(s), {} unmapped record(s), {} \
         inconsistent record(s), {} accepted record(s) ({} complete), {} conflicting curve(s), {} \
         pcurve endpoint evidence ({} complete), {} pcurve constraint(s), {} analytic domain(s), \
         {} NURBS endpoint constraint(s), {} directed endpoint conflict(s), {} solved.{}{}{}{}{}",
            diagnostics.boundary_curve_count,
            diagnostics.boundary_curve_missing_incidence_count,
            diagnostics.boundary_curve_unsolved_vertex_count,
            diagnostics.vertex_solve.topological_vertices,
            diagnostics.vertex_solve.carrier_points,
            diagnostics.vertex_solve.carrier_incident_vertices,
            diagnostics.vertex_solve.carrier_pair_candidates,
            diagnostics.vertex_solve.carrier_triple_candidates,
            diagnostics.vertex_solve.carrier_valid_candidates,
            diagnostics
                .vertex_solve
                .carrier_no_geometric_candidate_vertices
                + diagnostics.vertex_solve.carrier_no_valid_candidate_vertices,
            diagnostics
                .vertex_solve
                .carrier_ambiguous_candidate_vertices,
            diagnostics.vertex_solve.pcurve.records,
            diagnostics.vertex_solve.pcurve.paths(),
            diagnostics.vertex_solve.pcurve.missing_surfaces,
            diagnostics.vertex_solve.pcurve.unevaluable_paths,
            diagnostics.vertex_solve.pcurve.mapped_paths,
            diagnostics.vertex_solve.pcurve.unmapped_records,
            diagnostics.vertex_solve.pcurve.inconsistent_records,
            diagnostics.vertex_solve.pcurve.accepted_records,
            diagnostics.vertex_solve.pcurve.complete_records,
            diagnostics.vertex_solve.pcurve.conflicting_curves,
            diagnostics.vertex_solve.pcurve.evidence,
            diagnostics.vertex_solve.pcurve.complete_evidence,
            diagnostics.vertex_solve.pcurve_constraints,
            diagnostics.vertex_solve.analytic_domain_vertices,
            diagnostics.vertex_solve.nurbs_endpoint_constraints,
            diagnostics.vertex_solve.directed_endpoint_conflicts,
            diagnostics.vertex_solve.solved_vertices,
            pcurve_mismatch_evidence,
            pcurve_activity_evidence,
            pcurve_carrier_evidence,
            carrier_rejection_evidence,
            two_chart_mapping_evidence,
        ),
        "creo brep vertex evidence",
    )?;

    push_report_loss(ctx, losses, CreoLossCode::BrepTransferIncomplete, format_args!(
        "General model B-rep transfer remains incomplete. Native face components transfer \
         when every boundary edge has solved vertex orbits, face orientation is unique, and \
         every loop is complete; a multi-loop face additionally requires strict parameter-space \
         containment or a complete common-center, distinct-radius circular-loop proof on a plane. Selected \
         cylinders transfer when an exact `fc 05` record and placed cap outline binds a row, \
         a four-entry class-917 circular-sweep or class-911 simple-hole table with a complete \
         square cap outline establishes the complete axis placement and radius, or a compact \
         class-911 table owns a complete positional cylinder carrier, a class-911 \
         counterbore dimension replay agrees with its generated larger-cylinder carrier, or two same-feature \
         patches have complementary square outline bounds on one axis-normal plane. Later positional \
         instances do not inherit prototype placement or scalar \
         defaults; they require their per-instance parameter bodies \
         ([spec §4.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#32-surface-prototypes)). \
         Face admission considered {} candidate(s): {} passed, {} emitted, and {} were rejected. \
         First-failure rejection counts are: {rejection_details}. Component admission gate: \
         {component_gate}. {vertex_evidence} {geometry_section_count} PSB geometry section(s) were preserved \
         verbatim as unknown records.",
        diagnostics.candidate_face_count,
        diagnostics.admitted_face_count,
        diagnostics.emitted_face_count,
        rejected_face_count,
    ))?;
    Ok(())
}

pub(super) fn push_carrier_transfer_notes(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    scan: &ContainerScan,
    coverage: &cadmpeg_ir::report::decode::Coverage,
    container_only: bool,
    placed_plane_count: usize,
) -> Result<(), CodecError> {
    let topology_bound_plane_count =
        coverage_count(coverage, "transferred_topology_bound_plane_surface_count");
    let first_instance_prototype_surface_count = coverage_count(
        coverage,
        "transferred_first_instance_prototype_surface_count",
    );
    let paired_envelope_sphere_count =
        coverage_count(coverage, "transferred_paired_envelope_sphere_count");
    let positional_torus_count = coverage_count(coverage, "transferred_positional_torus_count");
    let positional_cylinder_count =
        coverage_count(coverage, "transferred_positional_cylinder_count");
    let positional_cone_count = coverage_count(coverage, "transferred_positional_cone_count");
    let positional_line_extrusion_plane_count = coverage_count(
        coverage,
        "transferred_positional_line_extrusion_plane_count",
    );
    let tabulated_cylinder_spline_extrusion_count = coverage_count(
        coverage,
        "transferred_tabulated_cylinder_spline_extrusion_count",
    );
    if !container_only && placed_plane_count != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierVisibGeomPlanes,
            format_args!(
                "Transferred {placed_plane_count} model-space plane carrier(s) from complete \
             VisibGeom local-system support frames."
            ),
        )?;
    }

    if !container_only && topology_bound_plane_count != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierTopologyBoundPlanes,
            format_args!(
                "Transferred {topology_bound_plane_count} model-space plane carrier(s) from \
             circle, ellipse, or line boundary carriers, coplanar NURBS control nets, or \
             three or more non-collinear solved boundary vertices of the same native face."
            ),
        )?;
    }

    if !container_only && first_instance_prototype_surface_count != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierFirstInstancePrototypes,
            format_args!(
                "Transferred {first_instance_prototype_surface_count} first-instance ND plane, \
             cylinder, cone, torus, or interpolation-spline carrier(s) from complete named \
             parameters."
            ),
        )?;
    }

    if !container_only && paired_envelope_sphere_count != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierPairedEnvelopeSpheres,
            format_args!(
                "Transferred {paired_envelope_sphere_count} sphere carrier(s) from complementary \
             five-coordinate type-26 hemisphere envelopes and their shared zero-major-radius \
             prototype."
            ),
        )?;
    }

    if !container_only && positional_torus_count != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierPositionalTori,
            format_args!(
                "Transferred {positional_torus_count} exact positional torus carrier(s) from \
             complete local-system, radius, and five-coordinate envelope bodies."
            ),
        )?;
    }

    if !container_only && positional_cylinder_count != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierPositionalCylinders,
            format_args!(
                "Transferred {positional_cylinder_count} exact positional cylinder carrier(s) \
             from complete per-instance parameter bodies."
            ),
        )?;
    }

    if !container_only && positional_cone_count != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierPositionalCones,
            format_args!(
                "Transferred {positional_cone_count} exact positional cone carrier(s) from \
             complete support-apex or planar-envelope bodies."
            ),
        )?;
    }

    if !container_only && positional_line_extrusion_plane_count != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierLineExtrusionPlanes,
            format_args!(
                "Transferred {positional_line_extrusion_plane_count} unbound straight positional \
             surface-of-extrusion carrier(s) from complete sweep-direction and directrix \
             frames."
            ),
        )?;
    }

    if !container_only && tabulated_cylinder_spline_extrusion_count != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierTabulatedCylinderExtrusions,
            format_args!(
                "Transferred {tabulated_cylinder_spline_extrusion_count} tabulated-cylinder \
             cubic spline extrusion carrier(s) from uniquely matched directrix and frame spans."
            ),
        )?;
    }

    if !container_only && !scan.planes.datums.is_empty() {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierDatumPlanes,
            format_args!(
            "Transferred {} exact model-space construction datum plane carrier(s) from ActDatums; \
             these are unbounded reference planes, not model B-rep faces.",
            scan.planes.datums.len()
        ),
        )?;
    }

    if !container_only && !scan.references.lines.is_empty() {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierReferenceLines,
            format_args!(
                "Transferred {} finite model-space reference line carrier(s) from MdlRefInfo; \
             their byte-exact endpoints remain attached as native line records.",
                scan.references.lines.len()
            ),
        )?;
    }

    if !container_only && !scan.references.circles.is_empty() {
        push_report_loss(ctx, losses, CreoLossCode::CarrierReferenceCircles, format_args!(
            "Transferred {} circular reference carrier(s) from MdlRefInfo rows whose stored center, radius, and endpoints satisfy the circle equation; byte-exact endpoints remain attached as native circle records.",
            scan.references.circles.len()
        ))?;
    }

    if !container_only && !scan.references.ellipses.is_empty() {
        push_report_loss(ctx, losses, CreoLossCode::CarrierReferenceEllipses, format_args!(
            "Transferred {} elliptical reference carrier(s) from MdlRefInfo conic rows whose frame, coefficient radii, and antipodal endpoints satisfy one ellipse equation; the source conic records remain byte-exact native records.",
            scan.references.ellipses.len()
        ))?;
    }

    let topological_point_count = coverage_count(coverage, "transferred_topological_point_count");
    if !container_only && topological_point_count != 0 {
        push_report_loss(ctx, losses, CreoLossCode::CarrierTopologicalPoints, format_args!(
            "Transferred {topological_point_count} exact model-space point(s) for native topological vertex orbits from unique placed-carrier intersections or pcurve endpoint domains constrained by agreeing face maps and incident analytic edge carriers."
        ))?;
    }

    let native_topological_edge_count =
        coverage_count(coverage, "transferred_native_topological_edge_count");
    if !container_only && native_topological_edge_count != 0 {
        push_report_loss(ctx, losses, CreoLossCode::CarrierTopologicalEdges, format_args!(
            "Transferred {native_topological_edge_count} native topological edge(s) whose endpoint vertex orbits have exact model-space points."
        ))?;
    }

    let analytic_pcurve_carrier_count =
        coverage_count(coverage, "transferred_analytic_pcurve_carrier_count");
    if !container_only && analytic_pcurve_carrier_count != 0 {
        push_report_loss(ctx, losses, CreoLossCode::CarrierAnalyticPcurves, format_args!(
            "Transferred {analytic_pcurve_carrier_count} exact analytic carrier(s) by mapping native linear pcurves through placed planar, cylindrical, conical, spherical, or toroidal face charts."
        ))?;
    }

    let extrusion_plane_boundary_curve_count =
        coverage_count(coverage, "transferred_extrusion_plane_boundary_curve_count");
    if !container_only && extrusion_plane_boundary_curve_count != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierExtrusionBoundaryCurves,
            format_args!(
                "Transferred {extrusion_plane_boundary_curve_count} exact NURBS boundary \
             carrier(s) where one tabulated-extrusion boundary lies in an adjacent plane \
             and every other control point lies strictly on one side."
            ),
        )?;
    }

    let extrusion_plane_section_generator_curve_count = coverage_count(
        coverage,
        "transferred_extrusion_plane_section_generator_curve_count",
    );
    if !container_only && extrusion_plane_section_generator_curve_count != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierExtrusionSectionGenerators,
            format_args!(
                "Transferred {extrusion_plane_section_generator_curve_count} exact NURBS \
             generator carrier(s) where an adjacent plane contains the sweep direction and \
             the cubic directrix has exactly one plane intersection."
            ),
        )?;
    }

    let shared_extrusion_generator_curve_count = coverage_count(
        coverage,
        "transferred_shared_extrusion_generator_curve_count",
    );
    if !container_only && shared_extrusion_generator_curve_count != 0 {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierSharedExtrusionGenerators,
            format_args!(
                "Transferred {shared_extrusion_generator_curve_count} exact shared NURBS \
             generator carrier(s) whose two tabulated-extrusion control nets meet on the \
             same linear boundary and lie strictly on opposite sides of a plane through it."
            ),
        )?;
    }

    let torus_coverage = torus_parameter_coverage(ctx, scan)?;
    if torus_coverage.radius_overrides != 0
        || torus_coverage.replayed_minor_radii != 0
        || torus_coverage.outline_extents != 0
        || torus_coverage.five_coordinate_envelopes != 0
        || torus_coverage.split_coordinate_envelopes != 0
    {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::CarrierTorusParameterRetention,
            format_args!(
                "Retained {} tagged type-26 radius override(s), {} prototype-minor-radius \
             replay(s), {} terminal outline extent(s), {} five-coordinate envelope(s), and \
             {} split-coordinate envelope(s). These row-local fields remain byte-exact native \
             data. Placement-complete paired sphere envelopes additionally transfer as \
             analytic carriers.",
                torus_coverage.radius_overrides,
                torus_coverage.replayed_minor_radii,
                torus_coverage.outline_extents,
                torus_coverage.five_coordinate_envelopes,
                torus_coverage.split_coordinate_envelopes,
            ),
        )?;
    }
    Ok(())
}

/// The refusal for half-edge orbits past the one-based topological vertex
/// identifier width.
///
/// The note names the lane and the instance: `orbits` carries the seed
/// half-edge of every orbit that stated no vertex, and the first of them is
/// what a report reader needs to find the dropped orbit.
fn unstatable_vertex_orbit_note(
    ctx: &DecodeContext<'_>,
    orbits: &[crate::topology::HalfEdgeId],
) -> Result<Option<LossNote>, CodecError> {
    let Some(first) = orbits.first() else {
        return Ok(None);
    };
    let count = orbits.len();
    Ok(Some(CreoLossCode::TopologyVertexIdentifierUnstatable.note(
        ctx.format_retained(
            format_args!(
                "{count} half-edge orbit(s) lie past the one-based topological vertex identifier \
         width, so they state no vertex and their half-edges carry no incidence. The first \
         is the orbit at half-edge curve {} side {}.",
                first.curve_id, first.side,
            ),
            "creo unstatable vertex orbit text",
        )?,
    )))
}

pub(super) fn push_structural_layer_notes(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    scan: &ContainerScan,
) -> Result<(), CodecError> {
    struct CurveExpressionTransfer(usize);
    impl std::fmt::Display for CurveExpressionTransfer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            if self.0 == 0 {
                f.write_str(
                    "Curve-equation assignments transfer with their source, dependencies, and closed numeric \
                     and string operator and deterministic function values."
                )
            } else {
                write!(f,
                    "Admitted curve-equation assignments transfer with their source, dependencies, and \
                     closed numeric and string operator and deterministic function values. \
                     {} active curve-equation record(s) \
                     containing prohibited datum-curve constructs or unresolved simultaneous-solve \
                     control retain \
                     source and dependencies without solve-dependent assignment values or derived curves.",
                    self.0
                )
            }
        }
    }

    if let Some(note) = unstatable_vertex_orbit_note(ctx, &scan.topology.unstatable_vertex_orbits)?
    {
        ctx.reserve_vec(losses, 1, "creo report losses")?;
        losses.push(note);
    }
    // Named prototype fields whose bounded scalar body the decoder refused.
    // The field bytes are retained opaque; the note states which record and
    // field, and the slot and byte the refusal stands at.
    for refusal in ctx
        .admit_iter(
            &scan.surfaces.prototype_field_refusals,
            "creo visible prototype refusal loss traversal",
        )?
        .chain(ctx.admit_iter(
            &scan.surfaces.nonvisible_prototype_field_refusals,
            "creo nonvisible prototype refusal loss traversal",
        )?)
    {
        push_report_loss(
            ctx,
            losses,
            CreoLossCode::SurfacePrototypeFieldRetained,
            refusal,
        )?;
    }

    // The specific undecoded PSB layers that gate per-instance geometry.
    push_report_loss(
        ctx,
        losses,
        CreoLossCode::GeometryInstanceCarriersGated,
        "Additional model-space carriers are gated by unresolved lane-specific scalar \
         prefixes, feature-local transform bindings, placement-incomplete or untagged \
         `0x26` torus/sphere variants, and the round/fillet feature evaluator. These gaps \
         prevent transfer of the remaining non-plane per-instance surfaces, curves, and \
         vertices.",
    )?;

    // Topology.
    push_report_loss(
        ctx,
        losses,
        CreoLossCode::TopologyIncompleteComponents,
        "Native curve half-edges and closed loops were decoded. Components with complete \
         solved boundaries and unique face orientations transfer as \
         body/region/shell/face/loop/coedge/edge/vertex graphs; multi-loop faces use \
         strict containment in a placed or boundary-proven plane. Remaining components \
         require face-instance partitioning, surface parameter bindings, curve geometry, \
         or vertex coordinates.",
    )?;

    let configuration_gap = match scan.framing.family_table.map(|record| record.pointer) {
        Some(crate::container::FamilyTablePointer::Null) => "",
        Some(crate::container::FamilyTablePointer::Entity(_)) => {
            ", configuration driver-table rows"
        }
        None => ", configuration presence",
    };
    let unevaluated_curve_expression_record_count = ctx
        .admit_iter(
            &scan.curves.expressions,
            "creo unevaluated curve expression coverage traversal",
        )?
        .try_fold(0usize, |total, record| {
            if record.backup {
                return Ok::<_, CodecError>(total);
            }
            let unresolved = if !record.prohibited_constructs.is_empty() {
                true
            } else {
                let mut unresolved_solve = false;
                for block in ctx.admit_iter(
                    &record.solve_blocks,
                    "creo unresolved solve block loss traversal",
                )? {
                    if ctx.any_by(
                        &block.unknowns,
                        |unknown| Ok(unknown.solution.is_none()),
                        "creo unresolved solve unknown loss traversal",
                    )? {
                        unresolved_solve = true;
                        break;
                    }
                }
                unresolved_solve || record.unresolved_solve_control
            };
            total.checked_add(usize::from(unresolved)).ok_or_else(|| {
                cadmpeg_core::decode::refuse_local_limit(
                    "creo unevaluated expression count",
                    u64::MAX,
                    u64::MAX,
                )
            })
        })?;

    let curve_expression_transfer =
        CurveExpressionTransfer(unevaluated_curve_expression_record_count);

    // Features, history, materials.
    push_report_loss(
        ctx,
        losses,
        CreoLossCode::FeatureNeutralSemanticsIncomplete,
        format_args!(
            "Named feature operations and their decoded dependency/input tables transfer as typed \
         or native design records. {curve_expression_transfer} \
         Full neutral operation semantics\
         {configuration_gap}, graph, case-study, cabling, and cross-model relation functions, \
         materials, and display data \
         remain untransferred."
        ),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{push_brep_transfer_note, push_report_loss, unstatable_vertex_orbit_note};
    use crate::decode::surfaces::brep::BrepTransferDiagnostics;
    use crate::loss::CreoLossCode;
    use crate::topology::{HalfEdgeId, Side};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn report_limit_error(
        dimension: ResourceDimension,
        limit: u64,
        run: impl FnOnce(&DecodeContext<'_>) -> Result<(), CodecError>,
    ) -> CodecError {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = limit,
            _ => panic!("unsupported report test dimension"),
        }
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        run(&ctx).expect_err("report allocation refused")
    }

    #[test]
    fn report_loss_text_refuses_retained_limit() {
        let error = report_limit_error(ResourceDimension::RetainedBytes, 0, |ctx| {
            let mut losses = Vec::new();
            push_report_loss(
                ctx,
                &mut losses,
                CreoLossCode::BrepTransferIncomplete,
                "report",
            )
        });
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo report loss text"));
    }

    #[test]
    fn report_loss_slots_refuse_collection_limit() {
        let error = report_limit_error(ResourceDimension::CollectionItems, 0, |ctx| {
            let mut losses = Vec::new();
            push_report_loss(
                ctx,
                &mut losses,
                CreoLossCode::BrepTransferIncomplete,
                "report",
            )
        });
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo report losses"));
    }

    #[test]
    fn brep_vertex_evidence_refuses_materialized_limit() {
        let error = report_limit_error(ResourceDimension::MaterializedBytes, 0, |ctx| {
            push_brep_transfer_note(ctx, &mut Vec::new(), &BrepTransferDiagnostics::default(), 0)
        });
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo brep vertex evidence"));
    }

    #[test]
    fn brep_report_preserves_component_and_vertex_evidence() {
        let mut diagnostics = BrepTransferDiagnostics::default();
        diagnostics.candidate_face_count = 3;
        diagnostics.admitted_face_count = 1;
        diagnostics.emitted_face_count = 1;
        diagnostics.body_count_mismatch = true;
        diagnostics.legacy_body_ownership_ambiguous = true;
        diagnostics.empty_component_count = 2;
        diagnostics.admitted_component_count = 1;
        diagnostics.selected_body_count = Some(2);
        let losses = crate::decode::with_test_decode_ctx(|ctx| {
            let mut losses = Vec::new();
            push_brep_transfer_note(ctx, &mut losses, &diagnostics, 4)
                .expect("service report admitted");
            losses
        });
        let message = &losses[0].message;
        assert!(message.contains("Face admission considered 3 candidate(s): 1 passed, 1 emitted, and 0 were rejected. First-failure rejection counts are: none."));
        assert!(message.contains("Component admission gate: 1 admitted component(s), selected body count 2; selected body count mismatch, legacy body ownership ambiguous, 2 empty admitted component(s)."));
        assert!(message.contains("Boundary evidence: 0 curve(s), 0 without a unique incidence pair, 0 with an unsolved endpoint vertex."));
        assert!(message.contains("0 solved. 4 PSB geometry section(s) were preserved"));
    }

    #[test]
    fn unstatable_orbit_note_refuses_retained_limit() {
        let error = report_limit_error(ResourceDimension::RetainedBytes, 0, |ctx| {
            unstatable_vertex_orbit_note(
                ctx,
                &[HalfEdgeId {
                    curve_id: 5,
                    side: Side::Zero,
                }],
            )
            .map(|_| ())
        });
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo unstatable vertex orbit text"));
    }

    /// A refusal names the instance and the lane it happened in. The orbit
    /// that stated no vertex is named by its seed half-edge, so a report
    /// reader can find it; the count alone names nothing.
    #[test]
    fn an_unstatable_vertex_orbit_note_names_the_orbit() {
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| unstatable_vertex_orbit_note(ctx, &[]))
                .expect("empty orbit report admitted")
                .is_none()
        );
        let note = crate::decode::with_test_decode_ctx(|ctx| {
            unstatable_vertex_orbit_note(
                ctx,
                &[
                    HalfEdgeId {
                        curve_id: 4_100,
                        side: Side::One,
                    },
                    HalfEdgeId {
                        curve_id: 4_101,
                        side: Side::Zero,
                    },
                ],
            )
        })
        .expect("orbit report allocation admitted")
        .expect("a stated orbit refusal");
        assert!(
            note.message.contains("half-edge curve 4100 side 1"),
            "{}",
            note.message
        );
        assert!(
            note.message.starts_with("2 half-edge orbit(s)"),
            "{}",
            note.message
        );
    }
}
