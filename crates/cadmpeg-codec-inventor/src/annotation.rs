// SPDX-License-Identifier: Apache-2.0
//! Part-model annotations, their tolerances, and their targets.
//!
//! A feature label names an annotation owner. A dimension owner names one
//! value record and its geometry intents. A relation joins each intent to a
//! selection item, and a naming selector names that item with a key of the
//! naming table of the segment. The key selects a name record. A generated
//! name record matches the generated-name attributes of the active body, and
//! an edge name record names an edge by its two faces. A frame or datum owner
//! takes the faces of the tolerance feature that lists it.

mod names;
mod records;

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::ids::{EdgeId, FaceId, PmiId};
use cadmpeg_ir::pmi::zones::ToleranceConstraints;
use cadmpeg_ir::pmi::{
    DatumReference, DatumReferences, DimensionKind, DimensionTolerance, GeometricToleranceKind,
    LimitsAndFits, PmiAnnotation, PmiDefinition, PmiDimension, PmiMagnitude, PmiQuantity,
    PmiTarget, PmiValue,
};
use cadmpeg_ir::report::loss::LossNote;

use crate::design::PmDcParameter;
use crate::feature::{
    PmDcFeatureLabel, PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily,
};
use crate::loss::InventorLossCode;
use crate::pmdc::PmDcReference;
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};
use names::{edge_name, generated_name, BodyNames, EntityKind, NameFamily, EDGE_NAME};
use records::{
    datum_feature, edge_selector, face_selector, frame_row, geometry_link, naming_table, owner,
    relation, relation_intent_offset, tolerance_feature, tolerance_owner, trailing_reference,
    value, Measure, Record, Segment, Selector, ToleranceFeature, Value, DATUM_FEATURE, DATUM_OWNER,
    EDGE_ITEM, EDGE_SELECTOR, FACE_RELATION, FACE_SELECTOR, FEATURE_FACE_SELECTOR, FRAME_OWNER,
    FRAME_ROW, GEOMETRY, GEOMETRY_LINK, INTENT, NAMING_TABLE, PAIR_RELATION, SIZE_RELATION,
    TOLERANCE_DEVIATION, TOLERANCE_FEATURE, TOLERANCE_SYMMETRIC, WORK_AXIS,
};

/// Internal centimetres per neutral millimetre.
const MILLIMETRES_PER_CENTIMETRE: f64 = 10.0;
/// Byte length of the trailing item, context, key, and flag fields of an edge
/// selector.
const EDGE_SELECTOR_TAIL_LEN: usize = 13;

/// Segment major versions whose annotation records follow this grammar.
pub(crate) const fn version_supported(version: u8) -> bool {
    matches!(version, 25 | 28)
}

pub(crate) struct Inputs<'a> {
    pub(crate) rse: &'a RseInventory<'a>,
    pub(crate) labels: &'a [PmDcFeatureLabel],
    pub(crate) parameters: &'a [PmDcParameter],
    pub(crate) properties: &'a [PmDcFeatureProperty],
}

/// A feature label of one segment: the record that it labels, its name, and
/// its participants.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Label {
    owner: u32,
    name: Option<String>,
    participants: Vec<u32>,
}

/// The face collections and face items of one segment.
#[derive(Debug, Default)]
struct Selections {
    /// Face collection to its face items.
    collections: HashMap<u32, Vec<u32>>,
    /// Face item to the source indexes of its face selectors.
    face_items: HashMap<u32, Vec<u32>>,
}

/// Per-segment indexes from intents to selectors, and the records that
/// failed their grammar.
#[derive(Default)]
struct Joins {
    items: HashMap<u32, Vec<u32>>,
    geometry: HashMap<u32, Vec<u32>>,
    selectors: HashMap<u32, Vec<Selector>>,
    feature_selectors: HashMap<u32, Vec<Selector>>,
    tables: HashMap<u32, HashMap<u32, u32>>,
    failed_intents: HashSet<u32>,
    failed_items: HashSet<u32>,
    failed_sources: HashSet<u32>,
    malformed: usize,
}

