// SPDX-License-Identifier: Apache-2.0
//! Face operands in the stored input revision namespace.
//!
//! A face item names native face selectors by `source_index`. A selector
//! stores its generator features and a key. The pair of the selector source
//! index and the key joins the selector to a matching attribute that the
//! construction-history archive attaches to one face revision.

use std::collections::{HashMap, HashSet};

use cadmpeg_asm::sab::{Record, Token};
use cadmpeg_core::{
    decode::{unique_index, DecodeContext, View},
    CodecError,
};
use cadmpeg_ir::{
    features::{
        DistinctMembers, FaceSelection, Feature, FeatureDefinition, FeatureInputTopology,
        FeatureOperation,
    },
    ids::{FeatureInputTopologyId, HistoricalFaceId},
};

use super::edge_selection::{generated_family, matched_qualified, source_names, values};
use super::{
    PmDcFeature, PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily,
};
use crate::pmdc::{state_content_header, u32_list_with_context, Cursor, PmDcReference};

/// Type of a face naming selector record.
pub(super) const SELECTOR_TYPE: [u8; 16] = [
    0x2c, 0x81, 0xaa, 0x03, 0xd2, 0x11, 0xff, 0x26, 0x00, 0x08, 0x1f, 0xbb, 0x32, 0xa3, 0xdc, 0x09,
];

/// Type of the surface-body wrapper that a selector names.
pub(super) const BODY_TYPE: [u8; 16] = crate::pmdc::inventor_id(0x9087_4d47);

/// Type of the context record that a selector names.
const CONTEXT_TYPE: [u8; 16] = [
    0x42, 0x20, 0xe9, 0xcc, 0xd1, 0x11, 0xc5, 0x71, 0, 8, 0xa7, 0xba, 0x32, 0xa3, 0xdc, 9,
];

/// Qualifier of the matching attribute that identifies a selected face.
const FACE_QUALIFIER: i64 = 1;

#[derive(Debug)]
pub(super) struct FaceSelector {
    pub(super) source: u32,
    generators: Vec<u32>,
    key: u32,
    /// The `topology` and `naming_form` words, which are equal.
    body_source: u32,
    body: PmDcReference,
    item: PmDcReference,
    context: PmDcReference,
}

/// A face revision that a selector selects.
#[derive(Debug)]
pub(in crate::feature) struct SelectedFace {
    /// Entity index of the matched face revision.
    pub(super) owner: i64,
    /// Record ordinal of the face item that lists the selector.
    pub(super) item: u32,
    /// The pair has more than one matching attribute on the owner.
    pub(super) repeated_attribute: bool,
    /// A generated-name family of the owner shares no feature source index
    /// with the generator array, and another family of the owner does.
    pub(super) unshared_family: bool,
}

/// Parses a face selector, or `None` for another selector form.
pub(super) fn parse(
    ctx: &DecodeContext<'_>,
    payload: View<'_>,
    version: u8,
) -> Result<Option<FaceSelector>, CodecError> {
    let mut cursor = Cursor::new(payload);
    let header = state_content_header(&mut cursor, version)?;
    if cursor.i32("face selector state")? != 0
        || cursor.u8("face selector enabled")? != 1
        || cursor.u8("face selector form")? != 5
    {
        return Ok(None);
    }
    let body = cursor.reference("face selector body")?;
    let body_source = cursor.u32("face selector topology")?;
    if (super::edge_selection::selector_has_qualifier(version)
        && cursor.u32("face selector qualifier")? != 0)
        || cursor.u32("face selector naming form")? != body_source
    {
        return Ok(None);
    }
    let generators = u32_list_with_context(ctx, &mut cursor, 8, "face selector generators")?;
    let secondary = u32_list_with_context(ctx, &mut cursor, 8, "face selector secondary names")?;
    if generators.values().is_empty()
        || generators.values().contains(&0)
        || secondary.values().contains(&0)
        || cursor.u8("face selector suffix form")? != 0
    {
        return Ok(None);
    }
    let context = cursor.reference("face selector context")?;
    let key = cursor.u32("face selector key")?;
    let count = cursor.u32("face selector entity count")?;
    // Each entity is eight bytes and the item reference follows them.
    let needed = usize::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(8))
        .and_then(|bytes| bytes.checked_add(4));
    if needed != Some(cursor.remaining()) || key == 0 {
        return Ok(None);
    }
    for _ in 0..count {
        if cursor.reference("face selector entity context")? != context
            || cursor.u32("face selector entity")? == 0
        {
            return Ok(None);
        }
    }
    let item = cursor.reference("face selector item")?;
    cursor.finish("face selector")?;
    let mut names = ctx.alloc_filled(
        generators.values().len(),
        0,
        "retain Inventor face selector generators",
    )?;
    names.copy_from_slice(generators.values());
    Ok(Some(FaceSelector {
        source: header.source_index,
        generators: names,
        key,
        body_source,
        body,
        item,
        context,
    }))
}

