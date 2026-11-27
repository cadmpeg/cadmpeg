// SPDX-License-Identifier: Apache-2.0
//! Flanges (`docs/formats/solidedge.md` §10.3): a `JMMultiFlangeFunction`
//! feature states one edge, a length, an angle, an extent, a bend radius and a
//! bend relief.
//!
//! A slot that does not read adds `sheet-metal.flange-field-unread` and leaves
//! its field unstated; the flange is still stated. Stored lengths are metres;
//! the design states millimetres.

use std::collections::BTreeSet;

use cadmpeg_core::decode::View;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sheet_metal::{
    BendPosition, BendSpec, Flange, FlangeHeight, FlangeWidth, HeightDatum, RadiusSide, Relief,
    ReliefShape, SheetMetal, SheetMetalKind,
};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::selection::OrderedSelection;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, NonNegativeLength, PositiveLength};

use super::{option_key, positive};
use crate::extents::{
    dimension_value, double_slot, is, named, slot, DIMENSION_CLASS, MM_PER_M, PART_CLASS,
};
use crate::features::{feature_id, function_span, Kind, Listed};
use crate::layout::standard_relation;
use crate::loss::SolidEdgeLossCode;
use crate::proxy::edge_selection;
use crate::psm::{Cluster, Object, TypeTable};

/// The function of a flange.
const FUNCTION: &str = "JMMultiFlangeFunction";
/// The class of an edge selection.
const PROXY_CLASS: &str = "CLSID_JMTopologyProxy";
/// The class of the object that binds a flange value to a document variable.
const RELATION_CLASS: &str = "Assoc subsystem Standard Relation implementation";

/// Bytes after the function name of each slot (§10.3).
const EDGE_AFTER_NAME: usize = 72;
const LENGTH_AFTER_NAME: usize = 100;
const ANGLE_AFTER_NAME: usize = 128;
const EXTENT_AFTER_NAME: usize = 184;
const MATERIAL_SIDE_AFTER_NAME: usize = 212;
const SIDE_AFTER_NAME: usize = 240;
const RADIUS_AFTER_NAME: usize = 296;
const RELIEF_WIDTH_AFTER_NAME: usize = 380;
const RELIEF_LENGTH_AFTER_NAME: usize = 408;
const PARTIAL_WIDTH_AFTER_NAME: usize = 492;
const RELIEF_TYPE_AFTER_NAME: usize = 496;
/// The distance the slots from [`PARTIAL_WIDTH_AFTER_NAME`] on move where the
/// flange has a centred partial width.
const PARTIAL_SHIFT: usize = 24;
/// The distance the slots from [`PARTIAL_WIDTH_AFTER_NAME`] on move where the
/// flange has an offset and a width: two `CLSID_JDim` slots.
const OFFSET_SHIFT: usize = 44;
const WIDTH_AFTER_NAME: usize = 512;

/// The extent kinds.
const EXTENT_FULL: f64 = 134.0;
const EXTENT_CENTRED: f64 = 135.0;
const EXTENT_OFFSET: f64 = 138.0;
const EXTENT_FROM_ENDS: f64 = 140.0;
/// The side value that turns the flange towards the base face normal.
const SIDE_TOWARDS_NORMAL: f64 = 2.0;
/// The side value that turns the flange against the base face normal.
const SIDE_AGAINST_NORMAL: f64 = 1.0;
/// The material side values.
const MATERIAL_INSIDE: f64 = 1.0;
const MATERIAL_OUTSIDE: f64 = 2.0;
const MATERIAL_BEND_OUTSIDE: f64 = 9.0;
/// The relief types.
const RELIEF_NONE: f64 = 53.0;
const RELIEF_SQUARE: f64 = 51.0;
const RELIEF_ROUND: f64 = 52.0;

/// The value of `result`, or `None` and a loss for its reason.
fn take<T>(losses: &mut Vec<LossNote>, object: u32, result: Result<T, String>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(reason) => {
            unread(losses, object, &reason);
            None
        }
    }
}