/// The ordinal that a qualified reference at `offset` of a payload names, when
/// it names a record of one of `types`.
fn reference_at(
    segment: &Segment<'_>,
    payload: View<'_>,
    offset: usize,
    types: &[[u8; 16]],
) -> Option<u32> {
    let reference = PmDcReference::from_packed(View::u32_le_at(payload.window(), offset)?);
    (reference.qualified()
        && segment
            .target_type(reference)
            .is_some_and(|type_id| types.contains(&type_id)))
    .then(|| reference.record_ordinal())
    .flatten()
}

impl Joins {
    fn new(ctx: &DecodeContext<'_>, segment: &Segment<'_>) -> Self {
        let mut joins = Self::default();
        let mut ordinals: Vec<_> = segment.records.keys().copied().collect();
        ordinals.sort_unstable();
        for record_ordinal in ordinals {
            let Some(record) = segment.records.get(&record_ordinal) else {
                continue;
            };
            joins.admit(ctx, segment, record_ordinal, record);
        }
        joins
    }

    fn admit(
        &mut self,
        ctx: &DecodeContext<'_>,
        segment: &Segment<'_>,
        record_ordinal: u32,
        record: &Record<'_>,
    ) {
        let payload = record.payload;
        let last = |back: usize, types: &[[u8; 16]]| {
            payload
                .window()
                .len()
                .checked_sub(back)
                .and_then(|offset| reference_at(segment, payload, offset, types))
        };
        match record.type_id {
            SIZE_RELATION | FACE_RELATION | PAIR_RELATION => match relation(segment, payload) {
                Ok((intent, items)) => self.items.entry(intent).or_default().extend(items),
                Err(_) => {
                    self.malformed += 1;
                    if let Some(intent) =
                        reference_at(segment, payload, relation_intent_offset(segment), &[INTENT])
                    {
                        self.failed_intents.insert(intent);
                    }
                }
            },
            GEOMETRY_LINK => match geometry_link(segment, payload) {
                Ok((item, geometry)) => self.geometry.entry(item).or_default().push(geometry),
                Err(_) => {
                    self.malformed += 1;
                    if let Some(item) = last(4, &[EDGE_ITEM, WORK_AXIS]) {
                        self.failed_items.insert(item);
                    }
                }
            },
            FACE_SELECTOR => match face_selector(ctx, segment, payload, &[GEOMETRY]) {
                Ok(selector) => self
                    .selectors
                    .entry(selector.item)
                    .or_default()
                    .push(selector),
                Err(_) => {
                    self.malformed += 1;
                    if let Some(item) = last(4, &[GEOMETRY]) {
                        self.failed_items.insert(item);
                    }
                }
            },
            FEATURE_FACE_SELECTOR => {
                let state_offset = segment.state_header_len();
                let state = View::i32_le_at(payload.window(), state_offset);
                if state == Some(0) {
                    return;
                }
                match face_selector(ctx, segment, payload, &[crate::feature::FACE_ITEM_TYPE]) {
                    Ok(selector) => self
                        .feature_selectors
                        .entry(selector.source)
                        .or_default()
                        .push(selector),
                    Err(_) => {
                        self.malformed += 1;
                        if let Some(source) = state_offset
                            .checked_sub(4)
                            .and_then(|offset| View::u32_le_at(payload.window(), offset))
                        {
                            self.failed_sources.insert(source);
                        }
                    }
                }
            }
            EDGE_SELECTOR => match edge_selector(ctx, segment, payload) {
                Ok(selector) => self
                    .selectors
                    .entry(selector.item)
                    .or_default()
                    .push(selector),
                Err(_) => {
                    self.malformed += 1;
                    if let Some(item) = last(EDGE_SELECTOR_TAIL_LEN, &[EDGE_ITEM, WORK_AXIS]) {
                        self.failed_items.insert(item);
                    }
                }
            },
            NAMING_TABLE => match naming_table(payload) {
                Ok(table) => {
                    self.tables.insert(record_ordinal, table);
                }
                Err(_) => self.malformed += 1,
            },
            _ => {}
        }
    }

