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
        DistinctMembers, FaceMotion, FaceSelection, Feature, FeatureDefinition,
        FeatureInputTopology, FeatureOperation,
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

/// Type of a direct face selector record.
pub(super) const DIRECT_SELECTOR_TYPE: [u8; 16] = crate::pmdc::inventor_id(0x9087_4d55);

/// The largest generator array that a direct face selector stores.
const DIRECT_GENERATOR_LIMIT: usize = 4;

/// Type of the surface-body wrapper that a selector names.
pub(super) const BODY_TYPE: [u8; 16] = crate::pmdc::inventor_id(0x9087_4d47);

/// Type of the context record that a selector names.
const CONTEXT_TYPE: [u8; 16] = [
    0x42, 0x20, 0xe9, 0xcc, 0xd1, 0x11, 0xc5, 0x71, 0, 8, 0xa7, 0xba, 0x32, 0xa3, 0xdc, 9,
];

/// Qualifier of the matching attribute that identifies a selected face.
const FACE_QUALIFIER: i64 = 1;

/// Stored state value for the feature-tag operand join.
pub(crate) const NAMING_STATE_MODIFIED: i32 = 2;

#[derive(Debug)]
pub(crate) struct FaceSelector {
    pub(super) source: u32,
    /// The stored `state` word: 0 or 2 for the naming form, -1 for the
    /// direct form.
    state: i32,
    generators: Vec<u32>,
    key: u32,
    /// The `topology` and `naming_form` words, which are equal.
    body_source: u32,
    body: PmDcReference,
    item: PmDcReference,
    context: PmDcReference,
    /// The selector has the direct form.
    direct: bool,
}

impl FaceSelector {
    pub(crate) const fn source_index(&self) -> u32 {
        self.source
    }

    pub(crate) const fn key(&self) -> u32 {
        self.key
    }

    /// The `topology` word.
    pub(crate) const fn topology(&self) -> u32 {
        self.body_source
    }

    pub(crate) const fn body(&self) -> PmDcReference {
        self.body
    }

    pub(crate) const fn item(&self) -> PmDcReference {
        self.item
    }

    pub(crate) const fn context(&self) -> PmDcReference {
        self.context
    }
}

/// The record form of a face selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::feature) enum SelectorForm {
    Naming,
    Direct,
}

/// A face revision that a selector selects.
#[derive(Debug)]
pub(in crate::feature) struct SelectedFace {
    pub(super) schema: u32,
    /// Entity index of the matched face revision.
    pub(super) owner: i64,
    /// Entity indexes of every matched owner, ascending.
    pub(super) owners: Vec<i64>,
    /// Length of the generator array of the selector.
    pub(super) generators: usize,
    /// Record ordinal of the face item that lists the selector.
    pub(super) item: u32,
    /// The pair has more than one matching attribute on the owner.
    pub(super) repeated_attribute: bool,
    /// A generated-name family of the owner shares no feature source index
    /// with the generator array, and another family of the owner does.
    pub(super) unshared_family: bool,
    /// The pair has several matched owners with identical surface geometry
    /// and sense, and `owner` is the one with the lowest entity index.
    pub(super) identical_owners: bool,
    /// The selector form. A face selection of Move Face, Shell, Thread, and
    /// Thicken does not join the direct form.
    pub(super) form: SelectorForm,
    /// Surface class of the owner.
    pub(super) surface: Surface,
    /// The stored `state` word of the selector.
    pub(super) state: i32,
    /// Source indexes that the `NMx_Brep_Feature_tag` and
    /// `NMx_tweak_reblend_tag` attributes of the owner name, ascending.
    pub(super) feature_sources: Vec<u32>,
}

/// Surface class of a selected face revision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::feature) enum Surface {
    /// The face stores no surface this reader classifies.
    Other,
    Plane,
    /// A cone with unit axis ratio within `EPS_CONE_RATIO`, or a non-finite
    /// ratio that `assumption_losses` names. `sine` is the sine
    /// of the half-angle, and within `EPS_CYLINDER_SINE` of zero for a cylinder;
    /// `radius` is the length of the stored major axis, in the length unit of
    /// the archive.
    Cone {
        ratio: f64,
        sine: f64,
        radius: f64,
    },
}

