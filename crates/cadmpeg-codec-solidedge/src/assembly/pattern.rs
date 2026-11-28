// SPDX-License-Identifier: Apache-2.0
//! Assembly patterns (`docs/formats/solidedge.md` §9.5): the pattern of an
//! occurrence along a layout sketch, stated as a component pattern of the
//! assembly's design.
//!
//! A `CLSID_JAsmPattern` object names its seed occurrence and the pattern-2d
//! object of the layout sketch, and lists its copies. The layout comes from
//! the pattern-2d object as for a part pattern (`crate::patterns`). A
//! circular pattern states its count and step as native options, and its
//! layout as native. An instance whose index repeats an earlier one, or is the
//! seed's, is left out of the pattern and adds `assembly.pattern-unread`; the
//! occurrence of that copy stays an occurrence. A pattern that does not read
//! adds `assembly.pattern-unread`.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::View;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::container::{Container, OrderedKind};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::design::names::{CodecName, InstanceIndex};
use cadmpeg_ir::design::ops::assembly::{ComponentInstance, ComponentPattern};
use cadmpeg_ir::design::ops::shared::{PatternLayout, Spacing};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::selection::{
    Anchor, Members, OccurrencePath, OrderedSelection, PathStep, PickContext, PickTarget, Picked,
    Picks, Selection, StepRef,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;

use crate::features::{container_id, feature_id};
use crate::layout::{asm_pattern, asm_pattern_occurrence, assembly_reference};
use crate::loss::SolidEdgeLossCode;
use crate::patterns::{sketch_layout, SketchLayout};
use crate::psm::{Cluster, Object, TypeTable};
use crate::sketches::{profile_frame, profile_members};

/// The class of an assembly pattern.
const PATTERN_CLASS: &str = "CLSID_JAsmPattern";
/// The class of one copy of an assembly pattern.
const COPY_CLASS: &str = "CLSID_JAsmPatternOcc";
/// The text that ends before the parameters of an assembly layout's
/// pattern-2d object (§9.5).
const PARAMETER_ANCHOR: &[u8] = b"JPattern2dGeomsFunction\0";
/// Bytes from the end of [`PARAMETER_ANCHOR`] to the offset `E` of §8.1.
const PARAMETER_BASE_AFTER_ANCHOR: usize = 84;
/// The class of the pattern parameters of a layout sketch.
const PATTERN_2D_CLASS: &str = "CLSID_JPattern2d";
/// The classes of a layout sketch's profile.
const PROFILE_CLASS: &str = "CLSID_JProfile";
/// Bytes of one item of a pattern's copy list (§9.5) besides the identifier's
/// decimal digits: the identifier, the byte `0x10` and a zero byte.
const LIST_ITEM_FIXED: usize = 6;
/// Bytes after the copy list.
const LIST_TRAILER_LEN: usize = 12;
/// Bytes between the list count and the first item.
const LIST_GAP_LEN: usize = 1;
/// The byte after each identifier in a copy list.
const LIST_ITEM_MARK: u8 = 0x10;

fn note(message: String) -> LossNote {
    SolidEdgeLossCode::AssemblyPatternUnread.note(message)
}

/// The items of the copy list that starts with the count at `at`, when they
/// end where the trailer begins.
fn copy_list_at(bytes: &[u8], at: usize) -> Option<Vec<u32>> {
    let items = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    let end = bytes.len().checked_sub(LIST_TRAILER_LEN)?;
    let mut next = at.checked_add(4)?.checked_add(LIST_GAP_LEN)?;
    // An item takes at least 7 bytes.
    if items == 0 || items > end.checked_sub(next)? / (LIST_ITEM_FIXED + 1) {
        return None;
    }
    let mut ids = Vec::new();
    for _ in 0..items {
        let id = View::u32_le_at(bytes, next)?;
        let text = id.to_string();
        let mark = next.checked_add(4)?;
        let text_at = mark.checked_add(1)?;
        let text_end = text_at.checked_add(text.len())?;
        if *bytes.get(mark)? != LIST_ITEM_MARK
            || bytes.get(text_at..text_end)? != text.as_bytes()
            || *bytes.get(text_end)? != 0
        {
            return None;
        }
        ids.push(id);
        next = text_end.checked_add(1)?;
    }
    (next == end).then_some(ids)
}

/// The identifiers of the copies a pattern object lists, in stored order. The
/// count is the one position from which the items run to the trailer.
fn copy_list(bytes: &[u8]) -> Option<Vec<u32>> {
    let last = bytes
        .len()
        .checked_sub(LIST_TRAILER_LEN + LIST_GAP_LEN + 4 + LIST_ITEM_FIXED + 1)?;
    let mut found = None;
    for at in 0..=last {
        if let Some(ids) = copy_list_at(bytes, at) {
            if found.is_some() {
                return None;
            }
            found = Some(ids);
        }
    }
    found
}

fn seed_selection(seed: &OccurrenceId) -> Result<OrderedSelection, String> {
    let pick = Picked {
        target: PickTarget::Component,
        context: Some(PickContext::Occurrence(OccurrencePath(vec![PathStep {
            occurrence: StepRef::Occurrence(seed.clone()),
            configuration: None,
        }]))),
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    };
    Ok(Selection {
        at: Anchor::Input,
        picks: Picks::Named(
            Members::new(vec![pick]).map_err(|_| "the seed selection is invalid".to_owned())?,
        ),
        native: None,
    })
}

/// The profile of the layout sketch that holds the pattern-2d object `id`.
fn layout_profile(cluster: &Cluster<'_>, types: &TypeTable, id: u32) -> Option<u32> {
    cluster
        .objects()
        .filter(|object| types.class_name(object.form, object.flags) == Some(PROFILE_CLASS))
        .map(|object| object.id)
        .find(|&profile| {
            profile_members(cluster, types, profile).is_some_and(|members| members.contains(&id))
        })
}

/// The offset `E` of the parameters of a layout's pattern-2d object.
fn parameter_base(object: &Object<'_>) -> Result<usize, String> {
    object
        .bytes
        .windows(PARAMETER_ANCHOR.len())
        .position(|window| window == PARAMETER_ANCHOR)
        .and_then(|at| at.checked_add(PARAMETER_ANCHOR.len()))
        .and_then(|end| end.checked_add(PARAMETER_BASE_AFTER_ANCHOR))
        .ok_or_else(|| "the pattern object holds no parameter anchor".to_owned())
}

fn word(object: &Object<'_>, at: usize, what: &str) -> Result<u32, String> {
    View::u32_le_at(object.bytes, at)
        .ok_or_else(|| format!("the object is too short for the {what}"))
}

fn native_key(name: &str) -> Result<CodecName, String> {
    CodecName::new(&cadmpeg_ir::codec_id!("solidedge"), name)
        .ok_or_else(|| format!("the option name {name} is invalid"))
}

/// The layout of the pattern object `object` (§9.5).
fn pattern_layout(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
) -> Result<PatternLayout, String> {
    let parameters = word(object, asm_pattern::LAYOUT, "layout")?;
    let parameters = cluster
        .object(parameters)
        .filter(|found| types.class_name(found.form, found.flags) == Some(PATTERN_2D_CLASS))
        .ok_or_else(|| format!("the layout object {parameters:#x} is not a {PATTERN_2D_CLASS}"))?;
    let profile = layout_profile(cluster, types, parameters.id)
        .ok_or_else(|| "no layout profile holds the pattern parameters".to_owned())?;
    let frame = profile_frame(cluster, types, profile)
        .ok_or_else(|| "the layout profile frame does not read".to_owned())?;
    let SketchLayout { layout, .. } = sketch_layout(
        cluster,
        types,
        &frame,
        &parameters,
        parameter_base(&parameters)?,
    )?;
    Ok(layout)
}

/// The component pattern of the pattern object `object`.
fn read_pattern(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    occurrences: &BTreeMap<u32, OccurrenceId>,
    losses: &mut Vec<LossNote>,
) -> Result<Feature, String> {
    let seed_object = word(object, asm_pattern::SEED, "seed")?;
    let seed = occurrences
        .get(&seed_object)
        .ok_or_else(|| format!("the seed {seed_object:#x} is not a stated occurrence"))?;
    let layout = pattern_layout(cluster, types, object)?;
    let circular = matches!(layout, PatternLayout::Circular(_));
    let mut native_options = NativeOptions::new();
    let layout = if let PatternLayout::Circular(circle) = layout {
        let Spacing::Pitch {
            count: Dimension::Literal(count),
            pitch: Dimension::Literal(step),
        } = circle.angular.spacing
        else {
            return Err("the circular layout states no literal count and step".to_owned());
        };
        native_options.insert(
            native_key("count")?,
            NativeOption::Integer(Dimension::Literal(i64::from(count))),
        );
        native_options.insert(
            native_key("step")?,
            NativeOption::Angle(Dimension::Literal(step)),
        );
        PatternLayout::Native(
            NonBlankString::new("circular".to_owned())
                .ok_or_else(|| "the layout name is blank".to_owned())?,
        )
    } else {
        layout
    };
    let listed = copy_list(object.bytes).ok_or_else(|| "the copy list does not read".to_owned())?;
    let mut instances = Vec::new();
    let mut indices = BTreeSet::new();
    for id in listed {
        let copy = cluster
            .object(id)
            .filter(|found| types.class_name(found.form, found.flags) == Some(COPY_CLASS))
            .ok_or_else(|| format!("the copy {id:#x} is not a {COPY_CLASS}"))?;
        let reference = word(&copy, asm_pattern_occurrence::REFERENCE, "reference")?;
        let first = word(&copy, asm_pattern_occurrence::FIRST_INDEX, "first index")?;
        let second = word(&copy, asm_pattern_occurrence::SECOND_INDEX, "second index")?;
        let mut coordinates =
            vec![i32::try_from(first).map_err(|_| "the first index is too large".to_owned())?];
        if !circular {
            coordinates.push(
                i32::try_from(second).map_err(|_| "the second index is too large".to_owned())?,
            );
        }
        if coordinates.iter().all(|&coordinate| coordinate == 0)
            || !indices.insert(coordinates.clone())
        {
            losses.push(note(format!(
                "the copy {id:#x} repeats instance index {coordinates:?}; it is left out of the \
                 pattern"
            )));
            continue;
        }
        let Some(occurrence) = occurrences.get(&reference) else {
            losses.push(note(format!(
                "the copy {id:#x} names occurrence {reference:#x}, which is not stated; the copy \
                 is left out of the pattern"
            )));
            continue;
        };
        instances.push(ComponentInstance {
            index: InstanceIndex(coordinates),
            occurrences: vec![occurrence.clone()],
        });
    }
    Ok(Feature {
        id: feature_id(object.id),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation: FeatureOperation::ComponentPattern(Box::new(ComponentPattern {
                seeds: seed_selection(seed)?,
                layout,
                instances,
                native_options,
                source_settings: NativeOptions::new(),
            })),
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    })
}

/// The role byte of a copy's reference (§9.2).
const COPY_ROLE: u8 = 0x40;
/// The class of an occurrence.
const REFERENCE_CLASS: &str = "CLSID_JAsmReference";

/// The reference objects that are not occurrences (§9.5): the references with
/// the copy role that no copy object, the first of its index in its pattern,
/// names.
pub(super) fn stale_references(cluster: &Cluster<'_>, types: &TypeTable) -> BTreeSet<u32> {
    let mut live = BTreeSet::new();
    for pattern in cluster
        .objects()
        .filter(|object| types.class_name(object.form, object.flags) == Some(PATTERN_CLASS))
    {
        let Some(list) = copy_list(pattern.bytes) else {
            continue;
        };
        // A circular pattern keys its copies by the first index alone, as
        // `read_pattern` does; a pattern whose layout does not read keys them
        // by both.
        let circular = matches!(
            pattern_layout(cluster, types, &pattern),
            Ok(PatternLayout::Circular(_))
        );
        let mut seen = BTreeSet::new();
        for id in list {
            let Some(copy) = cluster
                .object(id)
                .filter(|found| types.class_name(found.form, found.flags) == Some(COPY_CLASS))
            else {
                continue;
            };
            let (Ok(reference), Ok(first), Ok(second)) = (
                word(&copy, asm_pattern_occurrence::REFERENCE, "reference"),
                word(&copy, asm_pattern_occurrence::FIRST_INDEX, "first index"),
                word(&copy, asm_pattern_occurrence::SECOND_INDEX, "second index"),
            ) else {
                continue;
            };
            let key = (first, if circular { 0 } else { second });
            if (first, second) != (0, 0) && seen.insert(key) {
                live.insert(reference);
            }
        }
    }
    cluster
        .objects()
        .filter(|object| {
            types.class_name(object.form, object.flags) == Some(REFERENCE_CLASS)
                && object.bytes.get(assembly_reference::ROLE) == Some(&COPY_ROLE)
                && !live.contains(&object.id)
        })
        .map(|object| object.id)
        .collect()
}

/// State the patterns of the assembly on `root`. `occurrences` maps the
/// object of each stated occurrence to its identity.
pub(super) fn project(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    occurrences: &BTreeMap<u32, OccurrenceId>,
    root: &ProductDefinitionId,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) {
    let mut features = Vec::new();
    for object in cluster
        .objects()
        .filter(|object| types.class_name(object.form, object.flags) == Some(PATTERN_CLASS))
    {
        match read_pattern(cluster, types, &object, occurrences, losses) {
            Ok(feature) => features.push(feature),
            Err(reason) => losses.push(note(format!(
                "pattern object {:#x} does not read: {reason}; it is not transferred",
                object.id
            ))),
        }
    }
    if features.is_empty() {
        return;
    }
    if ir
        .design
        .scopes
        .iter()
        .any(|scope| scope.definition == *root)
    {
        losses.push(note(
            "the document already states a design scope; its patterns are not transferred".into(),
        ));
        return;
    }
    let id = container_id();
    ir.design.containers.push(Container::Ordered {
        id: id.clone(),
        kind: OrderedKind::Timeline,
        members: features
            .iter()
            .map(|feature| Member::Feature(feature.id.clone()))
            .collect(),
        folders: Vec::new(),
        rollback: None,
        placement: None,
    });
    ir.design.scopes.push(DesignScope {
        definition: root.clone(),
        roots: vec![id],
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::new(),
    });
    ir.design.features.extend(features);
}

#[cfg(test)]
mod tests;
