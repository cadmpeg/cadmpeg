// SPDX-License-Identifier: Apache-2.0
//! Unfold and refold (`docs/formats/solidedge.md` §10.6): a
//! `JMUnfoldFunction` feature states an unfold and a `JMRefoldFunction`
//! feature a refold. The bends and the fixed face are `CLSID_JMTopologyProxy`
//! face selections whose roles §10.6 states by function name. The decoder does
//! not resolve them to faces, so the operands are unstated and each
//! feature adds `sheet-metal.bend-operand-unread`.
//!
//! The bends proxy of an unfold states the width and the two layer radii of
//! the bend it selects; they are native options of the unfold. Stored lengths
//! are metres; the design states millimetres.

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sheet_metal::{SheetMetal, SheetMetalKind};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use self::bends::bend_faces;
use super::{features_of, millimetres, option_key};
use crate::extents::{named, slot};
use crate::features::{feature_id, Listed};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, Object, TypeTable};

/// The function of an unfold.
const UNFOLD_FUNCTION: &str = "JMUnfoldFunction";
/// The function of a refold.
const REFOLD_FUNCTION: &str = "JMRefoldFunction";

/// The class of a topology proxy.
const PROXY_CLASS: &str = "CLSID_JMTopologyProxy";
/// Bytes after the function name of the bends proxy of an unfold (§10.6).
const UNFOLD_BENDS_AFTER_NAME: usize = 96;

/// The native options the bends proxy of an unfold states. Empty where the
/// slot does not hold a readable bends proxy.
fn bend_options(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
) -> NativeOptions {
    let mut options = NativeOptions::new();
    let Ok(id) = slot(object, end, UNFOLD_BENDS_AFTER_NAME) else {
        return options;
    };
    let Ok((proxy, class)) = named(cluster, types, id, "bends") else {
        return options;
    };
    if class != PROXY_CLASS {
        return options;
    }
    let Some(faces) = bend_faces(proxy.bytes) else {
        return options;
    };
    for (name, metres) in [
        ("bend_width", faces.width),
        ("bend_inside_radius", faces.inside_radius),
        ("bend_outside_radius", faces.outside_radius),
    ] {
        if let (Some(key), Some(length)) = (option_key(name), millimetres(metres)) {
            options.insert(key, NativeOption::Length(Dimension::Literal(length)));
        }
    }
    options
}

/// State each unfold and refold feature, without operands.
pub(crate) fn project(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) {
    for (function, name) in [(UNFOLD_FUNCTION, "unfold"), (REFOLD_FUNCTION, "refold")] {
        for (entry, object, end) in features_of(cluster, types, listed, function) {
            let kind = if function == UNFOLD_FUNCTION {
                SheetMetalKind::Unfold {
                    fixed: None,
                    bends: None,
                }
            } else {
                SheetMetalKind::Refold { bends: None }
            };
            losses.push(SolidEdgeLossCode::SheetMetalBendOperandUnread.note(format!(
                "the {name} of feature object {entry:#x} selects its bends by topology proxies \
                 that are not read; its operands are not stated"
            )));
            let native_options = if function == UNFOLD_FUNCTION {
                bend_options(cluster, types, &object, end)
            } else {
                NativeOptions::new()
            };
            let id = feature_id(entry);
            if let Some(feature) = ir.design.features.iter_mut().find(|f| f.id == id) {
                feature.definition.operation = FeatureOperation::SheetMetal(Box::new(SheetMetal {
                    kind,
                    native_options,
                    source_settings: NativeOptions::new(),
                }));
            }
        }
    }
}

mod bends;

#[cfg(test)]
mod tests;
