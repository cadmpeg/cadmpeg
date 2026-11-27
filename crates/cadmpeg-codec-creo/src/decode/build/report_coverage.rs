// SPDX-License-Identifier: Apache-2.0
//! Coverage-drop and incomplete-feature loss notes.

use std::collections::{BTreeMap, BTreeSet};

use crate::container::ContainerScan;
use crate::feature::definitions::{FeatureDefinition, SolverSubtable};
use crate::loss::CreoLossCode;

use super::super::coverage::{constraint_kind_breakdown, surface_family, SURFACE_KINDS};
use super::super::feature_history::dimensions::feature_relation_table_expected_rows;
use super::super::sketch_ids::{feature_definition_has_sketch_design, model_sketch_id};
use super::super::sketch_transfer::transfer::expected_segment_rows;
use super::report_losses::{coverage_count, push_report_loss};
use cadmpeg_core::decode::{ArenaClass, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{DecodeParts, GivenUpPart, Part};

struct CountBreakdown<'a>(&'a [(&'a str, usize)]);

impl std::fmt::Display for CountBreakdown<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for (family, count) in self.0 {
            if *count == 0 {
                continue;
            }
            if !first {
                f.write_str(", ")?;
            }
            write!(f, "{family}={count}")?;
            first = false;
        }
        Ok(())
    }
}

/// One feature table whose declared row count exceeds its decoded rows.
struct UnreadRows {
    code: CreoLossCode,
    /// Offset of the table label.
    offset: usize,
    /// Offset where the row reader stopped, when the reader states it.
    rows_end: Option<usize>,
    expected: usize,
    decoded: usize,
}

impl UnreadRows {
    fn cause(&self) -> String {
        format!(
            "{}: {} of the {} row(s) the table declares did not decode",
            self.code.kind(),
            self.expected - self.decoded,
            self.expected
        )
    }
}

/// The unread rows of the tables whose first unread row starts at one offset.
struct UnreadRun<'a> {
    bytes: &'a [u8],
    tables: Vec<UnreadRows>,
}

