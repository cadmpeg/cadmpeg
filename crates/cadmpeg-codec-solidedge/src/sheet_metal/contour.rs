// SPDX-License-Identifier: Apache-2.0
//! Base contour flanges (`docs/formats/solidedge.md` §10.5): a
//! `JMBaseContourFlangeFunction` feature states its open profile and its
//! inside bend radius. The extent along the profile normal is a native option:
//! the stored side is not separated from a second slot that holds the same
//! value.
//!
//! A feature whose profile is not a stated sketch stays unresolved and adds
//! `sheet-metal.contour-flange-unread`. Stored lengths are metres; the design
//! states millimetres.

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sheet_metal::{BendSpec, SheetMetal, SheetMetalKind};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use super::{features_of, millimetres, option_key, positive};
use crate::extents::{double_slot, is, measure_slot, profile_of, profile_selection};
use crate::features::{feature_id, Listed};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, TypeTable};

/// The function of a base contour flange.
const FUNCTION: &str = "JMBaseContourFlangeFunction";

/// Bytes after the function name of each slot (§10.5).
const KIND_AFTER_NAME: usize = 152;
const WIDTH_AFTER_NAME: usize = 208;
const RADIUS_AFTER_NAME: usize = 236;

/// The extent kind of a finite extent.
const KIND_FINITE: f64 = 13.0;

fn unread(losses: &mut Vec<LossNote>, object: u32, reason: &str) {
    losses.push(
        SolidEdgeLossCode::SheetMetalContourFlangeUnread.note(format!(
            "the contour flange of feature object {object:#x} does not state a field: {reason}"
        )),
    );
}

/// State each base contour flange. A slot that does not read adds
/// `sheet-metal.contour-flange-unread` and leaves its field unstated.
pub(crate) fn project(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) {
    for (entry, object, end) in features_of(cluster, types, listed, FUNCTION) {
        let id = feature_id(entry);
        let profile = match profile_of(ir, &id)
            .ok_or_else(|| "the profile is not a stated sketch".to_owned())
            .and_then(|profile| profile_selection(&profile.id))
        {
            Ok(profile) => profile,
            Err(reason) => {
                losses.push(
                    SolidEdgeLossCode::SheetMetalContourFlangeUnread.note(format!(
                        "the contour flange of feature object {entry:#x} does not read: {reason}; \
                     the feature stays unresolved"
                    )),
                );
                continue;
            }
        };
        let radius = match double_slot(cluster, types, &object, end, RADIUS_AFTER_NAME, "radius") {
            Ok(metres) => {
                let radius = positive(metres);
                if radius.is_none() {
                    unread(
                        losses,
                        entry,
                        &format!("the bend radius {metres} is not positive"),
                    );
                }
                radius
            }
            Err(reason) => {
                unread(losses, entry, &reason);
                None
            }
        };
        let mut native_options = NativeOptions::new();
        match double_slot(cluster, types, &object, end, KIND_AFTER_NAME, "extent kind") {
            Ok(kind) if is(kind, KIND_FINITE) => {
                match measure_slot(cluster, types, &object, end, WIDTH_AFTER_NAME, "extent")
                    .map(|metres| (metres, millimetres(metres)))
                {
                    Ok((metres, Some(length))) if metres > 0.0 => {
                        if let Some(key) = option_key("extent_distance") {
                            native_options
                                .insert(key, NativeOption::Length(Dimension::Literal(length)));
                        }
                    }
                    Ok((metres, _)) => {
                        unread(
                            losses,
                            entry,
                            &format!("the extent {metres} is not positive"),
                        );
                    }
                    Err(reason) => unread(losses, entry, &reason),
                }
            }
            Ok(kind) => unread(losses, entry, &format!("the extent kind {kind} is not 13")),
            Err(reason) => unread(losses, entry, &reason),
        }
        losses.push(
            SolidEdgeLossCode::SheetMetalContourFlangeUnread.note(format!(
                "the contour flange of feature object {entry:#x} states its extent natively; its \
             direction, thickness side, reliefs and width are not stated"
            )),
        );
        let operation = FeatureOperation::SheetMetal(Box::new(SheetMetal {
            kind: SheetMetalKind::ContourFlange {
                profile,
                edges: None,
                width: None,
                extent: None,
                thickness: None,
                bend: BendSpec {
                    radius,
                    radius_side: None,
                    relief: None,
                },
                vertex_bends: Vec::new(),
                rule: ir
                    .design
                    .sheet_metal_rules
                    .first()
                    .map(|rule| rule.id.clone()),
            },
            native_options,
            source_settings: NativeOptions::new(),
        }));
        if let Some(feature) = ir.design.features.iter_mut().find(|f| f.id == id) {
            feature.definition.operation = operation;
        }
    }
}

#[cfg(test)]
mod tests;