/// Whether the selector names a surface body, a face item, and a context
/// through qualified references to records of those types, and its
/// `topology` word equals the `source_index` of the body record.
pub(super) fn links(
    selector: &FaceSelector,
    type_at: impl Fn(u32) -> Option<[u8; 16]>,
    body_source_at: impl Fn(u32) -> Option<u32>,
) -> bool {
    let target = |reference: PmDcReference, expected| {
        reference.qualified() && reference.record_ordinal().and_then(&type_at) == Some(expected)
    };
    target(selector.body, BODY_TYPE)
        && selector.body.record_ordinal().and_then(&body_source_at) == Some(selector.body_source)
        && target(selector.item, super::FACE_ITEM_TYPE)
        && target(selector.context, CONTEXT_TYPE)
}

fn is_face(record: &Record) -> bool {
    record.head() == "face"
        || (record.name == "End-of-ASM-History-Section"
            && matches!(record.tokens.first(), Some(Token::Ident(name)) if name == "face"))
}

/// Selects the face revision that each selector names.
///
/// A selector selects a face when its source index and key have one matched
/// owner, the owner is a face revision, the matching attribute carries the
/// face qualifier, and a generated-name family of the owner, if it has any,
/// includes a generator of the selector. A pair with several matched owners,
/// or with an owner of another kind or qualifier, selects nothing. The result
/// states when the pair repeats its attribute on the owner, and when another
/// family of the owner shares no generator.
pub(super) fn selected(
    records: &[Record],
    selectors: &[FaceSelector],
    schema: u32,
) -> HashMap<u32, SelectedFace> {
    // The generated names of a schema-43 owner have no defined grammar, so the
    // generator check cannot run.
    if schema == 43 {
        return HashMap::new();
    }
    let by_index = unique_index(
        records
            .iter()
            .filter_map(|record| Some((i64::try_from(record.index).ok()?, record))),
    );
    let mut matches = HashMap::<(u32, u32), HashMap<(i64, i64), usize>>::new();
    let mut generated = HashMap::<i64, HashMap<&str, HashSet<u32>>>::new();
    for record in records {
        if let Some((source, key, owner, qualifier)) = matched_qualified(record, schema) {
            *matches
                .entry((source, key))
                .or_default()
                .entry((owner, qualifier))
                .or_default() += 1;
        }
        let fields = values(record);
        if let (Some((owner, sources)), Some(family)) = (
            source_names(record, &fields, schema),
            generated_family(record, &fields),
        ) {
            generated
                .entry(owner)
                .or_default()
                .entry(family)
                .or_default()
                .extend(sources);
        }
    }
    let by_source = unique_index(selectors.iter().map(|selector| (selector.source, selector)));
    let mut faces = Vec::new();
    for selector in by_source.values().flatten() {
        let Some(owners) = matches.get(&(selector.source, selector.key)) else {
            continue;
        };
        let mut distinct = owners.iter();
        let (Some((&(owner, FACE_QUALIFIER), &attributes)), None) =
            (distinct.next(), distinct.next())
        else {
            continue;
        };
        let owner = &owner;
        let Some(Some(revision)) = by_index.get(owner) else {
            continue;
        };
        if !is_face(revision) {
            continue;
        }
        let families = generated.get(owner);
        let shared = families.map_or(0, |families| {
            families
                .values()
                .filter(|sources| {
                    selector
                        .generators
                        .iter()
                        .any(|generator| sources.contains(generator))
                })
                .count()
        });
        let family_count = families.map_or(0, HashMap::len);
        if family_count > 0 && shared == 0 {
            continue;
        }
        let Some(item) = selector.item.record_ordinal() else {
            continue;
        };
        faces.push((
            selector.source,
            SelectedFace {
                owner: *owner,
                item,
                repeated_attribute: attributes > 1,
                unshared_family: shared < family_count,
            },
        ));
    }
    unique_index(faces)
        .into_iter()
        .filter_map(|(source, face)| Some((source, face?)))
        .collect()
}

