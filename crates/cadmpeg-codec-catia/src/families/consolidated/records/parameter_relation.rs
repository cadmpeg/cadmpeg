// SPDX-License-Identifier: Apache-2.0
//! Shared target sites and arithmetic comparison for parameter relations.

use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

/// Only target sites that have serialized words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum TargetSites {
    Absent,
    Lower {
        parameter: FiniteReal,
    },
    Upper {
        parameter: FiniteReal,
    },
    Both {
        lower: FiniteReal,
        upper: FiniteReal,
    },
}

pub(super) fn endpoints_agree(first: f64, second: f64) -> bool {
    const ABSOLUTE: f64 = 1.0e-7;
    const RELATIVE: f64 = 1.0e-12;
    first.is_finite()
        && second.is_finite()
        && (first - second).abs() <= ABSOLUTE + RELATIVE * first.abs().max(second.abs())
}