    /// The selectors that name a selection item directly or through its
    /// stored geometry, or `None` when a selector of the item failed its
    /// grammar.
    fn item_selectors(&self, item: u32) -> Option<Vec<&Selector>> {
        if self.failed_items.contains(&item) {
            return None;
        }
        let mut found = Vec::new();
        found.extend(self.selectors.get(&item).into_iter().flatten());
        for geometry in self.geometry.get(&item).into_iter().flatten() {
            if self.failed_items.contains(geometry) {
                return None;
            }
            found.extend(self.selectors.get(geometry).into_iter().flatten());
        }
        Some(found)
    }
}

/// The joins of one segment that an annotation projection reads.
struct Scope<'s, 'a, 'm> {
    segment: &'s Segment<'a>,
    joins: &'s Joins,
    parameters: &'s HashMap<u32, f64>,
    selections: &'s Selections,
    /// The tolerance feature whose label lists each annotation owner, or
    /// `None` when the labels of two tolerance features list the owner.
    features: &'s HashMap<u32, Option<u32>>,
    /// Each tolerance feature of the segment, or `None` when it fails its
    /// grammar.
    tolerance_features: &'s HashMap<u32, Option<ToleranceFeature>>,
    /// The ordinals that a tolerance feature lists as members.
    members: &'s HashSet<u32>,
    names: &'s BodyNames<'m>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Outcome {
    dimensions: usize,
    frames: usize,
    frames_native: usize,
    datum_systems_unresolved: usize,
    unresolved_targets: usize,
    malformed: usize,
    tolerance_unresolved: usize,
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    inputs: &Inputs<'_>,
    model: &mut Model,
) -> Result<Vec<LossNote>, CodecError> {
    let names = BodyNames::new(model);
    let mut outcome = Outcome::default();
    let mut annotations = Vec::new();
    for rse_segment in &inputs.rse.segments {
        if rse_segment.kind != SegmentKind::PmDc {
            continue;
        }
        let Some(version) = rse_segment.registry.map(|join| join.version_major) else {
            continue;
        };
        if !version_supported(version) {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &rse_segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        let segment = Segment {
            token: rse_segment.pair.token.as_str().to_owned(),
            version,
            records: table
                .records
                .iter()
                .map(|record| {
                    (
                        record.ordinal,
                        Record {
                            type_id: record.type_id,
                            payload: record.payload,
                        },
                    )
                })
                .collect(),
        };
        let token_key = rse_segment.pair.token.key();
        let parameters = inputs
            .parameters
            .iter()
            .filter(|parameter| &parameter.identity.segment_token == token_key)
            .map(|parameter| {
                (
                    parameter.identity.record_ordinal,
                    parameter.model_value.get(),
                )
            })
            .collect::<HashMap<_, _>>();
        let mut selections = Selections::default();
        for property in inputs
            .properties
            .iter()
            .filter(|property| &property.identity.segment_token == token_key)
        {
            let ordinal = property.identity.record_ordinal;
            match &property.kind {
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::FaceCollection,
                    items,
                } => {
                    selections.collections.insert(
                        ordinal,
                        items
                            .references()
                            .iter()
                            .filter_map(|reference| reference.record_ordinal())
                            .collect(),
                    );
                }
                PmDcFeaturePropertyKind::FaceItem { index_references } => {
                    selections
                        .face_items
                        .insert(ordinal, index_references.values().to_vec());
                }
                _ => {}
            }
        }
        let labels: Vec<Label> = inputs
            .labels
            .iter()
            .filter(|label| &label.identity.segment_token == token_key)
            .filter_map(|label| {
                Some(Label {
                    owner: label.header.owner.record_ordinal()?,
                    name: label.name().map(str::to_owned),
                    participants: label
                        .participants
                        .references()
                        .iter()
                        .filter_map(|reference| reference.record_ordinal())
                        .collect(),
                })
            })
            .collect();
        annotations.extend(project_segment(
            ctx,
            &segment,
            &labels,
            (&parameters, &selections),
            &names,
            &mut outcome,
        )?);
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(annotations.len()),
        "Inventor part annotations",
    )?;
    model.pmi.extend(annotations);
    model
        .pmi
        .sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    Ok(losses(&outcome))
}