fn unread(losses: &mut Vec<LossNote>, object: u32, reason: &str) {
    losses.push(SolidEdgeLossCode::SheetMetalFlangeFieldUnread.note(format!(
        "the flange of feature object {object:#x} does not state a field: {reason}"
    )));
}

/// The flange object identifiers that a relation binds to a document
/// variable.
fn bound_objects(cluster: &Cluster<'_>, types: &TypeTable) -> BTreeSet<u32> {
    cluster
        .objects()
        .filter(|object| {
            object.bytes.len() == standard_relation::LEN
                && types.class_name(object.form, object.flags) == Some(RELATION_CLASS)
        })
        .filter(|object| {
            View::u32_le_at(object.bytes, standard_relation::VARIABLE_OBJECT)
                .is_some_and(|variable| variable != 0)
        })
        .filter_map(|object| View::u32_le_at(object.bytes, standard_relation::OWN_OBJECT))
        .collect()
}

/// The value of the `CLSID_JDim` in the slot `after` bytes past the name.
fn jdim(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
    after: usize,
    what: &str,
) -> Result<f64, String> {
    let id = slot(object, end, after)?;
    let (found, class) = named(cluster, types, id, what)?;
    if class != DIMENSION_CLASS {
        return Err(format!("the {what} object is of class {class}"));
    }
    dimension_value(&found, what)
}

fn positive_length(metres: f64, what: &str) -> Result<Dimension<PositiveLength>, String> {
    positive(metres).ok_or_else(|| format!("the {what} {metres} is not positive"))
}

fn setback(metres: f64, what: &str) -> Result<Dimension<NonNegativeLength>, String> {
    NonNegativeLength::new(metres * MM_PER_M)
        .map(Dimension::Literal)
        .ok_or_else(|| format!("the {what} {metres} is negative"))
}

/// The angle the flange turns through from the plane of the face that bounds
/// its edge: the half turn less the entered angle.
fn turn(entered: f64) -> Result<Dimension<Angle>, String> {
    if entered <= 0.0 || entered >= std::f64::consts::PI {
        return Err(format!("the entered angle {entered} is not in (0, pi)"));
    }
    Angle::new(std::f64::consts::PI - entered)
        .map(Dimension::Literal)
        .ok_or_else(|| "the flange angle is not finite".to_owned())
}

struct Slots<'a, 'b> {
    cluster: &'a Cluster<'b>,
    types: &'a TypeTable,
    object: &'a Object<'b>,
    end: usize,
}

impl Slots<'_, '_> {
    fn double(&self, after: usize, what: &str) -> Result<f64, String> {
        double_slot(self.cluster, self.types, self.object, self.end, after, what)
    }

    fn jdim(&self, after: usize, what: &str) -> Result<f64, String> {
        jdim(self.cluster, self.types, self.object, self.end, after, what)
    }

    /// Whether a relation binds the object in the slot to a document variable.
    fn bound(&self, after: usize, bound: &BTreeSet<u32>) -> Result<bool, String> {
        Ok(bound.contains(&slot(self.object, self.end, after)?))
    }

    fn edges(&self) -> Result<OrderedSelection, String> {
        let id = slot(self.object, self.end, EDGE_AFTER_NAME)?;
        let (proxy, class) = named(self.cluster, self.types, id, "edge")?;
        if class != PROXY_CLASS {
            return Err(format!("the edge object is of class {class}"));
        }
        let (selection, stated) = edge_selection(&proxy);
        if stated {
            Ok(selection)
        } else {
            Err("the edge object does not store a straight edge".to_owned())
        }
    }
}

/// The unread-edge selection: a flange states its edge as unread where the
/// proxy does not read.
fn unread_edges() -> OrderedSelection {
    cadmpeg_ir::design::selection::Selection {
        at: cadmpeg_ir::design::selection::Anchor::Input,
        picks: cadmpeg_ir::design::selection::Picks::Unread,
        native: None,
    }
}

