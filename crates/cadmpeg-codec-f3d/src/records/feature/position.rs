use super::assembly::DesignAssemblyOperandPath;
use super::scope::DesignParameterScope;
use crate::layout::{
    captured_position_matrix as matrix, captured_position_prefix as prefix,
    captured_position_row as row, captured_position_suffix as suffix,
};
use crate::records::identity::Located;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_position_matrix,
    Located<[FiniteReal; 16]>,
    "matrix"
);
cadmpeg_core::named_optional_field!(deserialize_position_path_index, usize, "path_index");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignCapturedPosition {
    pub(crate) target: Located<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_position_matrix"
    )]
    pub(crate) matrix: Option<Located<[FiniteReal; 16]>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_position_path_index"
    )]
    pub(crate) path_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignCapturedPositions {
    pub(crate) rows: Vec<DesignCapturedPosition>,
    pub(crate) paths: Vec<DesignAssemblyOperandPath>,
}

impl DesignCapturedPositions {
    pub(crate) fn valid_for_scope(&self, scope: &DesignParameterScope) -> bool {
        if scope.kind_name() != "Position" {
            return false;
        }
        let Some(mut at) = scope.byte_offset().checked_add(u64_from_index(prefix::LEN)) else {
            return false;
        };
        let mut next_path = 0;
        let mut targets = std::collections::BTreeSet::new();
        for position in &self.rows {
            if at.checked_add(u64_from_index(row::TARGET)) != Some(position.target.offset)
                || position.matrix.as_ref().is_some_and(|value| {
                    at.checked_add(u64_from_index(row::LEN)) != Some(value.offset)
                })
            {
                return false;
            }
            if let Some(index) = position.path_index {
                let Some(path) = self.paths.get(index) else {
                    return false;
                };
                if index > next_path || path.link().locator_record_index != position.target.value {
                    return false;
                }
                if index == next_path {
                    if path.link().locator_reference_offset != position.target.offset
                        || !targets.insert(position.target.value)
                        || scope
                            .paired_byte_offset()
                            .checked_add(u64_from_index(
                                crate::layout::indexed_design_record_header::LEN,
                            ))
                            .is_none_or(|minimum| path.link().locator_byte_offset < minimum)
                        || !path.admitted_by_type()
                    {
                        return false;
                    }
                    next_path += 1;
                }
            }
            let length = row::LEN + position.matrix.as_ref().map_or(0, |_| matrix::LEN);
            let Some(next) = at.checked_add(u64_from_index(length)) else {
                return false;
            };
            at = next;
        }
        next_path == self.paths.len()
            && at.checked_add(u64_from_index(suffix::LEN)) == Some(scope.reference_count_offset())
    }
}

rewrite_native_record!(DesignCapturedPosition, []; {target, matrix, path_index});
rewrite_native_record!(DesignCapturedPositions, []; {rows, paths});
