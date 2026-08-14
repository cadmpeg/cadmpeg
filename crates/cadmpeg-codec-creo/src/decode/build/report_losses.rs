// SPDX-License-Identifier: Apache-2.0
//! Loss notes derived from coverage counters and undecoded PSB layers.

use std::collections::BTreeMap;

use crate::container::ContainerScan;

use super::super::{LossNote, LossTaxonomy, Severity};
use super::coverage::torus_parameter_coverage;

pub(super) fn coverage_count(coverage: &BTreeMap<String, usize>, key: &str) -> usize {
    coverage.get(key).copied().unwrap_or(0)
}

pub(super) fn push_legacy_value_losses(
    losses: &mut Vec<LossNote>,
    coverage: &BTreeMap<String, usize>,
) {
    let unresolved_legacy_reals = coverage_count(coverage, "unresolved_legacy_real_value_count");
    if unresolved_legacy_reals != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_legacy_reals} legacy type-2 value row(s) did not form a complete \
                 finite scalar or dimension-complete real array."
            ),
            provenance: None,
        });
    }
    let unresolved_legacy_integers =
        coverage_count(coverage, "unresolved_legacy_integer_value_count");
    if unresolved_legacy_integers != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_legacy_integers} legacy type-1 value row(s) did not form a signed \
                 32-bit scalar or dimension-complete integer array."
            ),
            provenance: None,
        });
    }
    for type_code in [3u8, 4] {
        let unresolved = coverage_count(
            coverage,
            &format!("unresolved_legacy_type_{type_code}_value_count"),
        );
        if unresolved != 0 {
            losses.push(LossNote {
                code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
                severity: Severity::Warning,
                message: format!(
                    "{unresolved} legacy type-{type_code} value row(s) use an undefined \
                     continuation form."
                ),
                provenance: None,
            });
        }
        let undecoded = coverage_count(
            coverage,
            &format!("undecoded_legacy_type_{type_code}_encoding_count"),
        );
        if undecoded != 0 {
            losses.push(LossNote {
                code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::AttributesNotTransferred),
                severity: Severity::Warning,
                message: format!(
                    "{undecoded} legacy type-{type_code} byte-string value(s) retain exact \
                     source bytes because their character encoding is not UTF-8."
                ),
                provenance: None,
            });
        }
    }
    for type_code in [5u8, 7, 9, 11] {
        let unresolved = coverage_count(
            coverage,
            &format!("unresolved_legacy_type_{type_code}_value_count"),
        );
        if unresolved != 0 {
            losses.push(LossNote {
                code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
                severity: Severity::Warning,
                message: format!(
                    "{unresolved} legacy type-{type_code} value row(s) did not form an unsigned \
                     32-bit scalar or dimension-complete unsigned array."
                ),
                provenance: None,
            });
        }
    }
    let unresolved_legacy_type_6 = coverage_count(coverage, "unresolved_legacy_type_6_value_count");
    if unresolved_legacy_type_6 != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_legacy_type_6} legacy type-6 value row(s) did not form a complete \
                 finite compact-real scalar or dimension-complete real array."
            ),
            provenance: None,
        });
    }
    let incomplete_legacy_object_arrays =
        coverage_count(coverage, "incomplete_legacy_object_array_count");
    if incomplete_legacy_object_arrays != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{incomplete_legacy_object_arrays} legacy type-0 object array(s) have a direct \
                 element count that differs from their declared extents."
            ),
            provenance: None,
        });
    }
    let unresolved_legacy_objects =
        coverage_count(coverage, "unresolved_legacy_object_value_count");
    if unresolved_legacy_objects != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_legacy_objects} legacy type-0 value row(s) use an undefined object \
                 payload form."
            ),
            provenance: None,
        });
    }
    let incomplete_legacy_string_arrays =
        coverage_count(coverage, "incomplete_legacy_string_array_count");
    if incomplete_legacy_string_arrays != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{incomplete_legacy_string_arrays} legacy type-10 string array(s) have a direct \
                 element count that differs from their first extent."
            ),
            provenance: None,
        });
    }
    let unresolved_legacy_strings =
        coverage_count(coverage, "unresolved_legacy_string_value_count");
    if unresolved_legacy_strings != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::RecordNotTyped),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_legacy_strings} legacy type-10 value row(s) use an undefined \
                 continuation form."
            ),
            provenance: None,
        });
    }
    let undecoded_legacy_string_encodings =
        coverage_count(coverage, "undecoded_legacy_string_encoding_count");
    if undecoded_legacy_string_encodings != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::AttributesNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{undecoded_legacy_string_encodings} legacy type-10 string element(s) retain \
                 exact source bytes because their character encoding is not UTF-8."
            ),
            provenance: None,
        });
    }

    let conflicting_triangle_strip_representations = coverage_count(
        coverage,
        "conflicting_primitive_triangle_strip_representation_count",
    );
    if conflicting_triangle_strip_representations != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{conflicting_triangle_strip_representations} primitive triangle-strip record(s) \
                 contain complete position representations that disagree."
            ),
            provenance: None,
        });
    }
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
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {placed_plane_count} model-space plane carrier(s) from complete \
                 VisibGeom local-system support frames."
            ),
            provenance: None,
        });
    }

    if !container_only && topology_bound_plane_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {topology_bound_plane_count} model-space plane carrier(s) from \
                 circle, ellipse, or line boundary carriers, coplanar NURBS control nets, or \
                 three or more non-collinear solved boundary vertices of the same native face."
            ),
            provenance: None,
        });
    }

    if !container_only && first_instance_prototype_surface_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {first_instance_prototype_surface_count} first-instance ND plane, \
                 cylinder, cone, torus, or interpolation-spline carrier(s) from complete named \
                 parameters."
            ),
            provenance: None,
        });
    }

    if !container_only && paired_envelope_sphere_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {paired_envelope_sphere_count} sphere carrier(s) from complementary \
                 five-coordinate type-26 hemisphere envelopes and their shared zero-major-radius \
                 prototype."
            ),
            provenance: None,
        });
    }

    if !container_only && positional_torus_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {positional_torus_count} exact positional torus carrier(s) from \
                 complete local-system, radius, and five-coordinate envelope bodies."
            ),
            provenance: None,
        });
    }

    if !container_only && positional_cylinder_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {positional_cylinder_count} exact positional cylinder carrier(s) \
                 from complete per-instance parameter bodies."
            ),
            provenance: None,
        });
    }

    if !container_only && positional_cone_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {positional_cone_count} exact positional cone carrier(s) from \
                 complete support-apex or planar-envelope bodies."
            ),
            provenance: None,
        });
    }

    if !container_only && positional_line_extrusion_plane_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {positional_line_extrusion_plane_count} unbound straight positional \
                 surface-of-extrusion carrier(s) from complete sweep-direction and directrix \
                 frames."
            ),
            provenance: None,
        });
    }

    if !container_only && tabulated_cylinder_spline_extrusion_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {tabulated_cylinder_spline_extrusion_count} tabulated-cylinder \
                 cubic spline extrusion carrier(s) from uniquely matched directrix and frame spans."
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.planes.datums.is_empty() {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {} exact model-space construction datum plane carrier(s) from ActDatums; \
                 these are unbounded reference planes, not model B-rep faces.",
                scan.planes.datums.len()
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.references.lines.is_empty() {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {} finite model-space reference line carrier(s) from MdlRefInfo; \
                 their byte-exact endpoints remain attached as native line records.",
                scan.references.lines.len()
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.references.circles.is_empty() {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {} circular reference carrier(s) from MdlRefInfo rows whose stored center, radius, and endpoints satisfy the circle equation; byte-exact endpoints remain attached as native circle records.",
                scan.references.circles.len()
            ),
            provenance: None,
        });
    }

    if !container_only && !scan.references.ellipses.is_empty() {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {} elliptical reference carrier(s) from MdlRefInfo conic rows whose frame, coefficient radii, and antipodal endpoints satisfy one ellipse equation; the source conic records remain byte-exact native records.",
                scan.references.ellipses.len()
            ),
            provenance: None,
        });
    }

    let topological_point_count = coverage_count(coverage, "transferred_topological_point_count");
    if !container_only && topological_point_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {topological_point_count} exact model-space point(s) for native topological vertex orbits from unique placed-carrier intersections or pcurve endpoint domains constrained by agreeing face maps and incident analytic edge carriers."
            ),
            provenance: None,
        });
    }

    let native_topological_edge_count =
        coverage_count(coverage, "transferred_native_topological_edge_count");
    if !container_only && native_topological_edge_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {native_topological_edge_count} native topological edge(s) whose endpoint vertex orbits have exact model-space points."
            ),
            provenance: None,
        });
    }

    let analytic_pcurve_carrier_count =
        coverage_count(coverage, "transferred_analytic_pcurve_carrier_count");
    if !container_only && analytic_pcurve_carrier_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {analytic_pcurve_carrier_count} exact analytic carrier(s) by mapping native linear pcurves through placed planar, cylindrical, conical, spherical, or toroidal face charts."
            ),
            provenance: None,
        });
    }

    let extrusion_plane_boundary_curve_count =
        coverage_count(coverage, "transferred_extrusion_plane_boundary_curve_count");
    if !container_only && extrusion_plane_boundary_curve_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {extrusion_plane_boundary_curve_count} exact NURBS boundary \
                 carrier(s) where one tabulated-extrusion boundary lies in an adjacent plane \
                 and every other control point lies strictly on one side."
            ),
            provenance: None,
        });
    }

    let extrusion_plane_section_generator_curve_count = coverage_count(
        coverage,
        "transferred_extrusion_plane_section_generator_curve_count",
    );
    if !container_only && extrusion_plane_section_generator_curve_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {extrusion_plane_section_generator_curve_count} exact NURBS \
                 generator carrier(s) where an adjacent plane contains the sweep direction and \
                 the cubic directrix has exactly one plane intersection."
            ),
            provenance: None,
        });
    }

    let shared_extrusion_generator_curve_count = coverage_count(
        coverage,
        "transferred_shared_extrusion_generator_curve_count",
    );
    if !container_only && shared_extrusion_generator_curve_count != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
                "Transferred {shared_extrusion_generator_curve_count} exact shared NURBS \
                 generator carrier(s) whose two tabulated-extrusion control nets meet on the \
                 same linear boundary and lie strictly on opposite sides of a plane through it."
            ),
            provenance: None,
        });
    }

    let torus_coverage = torus_parameter_coverage(scan);
    if torus_coverage.radius_overrides != 0
        || torus_coverage.replayed_minor_radii != 0
        || torus_coverage.outline_extents != 0
        || torus_coverage.five_coordinate_envelopes != 0
        || torus_coverage.split_coordinate_envelopes != 0
    {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::CarrierSummary),
            severity: Severity::Info,
            message: format!(
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
            provenance: None,
        });
    }
}

