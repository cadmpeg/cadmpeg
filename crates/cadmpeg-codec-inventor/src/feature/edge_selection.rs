// SPDX-License-Identifier: Apache-2.0
//! Persistent edge operands joined through NMX matching attributes.

use super::{PmDcFeature, PmDcFeatureProperty, PmDcFeaturePropertyKind, ProjectionIndex};
use crate::{
    kernel::ActiveCarrierState,
    pmdc::{state_content_header, u32_list, Cursor},
    rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind},
};
use cadmpeg_asm::sab::{Record, Token};
use cadmpeg_core::{
    decode::{unique_index, DecodeContext, View},
    CodecError,
};
use cadmpeg_ir::features::{EdgeSelection, GeneratedEdgeRef};
use std::collections::{HashMap, HashSet};

mod archive;
pub(in crate::feature) mod geometry;

#[derive(Debug, Default)]
pub(super) struct EdgeInventory {
    pub(super) names: HashMap<(String, u32), Option<EdgeName>>,
    pub(super) geometry: HashMap<(String, u32), geometry::EdgeGeometry>,
    pub(super) generated_sources: Option<HashMap<u32, SourceNames>>,
    pub(super) generated_sources_complete: bool,
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
    let family = if record.name == "End-of-ASM-History-Section" {
        match record.tokens.first()? {
            Token::SubIdent(family) => family.as_str(),
            _ => return None,
        }
    } else {
        record.name.split('-').next()?
    };
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

fn selector(payload: View<'_>, version: u8) -> Result<Option<Selector>, CodecError> {
    let mut cursor = Cursor::new(payload);
    let header = state_content_header(&mut cursor, version)?;
    if cursor.i32("edge selector state")? != -1
        || cursor.u8("edge selector enabled")? != 1
        || cursor.u8("edge selector form")? != 5
    {
        return Ok(None);
    }
    cursor.reference("edge selector body")?;
    if cursor.u32("edge selector topology")? != 2
        || cursor.u32("edge selector qualifier")? != 0
        || cursor.u32("edge selector naming form")? != 2
    {
        return Ok(None);
    }
    let generators = u32_list(&mut cursor, 8, "edge selector generators")?;
    let [generator] = generators.values() else {
        return Ok(None);
    };
    if !u32_list(&mut cursor, 8, "edge selector secondary names")?
        .values()
        .is_empty()
        || cursor.u8("edge selector suffix form")? != 0
    {
        return Ok(None);
    }
    cursor.reference("edge selector item")?;
    cursor.reference("edge selector context")?;
    let key = cursor.u32("edge selector key")?;
    cursor.finish("edge selector")?;
    Ok(Some(Selector {
        source: header.source_index,
        generator: *generator,
        key,
    }))
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

fn matched(record: &Record) -> Option<(u32, u32, i64)> {
    if !custom(record) {
        return None;
    }
    let fields = values(record);
    let [Token::Ref(_), Token::Long(_), Token::Ref(_), Token::Ref(_), Token::Ref(owner), Token::Str(family), Token::Long(1), Token::Long(44), Token::Long(1), Token::Long(source), Token::Long(-1), Token::Long(key), Token::Long(0)] =
        fields.as_slice()
    else {
        return None;
    };
    if family != "INV_NMX_MATCHED_ATTRIB" || *owner < 0 {
        return None;
    }
    Some((
        u32::try_from(*source).ok()?,
        u32::try_from(*key).ok()?,
        *owner,
    ))
}

fn generated(record: &Record) -> Option<(i64, EdgeName)> {
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

fn is_edge(record: &Record) -> bool {
    matches!(record.head(), "edge" | "tedge")
        || (record.name == "End-of-ASM-History-Section"
            && matches!(record.tokens.first(), Some(Token::Ident(name)) if name == "edge"))
}

fn names(records: &[Record], selectors: &[Selector]) -> HashMap<u32, Option<EdgeName>> {
    let edges: HashSet<_> = records
        .iter()
        .filter(|record| is_edge(record))
        .filter_map(|record| i64::try_from(record.index).ok())
        .collect();
    let mut matches: HashMap<(u32, u32), HashSet<i64>> = HashMap::new();
    let mut generated_names: HashMap<i64, HashSet<EdgeName>> = HashMap::new();
    for record in records {
        if let Some((source, key, owner)) = matched(record) {
            if edges.contains(&owner) {
                matches.entry((source, key)).or_default().insert(owner);
            }
        }
        if let Some((owner, name)) = generated(record) {
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

pub(super) fn inventory(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
) -> Result<EdgeInventory, CodecError> {
    if ctx.container_only() || ctx.depth() != cadmpeg_core::decode::DecodeDepth::Full {
        return Ok(EdgeInventory::default());
    }
    let mut selectors = Vec::new();
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
        for record in &table.records {
            has_flange |= record.type_id == super::sheet_metal::flange::TYPE;
            sample_geometry |= record.type_id == super::sheet_metal::flange::TYPE
                && super::sheet_metal::style::prefix_len(version).is_some();
            if record.type_id != SELECTOR_TYPE {
                continue;
            }
            if let Ok(Some(selector)) = selector(record.payload, version) {
                selectors.push(selector);
            }
        }
    }
    let ActiveCarrierState::Selected(carrier) = &document.active_carrier else {
        return Ok(EdgeInventory::default());
    };
    let name_corners = has_flange && matches!(carrier.schema, 38 | 40 | 42 | 44);
    if tokens.len() != 1 || ((selectors.is_empty() || carrier.schema != 44) && !name_corners) {
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
    let (generated_sources, generated_sources_complete) = if name_corners {
        let (sources, complete) = generated_sources(&records, carrier.schema);
        (Some(sources), archive_complete && complete)
    } else {
        (None, false)
    };
    if selectors.is_empty() || carrier.schema != 44 {
        return Ok(EdgeInventory {
            generated_sources,
            generated_sources_complete,
            ..EdgeInventory::default()
        });
    }
    let names = names(&records, &selectors);
    let geometry = if sample_geometry {
        geometry::selected(&records, &selectors, &names)
            .into_iter()
            .map(|(source, geometry)| ((token.clone(), source), geometry))
            .collect()
    } else {
        HashMap::new()
    };
    Ok(EdgeInventory {
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

pub(crate) fn native_losses(
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