/// Largest `sine` magnitude of a cone that counts as a cylinder.
const EPS_CYLINDER_SINE: f64 = 1e-9;
/// Largest difference between a cylinder diameter and a stated thread
/// diameter, in millimetres.
const EPS_THREAD_DIAMETER_MM: f64 = 1e-6;
/// Millimetres per unit of the archive length.
const MM_PER_ARCHIVE_LENGTH: f64 = 10.0;
/// Largest difference of the axis ratio of a cone from one.
const EPS_CONE_RATIO: f64 = 1e-9;

/// Parses a feature-operand face selector, or `None` for another selector
/// form.
pub(super) fn parse(
    ctx: &DecodeContext<'_>,
    payload: View<'_>,
    version: u8,
) -> Result<Option<FaceSelector>, CodecError> {
    parse_with_states(ctx, payload, version, &[0, NAMING_STATE_MODIFIED])
}

/// Parses a face selector whose state is one of `states`, or `None` for
/// another selector form.
pub(crate) fn parse_with_states(
    ctx: &DecodeContext<'_>,
    payload: View<'_>,
    version: u8,
    states: &[i32],
) -> Result<Option<FaceSelector>, CodecError> {
    let mut cursor = Cursor::new(payload);
    let header = state_content_header(&mut cursor, version)?;
    let state = cursor.i32("face selector state")?;
    if !states.contains(&state)
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
        state,
        generators: names,
        key,
        body_source,
        body,
        item,
        context,
        direct: false,
    }))
}

