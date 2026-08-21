// SPDX-License-Identifier: Apache-2.0
//! Loss notes derived from coverage counters and undecoded PSB layers.

use std::collections::BTreeMap;

use crate::container::ContainerScan;
use crate::decode::surfaces::{BrepTransferDiagnostics, FaceAdmissionRejection};
use crate::loss::CreoLossCode;

use super::coverage::torus_parameter_coverage;
use cadmpeg_ir::report::LossNote;

pub(super) fn coverage_count(coverage: &BTreeMap<String, usize>, key: &str) -> usize {
    coverage.get(key).copied().unwrap_or(0)
}

pub(super) fn push_legacy_value_losses(
    losses: &mut Vec<LossNote>,
    coverage: &BTreeMap<String, usize>,
) {
    let unresolved_legacy_reals = coverage_count(coverage, "unresolved_legacy_real_value_count");
    if unresolved_legacy_reals != 0 {
        losses.push(CreoLossCode::LegacyRealValueUnresolved.note(format!(
            "{unresolved_legacy_reals} legacy type-2 value row(s) did not form a complete \
             finite scalar or dimension-complete real array."
        )));
    }
    let unresolved_legacy_integers =
        coverage_count(coverage, "unresolved_legacy_integer_value_count");
    if unresolved_legacy_integers != 0 {
        losses.push(CreoLossCode::LegacyIntegerValueUnresolved.note(format!(
            "{unresolved_legacy_integers} legacy type-1 value row(s) did not form a signed \
             32-bit scalar or dimension-complete integer array."
        )));
    }
    for type_code in [3u8, 4] {
        let unresolved = coverage_count(
            coverage,
            &format!("unresolved_legacy_type_{type_code}_value_count"),
        );
        if unresolved != 0 {
            losses.push(CreoLossCode::LegacyContinuationFormUndefined.note(format!(
                "{unresolved} legacy type-{type_code} value row(s) use an undefined \
                 continuation form."
            )));
        }
        let undecoded = coverage_count(
            coverage,
            &format!("undecoded_legacy_type_{type_code}_encoding_count"),
        );
        if undecoded != 0 {
            losses.push(CreoLossCode::LegacyByteStringEncodingRetained.note(format!(
                "{undecoded} legacy type-{type_code} byte-string value(s) retain exact \
                 source bytes because their character encoding is not UTF-8."
            )));
        }
    }
    for type_code in [5u8, 7, 9, 11] {
        let unresolved = coverage_count(
            coverage,
            &format!("unresolved_legacy_type_{type_code}_value_count"),
        );
        if unresolved != 0 {
            losses.push(CreoLossCode::LegacyUnsignedValueUnresolved.note(format!(
                "{unresolved} legacy type-{type_code} value row(s) did not form an unsigned \
                 32-bit scalar or dimension-complete unsigned array."
            )));
        }
    }
    let unresolved_legacy_type_6 = coverage_count(coverage, "unresolved_legacy_type_6_value_count");
    if unresolved_legacy_type_6 != 0 {
        losses.push(CreoLossCode::LegacyCompactRealUnresolved.note(format!(
            "{unresolved_legacy_type_6} legacy type-6 value row(s) did not form a complete \
             finite compact-real scalar or dimension-complete real array."
        )));
    }
    let incomplete_legacy_object_arrays =
        coverage_count(coverage, "incomplete_legacy_object_array_count");
    if incomplete_legacy_object_arrays != 0 {
        losses.push(CreoLossCode::LegacyObjectArrayIncomplete.note(format!(
            "{incomplete_legacy_object_arrays} legacy type-0 object array(s) have a direct \
             element count that differs from their declared extents."
        )));
    }
    let unresolved_legacy_objects =
        coverage_count(coverage, "unresolved_legacy_object_value_count");
    if unresolved_legacy_objects != 0 {
        losses.push(CreoLossCode::LegacyObjectPayloadUndefined.note(format!(
            "{unresolved_legacy_objects} legacy type-0 value row(s) use an undefined object \
             payload form."
        )));
    }
    let incomplete_legacy_string_arrays =
        coverage_count(coverage, "incomplete_legacy_string_array_count");
    if incomplete_legacy_string_arrays != 0 {
        losses.push(CreoLossCode::LegacyStringArrayIncomplete.note(format!(
            "{incomplete_legacy_string_arrays} legacy type-10 string array(s) have a direct \
             element count that differs from their first extent."
        )));
    }
    let unresolved_legacy_strings =
        coverage_count(coverage, "unresolved_legacy_string_value_count");
    if unresolved_legacy_strings != 0 {
        losses.push(
            CreoLossCode::LegacyStringContinuationUndefined.note(format!(
                "{unresolved_legacy_strings} legacy type-10 value row(s) use an undefined \
             continuation form."
            )),
        );
    }
    let undecoded_legacy_string_encodings =
        coverage_count(coverage, "undecoded_legacy_string_encoding_count");
    if undecoded_legacy_string_encodings != 0 {
        losses.push(CreoLossCode::LegacyStringEncodingRetained.note(format!(
            "{undecoded_legacy_string_encodings} legacy type-10 string element(s) retain \
             exact source bytes because their character encoding is not UTF-8."
        )));
    }

    let conflicting_triangle_strip_representations = coverage_count(
        coverage,
        "conflicting_primitive_triangle_strip_representation_count",
    );
    if conflicting_triangle_strip_representations != 0 {
        losses.push(
            CreoLossCode::TriangleStripRepresentationConflict.note(format!(
                "{conflicting_triangle_strip_representations} primitive triangle-strip record(s) \
             contain complete position representations that disagree."
            )),
        );
    }
}