fn relief(
    slots: &Slots<'_, '_>,
    shift: usize,
    object: u32,
    overrides: &mut Vec<&'static str>,
    bound: &BTreeSet<u32>,
    governed: bool,
    losses: &mut Vec<LossNote>,
) -> Option<Relief> {
    let after = RELIEF_TYPE_AFTER_NAME + shift;
    let code = match slots.double(after, "relief type") {
        Ok(code) => code,
        Err(reason) => {
            unread(losses, object, &reason);
            return None;
        }
    };
    let shape = if is(code, RELIEF_NONE) {
        return None;
    } else if is(code, RELIEF_ROUND) {
        Some(ReliefShape::Obround)
    } else if is(code, RELIEF_SQUARE) {
        NonBlankString::new("square".to_owned()).map(ReliefShape::Native)
    } else {
        unread(
            losses,
            object,
            &format!("the relief type {code} is not 51, 52 or 53"),
        );
        None
    };
    let mut length = |after: usize, what: &str, name: &'static str| {
        // A value bound to a document variable follows the rule (§10.3).
        if governed && matches!(slots.bound(after, bound), Ok(true)) {
            return None;
        }
        let metres = match slots.double(after, what) {
            Ok(metres) => metres,
            Err(reason) => {
                unread(losses, object, &reason);
                return None;
            }
        };
        if let Ok(false) = slots.bound(after, bound) {
            overrides.push(name);
        }
        if metres < 0.0 {
            unread(losses, object, &format!("the {what} {metres} is negative"));
            return None;
        }
        // A zero length states no relief dimension.
        positive(metres)
    };
    let width = length(RELIEF_WIDTH_AFTER_NAME, "relief width", "relief_width");
    let depth = length(RELIEF_LENGTH_AFTER_NAME, "relief length", "relief_length");
    Some(Relief {
        shape,
        width,
        width_ratio: None,
        depth,
    })
}