fn state_id(source: &PmDcFeature) -> Result<FeatureInputTopologyId, CodecError> {
    Ok(FeatureInputTopologyId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-input"),
        source.identity.key()?,
    ))
}

fn face_id(token: &str, source: u32) -> Result<HistoricalFaceId, CodecError> {
    HistoricalFaceId::mint(format!("inventor:input-face#{token}-{source}"))
        .map_err(CodecError::malformed)
}

/// The `(face item ordinal, selector source index)` operands of a face
/// collection in source order, or `None` for another form or a collection
/// that selects no face.
pub(crate) fn selection_sources<'a>(
    collection: &PmDcFeatureProperty,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> Option<Vec<(u32, u32)>> {
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::FaceCollection,
        items,
    } = &collection.kind
    else {
        return None;
    };
    if items.references().is_empty() {
        return None;
    }
    let mut sources = Vec::new();
    for reference in items.references() {
        let item = property(*reference)?;
        let PmDcFeaturePropertyKind::FaceItem {
            index_references, ..
        } = &item.kind
        else {
            return None;
        };
        // An item with an empty index list selects no face.
        for source in index_references.values() {
            if sources.iter().any(|(_, previous)| previous == source) {
                return None;
            }
            sources.push((item.identity.record_ordinal, *source));
        }
    }
    (!sources.is_empty()).then_some(sources)
}

/// Source tag of a face-motion feature.
const SOURCE_TAG: &str = "move_face";

fn selections_mut(definition: &mut FeatureDefinition) -> Option<&mut FaceSelection> {
    match definition {
        FeatureDefinition::Operation(FeatureOperation::MoveFace { faces, .. }) => Some(faces),
        _ => None,
    }
}

fn selection(feature: &Feature) -> Option<&FaceSelection> {
    if feature.source_tag.as_deref() != Some(SOURCE_TAG) {
        return None;
    }
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::MoveFace { faces, .. }) => Some(faces),
        _ => None,
    }
}