/// Gives up the rows of each feature table that the reader stops short of
/// its declared count, as one `feature-table-rows` part per first unread
/// row. The rows from the first one the grammar refuses have no framed end,
/// so the part runs to the next table or row the definition decodes, or to
/// the end of the definition. The scope of each table kind is the scope of
/// its count note.
pub(in crate::decode) fn give_up_unread_section_rows(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<GivenUpPart>, CodecError> {
    let mut runs = BTreeMap::<usize, UnreadRun<'_>>::new();
    for definition in &scan.features.definitions {
        let tables = unread_tables(scan, definition)?;
        if tables.is_empty() {
            continue;
        }
        let kept = decoded_offsets(definition);
        let body_end = definition
            .offset
            .checked_add(definition.body.len())
            .ok_or_else(|| {
                CodecError::malformed("feature definition ends past the address range")
            })?;
        for rows in tables {
            let start = rows.rows_end.unwrap_or(rows.offset);
            let after = start.checked_add(1).ok_or_else(|| {
                CodecError::malformed("feature table row ends past the address range")
            })?;
            let end = kept
                .range(after..)
                .next()
                .copied()
                .unwrap_or(body_end)
                .max(start);
            let bytes = start
                .checked_sub(definition.offset)
                .zip(end.checked_sub(definition.offset))
                .and_then(|(from, to)| definition.body.get(from..to))
                .unwrap_or_default();
            let run = runs.entry(start).or_insert(UnreadRun {
                bytes,
                tables: Vec::new(),
            });
            if !run.tables.iter().any(|table| table.code == rows.code) {
                run.tables.push(rows);
            }
        }
    }
    runs.into_iter()
        .map(|(start, run)| {
            let cause = run
                .tables
                .iter()
                .map(UnreadRows::cause)
                .collect::<Vec<_>>()
                .join("; ");
            ctx.give_up_part(
                Part::at(
                    "creo",
                    "feature-table-rows",
                    cadmpeg_core::decode::u64_from_index(start),
                    run.bytes,
                    [ArenaClass::Design],
                ),
                CodecError::malformed(cause),
            )
        })
        .collect()
}

/// The tables of `definition` whose decoded rows fall short of their
/// declared count.
fn unread_tables(
    scan: &ContainerScan,
    definition: &FeatureDefinition,
) -> Result<Vec<UnreadRows>, CodecError> {
    let mut tables = Vec::new();
    if let Some(table) = &definition.segments {
        if feature_definition_has_sketch_design(definition)
            && model_sketch_id(scan, definition).is_some()
        {
            tables.push(UnreadRows {
                code: CreoLossCode::SectionSegmentMissing,
                offset: table.offset,
                rows_end: table.rows.rows_end(),
                expected: expected_segment_rows(definition.identity.id(), table)?,
                decoded: table.rows.len(),
            });
        }
    }
    if let Some(relations) = &definition.relations {
        if let Some(expected) = feature_relation_table_expected_rows(relations) {
            tables.push(UnreadRows {
                code: CreoLossCode::SectionRelationMissing,
                offset: relations.offset,
                rows_end: relations.rows_end,
                expected,
                decoded: relations.rows.len(),
            });
        }
        if let Some(rows) = relations
            .skamps
            .as_ref()
            .and_then(|table| solver_rows(CreoLossCode::SectionIncidenceMissing, table))
        {
            tables.push(rows);
        }
        if let Some(rows) = relations
            .triples
            .as_ref()
            .and_then(|table| solver_rows(CreoLossCode::SectionRelationJoinMissing, table))
        {
            tables.push(rows);
        }
    }
    if let Some(variables) = &definition.variables {
        tables.push(UnreadRows {
            code: CreoLossCode::SectionSolverVariableMissing,
            offset: variables.offset,
            rows_end: variables.rows_end,
            expected: cadmpeg_core::decode::index_from_u32(variables.declared_count),
            decoded: variables.rows.len(),
        });
    }
    tables.retain(|rows| rows.decoded < rows.expected);
    Ok(tables)
}

/// The declared and decoded counts of a declared solver table.
fn solver_rows<T>(code: CreoLossCode, table: &SolverSubtable<T>) -> Option<UnreadRows> {
    let SolverSubtable::Declared {
        header, rows_end, ..
    } = table
    else {
        return None;
    };
    let expected = cadmpeg_core::decode::index_from_u32(header.declared_count);
    Some(UnreadRows {
        code,
        offset: header.offset,
        rows_end: *rows_end,
        expected,
        decoded: expected - table.missing_rows(),
    })
}

/// The offsets of the table labels and rows `definition` decodes.
fn decoded_offsets(definition: &FeatureDefinition) -> BTreeSet<usize> {
    let mut kept = BTreeSet::new();
    kept.extend(definition.parameter_frames.iter().map(|frame| frame.offset));
    kept.extend(definition.outlines.iter().map(|outline| outline.offset));
    if let Some(table) = &definition.variables {
        kept.insert(table.offset);
        kept.extend(table.rows.iter().map(|row| row.offset));
    }
    if let Some(table) = &definition.segments {
        kept.insert(table.offset);
        kept.extend(table.rows.offsets());
    }
    if let Some(table) = &definition.trim_entities {
        kept.insert(table.offset);
        kept.extend(table.rows.iter().map(|row| row.offset));
    }
    if let Some(table) = &definition.trim_vertices {
        kept.insert(table.offset);
        kept.extend(table.rows.iter().map(|row| row.offset));
    }
    if let Some(table) = &definition.order_table {
        kept.insert(table.offset);
        kept.extend(table.rows.iter().map(|row| row.offset));
    }
    if let Some(table) = &definition.section_3d {
        kept.insert(table.offset);
    }
    if let Some(table) = &definition.dimensions {
        kept.insert(table.offset);
        kept.extend(table.rows.iter().map(|row| row.offset));
    }
    if let Some(table) = &definition.relations {
        kept.insert(table.offset);
        kept.extend(table.rows.iter().map(|row| row.offset));
        let skamps = table.skamps.as_ref().and_then(SolverSubtable::header);
        let triples = table.triples.as_ref().and_then(SolverSubtable::header);
        kept.extend(
            skamps
                .into_iter()
                .chain(triples)
                .map(|header| header.offset),
        );
    }
    if let Some(table) = &definition.saved_section {
        kept.insert(table.offset);
    }
    kept
}

pub(super) fn push_coverage_drop_losses(
    losses: &mut Vec<LossNote>,
    coverage: &cadmpeg_ir::report::decode::Coverage,
    solve_work_caps: &[&str],
) -> Result<(), CodecError> {
    let untransferred_surface_rows =
        coverage_count(coverage, "untransferred_visible_surface_row_count");
    if untransferred_surface_rows != 0 {
        let family_counts = SURFACE_KINDS.map(|kind| {
            let family = surface_family(kind);
            let count = coverage
                .iter()
                .find_map(|(key, count)| {
                    (key.strip_prefix("untransferred_visible_")
                        .and_then(|name| name.strip_suffix("_surface_row_count"))
                        == Some(family))
                    .then_some(*count)
                })
                .unwrap_or(0);
            (family, count)
        });
        let unresolved_families = CountBreakdown(&family_counts);
        push_report_loss(
            losses,
            CreoLossCode::VisibGeomSurfaceUntransferred,
            format_args!(
                "{untransferred_surface_rows} unique VisibGeom surface row(s) were not \
             transferred as carriers and remain structural namespace records \
             ({unresolved_families})."
            ),
        );
    }
    let untransferred_curve_rows =
        coverage_count(coverage, "untransferred_visible_curve_row_count");
    if untransferred_curve_rows != 0 {
        push_report_loss(
            losses,
            CreoLossCode::VisibGeomCurveUntransferred,
            format_args!(
                "{untransferred_curve_rows} unique VisibGeom curve-topology row(s) were not \
             transferred as carriers and remain structural namespace records."
            ),
        );
    }
    let cross_section_rows = coverage_count(coverage, "decoded_cross_section_curve_row_count");
    if cross_section_rows != 0 {
        push_report_loss(
            losses,
            CreoLossCode::CrossSectionCurveBodyOpaque,
            format_args!(
                "{cross_section_rows} cross-section curve row(s) retain complete parameter-body bytes \
                 without independently established internal numeric roles."
            ),
        );
    }
    let ambiguous_surface_rows = coverage_count(coverage, "ambiguous_visible_surface_row_count");
    if ambiguous_surface_rows != 0 {
        push_report_loss(
            losses,
            CreoLossCode::VisibGeomSurfaceAmbiguous,
            format_args!(
                "{ambiguous_surface_rows} VisibGeom surface row(s) share a non-unique identity \
             and were not resolved to a single carrier."
            ),
        );
    }
    let ambiguous_curve_rows = coverage_count(coverage, "ambiguous_visible_curve_row_count");
    if ambiguous_curve_rows != 0 {
        push_report_loss(
            losses,
            CreoLossCode::VisibGeomCurveAmbiguous,
            format_args!(
                "{ambiguous_curve_rows} VisibGeom curve-topology row(s) share a non-unique \
             identity and were not resolved to a single carrier."
            ),
        );
    }
    let missing_segment_rows = coverage_count(coverage, "missing_feature_segment_row_count");
    if missing_segment_rows != 0 {
        push_report_loss(
            losses,
            CreoLossCode::SectionSegmentMissing,
            format_args!(
                "{missing_segment_rows} declared section segment row(s) did not decode and remain \
             unavailable to the defining sketch."
            ),
        );
    }
    let missing_relation_rows = coverage_count(coverage, "missing_feature_relation_row_count");
    if missing_relation_rows != 0 {
        push_report_loss(
            losses,
            CreoLossCode::SectionRelationMissing,
            format_args!(
                "{missing_relation_rows} declared section relation row(s) did not decode; the \
             affected complete-table solver identities remain unavailable."
            ),
        );
    }
    let malformed_relation_tables =
        coverage_count(coverage, "malformed_feature_relation_table_count");
    if malformed_relation_tables != 0 {
        push_report_loss(
            losses,
            CreoLossCode::SectionRelationTableMalformed,
            format_args!(
                "{malformed_relation_tables} section relation table(s) use the invalid zero \
             allocation count."
            ),
        );
    }
    let missing_skamp_rows = coverage_count(coverage, "missing_feature_skamp_row_count");
    if missing_skamp_rows != 0 {
        push_report_loss(
            losses,
            CreoLossCode::SectionIncidenceMissing,
            format_args!(
                "{missing_skamp_rows} declared section incidence row(s) did not decode; the \
             affected complete-table solver identities remain unavailable."
            ),
        );
    }
    let missing_triple_rows = coverage_count(coverage, "missing_feature_relation_triple_row_count");
    if missing_triple_rows != 0 {
        push_report_loss(
            losses,
            CreoLossCode::SectionRelationJoinMissing,
            format_args!(
                "{missing_triple_rows} declared section relation-incidence join row(s) did not \
             decode; the affected complete-table solver identities remain unavailable."
            ),
        );
    }
    let unresolved_segment_geometry =
        coverage_count(coverage, "unresolved_feature_segment_geometry_count");
    if unresolved_segment_geometry != 0 {
        push_report_loss(
            losses,
            CreoLossCode::SectionSegmentGeometryUnresolved,
            format_args!(
                "{unresolved_segment_geometry} decoded section segment(s) retain source-native \
             geometry because their exact neutral construction remains unresolved."
            ),
        );
    }
    let active_native_skamps =
        coverage_count(coverage, "active_native_feature_skamp_constraint_count");
    if active_native_skamps != 0 {
        let kinds = constraint_kind_breakdown(coverage, "active_native_feature_skamp_type_");
        push_report_loss(
            losses,
            CreoLossCode::SectionIncidenceNative,
            format_args!(
                "{active_native_skamps} active section incidence constraint(s) retain native \
             operands because their neutral semantics or referenced geometry remain unresolved \
             ({kinds})."
            ),
        );
    }
    let active_native_relations =
        coverage_count(coverage, "active_native_feature_relation_constraint_count");
    if active_native_relations != 0 {
        let kinds = constraint_kind_breakdown(coverage, "active_native_feature_relation_type_");
        push_report_loss(
            losses,
            CreoLossCode::SectionRelationNative,
            format_args!(
                "{active_native_relations} active section dimension relation(s) retain native \
             operands because their neutral semantics, incidence join, or referenced geometry \
             remain unresolved ({kinds})."
            ),
        );
    }
    let incomplete_sweeps = coverage_count(coverage, "transferred_incomplete_sweep_feature_count");
    if incomplete_sweeps != 0 {
        let family_counts = [
            (
                "extrude",
                coverage_count(coverage, "transferred_incomplete_extrude_feature_count"),
            ),
            (
                "revolve",
                coverage_count(coverage, "transferred_incomplete_revolve_feature_count"),
            ),
        ];
        let families = CountBreakdown(&family_counts);
        push_report_loss(
            losses,
            CreoLossCode::FeatureSweepIncomplete,
            format_args!(
                "{incomplete_sweeps} profile sweep history feature(s) retain incomplete required \
             construction operands ({families})."
            ),
        );
    }
    let incomplete_surface_operations = coverage_count(
        coverage,
        "transferred_incomplete_surface_operation_feature_count",
    );
    if incomplete_surface_operations != 0 {
        let family_counts = [
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
        ];
        let families = CountBreakdown(&family_counts);
        push_report_loss(
            losses,
            CreoLossCode::FeatureSurfaceOperationIncomplete,
            format_args!(
                "{incomplete_surface_operations} surface construction history feature(s) retain \
             incomplete required operands ({families})."
            ),
        );
    }
    let incomplete_other_constructions = coverage_count(
        coverage,
        "transferred_incomplete_other_construction_feature_count",
    );
    if incomplete_other_constructions != 0 {
        let family_counts = [
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
        ];
        let families = CountBreakdown(&family_counts);
        push_report_loss(
            losses,
            CreoLossCode::FeatureConstructionIncomplete,
            format_args!(
                "{incomplete_other_constructions} construction history feature(s) retain \
             unresolved neutral operands ({families})."
            ),
        );
    }
    let incomplete_recognized_features =
        coverage_count(coverage, "transferred_incomplete_recognized_feature_count");
    if incomplete_recognized_features != 0 {
        let family_counts = [
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
        ];
        let families = CountBreakdown(&family_counts);
        push_report_loss(
            losses,
            CreoLossCode::FeatureRecognizedIncomplete,
            format_args!(
                "{incomplete_recognized_features} recognized non-sweep history feature(s) retain \
             incomplete required construction operands ({families})."
            ),
        );
    }
    let explicitly_unresolved_features =
        coverage_count(coverage, "transferred_explicitly_unresolved_feature_count");
    let native_features = coverage_count(coverage, "transferred_native_feature_count");
    if native_features != 0 {
        push_report_loss(
            losses,
            CreoLossCode::FeatureNativeSemantics,
            format_args!(
                "{native_features} history feature definition(s) retain only source-native \
             semantics."
            ),
        );
    }
    if explicitly_unresolved_features != 0 {
        push_report_loss(
            losses,
            CreoLossCode::FeatureConstructionUnresolved,
            format_args!(
                "{explicitly_unresolved_features} typed history feature definition(s) retain an \
             explicitly unresolved model-space construction."
            ),
        );
    }
    let opaque_lanes = coverage_count(coverage, "opaque_section_variable_lane_count");
    if opaque_lanes != 0 {
        push_report_loss(losses, CreoLossCode::SectionVariableScalarOpaque,
            format_args!("{opaque_lanes} full-width section variable lane(s) retain exact bytes without independently established numeric roles."));
    }
    let missing_solver_variables =
        coverage_count(coverage, "missing_feature_solver_variable_count");
    if missing_solver_variables != 0 {
        push_report_loss(
            losses,
            CreoLossCode::SectionSolverVariableMissing,
            format_args!(
                "{missing_solver_variables} declared section solver variable row(s) did not \
             decode; stored and equation-derived coordinates are withheld for the incomplete \
             table."
            ),
        );
    }
    let unresolved_dimension_values =
        coverage_count(coverage, "unresolved_feature_dimension_value_count");
    if unresolved_dimension_values != 0 {
        push_report_loss(
            losses,
            CreoLossCode::SectionDimensionValueUnresolved,
            format_args!(
                "{unresolved_dimension_values} section dimension(s) retain source-native value \
             tokens because their exact scalar encodings remain unresolved."
            ),
        );
    }
    let unresolved_configuration_driver_tables = coverage_count(
        coverage,
        "decoded_configuration_driver_table_reference_count",
    )
    .checked_sub(coverage_count(
        coverage,
        "transferred_configuration_driver_table_count",
    ))
    .ok_or_else(|| CodecError::malformed("transferred driver count exceeds decoded count"))?;
    if unresolved_configuration_driver_tables != 0 {
        push_report_loss(
            losses,
            CreoLossCode::ConfigurationDriverUnresolved,
            format_args!(
                "{unresolved_configuration_driver_tables} referenced configuration driver \
             table(s) retain unresolved traversal and row semantics."
            ),
        );
    }
    let prohibited_records =
        coverage_count(coverage, "prohibited_active_curve_expression_record_count");
    if prohibited_records != 0 {
        push_report_loss(
            losses,
            CreoLossCode::CurveExpressionProhibited,
            format_args!(
                "{prohibited_records} active curve-equation record(s) containing prohibited \
             datum-curve constructs were not evaluated; source and dependencies were \
             retained without values or derived curves."
            ),
        );
    }
    let unresolved_solve_blocks = coverage_count(
        coverage,
        "decoded_active_curve_expression_solve_block_count",
    )
    .checked_sub(coverage_count(
        coverage,
        "evaluated_active_curve_expression_solve_block_count",
    ))
    .ok_or_else(|| CodecError::malformed("evaluated solve count exceeds decoded count"))?;
    if unresolved_solve_blocks != 0 {
        let bound_clause = if solve_work_caps.is_empty() {
            String::new()
        } else {
            format!(" ({})", solve_work_caps.join(", "))
        };
        push_report_loss(
            losses,
            CreoLossCode::CurveExpressionSolveUnresolved,
            format_args!(
                "{unresolved_solve_blocks} active curve-equation simultaneous-solve block(s) \
             retain their ordered equations and unknowns without solved values or derived \
             curves{bound_clause}."
            ),
        );
    }
    let unresolved_solve_controls = coverage_count(
        coverage,
        "unresolved_active_curve_expression_solve_control_count",
    );
    if unresolved_solve_controls != 0 {
        push_report_loss(
            losses,
            CreoLossCode::CurveExpressionSolveControlUnresolved,
            format_args!(
                "{unresolved_solve_controls} active curve-equation record(s) retain malformed or \
             incomplete simultaneous-solve control without sequentially interpreting its \
             bounded source lines."
            ),
        );
    }
    let prohibited_kinds =
        coverage_count(coverage, "prohibited_active_curve_expression_kind_count");
    if prohibited_kinds != 0 {
        push_report_loss(
            losses,
            CreoLossCode::CurveExpressionKindProhibited,
            format_args!(
                "{prohibited_kinds} prohibited datum-curve construct(s) across active \
             curve-equation records were not evaluated."
            ),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{give_up_unread_section_rows, push_coverage_drop_losses};
    use crate::feature::definitions::{
        DefinitionIdentity, FeatureDefinition, FeatureRelationTable, FeatureSolverTableHeader,
        FeatureVariableTable, SolverSubtable,
    };
    use cadmpeg_core::CodecError;

    fn solver_table<T>(declared_count: u32, offset: usize) -> SolverSubtable<T> {
        SolverSubtable::Declared {
            rows_end: Some(offset + 10),
            header: FeatureSolverTableHeader {
                declared_count,
                entity_ref: 0,
                offset,
            },
            rows: Vec::new(),
        }
    }

    fn definition_with_short_tables() -> FeatureDefinition {
        FeatureDefinition {
            identity: DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(42),
                owner_feature_id: Some(42),
            },
            body: (0..400_u16)
                .map(|at| u8::try_from(at % 251).expect("byte"))
                .collect(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: Some(FeatureVariableTable {
                rows_end: Some(320),
                declared_count: 3,
                entity_ref: None,
                rows: Vec::new(),
                offset: 300,
            }),
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: Some(FeatureRelationTable {
                rows_end: Some(110),
                declared_count: 5,
                entity_ref: None,
                rows: Vec::new(),
                skamps: Some(solver_table(2, 200)),
                triples: Some(solver_table(0, 250)),
                offset: 100,
            }),
            saved_section: None,
            offset: 0,
        }
    }

    #[test]
    fn each_short_feature_table_gives_up_its_unread_rows_as_one_part() {
        let mut scan = crate::test_support::empty_container_scan();
        // A definition read twice at one offset holds one set of tables.
        scan.features.definitions = vec![
            definition_with_short_tables(),
            definition_with_short_tables(),
        ];
        let body = definition_with_short_tables().body;
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("test root is admitted");
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let mut fidelity = cadmpeg_ir::SourceFidelity::for_root(root);
        let mut losses = Vec::new();
        for part in give_up_unread_section_rows(&ctx, &scan).expect("parts") {
            losses.push(part.commit(&ctx, &mut ir, &mut fidelity).expect("commit"));
        }
        assert!(losses
            .iter()
            .all(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind()));
        // Each part runs from the first unread row to the next table label or
        // to the end of the definition.
        let expected = [
            (
                110,
                200,
                "creo/section.relation-missing: 3 of the 3 row(s) the table declares did not decode",
            ),
            (
                210,
                250,
                "creo/section.incidence-missing: 2 of the 2 row(s) the table declares did not \
                 decode",
            ),
            (
                320,
                400,
                "creo/section.solver-variable-missing: 3 of the 3 row(s) the table declares did \
                 not decode",
            ),
        ];
        assert_eq!(losses.len(), expected.len());
        for (loss, (start, end, cause)) in losses.iter().zip(expected) {
            let message = loss.message.as_str();
            let head = format!(
                "part feature-table-rows stream=\"\" offset={start} length={} record=",
                end - start
            );
            assert!(message.starts_with(&head), "{message}");
            assert!(
                message.contains(&format!(": {cause}; dropped=[]")),
                "{message}"
            );
            assert!(!message.contains("featdefs:"), "{message}");
            let stub = message[head.len()..]
                .split(':')
                .take(3)
                .collect::<Vec<_>>()
                .join(":");
            assert!(stub.starts_with("creo:source:part#"), "{message}");
            let retained = fidelity.retained_record(&stub).expect("retained part");
            assert_eq!(retained.data(), Some(&body[start..end]));
        }
        assert_eq!(fidelity.retained_records().len(), expected.len());
    }

    #[test]
    fn driver_coverage_refuses_transferred_count_above_decoded() {
        ({
            let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
            coverage.record(
                crate::coverage::TRANSFERRED_CONFIGURATION_DRIVER_TABLE_COUNT,
                1,
            );
            assert!(matches!(
                push_coverage_drop_losses(&mut Vec::new(), &coverage, &[]),
                Err(CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn solve_coverage_refuses_evaluated_count_above_decoded() {
        ({
            let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
            coverage.record(
                crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_BLOCK_COUNT,
                1,
            );
            assert!(matches!(
                push_coverage_drop_losses(&mut Vec::new(), &coverage, &[]),
                Err(CodecError::Malformed(_))
            ));
        });
    }

    fn two_untransferred_planes() -> cadmpeg_ir::report::decode::Coverage {
        {
            let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
            coverage.record(crate::coverage::UNTRANSFERRED_VISIBLE_SURFACE_ROW_COUNT, 2);
            coverage.record(
                crate::coverage::UNTRANSFERRED_VISIBLE_PLANE_SURFACE_ROW_COUNT,
                2,
            );
            coverage
        }
    }

    #[test]
    fn coverage_family_loss_preserves_breakdown_under_service_profile() {
        let losses = {
            let mut losses = Vec::new();
            push_coverage_drop_losses(&mut losses, &two_untransferred_planes(), &[])
                .expect("record the coverage drop losses");
            losses
        };
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.ends_with("(plane=2)."));
    }
    #[test]
    fn conic_native_incidence_is_retained_and_counted_by_existing_loss() {
        let constraints =
            crate::decode::sketch_transfer::conic_owner_tests::authored_conic_incidence();
        let transfer = crate::decode::coverage::design_constraint_transfer_coverage(
            &constraints,
            ":skamp:",
            "creo:skamp:",
        );
        assert_eq!(transfer.transferred, 1);
        assert_eq!(transfer.active, 1);
        assert_eq!(transfer.native, 1);
        assert_eq!(transfer.active_native, 1);
        assert_eq!(transfer.typed().expect("consistent counts"), 0);
        assert_eq!(transfer.active_typed().expect("consistent counts"), 0);
        assert_eq!(
            transfer.native_by_kind,
            std::collections::BTreeMap::from([(0, 1)])
        );
        assert_eq!(
            transfer.active_native_by_kind,
            std::collections::BTreeMap::from([(0, 1)])
        );
        let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
        coverage.record(
            crate::coverage::ACTIVE_NATIVE_FEATURE_SKAMP_CONSTRAINT_COUNT,
            transfer.active_native,
        );
        for (kind, count) in transfer.active_native_by_kind {
            coverage.record_indexed(
                crate::coverage::ACTIVE_NATIVE_FEATURE_SKAMP_TYPE_CONSTRAINT_COUNT,
                kind,
                count,
            );
        }
        let mut losses = Vec::new();
        push_coverage_drop_losses(&mut losses, &coverage, &[]).expect("report retained incidence");
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::CreoLossCode::SectionIncidenceNative.kind()
        );
        assert_eq!(losses[0].message, "1 active section incidence constraint(s) retain native operands because their neutral semantics or referenced geometry remain unresolved (type 0=1).");
    }
}