fn flange(
    slots: &Slots<'_, '_>,
    object: u32,
    ir: &CadIr,
    bound: &BTreeSet<u32>,
    losses: &mut Vec<LossNote>,
) -> SheetMetal {
    let edges = take(losses, object, slots.edges()).unwrap_or_else(unread_edges);
    let height = take(
        losses,
        object,
        slots
            .jdim(LENGTH_AFTER_NAME, "length")
            .and_then(|metres| positive_length(metres, "flange length")),
    )
    .map(FlangeHeight::Distance);
    let angle = take(
        losses,
        object,
        slots.jdim(ANGLE_AFTER_NAME, "angle").and_then(turn),
    );
    // A value bound to a document variable follows the rule (§10.3), which
    // the feature states as `None`.
    let governed = !ir.design.sheet_metal_rules.is_empty();
    let radius = if governed && matches!(slots.bound(RADIUS_AFTER_NAME, bound), Ok(true)) {
        None
    } else {
        take(
            losses,
            object,
            slots
                .double(RADIUS_AFTER_NAME, "bend radius")
                .and_then(|metres| positive_length(metres, "bend radius")),
        )
    };
    let mut overrides = Vec::new();
    if radius.is_some() && matches!(slots.bound(RADIUS_AFTER_NAME, bound), Ok(false)) {
        overrides.push("bend_radius");
    }
    let side = take(losses, object, slots.double(SIDE_AFTER_NAME, "side"));
    match side {
        Some(side) if is(side, SIDE_TOWARDS_NORMAL) => {}
        Some(side) if is(side, SIDE_AGAINST_NORMAL) => unread(
            losses,
            object,
            "the side value 1 turns the flange against the base face normal, which the flange does not state",
        ),
        Some(side) => unread(
            losses,
            object,
            &format!("the side value {side} is not 1 or 2"),
        ),
        None => {}
    }
    let position = take(
        losses,
        object,
        slots.double(MATERIAL_SIDE_AFTER_NAME, "material side"),
    )
    .and_then(|code| {
        if is(code, MATERIAL_INSIDE) {
            Some(BendPosition::MaterialInside)
        } else if is(code, MATERIAL_OUTSIDE) {
            Some(BendPosition::MaterialOutside)
        } else {
            let what = if is(code, MATERIAL_BEND_OUTSIDE) {
                "the material side 9 (bend outside) has no stated bend position".to_owned()
            } else {
                format!("the material side {code} is not 1, 2 or 9")
            };
            unread(losses, object, &what);
            None
        }
    });
    let extent = take(
        losses,
        object,
        slots.double(EXTENT_AFTER_NAME, "extent kind"),
    );
    let shift = match extent {
        Some(kind) if is(kind, EXTENT_CENTRED) => Some(PARTIAL_SHIFT),
        Some(kind) if is(kind, EXTENT_OFFSET) || is(kind, EXTENT_FROM_ENDS) => Some(OFFSET_SHIFT),
        Some(kind) if is(kind, EXTENT_FULL) => Some(0),
        _ => None,
    };
    let width = match extent {
        Some(kind) if is(kind, EXTENT_FULL) => Some(FlangeWidth::Full),
        Some(kind) if is(kind, EXTENT_CENTRED) => take(
            losses,
            object,
            slots
                .jdim(PARTIAL_WIDTH_AFTER_NAME, "partial width")
                .and_then(|metres| positive_length(metres, "partial width")),
        )
        .map(|width| FlangeWidth::Centered { width }),
        Some(kind) if is(kind, EXTENT_OFFSET) => {
            // The end that the offset counts from is not established.
            unread(
                losses,
                object,
                "the offset of the extent kind 138 does not state its end",
            );
            None
        }
        Some(kind) if is(kind, EXTENT_FROM_ENDS) => {
            let first = take(
                losses,
                object,
                slots
                    .jdim(PARTIAL_WIDTH_AFTER_NAME, "start setback")
                    .and_then(|metres| setback(metres, "start setback")),
            );
            let second = take(
                losses,
                object,
                slots
                    .jdim(WIDTH_AFTER_NAME, "end setback")
                    .and_then(|metres| setback(metres, "end setback")),
            );
            first
                .zip(second)
                .map(|(first, second)| FlangeWidth::FromEnds { first, second })
        }
        Some(kind) => {
            unread(
                losses,
                object,
                &format!("the extent kind {kind} is not 134, 135, 138 or 140"),
            );
            None
        }
        None => None,
    };
    let relief = if let Some(shift) = shift {
        relief(
            slots,
            shift,
            object,
            &mut overrides,
            bound,
            governed,
            losses,
        )
    } else {
        None
    };
    let mut native_options = NativeOptions::new();
    for name in overrides {
        if let Some(key) = option_key(&format!("{name}_overrides_rule")) {
            native_options.insert(key, NativeOption::Bool(Dimension::Literal(true)));
        }
    }
    SheetMetal {
        kind: SheetMetalKind::Flange(Flange {
            edges,
            height,
            angle,
            width,
            bend: BendSpec {
                radius,
                radius_side: Some(RadiusSide::Inside),
                relief,
            },
            rule: ir
                .design
                .sheet_metal_rules
                .first()
                .map(|rule| rule.id.clone()),
            height_datum: Some(HeightDatum::OuterSharp),
            position,
            outline: None,
        }),
        native_options,
        source_settings: NativeOptions::new(),
    }
}

/// State each flange feature. A slot that does not read adds
/// `sheet-metal.flange-field-unread`.
pub(crate) fn project(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) {
    let bound = bound_objects(cluster, types);
    for entry in listed {
        let Kind::Function(Some(function)) = &entry.kind else {
            continue;
        };
        if function != FUNCTION {
            continue;
        }
        let Some(object) = cluster.object(entry.object) else {
            continue;
        };
        if types.class_name(object.form, object.flags) != Some(PART_CLASS) {
            continue;
        }
        let Some((_, end)) = function_span(&object) else {
            continue;
        };
        let slots = Slots {
            cluster,
            types,
            object: &object,
            end,
        };
        let operation = FeatureOperation::SheetMetal(Box::new(flange(
            &slots,
            entry.object,
            ir,
            &bound,
            losses,
        )));
        let id = feature_id(entry.object);
        if let Some(feature) = ir.design.features.iter_mut().find(|f| f.id == id) {
            feature.definition.operation = operation;
        }
    }
}

#[cfg(test)]
mod tests;
