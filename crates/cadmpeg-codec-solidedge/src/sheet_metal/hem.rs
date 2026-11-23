// SPDX-License-Identifier: Apache-2.0
//! Hems (`docs/formats/solidedge.md` §10.4): a `JMHemAssocFunction` feature
//! states the edge it folds and its form. The form is native: the length the
//! stored straight length gives depends on the sheet thickness and is not
//! stated.
//!
//! A slot that does not read adds `sheet-metal.hem-field-unread` and leaves its
//! field unstated; the hem is still stated. Stored lengths are metres; the
//! design states millimetres.

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sheet_metal::{Hem, HemForm, SheetMetal, SheetMetalKind};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::selection::{Anchor, OrderedSelection, Picks, Selection};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use super::{code_of, features_of, millimetres, option_key};
use crate::edge::edge_selection_from_end;
use crate::extents::{double_slot, is, named, slot};
use crate::features::{feature_id, Listed};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, Object, TypeTable};

/// The function of a hem.
const FUNCTION: &str = "JMHemAssocFunction";
/// The class of an edge selection.
const PROXY_CLASS: &str = "CLSID_JMTopologyProxy";

/// Bytes after the function name of each slot (§10.4).
const EDGE_AFTER_NAME: usize = 68;
const FORM_AFTER_NAME: usize = 96;
const RADIUS_AFTER_NAME: usize = 208;
const STRAIGHT_AFTER_NAME: usize = 236;

/// The form code of a single hem.
const FORM_SINGLE: f64 = 208.0;

fn unread(losses: &mut Vec<LossNote>, object: u32, reason: &str) {
    losses.push(SolidEdgeLossCode::SheetMetalHemFieldUnread.note(format!(
        "the hem of feature object {object:#x} does not state a field: {reason}"
    )));
}

fn edges(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
) -> Result<OrderedSelection, String> {
    let id = slot(object, end, EDGE_AFTER_NAME)?;
    let (proxy, class) = named(cluster, types, id, "edge")?;
    if class != PROXY_CLASS {
        return Err(format!("the edge object is of class {class}"));
    }
    let (selection, stated) = edge_selection_from_end(&proxy);
    if stated {
        Ok(selection)
    } else {
        Err("the edge object does not store a straight edge".to_owned())
    }
}

fn length_option(
    options: &mut NativeOptions,
    name: &str,
    metres: f64,
    losses: &mut Vec<LossNote>,
    object: u32,
) {
    match millimetres(metres).filter(|_| metres > 0.0) {
        Some(length) => {
            if let Some(key) = option_key(name) {
                options.insert(key, NativeOption::Length(Dimension::Literal(length)));
            }
        }
        None => unread(
            losses,
            object,
            &format!("the {name} {metres} is not positive"),
        ),
    }
}

/// State each hem feature. A slot that does not read adds
/// `sheet-metal.hem-field-unread`.
pub(crate) fn project(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) {
    for (entry, object, end) in features_of(cluster, types, listed, FUNCTION) {
        let edges = match edges(cluster, types, &object, end) {
            Ok(edges) => edges,
            Err(reason) => {
                unread(losses, entry, &reason);
                Selection {
                    at: Anchor::Input,
                    picks: Picks::Unread,
                    native: None,
                }
            }
        };
        let mut native_options = NativeOptions::new();
        let form = match double_slot(cluster, types, &object, end, FORM_AFTER_NAME, "form") {
            Ok(code) => match code_of(code) {
                Some(n) => {
                    if let Some(key) = option_key("hem_form_code") {
                        native_options.insert(key, NativeOption::Integer(Dimension::Literal(n)));
                    }
                    if is(code, FORM_SINGLE) {
                        for (name, after, what) in [
                            ("hem_inner_radius", RADIUS_AFTER_NAME, "inner radius"),
                            (
                                "hem_straight_length",
                                STRAIGHT_AFTER_NAME,
                                "straight length",
                            ),
                        ] {
                            match double_slot(cluster, types, &object, end, after, what) {
                                Ok(metres) => {
                                    length_option(&mut native_options, name, metres, losses, entry);
                                }
                                Err(reason) => unread(losses, entry, &reason),
                            }
                        }
                        "solidedge:hem-single".to_owned()
                    } else {
                        format!("solidedge:hem-form-{n}")
                    }
                }
                None => {
                    unread(
                        losses,
                        entry,
                        &format!("the form code {code} is not a whole number"),
                    );
                    "solidedge:hem-form".to_owned()
                }
            },
            Err(reason) => {
                unread(losses, entry, &reason);
                "solidedge:hem-form".to_owned()
            }
        };
        let Some(form) = NonBlankString::new(form) else {
            continue;
        };
        losses.push(SolidEdgeLossCode::SheetMetalHemFieldUnread.note(format!(
            "the hem of feature object {entry:#x} states its form natively; its length, gap, \
             side and position are not stated"
        )));
        let operation = FeatureOperation::SheetMetal(Box::new(SheetMetal {
            kind: SheetMetalKind::Hem(Hem {
                edges,
                form: HemForm::Native(form),
                side: None,
                position: None,
                rule: ir
                    .design
                    .sheet_metal_rules
                    .first()
                    .map(|rule| rule.id.clone()),
            }),
            native_options,
            source_settings: NativeOptions::new(),
        }));
        let id = feature_id(entry);
        if let Some(feature) = ir.design.features.iter_mut().find(|f| f.id == id) {
            feature.definition.operation = operation;
        }
    }
}

#[cfg(test)]
mod tests;
