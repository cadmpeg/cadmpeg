// SPDX-License-Identifier: Apache-2.0
//! Sheet-metal rule and base tab (`docs/formats/solidedge.md` §10): the
//! document variable table states one rule, and a `JMBaseTabFunction`
//! feature states a flat wall of the rule's thickness over its profile.
//!
//! A table that does not read adds `sheet-metal.rule-unread`; a base tab that
//! does not read stays unresolved and adds `sheet-metal.base-tab-unread`.
//! Stored lengths are metres; the design states millimetres.

use std::collections::BTreeMap;

use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ids::SheetMetalRuleId;
use cadmpeg_ir::design::names::CodecName;
use cadmpeg_ir::design::ops::sheet_metal::{
    BendAllowance, BendSpec, NormalSide, Relief, SheetMetal, SheetMetalKind, SheetMetalRule,
};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, Length, PositiveLength};

use crate::extents::{
    double_slot, is, profile_of, profile_selection, MM_PER_M, PART_CLASS, SIDE_AFTER_NAME,
    SIDE_AGAINST, SIDE_ALONG,
};
use crate::features::{feature_id, function_span, Kind, Listed};
use crate::layout::{variable_record as record, variable_table_header as header};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, TypeTable};

/// The stream that lists the document variables.
const STREAM: &str = "CustomPropertyVariableInfo";
/// The function of a sheet-metal base tab.
const BASE_TAB_FUNCTION: &str = "JMBaseTabFunction";
/// Bytes from the end of a function name to the thickness variable object.
const THICKNESS_AFTER_NAME: usize = 100;

/// The variable indices of §10.1.
const THICKNESS: usize = 4;
const BEND_RADIUS: usize = 5;
const RELIEF_WIDTH: usize = 6;
const RELIEF_LENGTH: usize = 7;
const NEUTRAL_FACTOR: usize = 8;
const MIN_ARC_LENGTH: usize = 9;
const DEVIATIONAL_TOLERANCE: usize = 10;
const BEND_DEDUCTION: usize = 11;
const BEND_ALLOWANCE: usize = 12;
const BEND_METHOD: usize = 13;
/// The key of index `i` is `(i - 1) << 24 | 2` from this index up to
/// [`KEY_GROUP_LIMIT`], and `i << 24` for the other indices (§10.1).
const KEY_GROUP_FIRST: u32 = 2;
/// The first index past the low key group.
const KEY_GROUP_LIMIT: u32 = 4;
/// The key of an index below [`KEY_GROUP_LIMIT`] stores this group.
const KEY_LOW_GROUP: u32 = 2;
/// The key stores its ordinal in bits 24..31.
const KEY_ORDINAL_SHIFT: u32 = 24;
/// The largest ordinal bits 24..31 hold.
const KEY_ORDINAL_MAX: u32 = 0xFF;
/// The number of index slots the reader keeps.
const SLOTS: usize = BEND_METHOD + 1;

/// The unit types of §10.1: length, density and unitless.
const UNIT_TYPES: [u16; 3] = [1, 16, 58];
/// The integral bend-method codes the reader states as an integer.
const METHOD_CODE_MAX: i32 = 255;
/// The bend-method codes of §10.1.
const METHOD_NEUTRAL_FACTOR: f64 = 0.0;
const METHOD_BEND_DEDUCTION: f64 = 1.0;
const METHOD_BEND_ALLOWANCE: f64 = 2.0;

/// The stored value of each variable index.
type Variables = [Option<f64>; SLOTS];

fn rule_id() -> SheetMetalRuleId {
    SheetMetalRuleId::from(cadmpeg_ir::ids::Identity::compose(
        &cadmpeg_ir::identity_namespace!("solidedge", "design", "sheet-metal-rule"),
        cadmpeg_ir::identity_key!("root"),
    ))
}

/// The key §10.1 states for `index`; `None` for index 0 or an index above
/// [`KEY_ORDINAL_MAX`].
fn expected_key(index: u32) -> Option<u32> {
    if index == 0 {
        None
    } else if (KEY_GROUP_FIRST..KEY_GROUP_LIMIT).contains(&index) {
        Some(((index - 1) << KEY_ORDINAL_SHIFT) | KEY_LOW_GROUP)
    } else {
        (index <= KEY_ORDINAL_MAX).then_some(index << KEY_ORDINAL_SHIFT)
    }
}