fn project_segment(
    ctx: &DecodeContext<'_>,
    segment: &Segment<'_>,
    labels: &[Label],
    (parameters, selections): (&HashMap<u32, f64>, &Selections),
    names: &BodyNames<'_>,
    outcome: &mut Outcome,
) -> Result<Vec<PmiAnnotation>, CodecError> {
    let joins = Joins::new(ctx, segment);
    outcome.malformed += joins.malformed;
    let mut features = HashMap::new();
    for label in labels {
        if segment.record(label.owner, TOLERANCE_FEATURE).is_none() {
            continue;
        }
        for participant in &label.participants {
            features
                .entry(*participant)
                .and_modify(|feature: &mut Option<u32>| {
                    if *feature != Some(label.owner) {
                        *feature = None;
                    }
                })
                .or_insert(Some(label.owner));
        }
    }
    let tolerance_features: HashMap<u32, Option<ToleranceFeature>> = segment
        .records
        .iter()
        .filter(|(_, record)| record.type_id == TOLERANCE_FEATURE)
        .map(|(ordinal, record)| {
            (
                *ordinal,
                tolerance_feature(ctx, segment, record.payload).ok(),
            )
        })
        .collect();
    let members: HashSet<u32> = tolerance_features
        .values()
        .flatten()
        .flat_map(|parsed| parsed.members.iter().copied())
        .collect();
    let scope = Scope {
        segment,
        joins: &joins,
        parameters,
        selections,
        features: &features,
        tolerance_features: &tolerance_features,
        members: &members,
        names,
    };
    let names_by_owner = labels
        .iter()
        .map(|label| (label.owner, label.name.clone()))
        .collect::<HashMap<_, _>>();
    let (mut annotations, datum_ids) = datums(ctx, &scope, &names_by_owner, outcome)?;
    for label in labels {
        let Some(record) = segment.records.get(&label.owner) else {
            continue;
        };
        if record.type_id == FRAME_OWNER {
            annotations.extend(frame(
                &scope,
                &datum_ids,
                (record.payload, label.owner),
                label.name.as_deref(),
                outcome,
            )?);
            continue;
        }
        let Some(measure) = Measure::of(record.type_id) else {
            continue;
        };
        if let Some(annotation) = dimension(
            ctx,
            &scope,
            measure,
            (record.payload, label.owner),
            label.name.as_deref(),
            outcome,
        )? {
            annotations.push(annotation);
        }
    }
    Ok(annotations)
}

