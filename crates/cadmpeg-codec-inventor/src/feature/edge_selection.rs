// SPDX-License-Identifier: Apache-2.0
//! Persistent edge operands joined through NMX matching attributes.

use super::{PmDcFeature, PmDcFeatureProperty, PmDcFeaturePropertyKind, ProjectionIndex};
use crate::{
    kernel::ActiveCarrierState,
    rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind},
};
use cadmpeg_asm::sab::{Record, Token};
#[cfg(test)]
use cadmpeg_core::decode::View;
use cadmpeg_core::{
    decode::{unique_index, DecodeContext},
    CodecError,
};
use cadmpeg_ir::features::{EdgeSelection, GeneratedEdgeRef};
use std::collections::{HashMap, HashSet};

mod archive;
pub(in crate::feature) mod geometry;
pub(crate) mod historical;

#[derive(Debug, Default)]
pub(super) struct EdgeInventory {
    pub(super) names: HashMap<(String, u32), Option<EdgeName>>,
    pub(super) geometry: HashMap<(String, u32), geometry::EdgeGeometry>,
    pub(super) generated_sources: Option<HashMap<u32, SourceNames>>,
    pub(super) generated_sources_complete: bool,
    pub(super) historical: HashMap<(String, u32), historical::SelectedEdge>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct SourceNames {
    pub(super) face_revisions: usize,
    pub(super) edge_revisions: usize,
}

fn generated_sources(records: &[Record], schema: u32) -> (HashMap<u32, SourceNames>, bool) {
    let mut sources = HashMap::<u32, SourceNames>::new();
    let mut complete = true;
    let mut named_owners = HashSet::new();
    let mut duplicate_owners = HashSet::new();
    for record in records {
        if !generated_candidate(record) {
            continue;
        }
        let fields = values(record);
        if let Some(owner) = duplicate_owner(record, &fields, schema) {
            duplicate_owners.insert(owner);
            continue;
        }
        let Some((owner, generators)) = source_names(record, &fields, schema) else {
            complete = false;
            continue;
        };
        named_owners.insert(owner);
        for source in &generators {
            sources.entry(*source).or_default();
        }
        let Some(owner) = usize::try_from(owner)
            .ok()
            .and_then(|owner| records.get(owner))
        else {
            continue;
        };
        for source in generators {
            if owner.head() == "face" {
                sources.entry(source).or_default().face_revisions += 1;
            }
            if is_edge(owner) {
                sources.entry(source).or_default().edge_revisions += 1;
            }
        }
    }
    complete &= duplicate_owners.iter().all(|owner| {
        named_owners.contains(owner)
            && usize::try_from(*owner)
                .ok()
                .and_then(|owner| records.get(owner))
                .is_some_and(|owner| matches!(owner.head(), "face" | "edge" | "tedge" | "vertex"))
    });
    (sources, complete)
}

fn duplicate_owner(record: &Record, fields: &[&Token], schema: u32) -> Option<i64> {
    if !custom(record) || schema != 44 {
        return None;
    }
    match fields {
        [Token::Ref(_), Token::Long(_), Token::Ref(_), Token::Ref(_), Token::Ref(owner), Token::Str(family), Token::Long(1), Token::Long(44), Token::Long(_), Token::Long(_), Token::Long(_), Token::Long(_)]
            if family == "INV_NMX_DUP" =>
        {
            Some(*owner)
        }
        _ => None,
    }
}

fn generated_candidate(record: &Record) -> bool {
    if custom(record) {
        return matches!(record.chunk(5), Some(Token::Str(family))
            if family.starts_with("INV_NMX_") && family != "INV_NMX_MATCHED_ATTRIB");
    }
    record.name.contains("NMx_Brep_Name_tag")
        || record
            .tokens
            .iter()
            .any(|token| matches!(token, Token::SubIdent(name) if name == "NMx_Brep_Name_tag"))
}

/// The leading class of an `NMx` naming attribute in a schema-38, 40, or 42
/// carrier, for an active record or an `End-of-ASM-History-Section` snapshot.
fn legacy_family(record: &Record) -> Option<&str> {
    if record.name == "End-of-ASM-History-Section" {
        match record.tokens.first()? {
            Token::SubIdent(family) => Some(family.as_str()),
            _ => None,
        }
    } else {
        record.name.split('-').next()
    }
}

/// Whether an ASM carrier schema stores the `NMx` naming attributes in the
/// class-chain form instead of the `ATTRIB_CUSTOM` form.
pub(super) const fn legacy_schema(schema: u32) -> bool {
    matches!(schema, 38 | 40 | 42)
}

fn source_names(record: &Record, fields: &[&Token], schema: u32) -> Option<(i64, HashSet<u32>)> {
    let [Token::Ref(_), Token::Long(_), Token::Ref(_), Token::Ref(_), Token::Ref(owner), rest @ ..] =
        fields
    else {
        return None;
    };
    if custom(record) {
        if schema != 44 {
            return None;
        }
        let [Token::Str(family), Token::Long(1), Token::Long(44), Token::Long(1), Token::Long(source), Token::Long(-1), ..] =
            rest
        else {
            return None;
        };
        if !matches!(
            family.as_str(),
            "INV_NMX_GEN_TAG"
                | "INV_NMX_SWEEPGENERATED_TAG"
                | "INV_NMX_BLEND_TAG"
                | "INV_NMX_FLANGE_TAG"
                | "INV_NMX_BEND_TAG"
        ) {
            return None;
        }
        return Some((*owner, HashSet::from([u32::try_from(*source).ok()?])));
    }
    let family = legacy_family(record)?;
    let tail_len = match family {
        "NMx_Generated_Brep_tag" => 3,
        "NMx_Sweep_Generated_Brep_tag" => 10,
        "NMx_flange_tag" | "NMx_blend_tag" => 7,
        "NMx_bend_tag" => 8,
        "NMx_Hole_Brep_tag" => 2,
        _ => return None,
    };
    let [Token::Long(stored_schema), Token::Long(count), values @ ..] = rest else {
        return None;
    };
    if !matches!(schema, 38 | 40 | 42) || *stored_schema != i64::from(schema) {
        return None;
    }
    let count = usize::try_from(*count).ok()?;
    let end = count.checked_mul(2)?;
    if count == 0
        || values.len() != end.checked_add(tail_len)?
        || !values.iter().all(|value| matches!(value, Token::Long(_)))
    {
        return None;
    }
    let mut sources = HashSet::new();
    for pair in values.get(..end)?.chunks_exact(2) {
        let Token::Long(source) = pair[0] else {
            return None;
        };
        sources.insert(u32::try_from(*source).ok()?);
    }
    Some((*owner, sources))
}

const SELECTOR_TYPE: [u8; 16] = crate::pmdc::inventor_id(0x9087_4d53);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Selector {
    source: u32,
    generator: u32,
    key: u32,
}

/// Whether the direct edge selector of a `PmDc` segment major version stores
/// the qualifier word between its topology and naming-form words.
const fn selector_has_qualifier(version: u8) -> bool {
    version != SELECTOR_WITHOUT_QUALIFIER
}

/// The `PmDc` segment major version whose direct edge selector has no qualifier word.
const SELECTOR_WITHOUT_QUALIFIER: u8 = 19;

#[cfg(test)]
fn selector(
    payload: cadmpeg_core::decode::View<'_>,
    version: u8,
) -> Result<Option<Selector>, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    Ok(
        historical::parse(&ctx, payload, version)?.and_then(|stored| {
            let [generator] = stored.generators.as_slice() else {
                return None;
            };
            Some(Selector {
                source: stored.source,
                generator: *generator,
                key: stored.key,
            })
        }),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct EdgeName {
    generator: u32,
    family: String,
    associative_id: i64,
    role: i64,
}

fn values(record: &Record) -> Vec<&Token> {
    record.chunks().collect()
}

fn custom(record: &Record) -> bool {
    record.name == "ATTRIB_CUSTOM-attrib"
        || (record.name == "End-of-ASM-History-Section"
            && matches!(record.tokens.as_ref(), [Token::SubIdent(family), Token::Ident(base), ..]
                if family == "ATTRIB_CUSTOM" && base == "attrib"))
}

fn matched(record: &Record, schema: u32) -> Option<(u32, u32, i64)> {
    let fields = values(record);
    let (owner, source, key) = if legacy_schema(schema) {
        if legacy_family(record)? != "NMx_Matched_Entity" {
            return None;
        }
        let [Token::Ref(_), Token::Long(_), Token::Ref(_), Token::Ref(_), Token::Ref(owner), Token::Long(stored_schema), Token::Long(1), Token::Long(source), Token::Long(-1), Token::Long(key), Token::Long(0)] =
            fields.as_slice()
        else {
            return None;
        };
        if *stored_schema != i64::from(schema) {
            return None;
        }
        (owner, source, key)
    } else {
        if !custom(record) {
            return None;
        }
        let [Token::Ref(_), Token::Long(_), Token::Ref(_), Token::Ref(_), Token::Ref(owner), Token::Str(family), Token::Long(1), Token::Long(44), Token::Long(1), Token::Long(source), Token::Long(-1), Token::Long(key), Token::Long(0)] =
            fields.as_slice()
        else {
            return None;
        };
        if family != "INV_NMX_MATCHED_ATTRIB" {
            return None;
        }
        (owner, source, key)
    };
    if *owner < 0 {
        return None;
    }
    Some((
        u32::try_from(*source).ok()?,
        u32::try_from(*key).ok()?,
        *owner,
    ))
}

fn generated(record: &Record, schema: u32) -> Option<(i64, EdgeName)> {
    if legacy_schema(schema) {
        return legacy_generated(record, schema);
    }
    if !custom(record) {
        return None;
    }
    let fields = values(record);
    let [Token::Ref(_), Token::Long(_), Token::Ref(_), Token::Ref(_), Token::Ref(owner), Token::Str(family), Token::Long(1), Token::Long(44), Token::Long(1), Token::Long(generator), Token::Long(-1), Token::Long(associative_id), Token::Long(0), Token::Long(role), tail @ ..] =
        fields.as_slice()
    else {
        return None;
    };
    if *owner < 0
        || *associative_id < 0
        || !match family.as_str() {
            "INV_NMX_GEN_TAG" => tail.is_empty() && matches!(role, 2 | 10 | 18),
            "INV_NMX_SWEEPGENERATED_TAG" => {
                tail.len() == 7
                    && tail.iter().all(|token| matches!(token, Token::Long(0)))
                    && matches!(role, 10 | 18)
            }
            _ => false,
        }
    {
        return None;
    }
    Some((
        *owner,
        EdgeName {
            generator: u32::try_from(*generator).ok()?,
            family: family.clone(),
            associative_id: *associative_id,
            role: *role,
        },
    ))
}

/// A generated edge name of a schema-38, 40, or 42 carrier. A name with one
/// source pair carries the same generator, associative identifier, and role
/// as the `ATTRIB_CUSTOM` name of a schema-44 carrier.
fn legacy_generated(record: &Record, schema: u32) -> Option<(i64, EdgeName)> {
    let family = legacy_family(record)?;
    let fields = values(record);
    let [Token::Ref(_), Token::Long(_), Token::Ref(_), Token::Ref(_), Token::Ref(owner), Token::Long(stored_schema), Token::Long(1), Token::Long(generator), Token::Long(-1), Token::Long(associative_id), Token::Long(0), Token::Long(role), tail @ ..] =
        fields.as_slice()
    else {
        return None;
    };
    if *owner < 0 || *associative_id < 0 || *stored_schema != i64::from(schema) {
        return None;
    }
    let admitted = match family {
        "NMx_Generated_Brep_tag" => tail.is_empty() && matches!(role, 2 | 10 | 18),
        "NMx_Sweep_Generated_Brep_tag" => {
            tail.len() == 7
                && tail.iter().all(|token| matches!(token, Token::Long(0)))
                && matches!(role, 10 | 18)
        }
        _ => false,
    };
    if !admitted {
        return None;
    }
    Some((
        *owner,
        EdgeName {
            generator: u32::try_from(*generator).ok()?,
            family: family.to_owned(),
            associative_id: *associative_id,
            role: *role,
        },
    ))
}

fn is_edge(record: &Record) -> bool {
    matches!(record.head(), "edge" | "tedge")
        || (record.name == "End-of-ASM-History-Section"
            && matches!(record.tokens.first(), Some(Token::Ident(name)) if name == "edge"))
}

fn names(
    records: &[Record],
    selectors: &[Selector],
    schema: u32,
) -> HashMap<u32, Option<EdgeName>> {
    let edges: HashSet<_> = records
        .iter()
        .filter(|record| is_edge(record))
        .filter_map(|record| i64::try_from(record.index).ok())
        .collect();
    let mut matches: HashMap<(u32, u32), HashSet<i64>> = HashMap::new();
    let mut generated_names: HashMap<i64, HashSet<EdgeName>> = HashMap::new();
    for record in records {
        if let Some((source, key, owner)) = matched(record, schema) {
            if edges.contains(&owner) {
                matches.entry((source, key)).or_default().insert(owner);
            }
        }
        if let Some((owner, name)) = generated(record, schema) {
            generated_names.entry(owner).or_default().insert(name);
        }
    }
    unique_index(selectors.iter().filter_map(|selector| {
        let owners = matches.get(&(selector.source, selector.key))?;
        let mut candidates = HashSet::new();
        for owner in owners {
            let names = generated_names.get(owner)?;
            if names.len() != 1 {
                return None;
            }
            candidates.extend(names.iter().cloned());
        }
        let mut candidates = candidates.into_iter();
        let name = candidates.next()?;
        (name.generator == selector.generator && candidates.next().is_none())
            .then_some((selector.source, name))
    }))
}

/// Whether the selector-to-name join runs for a carrier schema. A schema-44
/// carrier joins in every document; a schema-38, 40, or 42 carrier joins in a
/// document with a sheet-metal flange.
const fn names_joined(schema: u32, has_flange: bool) -> bool {
    schema == 44 || (legacy_schema(schema) && has_flange)
}

pub(super) fn inventory(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
) -> Result<EdgeInventory, CodecError> {
    if ctx.container_only() || ctx.depth() != cadmpeg_core::decode::DecodeDepth::Full {
        return Ok(EdgeInventory::default());
    }
    let ActiveCarrierState::Selected(carrier) = &document.active_carrier else {
        return Ok(EdgeInventory::default());
    };
    if !historical::supported_schema(carrier.schema) {
        return Ok(EdgeInventory::default());
    }
    let mut selectors = Vec::new();
    let mut stored_selectors = Vec::new();
    let mut sample_geometry = false;
    let mut has_flange = false;
    let mut tokens = HashSet::new();
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmDc {
            continue;
        }
        tokens.insert(segment.pair.token.key().as_str().to_owned());
        let Some(version) = segment.registry.map(|registry| registry.version_major) else {
            continue;
        };
        if !crate::pmdc::record_version_supported(version) {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        let record_types = unique_index(
            table
                .records
                .iter()
                .map(|record| (record.ordinal, record.type_id)),
        );
        let mut selector_sources = HashMap::<u32, usize>::new();
        let stored_start = stored_selectors.len();
        for record in &table.records {
            has_flange |= record.type_id == super::sheet_metal::flange::TYPE;
            sample_geometry |= record.type_id == super::sheet_metal::flange::TYPE
                && super::sheet_metal::style::prefix_len(version).is_some();
            if record.type_id != SELECTOR_TYPE {
                continue;
            }
            if let Ok(source) = historical::source(record.payload, version) {
                *selector_sources.entry(source).or_default() += 1;
            }
            let stored = match historical::parse(ctx, record.payload, version) {
                Ok(stored) => stored,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(_) => None,
            };
            if let Some(stored) = stored {
                if let [generator] = stored.generators.as_slice() {
                    selectors.push(Selector {
                        source: stored.source,
                        generator: *generator,
                        key: stored.key,
                    });
                }
                if historical::links(&stored, |ordinal| {
                    record_types.get(&ordinal).copied().flatten()
                }) {
                    stored_selectors.push(stored);
                }
            }
        }
        let mut position = 0;
        stored_selectors.retain(|selector| {
            let keep =
                position < stored_start || selector_sources.get(&selector.source) == Some(&1);
            position += 1;
            keep
        });
    }
    let name_corners = has_flange && matches!(carrier.schema, 38 | 40 | 42 | 44);
    if tokens.len() != 1
        || (stored_selectors.is_empty()
            && (selectors.is_empty() || !names_joined(carrier.schema, has_flange))
            && !name_corners)
    {
        return Ok(EdgeInventory::default());
    }
    let (records, archive_complete) = match archive::records(ctx, carrier) {
        Ok(records) => records,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(EdgeInventory::default()),
    };
    let Some(token) = tokens.into_iter().next() else {
        return Ok(EdgeInventory::default());
    };
    let historical = if archive_complete {
        historical::selected(&records, &stored_selectors, carrier.schema)
            .into_iter()
            .map(|(source, edge)| ((token.clone(), source), edge))
            .collect()
    } else {
        HashMap::new()
    };
    let (generated_sources, generated_sources_complete) = if name_corners {
        let (sources, complete) = generated_sources(&records, carrier.schema);
        (Some(sources), archive_complete && complete)
    } else {
        (None, false)
    };
    if selectors.is_empty() || !names_joined(carrier.schema, has_flange) {
        return Ok(EdgeInventory {
            generated_sources,
            generated_sources_complete,
            historical,
            ..EdgeInventory::default()
        });
    }
    let names = names(&records, &selectors, carrier.schema);
    let geometry = if sample_geometry {
        geometry::selected(&records, &selectors, &names, carrier.schema)
            .into_iter()
            .map(|(source, geometry)| ((token.clone(), source), geometry))
            .collect()
    } else {
        HashMap::new()
    };
    Ok(EdgeInventory {
        historical,
        generated_sources,
        generated_sources_complete,
        names: names
            .into_iter()
            .map(|(source, name)| ((token.clone(), source), name))
            .collect(),
        geometry,
    })
}

fn sketch_entity(
    inventory: &super::FeatureInventory,
    sketches: &crate::sketch::SketchInventory,
    feature: &PmDcFeature,
    name: &EdgeName,
) -> Option<cadmpeg_ir::sketches::SketchEntityId> {
    let token = feature.identity.segment_token.as_str();
    let mut labels = inventory.labels.iter().filter(|label| {
        label.identity.segment_token.as_str() == token
            && label.header.owner.record_ordinal() == Some(feature.identity.record_ordinal)
    });
    let label = labels.next()?;
    if labels.next().is_some() {
        return None;
    }
    let sketch_keys: HashSet<_> = sketches
        .sketches
        .iter()
        .filter(|sketch| sketch.identity.segment_token.as_str() == token)
        .map(|sketch| sketch.identity.record_ordinal)
        .collect();
    let sketch = match super::FeatureFamily::from_class_id(label.class_id())? {
        super::FeatureFamily::Extrusion => {
            let [participant] = label.participants.references() else {
                return None;
            };
            participant.record_ordinal()?
        }
        super::FeatureFamily::Plate => {
            let mut candidates = inventory
                .labels
                .iter()
                .filter(|label| label.identity.segment_token.as_str() == token)
                .filter_map(|label| {
                    let [sketch, plate] = label.participants.references() else {
                        return None;
                    };
                    let sketch = sketch.record_ordinal()?;
                    (plate.record_ordinal() == Some(feature.identity.record_ordinal)
                        && sketch_keys.contains(&sketch))
                    .then_some(sketch)
                });
            let sketch = candidates.next()?;
            if candidates.next().is_some() {
                return None;
            }
            sketch
        }
        _ => return None,
    };
    if !sketch_keys.contains(&sketch) {
        return None;
    }
    let entity_type = if name.role == 18 { 1 } else { 2 };
    let entities = unique_index(
        sketches
            .entities
            .iter()
            .filter(|entity| entity.identity.segment_token.as_str() == token)
            .map(|entity| (entity.identity.record_ordinal, entity)),
    );
    let mut candidates = inventory
        .entity_style_links
        .iter()
        .filter(|link| {
            link.identity.segment_token.as_str() == token
                && i64::from(link.associative_id) == name.associative_id
                && link.entity_type == entity_type
        })
        .filter_map(|link| {
            let entity = entities
                .get(&link.header.owner.record_ordinal()?)?
                .as_ref()?;
            (entity.sketch.record_ordinal() == Some(sketch)).then_some(*entity)
        });
    let entity = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    if (name.role == 18)
        != matches!(
            entity.kind,
            crate::sketch::PmDcSketchEntityKind::Point { .. }
        )
    {
        return None;
    }
    crate::sketch::entity_id(entity)
}

pub(super) fn generated_names(
    inventory: &super::FeatureInventory,
    history: &super::history::History,
    sketches: &crate::sketch::SketchInventory,
) -> HashMap<(String, u32), Option<GeneratedEdgeRef>> {
    let features = unique_index(inventory.features.iter().map(|feature| {
        (
            (
                feature.identity.segment_token.as_str(),
                feature.header.source_index,
            ),
            feature,
        )
    }));
    inventory
        .edge_inputs
        .names
        .iter()
        .filter_map(|((token, source), name)| {
            let name = name.as_ref()?;
            let feature: &PmDcFeature =
                features.get(&(token.as_str(), name.generator))?.as_ref()?;
            history.placement(feature.identity.record_ordinal)?;
            let entity = sketch_entity(inventory, sketches, feature, name)?;
            let feature = super::history_feature_id(&feature.identity).ok()?;
            let local_id = cadmpeg_ir::features::SelectionReference::new(format!(
                "inventor:nmx:{}:sketch-entity:{}:role:{}",
                name.family, entity, name.role
            ))
            .ok()?;
            Some((
                (token.clone(), *source),
                Some(GeneratedEdgeRef { feature, local_id }),
            ))
        })
        .collect()
}

pub(super) fn resolve(
    collection: &PmDcFeatureProperty,
    index: &ProjectionIndex<'_>,
) -> EdgeSelection {
    let fallback = || EdgeSelection::Native(collection.id());
    let PmDcFeaturePropertyKind::References { items, .. } = &collection.kind else {
        return fallback();
    };
    let token = collection.identity.segment_token.as_str();
    let mut edges = Vec::new();
    for item in items.references() {
        let Some(property) = super::resolve_property(token, item.index(), index) else {
            return fallback();
        };
        let PmDcFeaturePropertyKind::EdgeItem {
            index_references,
            index_reference_value: -1,
            value: 0,
        } = &property.kind
        else {
            return fallback();
        };
        for source in index_references.values() {
            let Some(Some(edge)) = index.edge_names.get(&(token.to_owned(), *source)) else {
                return fallback();
            };
            if edges.contains(edge) {
                return fallback();
            }
            edges.push(edge.clone());
        }
    }
    EdgeSelection::generated(edges, collection.id()).unwrap_or_else(|_| fallback())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemReason {
    Empty,
    Unresolved,
}

pub(crate) fn check_items<'a>(
    collection: &PmDcFeatureProperty,
    property: &impl Fn(crate::pmdc::PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> Result<(), ItemReason> {
    let PmDcFeaturePropertyKind::References {
        family: super::PmDcFeatureReferenceFamily::EdgeCollection,
        items,
    } = &collection.kind
    else {
        return Err(ItemReason::Unresolved);
    };
    if items.references().is_empty() {
        return Err(ItemReason::Unresolved);
    }
    for reference in items.references() {
        let Some(PmDcFeaturePropertyKind::EdgeItem {
            index_references, ..
        }) = property(*reference).map(|p| &p.kind)
        else {
            return Err(ItemReason::Unresolved);
        };
        if index_references.values().is_empty() {
            return Err(ItemReason::Empty);
        }
    }
    Ok(())
}

pub(crate) fn matches_selection(
    selection: &cadmpeg_ir::features::EdgeSelection,
    native: &str,
    direct: bool,
) -> bool {
    match selection {
        cadmpeg_ir::features::EdgeSelection::Native(actual) => actual == native,
        cadmpeg_ir::features::EdgeSelection::Generated { native: actual, .. } => {
            direct && actual.as_str() == native
        }
        cadmpeg_ir::features::EdgeSelection::Historical { native: actual, .. } => {
            direct && actual.as_str() == native
        }
        _ => false,
    }
}

pub(crate) fn native_losses(
    inventory: &super::FeatureInventory,
    features: &[cadmpeg_ir::features::Feature],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let mut losses = empty_item_losses(inventory, features);
    losses.extend(historical::native_losses(features));
    losses
}

fn collection_has_empty_item<'a>(
    kind: &PmDcFeaturePropertyKind,
    property_at: &impl Fn(crate::pmdc::PmDcReference) -> Option<&'a PmDcFeaturePropertyKind>,
) -> bool {
    let PmDcFeaturePropertyKind::References {
        family: super::PmDcFeatureReferenceFamily::EdgeCollection,
        items,
    } = kind
    else {
        return false;
    };
    items.references().iter().any(|reference| {
        matches!(property_at(*reference), Some(PmDcFeaturePropertyKind::EdgeItem {
            index_references, ..
        }) if index_references.values().is_empty())
    })
}

fn empty_item_losses(
    inventory: &super::FeatureInventory,
    features: &[cadmpeg_ir::features::Feature],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, NativeFeatureKind};

    let native = unique_index(features.iter().filter_map(|feature| {
        let family = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: NativeFeatureKind::Fillet,
                ..
            }) => super::PmDcFeatureReferenceFamily::FilletEdgeSets,
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: NativeFeatureKind::Chamfer,
                ..
            }) => super::PmDcFeatureReferenceFamily::EdgeCollection,
            _ => return None,
        };
        Some((feature.native_ref.as_deref()?, family))
    }));
    if native.is_empty() {
        return Vec::new();
    }
    let labels = unique_index(inventory.labels.iter().map(|label| {
        (
            (
                label.identity.segment_token.as_str(),
                label.header.owner.record_ordinal(),
            ),
            label,
        )
    }));
    let properties = unique_index(inventory.properties.iter().map(|property| {
        (
            (
                property.identity.segment_token.as_str(),
                property.identity.record_ordinal,
            ),
            &property.kind,
        )
    }));
    let count = inventory
        .features
        .iter()
        .filter(|source| {
            let source_id = source.id();
            let Some(Some(expected_family)) = native.get(source_id.as_str()) else {
                return false;
            };
            // A corner round reports its own sheet-metal operand loss.
            if labels
                .get(&(
                    source.identity.segment_token.as_str(),
                    Some(source.identity.record_ordinal),
                ))
                .is_some_and(|label| {
                    label.is_some_and(|label| label.class_id() == super::CORNER_ROUND_CLASS_ID)
                })
            {
                return false;
            }
            let property_at = |reference: crate::pmdc::PmDcReference| {
                properties
                    .get(&(
                        source.identity.segment_token.as_str(),
                        reference.record_ordinal()?,
                    ))?
                    .as_ref()
                    .copied()
            };
            let Some(kind) = source
                .properties
                .references()
                .first()
                .and_then(|reference| property_at(*reference))
            else {
                return false;
            };
            if !matches!(kind, PmDcFeaturePropertyKind::References { family, .. }
                if family == expected_family)
            {
                return false;
            }
            if let PmDcFeaturePropertyKind::References {
                family: super::PmDcFeatureReferenceFamily::FilletEdgeSets,
                items,
            } = kind
            {
                return items.references().iter().any(|reference| {
                    let Some(PmDcFeaturePropertyKind::FilletEdgeSet { edges, .. }) =
                        property_at(*reference)
                    else {
                        return false;
                    };
                    property_at(*edges)
                        .is_some_and(|kind| collection_has_empty_item(kind, &property_at))
                });
            }
            collection_has_empty_item(kind, &property_at)
        })
        .count();
    if count == 0 {
        Vec::new()
    } else {
        vec![crate::loss::InventorLossCode::FeatureEdgeSelectionEmptyItemsNative.note(format!(
            "Retained {count} edge-treatment feature(s) with empty edge-item index-reference lists; their input-edge naming joins are unresolved."
        ))]
    }
}