/// The values of the variable table `bytes`, by index.
fn variables(bytes: &[u8]) -> Result<Variables, String> {
    let count = View::u32_le_at(bytes, header::COUNT)
        .ok_or_else(|| "the table has no record count".to_owned())?;
    let version = View::u32_le_at(bytes, header::VERSION)
        .ok_or_else(|| "the table has no version".to_owned())?;
    if version != header::VERSION_VALUE {
        return Err(format!("the table version {version} is not 1"));
    }
    let count = usize::try_from(count).map_err(|_| "the record count is too large".to_owned())?;
    let body = bytes
        .len()
        .checked_sub(header::LEN)
        .ok_or_else(|| "the table is shorter than its header".to_owned())?;
    if count > body / record::LEN {
        return Err(format!(
            "the table states {count} record(s) and holds {body} byte(s) of records"
        ));
    }
    let mut found: Variables = [None; SLOTS];
    for ordinal in 0..count {
        let at = header::LEN + ordinal * record::LEN;
        let index = View::u32_le_at(bytes, at + record::INDEX)
            .ok_or_else(|| "a record is truncated".to_owned())?;
        let key = View::u32_le_at(bytes, at + record::KEY)
            .ok_or_else(|| "a record is truncated".to_owned())?;
        let object = View::u32_le_at(bytes, at + record::OBJECT)
            .ok_or_else(|| "a record is truncated".to_owned())?;
        let unit = View::u16_le_at(bytes, at + record::UNIT)
            .ok_or_else(|| "a record is truncated".to_owned())?;
        let reserved = View::u16_le_at(bytes, at + record::RESERVED)
            .ok_or_else(|| "a record is truncated".to_owned())?;
        if expected_key(index) != Some(key)
            || object == 0
            || !UNIT_TYPES.contains(&unit)
            || reserved != record::RESERVED_VALUE
        {
            return Err(format!("record {ordinal} does not match the record layout"));
        }
        let value = View::f64_le_at(bytes, at + record::VALUE)
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("record {ordinal} does not hold a finite value"))?;
        let Some(slot) = usize::try_from(index)
            .ok()
            .and_then(|index| found.get_mut(index))
        else {
            continue;
        };
        if slot.replace(value).is_some() {
            return Err(format!("index {index} is listed twice"));
        }
    }
    Ok(found)
}

fn millimetres(metres: f64) -> Option<Length> {
    Length::new(metres * MM_PER_M)
}

fn positive(metres: f64) -> Option<Dimension<PositiveLength>> {
    PositiveLength::new(metres * MM_PER_M).map(Dimension::Literal)
}

fn option_key(name: &str) -> Option<CodecName> {
    CodecName::new(&cadmpeg_ir::codec_id!("solidedge"), name)
}

/// The rule the variables state, with a loss for a bend method it does not
/// read. `Err` where the thickness is not a positive length.
fn rule(
    values: &Variables,
    scope: &ProductDefinitionId,
    losses: &mut Vec<LossNote>,
) -> Result<Option<SheetMetalRule>, String> {
    let Some(thickness) = values[THICKNESS] else {
        return Ok(None);
    };
    let thickness = positive(thickness)
        .ok_or_else(|| format!("the material thickness {thickness} is not positive"))?;
    let radius = values[BEND_RADIUS].filter(|v| *v > 0.0).and_then(positive);
    let relief = values[RELIEF_WIDTH]
        .filter(|v| *v > 0.0)
        .and_then(positive)
        .map(|width| Relief {
            shape: None,
            width: Some(width),
            width_ratio: None,
            depth: None,
        });
    let method = values[BEND_METHOD];
    let allowance = match (method, values[NEUTRAL_FACTOR]) {
        (Some(code), Some(factor)) if is(code, METHOD_NEUTRAL_FACTOR) => {
            FiniteReal::new(factor).map(|factor| BendAllowance::KFactor(Dimension::Literal(factor)))
        }
        (Some(code), None) if is(code, METHOD_NEUTRAL_FACTOR) => {
            losses.push(SolidEdgeLossCode::SheetMetalBendMethodUnread.note(
                "the bend method is the neutral factor and the table lists no neutral factor; the \
                 rule states no bend allowance",
            ));
            None
        }
        (Some(code), _) if is(code, METHOD_BEND_DEDUCTION) || is(code, METHOD_BEND_ALLOWANCE) => {
            let (method, option) = if is(code, METHOD_BEND_DEDUCTION) {
                ("bend deduction", "bend_deduction")
            } else {
                ("bend allowance", "bend_allowance")
            };
            losses.push(SolidEdgeLossCode::SheetMetalBendValueUnstated.note(format!(
                "the bend method is the {method}, which the rule states only as the native option \
                 {option}; the rule states no bend allowance"
            )));
            None
        }
        (Some(code), _) => {
            losses.push(SolidEdgeLossCode::SheetMetalBendMethodUnread.note(format!(
                "the bend method {code} is not a method of §10.1; the rule states no bend \
                 allowance"
            )));
            None
        }
        (None, _) => {
            losses.push(
                SolidEdgeLossCode::SheetMetalBendMethodUnread
                    .note("the table lists no bend method; the rule states no bend allowance"),
            );
            None
        }
    };
    let mut source_settings = NativeOptions::new();
    if let Some(code) =
        method.and_then(|code| (0..=METHOD_CODE_MAX).find(|n| is(code, f64::from(*n))))
    {
        if let Some(key) = option_key("bend_method") {
            source_settings.insert(
                key,
                NativeOption::Integer(Dimension::Literal(i64::from(code))),
            );
        }
    }
    let mut native_options = NativeOptions::new();
    for (name, index) in [
        ("minimum_arc_length", MIN_ARC_LENGTH),
        ("deviational_tolerance", DEVIATIONAL_TOLERANCE),
        ("bend_deduction", BEND_DEDUCTION),
        ("bend_allowance", BEND_ALLOWANCE),
        ("relief_length", RELIEF_LENGTH),
    ] {
        if let (Some(key), Some(length)) = (option_key(name), values[index].and_then(millimetres)) {
            native_options.insert(key, NativeOption::Length(Dimension::Literal(length)));
        }
    }
    Ok(Some(SheetMetalRule {
        id: rule_id(),
        scope: scope.clone(),
        name: None,
        thickness: Some(thickness),
        bend: BendSpec {
            radius,
            radius_side: None,
            relief,
        },
        allowance,
        native_options,
        source_settings,
        native_ref: Some(STREAM.to_owned()),
    }))
}

