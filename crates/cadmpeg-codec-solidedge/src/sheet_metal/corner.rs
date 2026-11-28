// SPDX-License-Identifier: Apache-2.0
//! Close corners (`docs/formats/solidedge.md` §10.7): the IR holds no
//! operation that closes the corner between two flanges, so a
//! `JMCloseCornerAssocFunction` feature stays a native operation and the
//! stored closure code, relief code, relief size and corner gap are its native
//! options.
//! Stored lengths are metres; the design states millimetres.

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::options::NativeOption;
use cadmpeg_ir::document::CadIr;

use super::{code_of, features_of, millimetres, option_key};
use crate::extents::{double_slot, measure_slot};
use crate::features::{feature_id, Listed};
use crate::psm::{Cluster, TypeTable};

/// The function of a close corner.
const FUNCTION: &str = "JMCloseCornerAssocFunction";

/// Bytes after the function name of each slot (§10.7).
const CLOSURE_AFTER_NAME: usize = 124;
const RELIEF_AFTER_NAME: usize = 152;
const SIZE_AFTER_NAME: usize = 180;
const GAP_AFTER_NAME: usize = 244;

/// Add the stored close-corner fields to the native options of each close
/// corner. A field that does not read is left out.
pub(crate) fn project(cluster: &Cluster<'_>, types: &TypeTable, listed: &[Listed], ir: &mut CadIr) {
    for (entry, object, end) in features_of(cluster, types, listed, FUNCTION) {
        let mut found = Vec::new();
        if let Some(code) = double_slot(cluster, types, &object, end, CLOSURE_AFTER_NAME, "closure")
            .ok()
            .and_then(code_of)
        {
            found.push((
                "corner_closure_code",
                NativeOption::Integer(Dimension::Literal(code)),
            ));
        }
        if let Some(code) = double_slot(cluster, types, &object, end, RELIEF_AFTER_NAME, "relief")
            .ok()
            .and_then(code_of)
        {
            found.push((
                "corner_relief_code",
                NativeOption::Integer(Dimension::Literal(code)),
            ));
        }
        for (name, after) in [
            ("corner_relief_size", SIZE_AFTER_NAME),
            ("corner_gap", GAP_AFTER_NAME),
        ] {
            if let Some(length) = measure_slot(cluster, types, &object, end, after, name)
                .ok()
                .filter(|metres| *metres >= 0.0)
                .and_then(millimetres)
            {
                found.push((name, NativeOption::Length(Dimension::Literal(length))));
            }
        }
        let id = feature_id(entry);
        let Some(FeatureOperation::Native { native_options, .. }) = ir
            .design
            .features
            .iter_mut()
            .find(|f| f.id == id)
            .map(|feature| &mut feature.definition.operation)
        else {
            continue;
        };
        for (name, value) in found {
            if let Some(key) = option_key(name) {
                native_options.insert(key, value);
            }
        }
    }
}

#[cfg(test)]
mod tests;