fn dimension(
    ctx: &DecodeContext<'_>,
    scope: &Scope<'_, '_, '_>,
    measure: Measure,
    (payload, owner_ordinal): (View<'_>, u32),
    name: Option<&str>,
    outcome: &mut Outcome,
) -> Result<Option<PmiAnnotation>, CodecError> {
    let segment = scope.segment;
    let Ok(owner) = owner(segment, payload, measure) else {
        outcome.malformed += 1;
        return Ok(None);
    };
    let Some(value_payload) = segment.record(owner.value, measure.value_type()) else {
        outcome.malformed += 1;
        return Ok(None);
    };
    let Ok(value) = value(ctx, segment, value_payload, measure) else {
        outcome.malformed += 1;
        return Ok(None);
    };
    let Some(&model_value) = scope.parameters.get(&value.nominal_parameter) else {
        outcome.malformed += 1;
        return Ok(None);
    };
    if value.measure != measure.code() {
        outcome.malformed += 1;
        return Ok(None);
    }
    let kind = match measure {
        Measure::Linear => DimensionKind::Location,
        Measure::Angular => DimensionKind::Angular,
        Measure::Diameter | Measure::HoleNote => DimensionKind::Diameter,
        Measure::Radial => DimensionKind::Radius,
    };
    let quantity = if measure == Measure::Angular {
        PmiQuantity::Angle
    } else {
        PmiQuantity::Length
    };
    let scale = if quantity == PmiQuantity::Length {
        MILLIMETRES_PER_CENTIMETRE
    } else {
        1.0
    };
    let nominal = PmiValue::new(model_value * scale, quantity);
    let Tolerance {
        bounds,
        modifier,
        resolved,
    } = tolerance(&value, model_value, scale, quantity);
    let mut modifiers = Vec::new();
    modifiers.extend(modifier.map(str::to_owned));
    match value.display {
        1 => modifiers.push("theoretical".to_owned()),
        2 => modifiers.push("auxiliary".to_owned()),
        _ => {}
    }
    let mut tolerance_resolved = resolved;
    let definition = if let Ok(definition) = PmiDimension::new(kind.clone(), nominal, bounds)
        .and_then(|definition| definition.with_modifiers(modifiers.clone()))
    {
        definition
    } else {
        tolerance_resolved = false;
        PmiDimension::new(kind, nominal, None)
            .and_then(|definition| definition.with_modifiers(modifiers))
            .map_err(|error| CodecError::malformed(format_args!("Inventor dimension: {error}")))?
    };
    if !tolerance_resolved {
        outcome.tolerance_unresolved += 1;
    }
    let targets = targets(scope, &owner.intents, outcome);
    outcome.dimensions += 1;
    Ok(Some(PmiAnnotation {
        id: annotation_id(segment, owner_ordinal)?,
        name: name.map(str::to_owned),
        visible: None,
        targets,
        definition: PmiDefinition::Dimension(definition),
    }))
}

fn annotation_id(segment: &Segment<'_>, owner: u32) -> Result<PmiId, CodecError> {
    PmiId::try_from(format!("inventor:annotation:pmi#{}-{owner}", segment.token))
        .map_err(|_| CodecError::malformed("invalid Inventor annotation identity"))
}

/// The distinct targets of the intents of an owner.
fn targets(scope: &Scope<'_, '_, '_>, intents: &[u32], outcome: &mut Outcome) -> Vec<PmiTarget> {
    let mut targets = Vec::new();
    for intent in intents {
        if scope.joins.failed_intents.contains(intent) {
            outcome.unresolved_targets += 1;
        }
        let resolved = intent_targets(scope, *intent);
        if resolved.is_empty() {
            outcome.unresolved_targets += 1;
        }
        for target in resolved {
            match target {
                Some(target) if !targets.contains(&target) => targets.push(target),
                Some(_) => {}
                None => outcome.unresolved_targets += 1,
            }
        }
    }
    targets
}

/// The faces or edges that the selectors of an intent name, one for each
/// selection item. `None` marks an unresolved item.
fn intent_targets(scope: &Scope<'_, '_, '_>, intent: u32) -> Vec<Option<PmiTarget>> {
    scope
        .joins
        .items
        .get(&intent)
        .into_iter()
        .flatten()
        .map(|item| {
            let selectors = scope.joins.item_selectors(*item)?;
            agreed_target(scope, &selectors)
        })
        .collect()
}

/// The target that every selector selects, when they agree.
fn agreed_target(scope: &Scope<'_, '_, '_>, selectors: &[&Selector]) -> Option<PmiTarget> {
    let mut found: Option<PmiTarget> = None;
    for selector in selectors {
        let target = selector_target(scope, selector)?;
        match &found {
            None => found = Some(target),
            Some(previous) if *previous == target => {}
            Some(_) => return None,
        }
    }
    found
}

fn selector_target(scope: &Scope<'_, '_, '_>, selector: &Selector) -> Option<PmiTarget> {
    let table = scope.joins.tables.get(&selector.table)?;
    if selector.face {
        let face = name_entity(scope, table, selector.key, EntityKind::Face)?;
        return Some(PmiTarget::Face {
            face: FaceId::mint(face).ok()?,
        });
    }
    let record = *table.get(&selector.key)?;
    let faces = edge_name(scope.segment.record(record, EDGE_NAME)?).ok()??;
    let first = name_entity(scope, table, faces[0], EntityKind::Face)?;
    let second = name_entity(scope, table, faces[1], EntityKind::Face)?;
    let edge = scope.names.shared_edge(first, second)?;
    Some(PmiTarget::Edge {
        edge: EdgeId::mint(edge).ok()?,
    })
}

fn name_entity<'m>(
    scope: &Scope<'_, '_, 'm>,
    table: &HashMap<u32, u32>,
    key: u32,
    kind: EntityKind,
) -> Option<&'m str> {
    let record = scope.segment.records.get(table.get(&key)?)?;
    let family = NameFamily::of(record.type_id)?;
    let name = generated_name(family, record.payload).ok()?;
    scope.names.select(&name, kind)
}

