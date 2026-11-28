// SPDX-License-Identifier: Apache-2.0
//! The value of a `CLSID_JDim` object (`docs/formats/solidedge.md` §7.1).

use cadmpeg_core::decode::View;

use crate::layout::{dimension, dimension_compact};

/// The offsets of one dimension layout: marker, target, repeated target and
/// value.
struct Form {
    marker: usize,
    marker_value: u32,
    target: usize,
    target_repeat: usize,
    value: usize,
}

/// The two layouts, in the order they are tried: `p` = 148, then `p` = 144.
const FORMS: [Form; 2] = [
    Form {
        marker: dimension::MARKER,
        marker_value: dimension::MARKER_VALUE,
        target: dimension::TARGET,
        target_repeat: dimension::TARGET_REPEAT,
        value: dimension::VALUE,
    },
    Form {
        marker: dimension_compact::MARKER,
        marker_value: dimension_compact::MARKER_VALUE,
        target: dimension_compact::TARGET,
        target_repeat: dimension_compact::TARGET_REPEAT,
        value: dimension_compact::VALUE,
    },
];

fn read(bytes: &[u8], form: &Form) -> Option<f64> {
    if View::u32_le_at(bytes, form.marker)? != form.marker_value {
        return None;
    }
    let target = View::u32_le_at(bytes, form.target)?;
    if target == 0 || View::u32_le_at(bytes, form.target_repeat)? != target {
        return None;
    }
    View::f64_le_at(bytes, form.value).filter(|value| value.is_finite())
}

/// The value of a dimension object's bytes, or `None` where neither layout
/// holds.
pub(crate) fn value(bytes: &[u8]) -> Option<f64> {
    FORMS.iter().find_map(|form| read(bytes, form))
}