/// Parses a direct face selector, or `None` for another selector form. The
/// direct form has the field order of the direct edge selector and ends with a
/// face-item reference, a context reference, and the key.
pub(super) fn parse_direct(
    ctx: &DecodeContext<'_>,
    payload: View<'_>,
    version: u8,
) -> Result<Option<FaceSelector>, CodecError> {
    let mut cursor = Cursor::new(payload);
    let header = state_content_header(&mut cursor, version)?;
    if cursor.i32("direct face selector state")? != -1
        || cursor.u8("direct face selector enabled")? != 1
        || cursor.u8("direct face selector form")? != 5
    {
        return Ok(None);
    }
    let body = cursor.reference("direct face selector body")?;
    let body_source = cursor.u32("direct face selector topology")?;
    if (super::edge_selection::selector_has_qualifier(version)
        && cursor.u32("direct face selector qualifier")? != 0)
        || cursor.u32("direct face selector naming form")? != body_source
    {
        return Ok(None);
    }
    let generators = u32_list_with_context(ctx, &mut cursor, 8, "direct face selector generators")?;
    let secondary = u32_list_with_context(ctx, &mut cursor, 8, "direct face selector secondary")?;
    let mut distinct = HashSet::new();
    if !(1..=DIRECT_GENERATOR_LIMIT).contains(&generators.values().len())
        || generators.values().contains(&0)
        || secondary.values().contains(&0)
        || !generators
            .values()
            .iter()
            .chain(secondary.values())
            .all(|value| distinct.insert(*value))
        || cursor.u8("direct face selector suffix form")? != 0
    {
        return Ok(None);
    }
    let item = cursor.reference("direct face selector item")?;
    let context = cursor.reference("direct face selector context")?;
    let key = cursor.u32("direct face selector key")?;
    cursor.finish("direct face selector")?;
    let mut names = ctx.alloc_filled(
        generators.values().len(),
        0,
        "retain Inventor face selector generators",
    )?;
    names.copy_from_slice(generators.values());
    Ok(Some(FaceSelector {
        source: header.source_index,
        state: -1,
        generators: names,
        key,
        body_source,
        body,
        item,
        context,
        direct: true,
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

/// Whether the record is a surface of kind `kind`, directly or as a history
/// snapshot.
fn is_surface(record: &Record, kind: &str) -> bool {
    record.head() == kind
        || (record.name == "End-of-ASM-History-Section"
            && matches!(record.tokens.first(), Some(Token::Ident(name)) if name == kind))
}

/// The surface record of a face revision. Field 7 of a face is its surface.
fn surface_record<'a>(
    face: &Record,
    by_index: &HashMap<i64, Option<&'a Record>>,
) -> Option<&'a Record> {
    let Some(Token::Ref(index)) = face.chunk(7) else {
        return None;
    };
    by_index.get(index).copied().flatten()
}

/// Classifies the surface of a face revision. Fields 3 to 5 of a cone are its
/// origin, axis, and major axis, field 6 is the axis ratio, and field 9 is the
/// sine of the half-angle.
fn classify_surface(face: &Record, by_index: &HashMap<i64, Option<&Record>>) -> Surface {
    let Some(surface) = surface_record(face, by_index) else {
        return Surface::Other;
    };
    if is_surface(surface, "plane") {
        return Surface::Plane;
    }
    if !is_surface(surface, "cone") {
        return Surface::Other;
    }
    let (Some(Token::Vector3(major)), Some(Token::Double(ratio)), Some(Token::Double(sine))) =
        (surface.chunk(5), surface.chunk(6), surface.chunk(9))
    else {
        return Surface::Other;
    };
    // A non-finite ratio stays a cone, and `assumption_losses` names it.
    if ratio.is_finite() && (ratio - 1.0).abs() > EPS_CONE_RATIO {
        return Surface::Other;
    }
    let radius = major.iter().map(|axis| axis * axis).sum::<f64>().sqrt();
    if radius.is_finite() && radius > 0.0 && sine.is_finite() {
        Surface::Cone {
            ratio: *ratio,
            sine: *sine,
            radius,
        }
    } else {
        Surface::Other
    }
}

/// Surface record kinds that a face revision can name.
const SURFACE_KINDS: [&str; 5] = ["plane", "cone", "sphere", "torus", "spline"];

/// The surface fields and the sense of a face revision, for the comparison of
/// the owners of one selector pair. `None` when the face stores no surface
/// record or no sense.
fn surface_signature(face: &Record, by_index: &HashMap<i64, Option<&Record>>) -> Option<String> {
    let surface = surface_record(face, by_index)?;
    if !SURFACE_KINDS.iter().any(|kind| is_surface(surface, kind)) {
        return None;
    }
    let sense = face
        .chunk(8)
        .filter(|token| matches!(token, Token::True | Token::False))?;
    let fields = surface
        .chunks()
        .skip(3)
        .chain(std::iter::once(sense))
        .map(|token| format!("{token:?}"))
        .collect::<Vec<_>>();
    (fields.len() > 3).then(|| format!("{}:{}", surface.head(), fields.join(",")))
}

fn is_face(record: &Record) -> bool {
    record.head() == "face"
        || (record.name == "End-of-ASM-History-Section"
            && matches!(record.tokens.first(), Some(Token::Ident(name)) if name == "face"))
}

/// Selects the face revision that each selector names.
///
/// A selector selects a face when every matched owner of its source index and
/// key is a face revision, the matching attributes carry the face qualifier,
/// and a generated-name family of each owner, if it has any, includes a
/// generator of the selector. Several matched owners select the owner with the
/// lowest entity index when their surface records and senses are identical.
/// A pair with several owners of different or unstored surfaces, or with an
/// owner of another kind or qualifier, selects nothing. The result states when
/// the pair repeats its attribute on an owner, when another family of an owner
/// shares no generator, and when several owners were identical.
pub(super) fn selected(
    records: &[Record],
    selectors: &[FaceSelector],
    schema: u32,
) -> HashMap<u32, SelectedFace> {
    if !super::edge_selection::historical::supported_schema(schema) {
        return HashMap::new();
    }
    let by_index = unique_index(
        records
            .iter()
            .filter_map(|record| Some((i64::try_from(record.index).ok()?, record))),
    );
    let mut matches = HashMap::<(u32, u32), HashMap<(i64, i64), usize>>::new();
    let mut generated = HashMap::<i64, HashMap<&str, HashSet<u32>>>::new();
    let mut tagged = HashMap::<i64, HashSet<u32>>::new();
    for record in records {
        if let Some((source, key, owner, qualifier)) = matched_qualified(record, schema) {
            *matches
                .entry((source, key))
                .or_default()
                .entry((owner, qualifier))
                .or_default() += 1;
        }
        let fields = values(record);
        if let Some((owner, sources)) = feature_tag_sources(record, &fields, schema) {
            tagged.entry(owner).or_default().extend(sources);
        }
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
        let mut ids = Vec::new();
        let mut repeated_attribute = false;
        for (&(owner, qualifier), &attributes) in owners {
            if qualifier != FACE_QUALIFIER {
                ids.clear();
                break;
            }
            ids.push(owner);
            repeated_attribute |= attributes > 1;
        }
        ids.sort_unstable();
        let mut revisions = Vec::new();
        for owner in &ids {
            match by_index.get(owner) {
                Some(Some(revision)) if is_face(revision) => revisions.push(*revision),
                _ => break,
            }
        }
        let Some(&first) = revisions.first() else {
            continue;
        };
        if revisions.len() != ids.len() {
            continue;
        }
        let identical_owners = revisions.len() > 1;
        if identical_owners {
            let signature = surface_signature(first, &by_index);
            if signature.is_none()
                || revisions
                    .iter()
                    .any(|revision| surface_signature(revision, &by_index) != signature)
            {
                continue;
            }
        }
        let mut unshared_family = false;
        let mut named = true;
        for owner in &ids {
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
                named = false;
                break;
            }
            unshared_family |= shared < family_count;
        }
        if !named {
            continue;
        }
        let Some(item) = selector.item.record_ordinal() else {
            continue;
        };
        faces.push((
            selector.source,
            SelectedFace {
                schema,
                owner: ids[0],
                owners: ids.clone(),
                generators: selector.generators.len(),
                item,
                repeated_attribute,
                unshared_family,
                identical_owners,
                form: if selector.direct {
                    SelectorForm::Direct
                } else {
                    SelectorForm::Naming
                },
                surface: classify_surface(first, &by_index),
                state: selector.state,
                feature_sources: tagged
                    .get(&ids[0])
                    .map(|sources| {
                        let mut sources = sources.iter().copied().collect::<Vec<_>>();
                        sources.sort_unstable();
                        sources
                    })
                    .unwrap_or_default(),
            },
        ));
    }
    unique_index(faces)
        .into_iter()
        .filter_map(|(source, face)| Some((source, face?)))
        .collect()
}

/// The owner and the feature source indexes of an `NMx_Brep_Feature_tag` or
/// `NMx_tweak_reblend_tag` attribute in a class-chain naming carrier.
/// After the five common fields the attribute stores the schema, a
/// positive pair count, and that many `(feature_source_index, name_value)`
/// pairs of integers.
fn feature_tag_sources(record: &Record, fields: &[&Token], schema: u32) -> Option<(i64, Vec<u32>)> {
    if !super::edge_selection::class_chain_matched_schema(schema) {
        return None;
    }
    let [Token::Ref(_), Token::Long(_), Token::Ref(_), Token::Ref(_), Token::Ref(owner), Token::Long(stored_schema), Token::Long(count), values @ ..] =
        fields
    else {
        return None;
    };
    if !matches!(
        super::edge_selection::legacy_family(record)?,
        "NMx_Brep_Feature_tag" | "NMx_tweak_reblend_tag"
    ) || *stored_schema != i64::from(schema)
    {
        return None;
    }
    let count = usize::try_from(*count).ok().filter(|count| *count > 0)?;
    let pairs = values.get(..count.checked_mul(2)?)?;
    pairs
        .chunks_exact(2)
        .map(|pair| match pair {
            [Token::Long(source), Token::Long(_)] => u32::try_from(*source).ok(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .map(|sources| (*owner, sources))
}

fn state_id(source: &PmDcFeature) -> Result<FeatureInputTopologyId, CodecError> {
    Ok(FeatureInputTopologyId::compose(
        &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-input"),
        source.identity.key()?,
    ))
}

pub(in crate::feature) fn face_id(
    token: &str,
    source: u32,
) -> Result<HistoricalFaceId, CodecError> {
    HistoricalFaceId::mint(format!("inventor:input-face#{token}-{source}"))
        .map_err(CodecError::malformed)
}

/// The face collections that a native face selection names. A Move Face,
/// Shell, or Thread selection is one face collection. A Thicken selection is
/// a selection list whose members each name one face collection.
fn face_collections<'a>(
    collection: &'a PmDcFeatureProperty,
    property: &impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> Option<Vec<&'a PmDcFeatureProperty>> {
    match &collection.kind {
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FaceCollection,
            ..
        } => Some(vec![collection]),
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::SelectionList,
            items,
        } => {
            let mut collections = Vec::new();
            for reference in items.references() {
                let PmDcFeaturePropertyKind::ThickenSelection { faces, .. } =
                    &property(*reference)?.kind
                else {
                    return None;
                };
                collections.push(property(*faces)?);
            }
            (!collections.is_empty()).then_some(collections)
        }
        _ => None,
    }
}

/// The `(face item ordinal, selector source index)` operands of a face
/// selection in source order, or `None` for another form or a selection
/// that selects no face.
pub(crate) fn selection_sources<'a>(
    collection: &'a PmDcFeatureProperty,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> Option<Vec<(u32, u32)>> {
    let mut sources = Vec::new();
    for collection in face_collections(collection, &property)? {
        let PmDcFeaturePropertyKind::References { items, .. } = &collection.kind else {
            return None;
        };
        if items.references().is_empty() {
            return None;
        }
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
    }
    (!sources.is_empty()).then_some(sources)
}