/// State the sheet-metal rule of the variable table, when it lists the
/// material thickness. A table that does not read adds
/// `sheet-metal.rule-unread`.
pub(crate) fn project_rule<'a>(
    ctx: &DecodeContext<'a>,
    snapshot: &CompoundSnapshot<'a>,
    definition: &ProductDefinitionId,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let Some(entry) = snapshot.stream(STREAM) else {
        return Ok(());
    };
    let stream = snapshot.open(ctx, entry)?;
    let mut notes = Vec::new();
    match variables(stream.window()).and_then(|values| rule(&values, definition, &mut notes)) {
        Ok(Some(rule)) => {
            ctx.charge_entities(1, "project Solid Edge sheet-metal rule")?;
            losses.extend(notes);
            ir.design.sheet_metal_rules.push(rule);
        }
        Ok(None) => {}
        Err(reason) => losses.push(SolidEdgeLossCode::SheetMetalRuleUnread.note(format!(
            "the {STREAM} stream does not read: {reason}; no sheet-metal rule is stated"
        ))),
    }
    Ok(())
}

fn wall(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &crate::psm::Object<'_>,
    end: usize,
    ir: &CadIr,
    id: &cadmpeg_ir::features::FeatureId,
) -> Result<FeatureOperation, String> {
    let profile =
        profile_of(ir, id).ok_or_else(|| "the profile is not a stated sketch".to_owned())?;
    let thickness = double_slot(
        cluster,
        types,
        object,
        end,
        THICKNESS_AFTER_NAME,
        "thickness",
    )?;
    let thickness =
        positive(thickness).ok_or_else(|| format!("the thickness {thickness} is not positive"))?;
    let side = double_slot(cluster, types, object, end, SIDE_AFTER_NAME, "side")?;
    let side = if is(side, SIDE_ALONG) {
        NormalSide::Along
    } else if is(side, SIDE_AGAINST) {
        NormalSide::Against
    } else {
        return Err(format!("the side value {side} is not 1 or 2"));
    };
    Ok(FeatureOperation::SheetMetal(Box::new(SheetMetal {
        kind: SheetMetalKind::Wall {
            profile: profile_selection(&profile.id)?,
            thickness: Some(thickness),
            side: Some(side),
            rule: ir
                .design
                .sheet_metal_rules
                .first()
                .map(|rule| rule.id.clone()),
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    })))
}

/// State each base tab as a wall. A feature that does not read keeps its
/// unresolved operation and adds `sheet-metal.base-tab-unread`.
pub(crate) fn project_base_tabs(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) {
    for entry in listed {
        let Kind::Function(Some(function)) = &entry.kind else {
            continue;
        };
        if function != BASE_TAB_FUNCTION {
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
        let id = feature_id(entry.object);
        match wall(cluster, types, &object, end, ir, &id) {
            Ok(operation) => {
                if let Some(feature) = ir.design.features.iter_mut().find(|f| f.id == id) {
                    feature.definition.operation = operation;
                }
            }
            Err(reason) => losses.push(SolidEdgeLossCode::SheetMetalBaseTabUnread.note(format!(
                "the base tab of feature object {:#x} does not read: {reason}; the feature stays \
                 unresolved",
                entry.object
            ))),
        }
    }
}

pub(crate) mod flange;

#[cfg(test)]
mod tests;