pub(super) fn push_brep_transfer_note(
    losses: &mut Vec<LossNote>,
    diagnostics: &BrepTransferDiagnostics,
    geometry_section_count: usize,
) {
    let rejected_face_count = diagnostics
        .rejected_faces
        .values()
        .map(|evidence| evidence.count)
        .sum::<usize>();
    let rejection_details = FaceAdmissionRejection::ALL
        .into_iter()
        .filter_map(|reason| {
            let evidence = diagnostics.rejected_faces.get(&reason)?;
            let samples = evidence
                .sample_details
                .iter()
                .map(|detail| {
                    let half_edges = detail
                        .boundary_half_edges
                        .iter()
                        .map(|half_edge| format!("{}:{}", half_edge.curve_id, half_edge.side))
                        .collect::<Vec<_>>()
                        .join("|");
                    let vertices = detail
                        .vertex_ids
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("|");
                    match (half_edges.is_empty(), vertices.is_empty()) {
                        (true, true) => detail.face_id.to_string(),
                        (false, true) => format!("{}[edges:{half_edges}]", detail.face_id),
                        (true, false) => format!("{}[vertices:{vertices}]", detail.face_id),
                        (false, false) => {
                            format!("{}[edges:{half_edges};vertices:{vertices}]", detail.face_id)
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            let samples = if samples.is_empty() {
                evidence
                    .sample_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            } else {
                samples
            };
            Some(if samples.is_empty() {
                format!("{}={}", reason.label(), evidence.count)
            } else {
                format!(
                    "{}={} (sample faces: {samples})",
                    reason.label(),
                    evidence.count
                )
            })
        })
        .collect::<Vec<_>>()
        .join(", ");
    let rejection_details = if rejection_details.is_empty() {
        "none".to_string()
    } else {
        rejection_details
    };
    let mut component_gate_reasons = Vec::new();
    if diagnostics.body_count_mismatch {
        component_gate_reasons.push("selected body count mismatch".to_string());
    }
    if diagnostics.legacy_body_ownership_ambiguous {
        component_gate_reasons.push("legacy body ownership ambiguous".to_string());
    }
    if diagnostics.empty_component_count != 0 {
        component_gate_reasons.push(format!(
            "{} empty admitted component(s)",
            diagnostics.empty_component_count
        ));
    }
    let component_gate = component_gate_reasons.join(", ");
    let component_gate = if component_gate.is_empty() {
        "passed".to_string()
    } else {
        component_gate
    };
    let pcurve_mismatch_samples = diagnostics
        .vertex_solve
        .pcurve
        .mismatch_samples
        .iter()
        .map(|detail| {
            format!(
                "{}[faces:{}|{};same:{:.3e};reverse:{:.3e}]",
                detail.curve_id,
                detail.faces[0],
                detail.faces[1],
                detail.same_order_error,
                detail.reverse_order_error,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let pcurve_mismatch_evidence = if pcurve_mismatch_samples.is_empty() {
        String::new()
    } else {
        format!(" Pcurve mismatch samples: {pcurve_mismatch_samples}.")
    };
    let pcurve_activity_evidence = {
        let pcurve = &diagnostics.vertex_solve.pcurve;
        if pcurve.inactive_paths > 0
            || pcurve.inactive_records > 0
            || pcurve.partial_records > 0
            || pcurve.topology_mismatch_records > 0
        {
            format!(
                " Pcurve path activity: inactive paths={}, inactive records={}, partial records={}, topology mismatches={}.",
                pcurve.inactive_paths,
                pcurve.inactive_records,
                pcurve.partial_records,
                pcurve.topology_mismatch_records,
            )
        } else {
            String::new()
        }
    };
    let carrier_rejection_samples = diagnostics
        .vertex_solve
        .carrier_rejection_samples
        .iter()
        .map(|sample| {
            format!(
                "{}[faces:{};carriers:{};pair:{};triple:{};valid:{};unique:{}]",
                sample.vertex_id,
                sample
                    .incident_face_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("|"),
                sample.carrier_kinds.join("|"),
                sample.pair_intersections,
                sample.triple_intersections,
                sample.valid_candidates,
                sample.unique_solutions,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let carrier_rejection_evidence = if diagnostics
        .vertex_solve
        .carrier_no_geometric_candidate_vertices
        == 0
        && diagnostics.vertex_solve.carrier_no_valid_candidate_vertices == 0
        && carrier_rejection_samples.is_empty()
    {
        String::new()
    } else {
        format!(
            " Carrier solver classification: no geometric candidate={}, no valid candidate={}; \
             rejection samples: {carrier_rejection_samples}.",
            diagnostics
                .vertex_solve
                .carrier_no_geometric_candidate_vertices,
            diagnostics.vertex_solve.carrier_no_valid_candidate_vertices,
        )
    };
    let vertex_evidence = format!(
        "Boundary evidence: {} curve(s), {} without a unique incidence pair, {} with an \
         unsolved endpoint vertex. Vertex solver: {} topological, {} carrier intersections, \
         {} carrier-bearing vertices, {} pair-intersection candidate(s), {} triple-intersection \
         candidate(s), {} validated carrier candidate(s), {} carrier vertices with no candidate, \
         {} ambiguous carrier vertices, {} pcurve record(s), {} pcurve path(s), {} path(s) without \
         a unique surface, {} unevaluable path(s), {} mapped path(s), {} unmapped record(s), {} \
         inconsistent record(s), {} accepted record(s) ({} complete), {} conflicting curve(s), {} \
         pcurve endpoint evidence ({} complete), {} pcurve constraint(s), {} analytic domain(s), \
         {} NURBS endpoint constraint(s), {} directed endpoint conflict(s), {} solved.{}{}{}",
        diagnostics.boundary_curve_count,
        diagnostics.boundary_curve_missing_incidence_count,
        diagnostics.boundary_curve_unsolved_vertex_count,
        diagnostics.vertex_solve.topological_vertices,
        diagnostics.vertex_solve.carrier_points,
        diagnostics.vertex_solve.carrier_incident_vertices,
        diagnostics.vertex_solve.carrier_pair_candidates,
        diagnostics.vertex_solve.carrier_triple_candidates,
        diagnostics.vertex_solve.carrier_valid_candidates,
        diagnostics.vertex_solve.carrier_zero_candidate_vertices,
        diagnostics
            .vertex_solve
            .carrier_ambiguous_candidate_vertices,
        diagnostics.vertex_solve.pcurve.records,
        diagnostics.vertex_solve.pcurve.paths,
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
        carrier_rejection_evidence,
    );

    losses.push(CreoLossCode::BrepTransferIncomplete.note(format!(
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
    )));
}

pub(super) fn push_carrier_transfer_notes(
    losses: &mut Vec<LossNote>,
    scan: &ContainerScan,
    coverage: &BTreeMap<String, usize>,
    container_only: bool,
    placed_plane_count: usize,
) {
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
        losses.push(CreoLossCode::CarrierVisibGeomPlanes.note(format!(
            "Transferred {placed_plane_count} model-space plane carrier(s) from complete \
             VisibGeom local-system support frames."
        )));
    }

    if !container_only && topology_bound_plane_count != 0 {
        losses.push(CreoLossCode::CarrierTopologyBoundPlanes.note(format!(
            "Transferred {topology_bound_plane_count} model-space plane carrier(s) from \
             circle, ellipse, or line boundary carriers, coplanar NURBS control nets, or \
             three or more non-collinear solved boundary vertices of the same native face."
        )));
    }

    if !container_only && first_instance_prototype_surface_count != 0 {
        losses.push(CreoLossCode::CarrierFirstInstancePrototypes.note(format!(
            "Transferred {first_instance_prototype_surface_count} first-instance ND plane, \
             cylinder, cone, torus, or interpolation-spline carrier(s) from complete named \
             parameters."
        )));
    }

    if !container_only && paired_envelope_sphere_count != 0 {
        losses.push(CreoLossCode::CarrierPairedEnvelopeSpheres.note(format!(
            "Transferred {paired_envelope_sphere_count} sphere carrier(s) from complementary \
             five-coordinate type-26 hemisphere envelopes and their shared zero-major-radius \
             prototype."
        )));
    }

    if !container_only && positional_torus_count != 0 {
        losses.push(CreoLossCode::CarrierPositionalTori.note(format!(
            "Transferred {positional_torus_count} exact positional torus carrier(s) from \
             complete local-system, radius, and five-coordinate envelope bodies."
        )));
    }

    if !container_only && positional_cylinder_count != 0 {
        losses.push(CreoLossCode::CarrierPositionalCylinders.note(format!(
            "Transferred {positional_cylinder_count} exact positional cylinder carrier(s) \
             from complete per-instance parameter bodies."
        )));
    }

    if !container_only && positional_cone_count != 0 {
        losses.push(CreoLossCode::CarrierPositionalCones.note(format!(
            "Transferred {positional_cone_count} exact positional cone carrier(s) from \
             complete support-apex or planar-envelope bodies."
        )));
    }

    if !container_only && positional_line_extrusion_plane_count != 0 {
        losses.push(CreoLossCode::CarrierLineExtrusionPlanes.note(format!(
            "Transferred {positional_line_extrusion_plane_count} unbound straight positional \
             surface-of-extrusion carrier(s) from complete sweep-direction and directrix \
             frames."
        )));
    }

    if !container_only && tabulated_cylinder_spline_extrusion_count != 0 {
        losses.push(
            CreoLossCode::CarrierTabulatedCylinderExtrusions.note(format!(
                "Transferred {tabulated_cylinder_spline_extrusion_count} tabulated-cylinder \
             cubic spline extrusion carrier(s) from uniquely matched directrix and frame spans."
            )),
        );
    }

    if !container_only && !scan.planes.datums.is_empty() {
        losses.push(CreoLossCode::CarrierDatumPlanes.note(format!(
            "Transferred {} exact model-space construction datum plane carrier(s) from ActDatums; \
             these are unbounded reference planes, not model B-rep faces.",
            scan.planes.datums.len()
        )));
    }

    if !container_only && !scan.references.lines.is_empty() {
        losses.push(CreoLossCode::CarrierReferenceLines.note(format!(
            "Transferred {} finite model-space reference line carrier(s) from MdlRefInfo; \
             their byte-exact endpoints remain attached as native line records.",
            scan.references.lines.len()
        )));
    }

    if !container_only && !scan.references.circles.is_empty() {
        losses.push(CreoLossCode::CarrierReferenceCircles.note(format!(
            "Transferred {} circular reference carrier(s) from MdlRefInfo rows whose stored center, radius, and endpoints satisfy the circle equation; byte-exact endpoints remain attached as native circle records.",
            scan.references.circles.len()
        )));
    }

    if !container_only && !scan.references.ellipses.is_empty() {
        losses.push(CreoLossCode::CarrierReferenceEllipses.note(format!(
            "Transferred {} elliptical reference carrier(s) from MdlRefInfo conic rows whose frame, coefficient radii, and antipodal endpoints satisfy one ellipse equation; the source conic records remain byte-exact native records.",
            scan.references.ellipses.len()
        )));
    }

    let topological_point_count = coverage_count(coverage, "transferred_topological_point_count");
    if !container_only && topological_point_count != 0 {
        losses.push(CreoLossCode::CarrierTopologicalPoints.note(format!(
            "Transferred {topological_point_count} exact model-space point(s) for native topological vertex orbits from unique placed-carrier intersections or pcurve endpoint domains constrained by agreeing face maps and incident analytic edge carriers."
        )));
    }

    let native_topological_edge_count =
        coverage_count(coverage, "transferred_native_topological_edge_count");
    if !container_only && native_topological_edge_count != 0 {
        losses.push(CreoLossCode::CarrierTopologicalEdges.note(format!(
            "Transferred {native_topological_edge_count} native topological edge(s) whose endpoint vertex orbits have exact model-space points."
        )));
    }

    let analytic_pcurve_carrier_count =
        coverage_count(coverage, "transferred_analytic_pcurve_carrier_count");
    if !container_only && analytic_pcurve_carrier_count != 0 {
        losses.push(CreoLossCode::CarrierAnalyticPcurves.note(format!(
            "Transferred {analytic_pcurve_carrier_count} exact analytic carrier(s) by mapping native linear pcurves through placed planar, cylindrical, conical, spherical, or toroidal face charts."
        )));
    }

    let extrusion_plane_boundary_curve_count =
        coverage_count(coverage, "transferred_extrusion_plane_boundary_curve_count");
    if !container_only && extrusion_plane_boundary_curve_count != 0 {
        losses.push(CreoLossCode::CarrierExtrusionBoundaryCurves.note(format!(
            "Transferred {extrusion_plane_boundary_curve_count} exact NURBS boundary \
             carrier(s) where one tabulated-extrusion boundary lies in an adjacent plane \
             and every other control point lies strictly on one side."
        )));
    }

    let extrusion_plane_section_generator_curve_count = coverage_count(
        coverage,
        "transferred_extrusion_plane_section_generator_curve_count",
    );
    if !container_only && extrusion_plane_section_generator_curve_count != 0 {
        losses.push(
            CreoLossCode::CarrierExtrusionSectionGenerators.note(format!(
                "Transferred {extrusion_plane_section_generator_curve_count} exact NURBS \
             generator carrier(s) where an adjacent plane contains the sweep direction and \
             the cubic directrix has exactly one plane intersection."
            )),
        );
    }

    let shared_extrusion_generator_curve_count = coverage_count(
        coverage,
        "transferred_shared_extrusion_generator_curve_count",
    );
    if !container_only && shared_extrusion_generator_curve_count != 0 {
        losses.push(CreoLossCode::CarrierSharedExtrusionGenerators.note(format!(
            "Transferred {shared_extrusion_generator_curve_count} exact shared NURBS \
             generator carrier(s) whose two tabulated-extrusion control nets meet on the \
             same linear boundary and lie strictly on opposite sides of a plane through it."
        )));
    }

    let torus_coverage = torus_parameter_coverage(scan);
    if torus_coverage.radius_overrides != 0
        || torus_coverage.replayed_minor_radii != 0
        || torus_coverage.outline_extents != 0
        || torus_coverage.five_coordinate_envelopes != 0
        || torus_coverage.split_coordinate_envelopes != 0
    {
        losses.push(CreoLossCode::CarrierTorusParameterRetention.note(format!(
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
        )));
    }
}

pub(super) fn push_structural_layer_notes(losses: &mut Vec<LossNote>, scan: &ContainerScan) {
    // The specific undecoded PSB layers that gate per-instance geometry.
    losses.push(CreoLossCode::GeometryInstanceCarriersGated.note(
        "Additional model-space carriers are gated by unresolved lane-specific scalar \
         prefixes, feature-local transform bindings, placement-incomplete or untagged \
         `0x26` torus/sphere variants, and the round/fillet feature evaluator. These gaps \
         prevent transfer of the remaining non-plane per-instance surfaces, curves, and \
         vertices.",
    ));

    // Topology.
    losses.push(CreoLossCode::TopologyIncompleteComponents.note(
        "Native curve half-edges and closed loops were decoded. Components with complete \
         solved boundaries and unique face orientations transfer as \
         body/region/shell/face/loop/coedge/edge/vertex graphs; multi-loop faces use \
         strict containment in a placed or boundary-proven plane. Remaining components \
         require face-instance partitioning, surface parameter bindings, curve geometry, \
         or vertex coordinates.",
    ));

    let configuration_gap = match scan.framing.family_table.map(|record| record.pointer) {
        Some(crate::container::FamilyTablePointer::Null) => "",
        Some(crate::container::FamilyTablePointer::Entity(_)) => {
            ", configuration driver-table rows"
        }
        None => ", configuration presence",
    };
    let unevaluated_curve_expression_record_count = scan
        .curves
        .expressions
        .iter()
        .filter(|record| {
            !record.backup
                && (!record.prohibited_constructs.is_empty()
                    || record.solve_blocks.iter().any(|block| {
                        block.solutions.is_empty() || block.solutions.iter().any(Option::is_none)
                    })
                    || record.unresolved_solve_control)
        })
        .count();
    let curve_expression_transfer = if unevaluated_curve_expression_record_count == 0 {
        "Curve-equation assignments transfer with their source, dependencies, and closed numeric \
         and string operator and deterministic function values."
            .to_string()
    } else {
        format!(
            "Admitted curve-equation assignments transfer with their source, dependencies, and \
             closed numeric and string operator and deterministic function values. \
             {unevaluated_curve_expression_record_count} active curve-equation record(s) \
             containing prohibited datum-curve constructs or unresolved simultaneous-solve \
             control retain \
             source and dependencies without solve-dependent assignment values or derived curves."
        )
    };

    // Features, history, materials.
    losses.push(
        CreoLossCode::FeatureNeutralSemanticsIncomplete.note(format!(
            "Named feature operations and their decoded dependency/input tables transfer as typed \
         or native design records. {curve_expression_transfer} \
         Full neutral operation semantics\
         {configuration_gap}, graph, case-study, cabling, and cross-model relation functions, \
         materials, and display data \
         remain untransferred."
        )),
    );
}
