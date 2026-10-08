// SPDX-License-Identifier: Apache-2.0
//! Definition and trim fixture construction.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

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
    table.declared_count -=
        u32::try_from(previous_count - table.rows.len()).expect("fixture value fits u32");
    append_points(table, points);
}

pub(super) fn reconciled_points(
    variables: &FeatureVariableTable,
) -> (
    std::collections::BTreeMap<u32, [Option<f64>; 2]>,
    std::collections::BTreeSet<u32>,
) {
    crate::decode::with_test_decode_ctx(|ctx| {
        variables
            .reconciled_points(ctx)
            .map(|result| (result.points, result.ambiguous))
    })
    .expect("test point reconciliation")
}


pub(super) fn with_trim_limits<T>(
    collection_limit: u64,
    work_limit: u64,
    run: impl FnOnce(&DecodeContext<'_>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_work_units = work_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits the trim policy");
    run(&ctx)
}