fn retain_producers(selection: &mut EdgeSelection, ordinal: u64, producers: &HashMap<String, u64>) {
    if let EdgeSelection::Generated { edges, native } = selection {
        if edges.iter().any(|edge| {
            producers
                .get(edge.feature.as_str())
                .is_none_or(|producer| *producer >= ordinal)
        }) {
            *selection = EdgeSelection::Native(native.as_str().to_owned());
        }
    }
}

pub(super) fn bind_generated(
    features: &mut [cadmpeg_ir::features::Feature],
    results: &mut [cadmpeg_ir::features::FeatureResultTopology],
) -> Result<(), CodecError> {
    use cadmpeg_core::text::NonBlankString;
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, FeatureResultMembers, FeatureResultTopology,
    };
    let producers = features
        .iter()
        .map(|feature| (feature.id.as_str().to_owned(), feature.ordinal))
        .collect::<HashMap<_, _>>();
    let mut outputs: HashMap<String, Vec<NonBlankString>> = HashMap::new();
    for feature in features.iter_mut() {
        let ordinal = feature.ordinal;
        feature.evaluation.edit(|definition, _| match definition {
            FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
                for group in groups.iter_mut() {
                    retain_producers(&mut group.edges, ordinal, &producers);
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) => {
                for group in groups.iter_mut() {
                    retain_producers(&mut group.edges, ordinal, &producers);
                }
            }
            FeatureDefinition::Operation(
                FeatureOperation::SheetMetalAttachedPlate { attachment, .. }
                | FeatureOperation::SheetMetalBend { attachment, .. },
            ) => {
                retain_producers(attachment, ordinal, &producers);
            }
            _ => {}
        });
        let selections: Vec<_> = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => {
                groups.iter().map(|group| &group.edges).collect()
            }
            FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) => {
                groups.iter().map(|group| &group.edges).collect()
            }
            FeatureDefinition::Operation(
                FeatureOperation::SheetMetalAttachedPlate { attachment, .. }
                | FeatureOperation::SheetMetalBend { attachment, .. },
            ) => vec![attachment],
            _ => Vec::new(),
        };
        for selection in selections {
            if let EdgeSelection::Generated { edges, .. } = selection {
                for edge in edges {
                    feature.dependencies.insert(edge.feature.clone());
                    let members = outputs.entry(edge.feature.as_str().to_owned()).or_default();
                    let member = NonBlankString::new(edge.local_id.as_str().to_owned())
                        .ok_or_else(|| {
                            CodecError::malformed("Inventor generated edge has no local name")
                        })?;
                    if !members.contains(&member) {
                        members.push(member);
                    }
                }
            }
        }
    }
    for feature in features {
        let Some(selected) = outputs.remove(feature.id.as_str()) else {
            continue;
        };
        if let Some(result) = results
            .iter_mut()
            .find(|result| result.output_of == feature.id)
        {
            let mut edges = result.edges().to_vec();
            for edge in selected {
                if !edges.contains(&edge) {
                    edges.push(edge);
                }
            }
            let members = FeatureResultMembers::new(
                result.bodies().to_vec(),
                result.faces().to_vec(),
                edges,
                result.vertices().to_vec(),
            )
            .map_err(CodecError::malformed)?;
            *result = FeatureResultTopology::new(
                result.id.clone(),
                result.output_of.clone(),
                members,
                result.native_ref.clone(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