/// Feature kinds whose face operands take the join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Move Face; the face collection is the native selection.
    MoveFace,
    /// Shell; the removed faces are the native selection.
    Shell,
    /// Thread annotation; the single threaded face is the native selection.
    Thread,
    /// Thicken without the face-offset form; the selection list is the native selection.
    Thicken,
    /// Thicken in the face-offset form, a Move Face operation that carries the
    /// Thicken source tag; the selection list is the native selection.
    ThickenOffset,
}

fn selections_mut(definition: &mut FeatureDefinition) -> Option<&mut FaceSelection> {
    match definition {
        FeatureDefinition::Operation(
            FeatureOperation::MoveFace { faces, .. }
            | FeatureOperation::Thicken { faces, .. }
            | FeatureOperation::Shell {
                removed_faces: faces,
                ..
            }
            | FeatureOperation::CosmeticThread { face: faces, .. },
        ) => Some(faces),
        _ => None,
    }
}

/// The kind and face selection of a feature that takes the join. A Thicken
/// feature in the face-offset form is a Move Face operation that carries the
/// Thicken source tag.
fn selection(feature: &Feature) -> Option<(Kind, &FaceSelection)> {
    let tag = feature.source_tag.as_deref();
    match (tag, feature.evaluation.definition()) {
        (
            Some("move_face"),
            FeatureDefinition::Operation(FeatureOperation::MoveFace { faces, .. }),
        ) => Some((Kind::MoveFace, faces)),
        (
            Some("shell"),
            FeatureDefinition::Operation(FeatureOperation::Shell { removed_faces, .. }),
        ) => Some((Kind::Shell, removed_faces)),
        (
            Some("thread"),
            FeatureDefinition::Operation(FeatureOperation::CosmeticThread { face, .. }),
        ) => Some((Kind::Thread, face)),
        (
            Some("thicken"),
            FeatureDefinition::Operation(FeatureOperation::Thicken { faces, .. }),
        ) => Some((Kind::Thicken, faces)),
        (
            Some("thicken"),
            FeatureDefinition::Operation(FeatureOperation::MoveFace {
                faces,
                motion: FaceMotion::Offset { .. },
            }),
        ) => Some((Kind::ThickenOffset, faces)),
        _ => None,
    }
}

