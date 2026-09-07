// SPDX-License-Identifier: Apache-2.0
//! Variable-table fixture construction from section coordinates.

use super::{
    FeatureSectionPoint, FeatureVariableRow, FeatureVariableTable, ScalarLane, VariableType,
};

pub(crate) fn with_points(
    mut table: FeatureVariableTable,
    points: Vec<FeatureSectionPoint>,
) -> FeatureVariableTable {
    append_points(&mut table, points);
    table
}

pub(crate) fn append_points(table: &mut FeatureVariableTable, points: Vec<FeatureSectionPoint>) {
    for point in points {
        for (variable_type, value) in [(VariableType::U, point.u), (VariableType::V, point.v)] {
            table.rows.push(FeatureVariableRow {
                variable_type,
                key: point.point_id,
                value: value.map_or(ScalarLane::Undefined, ScalarLane::Value),
                value_body: Vec::new(),
                guess: ScalarLane::Undefined,
                guess_body: Vec::new(),
                known: None,
                homogeneity: None,
                uvar_id: None,
                offset: 0,
            });
            table.declared_count += 1;
        }
    }
}

pub(crate) fn replace_points(table: &mut FeatureVariableTable, points: Vec<FeatureSectionPoint>) {
    let previous_count = table.rows.len();
    table
        .rows
        .retain(|row| !matches!(row.variable_type, VariableType::U | VariableType::V));
    table.declared_count -= (previous_count - table.rows.len()) as u32;
    append_points(table, points);
}