/// The targets of a frame or datum owner: those of its intent, or else the
/// faces of the tolerance feature that lists it.
fn owner_targets(
    scope: &Scope<'_, '_, '_>,
    owner: u32,
    intents: &[u32],
    outcome: &mut Outcome,
) -> Vec<PmiTarget> {
    let mut intent_outcome = Outcome::default();
    let found = targets(scope, intents, &mut intent_outcome);
    if found.is_empty() {
        match scope.features.get(&owner) {
            Some(Some(feature)) => return feature_faces(scope, *feature, 0, outcome),
            Some(None) => outcome.unresolved_targets += 1,
            None => {}
        }
    }
    outcome.unresolved_targets += intent_outcome.unresolved_targets;
    found
}

/// The faces of a tolerance feature and of its member tolerance features.
fn feature_faces(
    scope: &Scope<'_, '_, '_>,
    feature: u32,
    depth: usize,
    outcome: &mut Outcome,
) -> Vec<PmiTarget> {
    let Some(parsed) = scope
        .tolerance_features
        .get(&feature)
        .and_then(Option::as_ref)
        .filter(|parsed| (parsed.attachments == 0) == scope.members.contains(&feature))
    else {
        outcome.malformed += 1;
        return Vec::new();
    };
    let mut faces = Vec::new();
    if let Some(collection) = parsed.face_collection {
        match scope.selections.collections.get(&collection) {
            Some(items) => {
                for item in items {
                    match face_item_faces(scope, *item) {
                        Some(item_faces) => {
                            for face in item_faces {
                                if !faces.contains(&face) {
                                    faces.push(face);
                                }
                            }
                        }
                        None => outcome.unresolved_targets += 1,
                    }
                }
            }
            None => outcome.unresolved_targets += 1,
        }
    }
    if depth == 0 {
        for member in &parsed.members {
            for face in feature_faces(scope, *member, depth + 1, outcome) {
                if !faces.contains(&face) {
                    faces.push(face);
                }
            }
        }
    }
    faces
}

/// The faces that the face selectors listed by a face item select, or
/// `None` when a listed selector is missing, malformed, or selects nothing.
fn face_item_faces(scope: &Scope<'_, '_, '_>, item: u32) -> Option<Vec<PmiTarget>> {
    let sources = scope.selections.face_items.get(&item)?;
    let mut faces = Vec::new();
    for source in sources {
        if scope.joins.failed_sources.contains(source) {
            return None;
        }
        let [selector] = scope.joins.feature_selectors.get(source)?.as_slice() else {
            return None;
        };
        if selector.item != item {
            return None;
        }
        faces.push(selector_target(scope, selector)?);
    }
    Some(faces)
}

/// The tolerance of a value record and its single-limit modifier.
struct Tolerance {
    bounds: Option<DimensionTolerance>,
    modifier: Option<&'static str>,
    /// The tolerance form is one of the established codes.
    resolved: bool,
}

fn tolerance(value: &Value, model_value: f64, scale: f64, quantity: PmiQuantity) -> Tolerance {
    let at = |internal: f64| PmiValue::new(internal * scale, quantity);
    let mut modifier = None;
    let tolerance = match value.tolerance {
        0 => Some(None),
        TOLERANCE_SYMMETRIC => at(-value.upper.abs())
            .zip(at(value.upper.abs()))
            .map(|(lower, upper)| Some(DimensionTolerance::PlusMinus { lower, upper })),
        TOLERANCE_DEVIATION => at(value.lower)
            .zip(at(value.upper))
            .map(|(lower, upper)| Some(DimensionTolerance::PlusMinus { lower, upper })),
        3 => at(model_value + value.lower)
            .zip(at(model_value + value.upper))
            .map(|(lower, upper)| Some(DimensionTolerance::Limits { lower, upper })),
        2 => at(value.lower)
            .zip(at(value.upper))
            .zip(fit(&value.hole_fit, &value.shaft_fit))
            .map(|((lower, upper), fit)| {
                Some(DimensionTolerance::PlusMinusFit { lower, upper, fit })
            }),
        4 => {
            modifier = Some("maximum");
            Some(None)
        }
        _ => None,
    };
    Tolerance {
        resolved: tolerance.is_some(),
        bounds: tolerance.flatten(),
        modifier,
    }
}