/// The stated nominal diameter of a thread annotation, in millimetres.
fn thread_diameter(feature: &Feature) -> Option<f64> {
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::CosmeticThread { diameter, .. }) => {
            diameter.map(cadmpeg_ir::scalar::PositiveLength::get)
        }
        _ => None,
    }
}

/// Whether the surface of a joined face admits a thread annotation. The face
/// is a cylinder or a cone. A cylinder agrees with the stated diameter.
fn thread_surface_admitted(surface: Surface, diameter: Option<f64>) -> bool {
    match surface {
        Surface::Cone { sine, radius, .. } if sine.abs() > EPS_CYLINDER_SINE && radius > 0.0 => {
            true
        }
        Surface::Cone { radius, .. } => diameter.is_none_or(|diameter| {
            (2.0 * radius * MM_PER_ARCHIVE_LENGTH - diameter).abs() <= EPS_THREAD_DIAMETER_MM
        }),
        Surface::Plane | Surface::Other => false,
    }
}

/// Whether a thread face that the tolerances admit differs from the exact form:
/// a unit axis ratio, a zero sine for a cylinder, and the stated diameter.
fn thread_tolerance_used(surface: Surface, diameter: Option<f64>) -> bool {
    let Surface::Cone {
        ratio,
        sine,
        radius,
    } = surface
    else {
        return false;
    };
    let cylinder = sine.abs() <= EPS_CYLINDER_SINE;
    (ratio.is_finite() && ratio != 1.0)
        || (cylinder && sine != 0.0)
        || (cylinder
            && diameter.is_some_and(|diameter| 2.0 * radius * MM_PER_ARCHIVE_LENGTH != diameter))
}