/// Replaces the native face selection of each Move Face feature whose face
/// operands join stored face revisions, and returns the input states that
/// hold those faces.
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    features: &mut [Feature],
    inventory: &crate::feature::FeatureInventory,
) -> Result<Vec<FeatureInputTopology>, CodecError> {
    if inventory.edge_inputs.faces.is_empty() {
        return Ok(Vec::new());
    }
    let sources = unique_index(
        inventory
            .features
            .iter()
            .map(|source| (source.id(), source)),
    );
    let properties = unique_index(
        inventory
            .properties
            .iter()
            .map(|property| (property.id(), property)),
    );
    let by_ordinal = unique_index(inventory.properties.iter().map(|property| {
        (
            (
                property.identity.segment_token.as_str(),
                property.identity.record_ordinal,
            ),
            property,
        )
    }));
    let mut states = Vec::new();
    for feature in features {
        let Some(FaceSelection::Native(native)) = selection(feature) else {
            continue;
        };
        let native = native.clone();
        let Some(Some(source)) = feature
            .native_ref
            .as_deref()
            .and_then(|native| sources.get(native))
        else {
            continue;
        };
        let token = source.identity.segment_token.as_str();
        let Some(Some(collection)) = properties.get(native.as_str()) else {
            continue;
        };
        let property = |reference: PmDcReference| {
            by_ordinal
                .get(&(token, reference.record_ordinal()?))?
                .as_ref()
                .copied()
        };
        let Some(operands) = selection_sources(collection, property) else {
            continue;
        };
        let mut faces = Vec::new();
        let mut unresolved = Vec::new();
        let mut owners = HashSet::new();
        let mut conflict = false;
        for (item, selector) in operands {
            let selected = inventory
                .edge_inputs
                .faces
                .get(&(token.to_owned(), selector))
                .filter(|face| face.item == item);
            match selected {
                Some(face) => {
                    conflict |= !owners.insert(face.owner);
                    faces.push(face_id(token, selector)?);
                }
                None => {
                    if let Some(Some(item)) = by_ordinal.get(&(token, item)) {
                        let id = item.id();
                        if !unresolved.contains(&id) {
                            unresolved.push(id);
                        }
                    }
                }
            }
        }
        if conflict || faces.is_empty() {
            continue;
        }
        let state = state_id(source)?;
        let replacement = if unresolved.is_empty() {
            FaceSelection::historical(state.clone(), faces.clone(), native.clone())
        } else {
            FaceSelection::historical_partial(state.clone(), faces.clone(), unresolved, native)
        }
        .map_err(CodecError::malformed)?;
        ctx.charge_entities(
            1 + cadmpeg_core::decode::u64_from_index(faces.len()),
            "project Inventor historical faces",
        )?;
        feature.evaluation.edit(|definition, _| {
            if let Some(selection) = selections_mut(definition) {
                *selection = replacement;
            }
        });
        states.push(FeatureInputTopology {
            id: state,
            input_of: feature.id.clone(),
            bodies: DistinctMembers::default(),
            faces: faces.try_into().map_err(CodecError::malformed)?,
            edges: DistinctMembers::default(),
            vertices: DistinctMembers::default(),
            edge_geometry: Vec::new(),
            native_ref: Some(source.id()),
        });
    }
    Ok(states)
}

/// Whether a historical face selection of a feature re-derives from its
/// native face collection and its input state.
pub(crate) fn valid<'a>(
    feature: &Feature,
    source: &PmDcFeature,
    states: &[FeatureInputTopology],
    property: impl Fn(&str) -> Option<&'a PmDcFeatureProperty>,
    property_at: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> bool {
    let (state, faces, native, unresolved) = match selection(feature) {
        Some(FaceSelection::Historical {
            state,
            faces,
            native,
        }) => (state, faces.as_slice(), native.as_str(), None),
        Some(FaceSelection::HistoricalPartial {
            state,
            faces,
            unresolved,
            native,
        }) => (
            state,
            faces.as_slice(),
            native.as_str(),
            Some(unresolved.as_slice()),
        ),
        _ => return true,
    };
    let token = source.identity.segment_token.as_str();
    let Some(collection) = property(native) else {
        return false;
    };
    if collection.identity.segment_token != source.identity.segment_token {
        return false;
    }
    let Some(operands) = selection_sources(collection, &property_at) else {
        return false;
    };
    let Ok(expected_state) = state_id(source) else {
        return false;
    };
    let Ok(expected) = operands
        .iter()
        .map(|(_, selector)| face_id(token, *selector))
        .collect::<Result<Vec<_>, _>>()
    else {
        return false;
    };
    let mut rows = states
        .iter()
        .filter(|row| row.id == *state && row.input_of == feature.id);
    let Some(row) = rows.next() else {
        return false;
    };
    let faces_match = match unresolved {
        None => faces == expected,
        Some(unresolved) => {
            let mut remaining = expected.iter();
            let in_order = faces
                .iter()
                .all(|face| remaining.by_ref().any(|candidate| candidate == face));
            let items = operands
                .iter()
                .filter_map(|(item, _)| {
                    by_item(&property_at, collection, *item).map(PmDcFeatureProperty::id)
                })
                .collect::<HashSet<_>>();
            in_order
                && faces.len() < expected.len()
                && unresolved.iter().all(|name| items.contains(name))
        }
    };
    *state == expected_state
        && faces_match
        && rows.next().is_none()
        && row.native_ref.as_deref() == Some(source.id().as_str())
        && row.edges.is_empty()
        && row.edge_geometry.is_empty()
        && faces.iter().all(|face| row.faces.contains(face))
}