/// An ISO 286 tolerance class: fundamental-deviation letters and grade.
fn fit(hole: &str, shaft: &str) -> Option<LimitsAndFits> {
    let (class, zone) = match (hole.is_empty(), shaft.is_empty()) {
        (false, true) => (hole, "hole"),
        (true, false) => (shaft, "shaft"),
        _ => return None,
    };
    let split = class.find(|character: char| character.is_ascii_digit())?;
    let (letters, grade) = class.split_at(split);
    let valid_letters = if zone == "hole" {
        letters
            .chars()
            .all(|character| character.is_ascii_uppercase())
    } else {
        letters
            .chars()
            .all(|character| character.is_ascii_lowercase())
    };
    (!letters.is_empty()
        && valid_letters
        && grade.chars().all(|character| character.is_ascii_digit()))
    .then(|| LimitsAndFits {
        form_variance: letters.to_owned(),
        zone_variance: zone.to_owned(),
        grade: grade.to_owned(),
        source: String::new(),
    })
}

/// The datum annotations of a segment, and the datum annotation of each
/// datum feature.
fn datums(
    ctx: &DecodeContext<'_>,
    scope: &Scope<'_, '_, '_>,
    labels: &HashMap<u32, Option<String>>,
    outcome: &mut Outcome,
) -> Result<(Vec<PmiAnnotation>, HashMap<u32, PmiId>), CodecError> {
    let segment = scope.segment;
    let mut annotations = Vec::new();
    let mut ids = HashMap::new();
    let mut features: Vec<_> = segment
        .records
        .iter()
        .filter(|(_, record)| record.type_id == DATUM_FEATURE)
        .map(|(ordinal, record)| (*ordinal, record.payload))
        .collect();
    features.sort_by_key(|(ordinal, _)| *ordinal);
    for (feature, payload) in features {
        let Ok((owner, label)) = datum_feature(ctx, segment, payload) else {
            outcome.malformed += 1;
            continue;
        };
        let Some(Ok(intent)) = segment
            .record(owner, DATUM_OWNER)
            .map(|owner_payload| tolerance_owner(segment, owner_payload))
        else {
            outcome.malformed += 1;
            continue;
        };
        if label.is_empty() {
            outcome.malformed += 1;
            continue;
        }
        let id = annotation_id(segment, owner)?;
        let targets = owner_targets(scope, owner, intent.as_slice(), outcome);
        ids.insert(feature, id.clone());
        annotations.push(PmiAnnotation {
            id,
            name: labels.get(&owner).cloned().flatten(),
            visible: None,
            targets,
            definition: PmiDefinition::Datum {
                identification: label,
            },
        });
    }
    Ok((annotations, ids))
}

fn characteristic(code: u32) -> Option<GeometricToleranceKind> {
    Some(match code {
        1 => GeometricToleranceKind::Straightness,
        2 => GeometricToleranceKind::Flatness,
        3 => GeometricToleranceKind::Roundness,
        5 => GeometricToleranceKind::SurfaceProfile,
        7 => GeometricToleranceKind::Perpendicularity,
        9 => GeometricToleranceKind::Position,
        10 => GeometricToleranceKind::Concentricity,
        11 => GeometricToleranceKind::CircularRunout,
        12 => GeometricToleranceKind::Symmetry,
        13 => GeometricToleranceKind::TotalRunout,
        _ => return None,
    })
}