/// Whether a thread face stores a cone axis ratio that is not a finite number.
fn thread_ratio_nonfinite(surface: Surface) -> bool {
    matches!(surface, Surface::Cone { ratio, .. } if !ratio.is_finite())
}

/// Replaces the native face selection of each Move Face, Shell, Thread, and
/// Thicken feature whose face operands join stored face revisions, and
/// returns the input states that hold those faces. Only Move Face keeps a
/// partly joined collection. A Thread annotation joins one face whose surface
/// is a cone or a cylinder of the stated diameter.
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
        let Some((kind, FaceSelection::Native(native))) = selection(feature) else {
            continue;
        };
        let native = native.clone();
        let diameter = thread_diameter(feature);
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
        let admitted_count = match kind {
            Kind::Thread => operands.len() == 1,
            _ => true,
        };
        for (item, selector) in operands {
            let selected = inventory
                .edge_inputs
                .faces
                .get(&(token.to_owned(), selector))
                .filter(|face| face.item == item && face.form == SelectorForm::Naming)
                .filter(|face| state_admitted(kind, face, source.header.source_index));
            match selected {
                Some(face) => {
                    conflict |= !owners.insert(face.owner);
                    conflict |=
                        kind == Kind::Thread && !thread_surface_admitted(face.surface, diameter);
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
        if conflict
            || faces.is_empty()
            || !admitted_count
            || (kind != Kind::MoveFace && !unresolved.is_empty())
        {
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

/// Whether the selector state of a joined face admits the feature kind. A
/// state-0 selector joins every kind. A state-2 selector joins schema-42 Move
/// Face or either Thicken form through one owner with a feature tag for the
/// selecting source and one matching attribute.
fn state_admitted(kind: Kind, face: &SelectedFace, own_source: u32) -> bool {
    match face.state {
        0 => true,
        NAMING_STATE_MODIFIED => {
            (matches!(kind, Kind::Thicken | Kind::ThickenOffset)
                || (kind == Kind::MoveFace && face.schema == 42))
                && face.owners.len() == 1
                && !face.repeated_attribute
                && face.feature_sources.contains(&own_source)
        }
        _ => false,
    }
}

/// Whether a historical face selection of a feature re-derives from its
/// native face collection and its input state.
pub(crate) fn valid<'a>(
    feature: &Feature,
    source: &PmDcFeature,
    states: &[FeatureInputTopology],
    carrier: &crate::native::ActiveCarrierRecord,
    property: impl Fn(&str) -> Option<&'a PmDcFeatureProperty>,
    property_at: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> bool {
    let (kind, state, faces, native, unresolved) = match selection(feature) {
        Some((
            kind,
            FaceSelection::Historical {
                state,
                faces,
                native,
            },
        )) => (kind, state, faces.as_slice(), native.as_str(), None),
        Some((
            kind,
            FaceSelection::HistoricalPartial {
                state,
                faces,
                unresolved,
                native,
            },
        )) => (
            kind,
            state,
            faces.as_slice(),
            native.as_str(),
            Some(unresolved.as_slice()),
        ),
        _ => return true,
    };
    if !matches!(carrier, crate::native::ActiveCarrierRecord::Selected { family: crate::kernel::KernelFamily::Asm, schema, .. } if super::edge_selection::historical::supported_schema(*schema))
    {
        return false;
    }
    // Only Move Face keeps a partly joined collection, and a Thread
    // annotation selects one face.
    if (kind != Kind::MoveFace && unresolved.is_some())
        || (kind == Kind::Thread && faces.len() != 1)
    {
        return false;
    }
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
                Some((
                    Kind::MoveFace,
                    FaceSelection::Native(_) | FaceSelection::HistoricalPartial { .. }
                ))
            )
        })
        .count();
    if count == 0 {
        Vec::new()
    } else {
        vec![crate::loss::InventorLossCode::FeatureMoveFaceSelectionNative.note(format!("Retained {count} Move Face feature(s) with face operands that do not join exactly one stored face revision."))]
    }
}

/// Notes counting the face-selecting features whose selected faces rest on a
/// reading: that a matched revision is the input face of the feature, of a
/// repeated matching attribute, of an unshared generated-name family, or of
/// several owners of identical surface geometry.
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
                    Some((_, FaceSelection::Historical { faces, .. })) => faces.as_slice(),
                    Some((_, FaceSelection::HistoricalPartial { faces, .. })) => faces.as_slice(),
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
                Some((
                    _,
                    FaceSelection::Historical { .. } | FaceSelection::HistoricalPartial { .. }
                ))
            )
        })
        .count();
    let identical = count(&ids(|face| face.identical_owners));
    let thread_count = |flag: &dyn Fn(Surface, Option<f64>) -> bool| {
        features
            .iter()
            .filter(|feature| {
                let Some((Kind::Thread, FaceSelection::Historical { faces, .. })) =
                    selection(feature)
                else {
                    return false;
                };
                inventory
                    .edge_inputs
                    .faces
                    .iter()
                    .any(|((token, source), face)| {
                        flag(face.surface, thread_diameter(feature))
                            && face_id(token, *source)
                                .is_ok_and(|id| faces.as_slice().contains(&id))
                    })
            })
            .count()
    };
    let tolerated = thread_count(&thread_tolerance_used);
    let nonfinite = thread_count(&|surface, _| thread_ratio_nonfinite(surface));
    let mut notes = Vec::new();
    if selecting > 0 {
        notes.push(crate::loss::InventorLossCode::FeatureFaceJoinInputRevisionAssumed.note(format!("{selecting} face-selecting feature(s) keep historical face selections; the join does not establish the stored body state of a matched revision, so the selection assumes that the revision represents the input face of the feature.")));
    }
    if repeated > 0 {
        notes.push(crate::loss::InventorLossCode::FeatureFaceJoinRepeatedAttribute.note(format!("{repeated} face-selecting feature(s) select a face through a matching attribute that repeats on its owner; the selection assumes that the copies state one selection.")));
    }
    if unshared > 0 {
        notes.push(crate::loss::InventorLossCode::FeatureFaceJoinUnsharedFamily.note(format!("{unshared} face-selecting feature(s) select a face with a generated-name family that shares no feature with the selector; the selection assumes that another family of the face identifies it.")));
    }
    if nonfinite > 0 {
        notes.push(crate::loss::InventorLossCode::FeatureFaceJoinThreadRatioNonfinite.note(format!("{nonfinite} Thread feature(s) select a cone face whose stored axis ratio is not a finite number; the selection assumes that the face is a circular cone.")));
    }
    if tolerated > 0 {
        notes.push(crate::loss::InventorLossCode::FeatureFaceJoinThreadToleranceAssumed.note(format!("{tolerated} Thread feature(s) select a face within the tolerances of the cone axis ratio, the cylinder sine, or the stated diameter; the selection assumes that the difference is rounding.")));
    }
    if identical > 0 {
        notes.push(crate::loss::InventorLossCode::FeatureFaceJoinIdenticalOwners.note(format!("{identical} face-selecting feature(s) select a face through a selector pair with several matched owners of identical surface geometry; the selection takes the owner with the lowest entity index.")));
    }
    notes
}

#[cfg(test)]
mod tests;
