// SPDX-License-Identifier: Apache-2.0
//! Base contour flanges (`docs/formats/solidedge.md` §10.5): a
//! `JMBaseContourFlangeFunction` feature states its open profile, its inside
//! bend radius, its thickness side, its projection side and its extent
//! distance as native options.
//!
//! A feature whose profile is not a stated sketch stays unresolved and adds
//! `sheet-metal.contour-flange-unread`. Stored lengths are metres; the design
//! states millimetres.

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use super::{features_of, millimetres, option_key};
use crate::extents::{double_slot, is, measure_slot, profile_of, profile_selection};
use crate::features::{feature_id, Listed};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, TypeTable};

/// The function of a base contour flange.
const FUNCTION: &str = "JMBaseContourFlangeFunction";

/// Bytes after the function name of each slot (§10.5).
const THICKNESS_SIDE_AFTER_NAME: usize = 124;
const KIND_AFTER_NAME: usize = 152;
const PROJECTION_SIDE_AFTER_NAME: usize = 180;
const WIDTH_AFTER_NAME: usize = 208;
const RADIUS_AFTER_NAME: usize = 236;

/// The extent kind of a finite extent.
const KIND_FINITE: f64 = 13.0;
/// The side values of the thickness side slot.
const THICKNESS_SIDES: [(f64, i64); 2] = [(1.0, 1), (2.0, 2)];
/// The side values of the projection side slot (§6.2).
const PROJECTION_SIDES: [(f64, i64); 3] = [(1.0, 1), (2.0, 2), (3.0, 3)];

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
        let mut native_options = NativeOptions::new();
        if let Some(key) = option_key("profile") {
            native_options.insert(key, NativeOption::OrderedSelection(profile));
        }
        match double_slot(cluster, types, &object, end, RADIUS_AFTER_NAME, "radius") {
            Ok(metres) => match millimetres(metres).filter(|_| metres > 0.0) {
                Some(radius) => {
                    if let Some(key) = option_key("inside_bend_radius") {
                        native_options
                            .insert(key, NativeOption::Length(Dimension::Literal(radius)));
                    }
                }
                None => unread(
                    losses,
                    entry,
                    &format!("the bend radius {metres} is not positive"),
                ),
            },
            Err(reason) => unread(losses, entry, &reason),
        }
        for (name, after, what, allowed) in [
            (
                "thickness_side",
                THICKNESS_SIDE_AFTER_NAME,
                "thickness side",
                &THICKNESS_SIDES[..],
            ),
            (
                "projection_side",
                PROJECTION_SIDE_AFTER_NAME,
                "projection side",
                &PROJECTION_SIDES[..],
            ),
        ] {
            match double_slot(cluster, types, &object, end, after, what) {
                Ok(side) => match allowed.iter().find(|(stored, _)| is(side, *stored)) {
                    Some((_, code)) => {
                        if let Some(key) = option_key(name) {
                            native_options
                                .insert(key, NativeOption::Integer(Dimension::Literal(*code)));
                        }
                    }
                    None => unread(losses, entry, &format!("the {what} {side} is not stated")),
                },
                Err(reason) => unread(losses, entry, &reason),
            }
        }
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
                "the contour flange of feature object {entry:#x} states its profile, bend radius, \
                 sides and extent distance natively; its reliefs and width are not stated"
            )),
        );
        if let Some(feature) = ir.design.features.iter_mut().find(|f| f.id == id) {
            if let Some(kind) = option_key(FUNCTION) {
                feature.definition.operation = FeatureOperation::Native {
                    kind,
                    native_options,
                    source_settings: NativeOptions::new(),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests;