/// A geometric tolerance and its datum system from a frame owner.
fn frame(
    scope: &Scope<'_, '_, '_>,
    datum_ids: &HashMap<u32, PmiId>,
    (payload, owner_ordinal): (View<'_>, u32),
    name: Option<&str>,
    outcome: &mut Outcome,
) -> Result<Vec<PmiAnnotation>, CodecError> {
    let segment = scope.segment;
    let Ok(intent) = tolerance_owner(segment, payload) else {
        outcome.malformed += 1;
        return Ok(Vec::new());
    };
    let Some(row_payload) =
        trailing_reference(payload).and_then(|row| segment.record(row, FRAME_ROW))
    else {
        outcome.malformed += 1;
        return Ok(Vec::new());
    };
    let Ok(row) = frame_row(segment, row_payload) else {
        outcome.malformed += 1;
        return Ok(Vec::new());
    };
    let secondary_zero = row
        .secondary
        .iter()
        .all(|parameter| scope.parameters.get(parameter) == Some(&0.0));
    let magnitude = scope
        .parameters
        .get(&row.magnitude)
        .and_then(|value| PmiValue::new(value * MILLIMETRES_PER_CENTIMETRE, PmiQuantity::Length))
        .and_then(PmiMagnitude::new);
    let (Some(kind), Some(magnitude), true, true) = (
        characteristic(row.characteristic),
        magnitude,
        row.plain,
        secondary_zero,
    ) else {
        outcome.frames_native += 1;
        return Ok(Vec::new());
    };
    let mut annotations = Vec::new();
    let datum_system = if row.frame_reference.is_some() {
        outcome.datum_systems_unresolved += 1;
        None
    } else {
        let references: Option<Vec<DatumReference>> = row
            .datums
            .iter()
            .zip(1_u32..)
            .map(|(datum, precedence)| {
                Some(DatumReference {
                    datum: datum_ids.get(datum)?.clone(),
                    precedence: std::num::NonZeroU32::new(precedence)?,
                    common_group: None,
                    modifiers: Vec::new(),
                })
            })
            .collect();
        match references {
            None => {
                outcome.datum_systems_unresolved += 1;
                None
            }
            Some(references) if references.is_empty() => None,
            Some(references) => {
                let id = PmiId::try_from(format!(
                    "inventor:annotation:pmi#{}-{owner_ordinal}-datums",
                    segment.token
                ))
                .map_err(|_| CodecError::malformed("invalid Inventor datum-system identity"))?;
                let references = DatumReferences::try_from(references).map_err(|error| {
                    CodecError::malformed(format_args!("Inventor datum system: {error}"))
                })?;
                annotations.push(PmiAnnotation {
                    id: id.clone(),
                    name: None,
                    visible: None,
                    targets: Vec::new(),
                    definition: PmiDefinition::DatumSystem { references },
                });
                Some(id)
            }
        }
    };
    annotations.push(PmiAnnotation {
        id: annotation_id(segment, owner_ordinal)?,
        name: name.map(str::to_owned),
        visible: None,
        targets: owner_targets(scope, owner_ordinal, intent.as_slice(), outcome),
        definition: PmiDefinition::GeometricTolerance {
            tolerance: kind,
            magnitude,
            defined_unit: None,
            defined_area_unit: None,
            defined_area_second_unit: None,
            datum_system,
            modifiers: Vec::new(),
            constraints: ToleranceConstraints::default(),
        },
    });
    outcome.frames += 1;
    Ok(annotations)
}

fn losses(outcome: &Outcome) -> Vec<LossNote> {
    let mut losses = Vec::new();
    if outcome.unresolved_targets > 0 {
        losses.push(InventorLossCode::PmiTargetUnresolved.note(format!(
            "{} annotation selection items name no unique active-body face or edge; their annotations carry no target for them",
            outcome.unresolved_targets
        )));
    }
    if outcome.tolerance_unresolved > 0 {
        losses.push(InventorLossCode::PmiToleranceUnresolved.note(format!(
            "{} dimensions store a tolerance outside the established forms or values; they transfer without a tolerance",
            outcome.tolerance_unresolved
        )));
    }
    if outcome.frames_native > 0 {
        losses.push(InventorLossCode::PmiFrameNative.note(format!(
            "{} feature control frames store a characteristic code, zone or datum modifiers, or further values outside the established grammar; they stay native",
            outcome.frames_native
        )));
    }
    if outcome.datum_systems_unresolved > 0 {
        losses.push(InventorLossCode::PmiDatumSystemUnresolved.note(format!(
            "{} feature control frames name datums through a datum reference frame or an unresolved datum feature; they transfer without a datum system",
            outcome.datum_systems_unresolved
        )));
    }
    if outcome.malformed > 0 {
        losses.push(InventorLossCode::PmiRecordMalformed.note(format!(
            "{} annotation records are outside the established grammar; their annotations or joins stay native",
            outcome.malformed
        )));
    }
    losses
}

#[cfg(test)]
mod tests;