fn by_item<'a>(
    property_at: &impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    collection: &PmDcFeatureProperty,
    ordinal: u32,
) -> Option<&'a PmDcFeatureProperty> {
    let PmDcFeaturePropertyKind::References { items, .. } = &collection.kind else {
        return None;
    };
    items
        .references()
        .iter()
        .filter_map(|reference| property_at(*reference))
        .find(|item| item.identity.record_ordinal == ordinal)
}

/// One note counting Move Face features that retain an unresolved face
/// operand.
pub(crate) fn native_losses(features: &[Feature]) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let count = features
        .iter()
        .filter(|feature| {
            matches!(
                selection(feature),
                Some(FaceSelection::Native(_) | FaceSelection::HistoricalPartial { .. })
            )
        })
        .count();
    if count == 0 {
        Vec::new()
    } else {
        vec![crate::loss::InventorLossCode::FeatureMoveFaceSelectionNative.note(format!("Retained {count} Move Face feature(s) with face operands that do not join exactly one stored face revision."))]
    }
}

/// Notes counting the Move Face features whose selected faces rest on a reading:
/// that a matched revision is the input face of the feature, of a repeated
/// matching attribute, or of an unshared generated-name family.
pub(crate) fn assumption_losses(
    features: &[Feature],
    inventory: &crate::feature::FeatureInventory,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let ids = |flag: fn(&SelectedFace) -> bool| {
        inventory
            .edge_inputs
            .faces
            .iter()
            .filter(|(_, face)| flag(face))
            .filter_map(|((token, source), _)| face_id(token, *source).ok())
            .collect::<Vec<_>>()
    };
    let count = |ids: &[HistoricalFaceId]| {
        features
            .iter()
            .filter(|feature| {
                let faces = match selection(feature) {
                    Some(FaceSelection::Historical { faces, .. }) => faces.as_slice(),
                    Some(FaceSelection::HistoricalPartial { faces, .. }) => faces.as_slice(),
                    _ => return false,
                };
                faces.iter().any(|face| ids.contains(face))
            })
            .count()
    };
    let repeated = count(&ids(|face| face.repeated_attribute));
    let unshared = count(&ids(|face| face.unshared_family));
    let selecting = features
        .iter()
        .filter(|feature| {
            matches!(
                selection(feature),
                Some(FaceSelection::Historical { .. } | FaceSelection::HistoricalPartial { .. })
            )
        })
        .count();
    let mut notes = Vec::new();
    if selecting > 0 {
        notes.push(crate::loss::InventorLossCode::FeatureFaceJoinInputRevisionAssumed.note(format!("{selecting} Move Face feature(s) keep historical face selections; the join does not establish the stored body state of a matched revision, so the selection assumes that the revision represents the input face of the feature.")));
    }
    if repeated > 0 {
        notes.push(crate::loss::InventorLossCode::FeatureFaceJoinRepeatedAttribute.note(format!("{repeated} Move Face feature(s) select a face through a matching attribute that repeats on its owner; the selection assumes that the copies state one selection.")));
    }
    if unshared > 0 {
        notes.push(crate::loss::InventorLossCode::FeatureFaceJoinUnsharedFamily.note(format!("{unshared} Move Face feature(s) select a face with a generated-name family that shares no feature with the selector; the selection assumes that another family of the face identifies it.")));
    }
    notes
}

#[cfg(test)]
mod tests;