pub(super) fn push_structural_layer_notes(losses: &mut Vec<LossNote>, scan: &ContainerScan) {
    // The specific undecoded PSB layers that gate per-instance geometry.
    losses.push(LossNote {
        code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
        severity: Severity::Blocking,
        message: "Additional model-space carriers are gated by unresolved lane-specific scalar \
                  prefixes, feature-local transform bindings, placement-incomplete or untagged \
                  `0x26` torus/sphere variants, and the round/fillet feature evaluator. These gaps \
                  prevent transfer of the remaining non-plane per-instance surfaces, curves, and \
                  vertices."
            .to_string(),
        provenance: None,
    });

    // Topology.
    losses.push(LossNote {
        code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::TopologyNotTransferred),
        severity: Severity::Blocking,
        message: "Native curve half-edges and closed loops were decoded. Components with complete \
                  solved boundaries and unique face orientations transfer as \
                  body/region/shell/face/loop/coedge/edge/vertex graphs; multi-loop faces use \
                  strict containment in a placed or boundary-proven plane. Remaining components \
                  require face-instance partitioning, surface parameter bindings, curve geometry, \
                  or vertex coordinates."
            .to_string(),
        provenance: None,
    });

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
    losses.push(LossNote {
        code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
        severity: Severity::Warning,
        message: format!(
            "Named feature operations and their decoded dependency/input tables transfer as typed \
             or native design records. {curve_expression_transfer} \
             Full neutral operation semantics\
             {configuration_gap}, graph, case-study, cabling, and cross-model relation functions, \
             materials, and display data \
             remain untransferred."
        ),
        provenance: None,
    });
}
