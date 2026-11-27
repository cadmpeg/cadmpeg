// SPDX-License-Identifier: Apache-2.0
//! Unfold and refold (`docs/formats/solidedge.md` §10.6): a
//! `JMUnfoldFunction` feature states an unfold and a `JMRefoldFunction`
//! feature a refold. The bends and the fixed face are `CLSID_JMTopologyProxy`
//! face selections whose roles §10.6 states by function name. The decoder does
//! not resolve them to faces, so the operands are unstated and each
//! feature adds `sheet-metal.bend-operand-unread`.

use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sheet_metal::{SheetMetal, SheetMetalKind};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use super::features_of;
use crate::features::{feature_id, Listed};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, TypeTable};

/// The function of an unfold.
const UNFOLD_FUNCTION: &str = "JMUnfoldFunction";
/// The function of a refold.
const REFOLD_FUNCTION: &str = "JMRefoldFunction";

/// State each unfold and refold feature, without operands.
pub(crate) fn project(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) {
    for (function, name) in [(UNFOLD_FUNCTION, "unfold"), (REFOLD_FUNCTION, "refold")] {
        for (entry, _, _) in features_of(cluster, types, listed, function) {
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
            let id = feature_id(entry);
            if let Some(feature) = ir.design.features.iter_mut().find(|f| f.id == id) {
                feature.definition.operation = FeatureOperation::SheetMetal(Box::new(SheetMetal {
                    kind,
                    native_options: NativeOptions::new(),
                    source_settings: NativeOptions::new(),
                }));
            }
        }
    }
}

#[cfg(test)]
mod tests;
