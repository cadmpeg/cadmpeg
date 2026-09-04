// SPDX-License-Identifier: Apache-2.0
//! Feature-tree emission, dimension parameters, and expression coverage.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition as IrFeatureDefinition, FeatureId as IrFeatureId,
};
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;

use crate::container::ContainerScan;

use super::super::curve_expressions::transfer_curve_expression_features;
use super::super::feature_history::{
    datum_plane_feature_definition, emit_feature_result_topologies, feature_dependencies,
    feature_output_bodies, feature_parameters, feature_reference_name, feature_source_properties,
    geometry_generator_features, link_feature_sketch_history, named_feature_definition,
    named_or_referenced_feature_definition, reconcile_feature_links,
    retain_native_feature_parameters, schema_feature_definition, schema_operation_kind,
    surface_prototype_feature_dependencies, transfer_feature_dimensions,
    unbounded_feature_plane_definition,
};
use super::super::native::annotate;
use super::super::sketch_ids::owning_feature_definition_ref;
use super::super::sketch_transfer::{
    close_sketch_constraint_parameter_references, current_feature_operation,
    current_feature_recipe, current_feature_recipe_parent, feature_schema_class,
    row_feature_schema_classes,
};
use super::super::uniqueness::unique_feature_datum_plane;

fn refresh_feature_outputs(scan: &ContainerScan, ir: &mut CadIr) {
    let output_updates = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let feature_id = feature
                .id
                .as_str()
                .strip_prefix("creo:model:feature#")
                .and_then(|value| value.parse::<u32>().ok())?;
            Some((
                feature.id.clone(),
                feature_output_bodies(scan, ir, feature_id),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    for feature in &mut ir.model.features {
        if let Some(outputs) = output_updates.get(&feature.id) {
            feature.outputs.clone_from(outputs);
        }
    }
}

fn ordered_row_feature_ids(rows: &[crate::feature::FeatureRow]) -> Vec<u32> {
    let mut seen = BTreeSet::new();
    rows.iter()
        .filter_map(|row| seen.insert(row.feature_id).then_some(row.feature_id))
        .collect()
}

pub(super) fn emit_model_features(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut tree_edges = Vec::new();
    let mut regeneration_edges = Vec::new();
    let prototype_feature_dependencies = surface_prototype_feature_dependencies(scan);
    let operation_feature_ids = scan
        .features
        .operations
        .iter()
        .map(|operation| operation.feature_id)
        .collect::<BTreeSet<_>>();
    for datum in &scan.planes.datums {
        if operation_feature_ids.contains(&datum.feature_id) {
            continue;
        }
        let id = IrFeatureId(format!("creo:model:feature#{}", datum.feature_id));
        if ir.model.features.iter().any(|feature| feature.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "ActDatums",
            datum.offset_in_payload as u64,
            "datum_plane_feature",
            Exactness::Derived,
        );
        ir.model.features.push(Feature {
            id,
            ordinal: ir.model.features.len() as u64,
            name: None,
            suppressed: Some(false),
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: if unique_feature_datum_plane(&scan.planes.datums, datum.feature_id)
                .is_some()
            {
                datum_plane_feature_definition(datum)
            } else {
                IrFeatureDefinition::DatumPlaneUnresolved
            },
            native_ref: None,
        });
    }
    let row_feature_ids = ordered_row_feature_ids(&scan.features.rows);
    let mut geometry_generator_feature_count = 0;
    for generator in geometry_generator_features(scan) {
        let feature_id = generator.feature_id;
        let id = IrFeatureId(format!("creo:model:feature#{feature_id}"));
        if ir.model.features.iter().any(|feature| feature.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            generator.offset as u64,
            "geometry_generator_feature",
            Exactness::ByteExact,
        );
        ir.model.features.push(Feature {
            id,
            ordinal: ir.model.features.len() as u64,
            name: None,
            suppressed: Some(false),
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: feature_output_bodies(scan, ir, feature_id),
            definition: if scan
                .features
                .legacy_rounds
                .iter()
                .any(|round| round.feature_id == feature_id)
            {
                schema_feature_definition(scan, ir, feature_id, 913, "Fillet")
            } else {
                IrFeatureDefinition::StoredGeometry
            },
            native_ref: None,
        });
        refresh_feature_outputs(scan, ir);
        geometry_generator_feature_count += 1;
    }
    let operation_ordinal_base = ir.model.features.len();
    for (operation_index, operation) in scan.features.operations.iter().enumerate() {
        let id = IrFeatureId(format!("creo:model:feature#{}", operation.feature_id));
        let current_operation =
            current_feature_operation(&scan.features.operations, operation.feature_id);
        let outputs = feature_output_bodies(scan, ir, operation.feature_id);
        let mut source_properties = feature_source_properties(scan, operation.feature_id);
        if let Some(prefix) = current_operation.and_then(|operation| operation.stored_name_prefix) {
            source_properties.insert(
                "mdl_stored_name_prefix".to_string(),
                char::from(prefix).to_string(),
            );
        }
        let parameters = feature_parameters(scan, operation.feature_id);
        let schema_class = feature_schema_class(scan, operation.feature_id);
        let definition = schema_class.map_or_else(
            || {
                current_feature_recipe(&scan.features.operations, operation.feature_id)
                    .map(|_| {
                        schema_feature_definition(
                            scan,
                            ir,
                            operation.feature_id,
                            0,
                            &operation.kind,
                        )
                    })
                    .or_else(|| {
                        current_operation.and_then(|operation| {
                            named_or_referenced_feature_definition(
                                scan,
                                ir,
                                operation.feature_id,
                                &operation.kind,
                            )
                        })
                    })
                    .or_else(|| unbounded_feature_plane_definition(scan, ir, operation.feature_id))
                    .unwrap_or_else(|| IrFeatureDefinition::Native {
                        kind: current_operation
                            .map_or("Native Feature", |operation| operation.kind.as_str())
                            .into(),
                        parameters: parameters.clone(),
                    })
            },
            |schema_class| {
                schema_feature_definition(
                    scan,
                    ir,
                    operation.feature_id,
                    schema_class,
                    &operation.kind,
                )
            },
        );
        retain_native_feature_parameters(&mut source_properties, &definition, &parameters);
        let dependencies = feature_dependencies(
            scan,
            ir,
            operation.feature_id,
            &prototype_feature_dependencies,
        );
        let parent = current_feature_recipe_parent(&scan.features.operations, operation.feature_id)
            .and_then(|parent_feature_id| {
                let parent = IrFeatureId(format!("creo:model:feature#{parent_feature_id}"));
                ir.model
                    .features
                    .iter()
                    .any(|feature| feature.id == parent)
                    .then_some(parent)
            });
        if let Some(parent) = parent {
            if ir.model.features.iter().any(|feature| {
                feature.id == parent
                    && matches!(feature.definition, IrFeatureDefinition::TreeNode { .. })
            }) {
                tree_edges.push((parent, id.clone()));
            } else {
                regeneration_edges.push((id.clone(), parent));
            }
        }
        let operation_section = scan
            .framing
            .sections
            .iter()
            .find(|section| {
                operation.offset >= section.offset
                    && operation.offset < section.offset.saturating_add(section.length)
            })
            .map_or("MdlStatus", |section| section.name.as_str());
        let name = current_operation.and_then(|operation| {
            operation.display_name_stored.then_some(())?;
            let stored_name = operation.stored_name.as_deref()?;
            Some(
                operation
                    .stored_name_prefix
                    .and_then(|prefix| stored_name.strip_prefix(char::from(prefix)))
                    .unwrap_or(stored_name)
                    .to_string(),
            )
        });
        let source_tag = current_feature_recipe(&scan.features.operations, operation.feature_id)
            .map(|recipe| recipe.name().to_string());
        let native_ref = owning_feature_definition_ref(scan, operation.feature_id);
        if let Some(existing) = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.id == id)
        {
            let upgrade_legacy_round = scan
                .features
                .legacy_rounds
                .iter()
                .any(|round| round.feature_id == operation.feature_id)
                && matches!(&definition, IrFeatureDefinition::Fillet { .. })
                && matches!(existing.definition, IrFeatureDefinition::StoredGeometry);
            if upgrade_legacy_round {
                existing.definition = definition;
            }
            if name.is_some() {
                existing.name = name;
            }
            for dependency in dependencies {
                if !existing.dependencies.contains(&dependency) {
                    existing.dependencies.push(dependency);
                }
            }
            existing.source_properties.extend(source_properties);
            if source_tag.is_some() {
                existing.source_tag = source_tag;
            }
            if existing.native_ref.is_none() {
                existing.native_ref = native_ref;
            }
            for output in outputs {
                if !existing.outputs.contains(&output) {
                    existing.outputs.push(output);
                }
            }
            refresh_feature_outputs(scan, ir);
            continue;
        }
        let (operation_annotation_kind, operation_exactness) = if operation.display_state_conflict {
            ("feature_operation_state_consensus", Exactness::Derived)
        } else if operation.display_name_stored {
            ("feature_operation_name", Exactness::ByteExact)
        } else {
            ("feature_recipe", Exactness::ByteExact)
        };
        annotate(
            annotations,
            &id,
            operation_section,
            operation.offset as u64,
            operation_annotation_kind,
            operation_exactness,
        );
        ir.model.features.push(Feature {
            id,
            ordinal: (operation_ordinal_base + operation_index) as u64,
            name,
            suppressed: Some(false),
            dependencies,
            source_properties,
            source_tag,
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition,
            native_ref,
        });
        refresh_feature_outputs(scan, ir);
    }
    for feature_id in row_feature_ids {
        let id = IrFeatureId(format!("creo:model:feature#{feature_id}"));
        if ir.model.features.iter().any(|feature| feature.id == id) {
            continue;
        }
        let schema_class = feature_schema_class(scan, feature_id);
        let Some(offset) = scan
            .features
            .rows
            .iter()
            .filter(|row| row.feature_id == feature_id)
            .map(|row| row.offset)
            .min()
        else {
            continue;
        };
        let reference_name = feature_reference_name(scan, feature_id);
        let kind = reference_name.unwrap_or_else(|| {
            schema_class
                .and_then(schema_operation_kind)
                .unwrap_or("Native Feature")
        });
        annotate(
            annotations,
            &id,
            "AllFeatur",
            offset as u64,
            "schema_feature_operation",
            Exactness::ByteExact,
        );
        let parameters = feature_parameters(scan, feature_id);
        let mut source_properties = feature_source_properties(scan, feature_id);
        let definition = schema_class.map_or_else(
            || {
                named_feature_definition(scan, ir, feature_id, kind)
                    .or_else(|| unbounded_feature_plane_definition(scan, ir, feature_id))
                    .unwrap_or_else(|| IrFeatureDefinition::Native {
                        kind: kind.into(),
                        parameters: parameters.clone(),
                    })
            },
            |schema_class| schema_feature_definition(scan, ir, feature_id, schema_class, kind),
        );
        let row_schema_classes = row_feature_schema_classes(&scan.features.rows, feature_id);
        if schema_class.is_none() {
            source_properties.insert(
                "featdefs_schema_state".to_string(),
                if row_schema_classes.is_empty() {
                    "absent"
                } else {
                    "ambiguous"
                }
                .to_string(),
            );
        }
        if !row_schema_classes.is_empty() {
            source_properties.insert(
                "featdefs_row_schema_classes".to_string(),
                row_schema_classes
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        retain_native_feature_parameters(&mut source_properties, &definition, &parameters);
        ir.model.features.push(Feature {
            id,
            ordinal: ir.model.features.len() as u64,
            name: Some(
                reference_name.map_or_else(|| format!("{kind} id {feature_id}"), str::to_string),
            ),
            suppressed: Some(false),
            dependencies: feature_dependencies(
                scan,
                ir,
                feature_id,
                &prototype_feature_dependencies,
            ),
            source_properties,
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: feature_output_bodies(scan, ir, feature_id),
            definition,
            native_ref: owning_feature_definition_ref(scan, feature_id),
        });
        refresh_feature_outputs(scan, ir);
    }
    for (parent, child) in tree_edges {
        let Some(feature) = ir
            .model
            .features
            .iter_mut()
            .find(|feature| feature.id == parent)
        else {
            continue;
        };
        let IrFeatureDefinition::TreeNode { children, .. } = &mut feature.definition else {
            continue;
        };
        if !children.contains(&child) {
            children.push(child);
        }
    }
    for (child, parent) in regeneration_edges {
        if ir
            .model
            .set_feature_regeneration_parent(child, parent)
            .is_err()
        {
            continue;
        }
    }
    geometry_generator_feature_count
}

pub(super) fn finish_feature_transfers(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    coverage: &mut cadmpeg_ir::Coverage,
) -> (usize, usize) {
    let prototype_feature_dependencies = surface_prototype_feature_dependencies(scan);
    link_feature_sketch_history(scan, ir);
    reconcile_feature_links(scan, ir, &prototype_feature_dependencies);
    let feature_result_topology_count = emit_feature_result_topologies(scan, ir);
    let feature_result_edge_count = ir
        .model
        .feature_result_topologies
        .iter()
        .map(|state| state.edges.len())
        .sum::<usize>();
    let (transferred_feature_dimension_count, dimension_parameters) =
        transfer_feature_dimensions(scan, ir, annotations);
    let transferred_curve_expression_parameter_count =
        transfer_curve_expression_features(scan, ir, annotations, &dimension_parameters);
    {
        let active_expressions = scan
            .curves
            .expressions
            .iter()
            .filter(|record| !record.backup);
        let decoded_curve_expression_assignment_count = active_expressions
            .clone()
            .map(|record| record.assignments.len())
            .sum::<usize>();
        let decoded_curve_expression_table_cell_assignment_count = active_expressions
            .clone()
            .flat_map(|record| &record.assignments)
            .filter(|assignment| {
                matches!(
                    &assignment.target,
                    crate::curve::CurveExpressionTarget::TableCell { .. }
                )
            })
            .count();
        let decoded_curve_expression_scoped_symbol_assignment_count = active_expressions
            .clone()
            .flat_map(|record| &record.assignments)
            .filter(|assignment| {
                matches!(
                    &assignment.target,
                    crate::curve::CurveExpressionTarget::ScopedSymbol { .. }
                )
            })
            .count();
        let decoded_curve_expression_system_symbol_assignment_count = active_expressions
            .clone()
            .flat_map(|record| &record.assignments)
            .filter(|assignment| {
                matches!(
                    &assignment.target,
                    crate::curve::CurveExpressionTarget::SystemSymbol { .. }
                )
            })
            .count();
        let decoded_curve_expression_function_write_assignment_count = active_expressions
            .clone()
            .flat_map(|record| &record.assignments)
            .filter(|assignment| {
                matches!(
                    &assignment.target,
                    crate::curve::CurveExpressionTarget::FunctionWrite { .. }
                )
            })
            .count();
        let evaluated_curve_expression_assignment_count = active_expressions
            .clone()
            .flat_map(|record| &record.assignments)
            .filter(|assignment| assignment.value.is_some())
            .count();
        let decoded_curve_expression_solve_block_count = active_expressions
            .clone()
            .map(|record| record.solve_blocks.len())
            .sum::<usize>();
        let decoded_curve_expression_simultaneous_equation_count = active_expressions
            .clone()
            .flat_map(|record| &record.solve_blocks)
            .map(|block| block.equations.len())
            .sum::<usize>();
        let decoded_curve_expression_solve_assignment_count = active_expressions
            .clone()
            .flat_map(|record| &record.solve_blocks)
            .map(|block| block.assignments.len())
            .sum::<usize>();
        let decoded_curve_expression_solve_variable_count = active_expressions
            .clone()
            .flat_map(|record| &record.solve_blocks)
            .map(|block| block.variables.len())
            .sum::<usize>();
        let evaluated_curve_expression_solve_block_count = active_expressions
            .clone()
            .flat_map(|record| &record.solve_blocks)
            .filter(|block| {
                !block.solutions.is_empty() && block.solutions.iter().all(Option::is_some)
            })
            .count();
        let evaluated_curve_expression_solve_variable_count = active_expressions
            .clone()
            .flat_map(|record| &record.solve_blocks)
            .flat_map(|block| &block.solutions)
            .filter(|solution| solution.is_some())
            .count();
        let unresolved_curve_expression_solve_control_count = active_expressions
            .clone()
            .filter(|record| record.unresolved_solve_control)
            .count();
        let prohibited_curve_expression_record_count = active_expressions
            .clone()
            .filter(|record| !record.prohibited_constructs.is_empty())
            .count();
        let prohibited_curve_expression_kind_count = active_expressions
            .clone()
            .map(|record| record.prohibited_constructs.len())
            .sum::<usize>();
        let activation_count = |activation| {
            active_expressions
                .clone()
                .flat_map(|record| &record.assignments)
                .filter(|assignment| assignment.activation == activation)
                .count()
        };
        coverage.record(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
            decoded_curve_expression_assignment_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_CURVE_EXPRESSION_PARAMETER_COUNT,
            transferred_curve_expression_parameter_count,
        );
        coverage.record(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_TABLE_CELL_ASSIGNMENT_COUNT,
            decoded_curve_expression_table_cell_assignment_count,
        );
        coverage.record(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SCOPED_SYMBOL_ASSIGNMENT_COUNT,
            decoded_curve_expression_scoped_symbol_assignment_count,
        );
        coverage.record(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SYSTEM_SYMBOL_ASSIGNMENT_COUNT,
            decoded_curve_expression_system_symbol_assignment_count,
        );
        coverage.record(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_FUNCTION_WRITE_ASSIGNMENT_COUNT,
            decoded_curve_expression_function_write_assignment_count,
        );
        coverage.record(
            crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
            evaluated_curve_expression_assignment_count,
        );
        coverage.record(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_BLOCK_COUNT,
            decoded_curve_expression_solve_block_count,
        );
        coverage.record(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SIMULTANEOUS_EQUATION_COUNT,
            decoded_curve_expression_simultaneous_equation_count,
        );
        coverage.record(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_ASSIGNMENT_COUNT,
            decoded_curve_expression_solve_assignment_count,
        );
        coverage.record(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_VARIABLE_COUNT,
            decoded_curve_expression_solve_variable_count,
        );
        coverage.record(
            crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_BLOCK_COUNT,
            evaluated_curve_expression_solve_block_count,
        );
        coverage.record(
            crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_VARIABLE_COUNT,
            evaluated_curve_expression_solve_variable_count,
        );
        coverage.record(
            crate::coverage::UNRESOLVED_ACTIVE_CURVE_EXPRESSION_SOLVE_CONTROL_COUNT,
            unresolved_curve_expression_solve_control_count,
        );
        coverage.record(
            crate::coverage::PROHIBITED_ACTIVE_CURVE_EXPRESSION_RECORD_COUNT,
            prohibited_curve_expression_record_count,
        );
        coverage.record(
            crate::coverage::PROHIBITED_ACTIVE_CURVE_EXPRESSION_KIND_COUNT,
            prohibited_curve_expression_kind_count,
        );
        for (key, activation) in [
            (
                crate::coverage::ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
                crate::curve::CurveExpressionActivation::Active,
            ),
            (
                crate::coverage::INACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
                crate::curve::CurveExpressionActivation::Inactive,
            ),
            (
                crate::coverage::CONDITIONAL_CURVE_EXPRESSION_ASSIGNMENT_COUNT,
                crate::curve::CurveExpressionActivation::Conditional,
            ),
        ] {
            coverage.record(key, activation_count(activation));
        }
        let (decoded_dimension_count, resolved_dimension_count) = scan
            .features
            .definitions
            .iter()
            .filter_map(|definition| definition.dimensions.as_ref())
            .flat_map(|table| &table.rows)
            .fold((0usize, 0usize), |(decoded, resolved), dimension| {
                (
                    decoded + 1,
                    resolved + usize::from(dimension.value.is_some()),
                )
            });
        coverage.record(
            crate::coverage::DECODED_FEATURE_DIMENSION_COUNT,
            decoded_dimension_count,
        );
        coverage.record(
            crate::coverage::TRANSFERRED_FEATURE_DIMENSION_PARAMETER_COUNT,
            transferred_feature_dimension_count,
        );
        coverage.record(
            crate::coverage::RESOLVED_FEATURE_DIMENSION_VALUE_COUNT,
            resolved_dimension_count,
        );
        coverage.record(
            crate::coverage::UNRESOLVED_FEATURE_DIMENSION_VALUE_COUNT,
            decoded_dimension_count.saturating_sub(resolved_dimension_count),
        );
    }
    close_sketch_constraint_parameter_references(ir);
    (feature_result_topology_count, feature_result_edge_count)
}

#[cfg(test)]
mod tests;
