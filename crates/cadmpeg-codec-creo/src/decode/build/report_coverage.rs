// SPDX-License-Identifier: Apache-2.0
//! Coverage-drop and incomplete-feature loss notes.

use std::collections::BTreeMap;

use super::super::{
    constraint_kind_breakdown, surface_family, LossNote, LossTaxonomy, Severity, SURFACE_KINDS,
};
use super::report_losses::coverage_count;

pub(super) fn push_coverage_drop_losses(
    losses: &mut Vec<LossNote>,
    coverage: &BTreeMap<String, usize>,
) {
    let untransferred_surface_rows =
        coverage_count(coverage, "untransferred_visible_surface_row_count");
    if untransferred_surface_rows != 0 {
        let unresolved_families = SURFACE_KINDS
            .into_iter()
            .filter_map(|kind| {
                let family = surface_family(kind);
                let count = coverage_count(
                    coverage,
                    &format!("untransferred_visible_{family}_surface_row_count"),
                );
                (count != 0).then_some(format!("{family}={count}"))
            })
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{untransferred_surface_rows} unique VisibGeom surface row(s) were not \
                 transferred as carriers and remain structural namespace records \
                 ({unresolved_families})."
            ),
            provenance: None,
        });
    }
    let untransferred_curve_rows =
        coverage_count(coverage, "untransferred_visible_curve_row_count");
    if untransferred_curve_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{untransferred_curve_rows} unique VisibGeom curve-topology row(s) were not \
                 transferred as carriers and remain structural namespace records."
            ),
            provenance: None,
        });
    }
    let ambiguous_surface_rows = coverage_count(coverage, "ambiguous_visible_surface_row_count");
    if ambiguous_surface_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Info,
            message: format!(
                "{ambiguous_surface_rows} VisibGeom surface row(s) share a non-unique identity \
                 and were not resolved to a single carrier."
            ),
            provenance: None,
        });
    }
    let ambiguous_curve_rows = coverage_count(coverage, "ambiguous_visible_curve_row_count");
    if ambiguous_curve_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Info,
            message: format!(
                "{ambiguous_curve_rows} VisibGeom curve-topology row(s) share a non-unique \
                 identity and were not resolved to a single carrier."
            ),
            provenance: None,
        });
    }
    let missing_segment_rows = coverage_count(coverage, "missing_feature_segment_row_count");
    if missing_segment_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{missing_segment_rows} declared section segment row(s) did not decode and remain \
                 unavailable to the defining sketch."
            ),
            provenance: None,
        });
    }
    let missing_relation_rows = coverage_count(coverage, "missing_feature_relation_row_count");
    if missing_relation_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{missing_relation_rows} declared section relation row(s) did not decode; the \
                 affected complete-table solver identities remain unavailable."
            ),
            provenance: None,
        });
    }
    let malformed_relation_tables =
        coverage_count(coverage, "malformed_feature_relation_table_count");
    if malformed_relation_tables != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{malformed_relation_tables} section relation table(s) use the invalid zero \
                 allocation count."
            ),
            provenance: None,
        });
    }
    let missing_skamp_rows = coverage_count(coverage, "missing_feature_skamp_row_count");
    if missing_skamp_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{missing_skamp_rows} declared section incidence row(s) did not decode; the \
                 affected complete-table solver identities remain unavailable."
            ),
            provenance: None,
        });
    }
    let missing_triple_rows = coverage_count(coverage, "missing_feature_relation_triple_row_count");
    if missing_triple_rows != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{missing_triple_rows} declared section relation-incidence join row(s) did not \
                 decode; the affected complete-table solver identities remain unavailable."
            ),
            provenance: None,
        });
    }
    let unresolved_segment_geometry =
        coverage_count(coverage, "unresolved_feature_segment_geometry_count");
    if unresolved_segment_geometry != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::GeometryNotTransferred),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_segment_geometry} decoded section segment(s) retain source-native \
                 geometry because their exact neutral construction remains unresolved."
            ),
            provenance: None,
        });
    }
    let active_native_skamps =
        coverage_count(coverage, "active_native_feature_skamp_constraint_count");
    if active_native_skamps != 0 {
        let kinds = constraint_kind_breakdown(coverage, "active_native_feature_skamp_type_");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{active_native_skamps} active section incidence constraint(s) retain native \
                 operands because their neutral semantics or referenced geometry remain unresolved \
                 ({kinds})."
            ),
            provenance: None,
        });
    }
    let active_native_relations =
        coverage_count(coverage, "active_native_feature_relation_constraint_count");
    if active_native_relations != 0 {
        let kinds = constraint_kind_breakdown(coverage, "active_native_feature_relation_type_");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{active_native_relations} active section dimension relation(s) retain native \
                 operands because their neutral semantics, incidence join, or referenced geometry \
                 remain unresolved ({kinds})."
            ),
            provenance: None,
        });
    }
    let incomplete_sweeps = coverage_count(coverage, "transferred_incomplete_sweep_feature_count");
    if incomplete_sweeps != 0 {
        let families = [
            (
                "extrude",
                coverage_count(coverage, "transferred_incomplete_extrude_feature_count"),
            ),
            (
                "revolve",
                coverage_count(coverage, "transferred_incomplete_revolve_feature_count"),
            ),
        ]
        .into_iter()
        .filter_map(|(family, count)| (count != 0).then_some(format!("{family}={count}")))
        .collect::<Vec<_>>()
        .join(", ");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{incomplete_sweeps} profile sweep history feature(s) retain incomplete required \
                 construction operands ({families})."
            ),
            provenance: None,
        });
    }
    let incomplete_surface_operations = coverage_count(
        coverage,
        "transferred_incomplete_surface_operation_feature_count",
    );
    if incomplete_surface_operations != 0 {
        let families = [
            (
                "fill",
                coverage_count(
                    coverage,
                    "transferred_incomplete_filled_surface_feature_count",
                ),
            ),
            (
                "knit",
                coverage_count(
                    coverage,
                    "transferred_incomplete_knit_surface_feature_count",
                ),
            ),
            (
                "thicken",
                coverage_count(coverage, "transferred_incomplete_thicken_feature_count"),
            ),
        ]
        .into_iter()
        .filter_map(|(family, count)| (count != 0).then_some(format!("{family}={count}")))
        .collect::<Vec<_>>()
        .join(", ");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{incomplete_surface_operations} surface construction history feature(s) retain \
                 incomplete required operands ({families})."
            ),
            provenance: None,
        });
    }
    let incomplete_other_constructions = coverage_count(
        coverage,
        "transferred_incomplete_other_construction_feature_count",
    );
    if incomplete_other_constructions != 0 {
        let families = [
            (
                "section shape",
                coverage_count(
                    coverage,
                    "transferred_incomplete_section_shape_feature_count",
                ),
            ),
            (
                "pattern",
                coverage_count(coverage, "transferred_incomplete_pattern_feature_count"),
            ),
            (
                "native-axis helix",
                coverage_count(coverage, "transferred_native_axis_helix_feature_count"),
            ),
        ]
        .into_iter()
        .filter_map(|(family, count)| (count != 0).then_some(format!("{family}={count}")))
        .collect::<Vec<_>>()
        .join(", ");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{incomplete_other_constructions} construction history feature(s) retain \
                 unresolved neutral operands ({families})."
            ),
            provenance: None,
        });
    }
    let incomplete_recognized_features =
        coverage_count(coverage, "transferred_incomplete_recognized_feature_count");
    if incomplete_recognized_features != 0 {
        let families = [
            (
                "hole",
                coverage_count(coverage, "transferred_incomplete_hole_feature_count"),
            ),
            (
                "fillet",
                coverage_count(coverage, "transferred_incomplete_fillet_feature_count"),
            ),
            (
                "chamfer",
                coverage_count(coverage, "transferred_incomplete_chamfer_feature_count"),
            ),
            (
                "draft",
                coverage_count(coverage, "transferred_incomplete_draft_feature_count"),
            ),
        ]
        .into_iter()
        .filter_map(|(family, count)| (count != 0).then_some(format!("{family}={count}")))
        .collect::<Vec<_>>()
        .join(", ");
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{incomplete_recognized_features} recognized non-sweep history feature(s) retain \
                 incomplete required construction operands ({families})."
            ),
            provenance: None,
        });
    }
    let explicitly_unresolved_features =
        coverage_count(coverage, "transferred_explicitly_unresolved_feature_count");
    let native_features = coverage_count(coverage, "transferred_native_feature_count");
    if native_features != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{native_features} history feature definition(s) retain only source-native \
                 semantics."
            ),
            provenance: None,
        });
    }
    if explicitly_unresolved_features != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{explicitly_unresolved_features} typed history feature definition(s) retain an \
                 explicitly unresolved model-space construction."
            ),
            provenance: None,
        });
    }
    let unresolved_dimension_driven_variables = coverage_count(
        coverage,
        "unresolved_feature_dimension_driven_variable_count",
    );
    if unresolved_dimension_driven_variables != 0 {
        let unresolved_coordinate_variables = coverage_count(
            coverage,
            "unresolved_feature_dimension_driven_coordinate_variable_count",
        );
        let other_variables = coverage_count(
            coverage,
            "unresolved_feature_dimension_driven_other_variable_count",
        );
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_dimension_driven_variables} dimension-driven section solver \
                 variable(s) retain unresolved exact values: {unresolved_coordinate_variables} \
                 coordinate variable(s) lack a complete dimension equation and {other_variables} \
                 variable(s) have a non-coordinate family whose dimension semantics are \
                 unresolved."
            ),
            provenance: None,
        });
    }
    let unresolved_dimension_driven_guesses =
        coverage_count(coverage, "unresolved_feature_dimension_driven_guess_count");
    if unresolved_dimension_driven_guesses != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_dimension_driven_guesses} section solver variable pre-solve \
                 estimate(s) use a dimension-driven sentinel whose dimension join is unresolved."
            ),
            provenance: None,
        });
    }
    let missing_solver_variables =
        coverage_count(coverage, "missing_feature_solver_variable_count");
    if missing_solver_variables != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{missing_solver_variables} declared section solver variable row(s) did not \
                 decode; stored and equation-derived coordinates are withheld for the incomplete \
                 table."
            ),
            provenance: None,
        });
    }
    let unresolved_dimension_values =
        coverage_count(coverage, "unresolved_feature_dimension_value_count");
    if unresolved_dimension_values != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_dimension_values} section dimension(s) retain source-native value \
                 tokens because their exact scalar encodings remain unresolved."
            ),
            provenance: None,
        });
    }
    let unresolved_configuration_driver_tables = coverage_count(
        coverage,
        "decoded_configuration_driver_table_reference_count",
    )
    .saturating_sub(coverage_count(
        coverage,
        "transferred_configuration_driver_table_count",
    ));
    if unresolved_configuration_driver_tables != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_configuration_driver_tables} referenced configuration driver \
                 table(s) retain unresolved traversal and row semantics."
            ),
            provenance: None,
        });
    }
    let prohibited_records =
        coverage_count(coverage, "prohibited_active_curve_expression_record_count");
    if prohibited_records != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{prohibited_records} active curve-equation record(s) containing prohibited \
                 datum-curve constructs were not evaluated; source and dependencies were \
                 retained without values or derived curves."
            ),
            provenance: None,
        });
    }
    let unresolved_solve_blocks = coverage_count(
        coverage,
        "decoded_active_curve_expression_solve_block_count",
    )
    .saturating_sub(coverage_count(
        coverage,
        "evaluated_active_curve_expression_solve_block_count",
    ));
    if unresolved_solve_blocks != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_solve_blocks} active curve-equation simultaneous-solve block(s) \
                 retain their ordered equations and unknowns without solved values or derived \
                 curves."
            ),
            provenance: None,
        });
    }
    let unresolved_solve_controls = coverage_count(
        coverage,
        "unresolved_active_curve_expression_solve_control_count",
    );
    if unresolved_solve_controls != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{unresolved_solve_controls} active curve-equation record(s) retain malformed or \
                 incomplete simultaneous-solve control without sequentially interpreting its \
                 bounded source lines."
            ),
            provenance: None,
        });
    }
    let prohibited_kinds =
        coverage_count(coverage, "prohibited_active_curve_expression_kind_count");
    if prohibited_kinds != 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossKind::shared(LossTaxonomy::FeatureHistoryRetained),
            severity: Severity::Warning,
            message: format!(
                "{prohibited_kinds} prohibited datum-curve construct(s) across active \
                 curve-equation records were not evaluated."
            ),
            provenance: None,
        });
    }
}
