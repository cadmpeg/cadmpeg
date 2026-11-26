//! What the semantic writer does with each native record (cadir-v11.md §3.4,
//! Rule W3), read from the entities it emits.
//!
//! Each emitted entity lists the neutral records it states. [`Sources`]
//! resolves each of those ids to the Directory sequence of the source entity
//! it was decoded from, and [`Held`] maps each source sequence to the emitted
//! entities that state it. A native entity is Regenerated when the map holds
//! it and Dropped otherwise; every other native record follows the entity it
//! belongs to. The Directory display fields of a held entity are Regenerated
//! when every emitted entity that states it writes them with the source
//! values.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};
use cadmpeg_ir::ids::{BodyId, FaceId};
use cadmpeg_ir::native::{NativeNamespace, NativeRecord};
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::CadIr;
use serde_json::Value;

use super::{DirectoryColor, Entity, Presentation, Synthesis};
use crate::entities::geometry::SourceObjectId;

/// Native arenas whose records the semantic writer states from the neutral
/// model, each record following the entity it belongs to.
const REGENERATED_NATIVE_ARENAS: &[&str] = &[
    "boundary_vertex_sewing",
    "cards",
    "colors",
    "copious_data",
    "directions",
    "display_attributes",
    "entities",
    "manifold_solids",
    "product_properties",
    "properties",
    "transformations",
];

/// The arenas whose records each name the entity they belong to in
/// `source_entity`.
const ENTITY_VIEW_ARENAS: &[&str] = &[
    "boundary_vertex_sewing",
    "colors",
    "copious_data",
    "directions",
    "manifold_solids",
    "product_properties",
    "properties",
    "transformations",
];

/// Native arenas the semantic writer does not regenerate, with the reason the
/// loss gives.
const DROPPED_NATIVE_ARENAS: &[(&str, &str)] = &[
    (
        "product_occurrence_expansion",
        "which the bounded semantic writer does not regenerate",
    ),
    (
        "quarantined_directory_records",
        "which the bounded semantic writer does not regenerate, because a record that failed \
         typing has no fields to write",
    ),
    (
        "quarantined_parameter_records",
        "which the bounded semantic writer does not regenerate, because a record that failed \
         typing has no fields to write",
    ),
];

const UNREFERENCED_DIRECTION: &str =
    "that are directions no written analytic surface references, which the semantic writer \
     does not regenerate";
const WITHHELD_SURFACE: &str = "whose neutral surface the semantic writer withholds";
const WITHHELD_LOCATION: &str =
    "that are points that locate only analytic surfaces the semantic writer withholds";
const UNWRITTEN_LOCATION: &str = "that are points that locate analytic surfaces the semantic \
                                  writer writes without a location point";
const WITHHELD_CURVE: &str = "whose neutral curve the semantic writer withholds";
const UNCARRIED_PCURVE: &str = "that are parameter curves no written boundary carries";
const WITHHELD_POINT: &str = "whose neutral point the semantic writer withholds";
const START_CARD: &str =
    "that are Start section cards, whose text the semantic writer replaces with its own";
const UNOWNED_CARD: &str =
    "that are Directory or Parameter cards of no typed entity, which the semantic writer does \
     not write";
const OUTSIDE_SECTIONS: &str =
    "that are lines outside the five IGES sections, which the semantic writer does not write";
const UNUSED_TRANSFORMATION: &str =
    "that are transformations no written record uses, which the semantic writer does not \
     regenerate";
const UNENCODED_ENTITY: &str = "whose entity the semantic writer does not encode";
const UNENCODED_PROPERTY: &str =
    "that are properties of a form the semantic writer does not encode";
const OMITTED_NAME: &str = "whose name properties the semantic writer omits; body names \
                            transfer through the neutral model";
const DEGENERATE_LINE: &str =
    "that are lines with coincident endpoints, which the semantic writer omits";
const WITHOUT_GEOMETRY: &str =
    "whose entity has no neutral geometry for the semantic writer to encode";
const WITHOUT_SEQUENCE: &str =
    "whose record states no Directory sequence that ties it to a neutral geometry record";
const NATIVE_TOPOLOGY: &str = "that are native topology, which has no neutral projection, so \
                               the semantic writer omits it";
const WITHHELD_TOPOLOGY: &str = "whose neutral topology the semantic writer withholds";
const OPEN_SHELL: &str = "that are shells the semantic writer writes as separate trimmed \
                          surfaces with no shell record";
const JOINED_LOOP: &str = "that are loops of more than one edge of shells the semantic writer \
                           writes as trimmed surfaces, whose boundaries state the edges of a \
                           loop as one composite curve";
const UNUSED_COLOR: &str =
    "that are colour definitions no written body or face states with their colour and name";
const COLOR_NUMBER: &str = "that are colour definitions whose Directory colour number the \
                            semantic writer writes as 0";
const DISPLAY_FIELDS: &str = "that are Directory line font, level, view, or line weight \
                              fields the semantic writer writes with its own values";
const DISPLAY_COLOR: &str =
    "that are Directory colours the semantic writer does not write on the entity";
const DISPLAY_VISIBILITY: &str =
    "that are Directory blank statuses the semantic writer does not write on the entity";
const SHEET_TOPOLOGY: &str = "that are vertex or edge lists of shells the semantic writer \
                              writes as trimmed surfaces, whose boundaries state no vertex or \
                              edge list";

/// The colour and blank status the Directory entry of a written entity
/// states, and the body and the face whose colour it states.
pub(super) struct Presented {
    color: Option<Color>,
    visible: bool,
    body: Option<BodyId>,
    face: Option<FaceId>,
}

impl Presented {
    pub(super) fn of(presentation: &Presentation) -> Self {
        Self {
            color: match presentation.color {
                DirectoryColor::Standard(number) => {
                    crate::entities::presentation::standard_color(number)
                }
                DirectoryColor::Custom(color) => Color::new(color.r(), color.g(), color.b(), 1.0),
                DirectoryColor::Definition(_) => None,
            },
            visible: presentation.visible != Some(false),
            body: presentation.body.clone(),
            face: presentation.face.clone(),
        }
    }
}

/// The Directory sequence of the source Type 314 whose appearance is `id`.
pub(super) fn color_definition_sequence(id: &str) -> Option<u32> {
    let sequence = id.strip_prefix("iges:appearance:color#D")?.parse().ok()?;
    (crate::ids::appearance_color(&crate::ids::Stem::directory(sequence)).as_str() == id)
        .then_some(sequence)
}

/// What the semantic writer does with each native arena and record, from the
/// entities it emitted. An arena this declares nothing for is dropped, and
/// the export wrapper charges every dropped record.
pub(super) fn native_dispositions(
    ir: &CadIr,
    synthesis: &Synthesis,
    version: crate::IgesVersion,
) -> NativeDispositions {
    let entities = synthesis.entities.as_slice();
    let mut native = NativeDispositions::dropped("which the semantic writer does not encode");
    let Some(namespace) = ir.native.namespace("iges") else {
        return native;
    };
    for arena in REGENERATED_NATIVE_ARENAS {
        native.arena("iges", arena, NativeDisposition::Regenerated);
    }
    for (arena, reason) in DROPPED_NATIVE_ARENAS {
        native.drop_arena("iges", arena, reason);
    }
    let sources = Sources::new(ir, namespace);
    let held = Held::of(&sources, entities);
    let reasons = Reasons::new(ir, &sources, &held, entities);
    let mut drops = BTreeMap::<String, &'static str>::new();
    for record in sources.records.values() {
        let Some(sequence) = directory_sequence(record) else {
            continue;
        };
        if !held.holds(sequence) {
            drops.insert(record.id().to_owned(), reasons.of(record, sequence));
        } else if reasons.joins_edges(sequence, &sources) {
            drops.insert(record.id().to_owned(), JOINED_LOOP);
        }
    }
    for record in &sources.unsequenced {
        drops.insert(record.id().to_owned(), WITHOUT_SEQUENCE);
    }
    let mut dropped = Vec::<(String, String)>::new();
    for arena in ENTITY_VIEW_ARENAS {
        for record in namespace.arenas().get(*arena).into_iter().flatten() {
            let source = record.field("source_entity");
            match source.as_ref().and_then(|value| value.as_str()) {
                Some(source) => {
                    if let Some(reason) = drops.get(source) {
                        dropped.push((record.id().to_owned(), (*reason).to_owned()));
                    }
                }
                None if *arena == "directions" => {
                    dropped.push((record.id().to_owned(), UNREFERENCED_DIRECTION.to_owned()));
                }
                None if matches!(*arena, "product_properties" | "properties") => {
                    dropped.push((record.id().to_owned(), UNENCODED_PROPERTY.to_owned()));
                }
                None => {}
            }
        }
    }
    dropped.extend(color_number_drops(namespace, &drops));
    dropped.extend(display_drops(
        namespace,
        &sources,
        &held,
        &drops,
        &Written {
            entities,
            presented: &synthesis.presented,
            version,
        },
    ));
    dropped.extend(card_drops(namespace, &drops));
    let mut by_reason = BTreeMap::<String, Vec<String>>::new();
    for (id, reason) in drops
        .into_iter()
        .map(|(id, reason)| (id, reason.to_owned()))
        .chain(dropped)
    {
        by_reason.entry(reason).or_default().push(id);
    }
    for (reason, ids) in by_reason {
        native.drop_records("iges", &reason, ids);
    }
    native
}

/// The native entity records of the source, and the neutral records
/// projected from them.
struct Sources<'a> {
    /// Each native entity record, by Directory sequence.
    records: BTreeMap<u32, &'a NativeRecord>,
    /// The type and Parameter Data of each entity whose pointers the
    /// resolver walks: boundaries, trimmed surfaces, loops, and analytic
    /// surfaces.
    pointers: BTreeMap<u32, (i64, Vec<Value>)>,
    /// The native entity records that state no Directory sequence.
    unsequenced: Vec<&'a NativeRecord>,
    /// The source sequence of each neutral curve, surface, and point that
    /// states an IGES source object.
    objects: BTreeMap<&'a str, u32>,
    /// The owners of each source name property the writer can state.
    name_properties: BTreeMap<u32, BTreeSet<u32>>,
}

impl<'a> Sources<'a> {
    fn new(ir: &'a CadIr, namespace: &'a NativeNamespace) -> Self {
        let mut records = BTreeMap::new();
        let mut pointers = BTreeMap::new();
        let mut unsequenced = Vec::new();
        for record in namespace.arenas().get("entities").into_iter().flatten() {
            let Some(sequence) = directory_sequence(record) else {
                unsequenced.push(record);
                continue;
            };
            records.insert(sequence, record);
            if let Some(entity_type @ (141..=144 | 190..=198 | 508)) = entity_type(record) {
                let parameters = record
                    .field("parameters")
                    .and_then(|value| match value {
                        Value::Array(parameters) => Some(parameters),
                        _ => None,
                    })
                    .unwrap_or_default();
                pointers.insert(sequence, (entity_type, parameters));
            }
        }
        let object = |source: &Option<cadmpeg_ir::SourceObjectAssociation>| {
            source
                .as_ref()
                .filter(|source| source.format == cadmpeg_ir::codec_id!("iges"))
                .and_then(|source| SourceObjectId::parse(source.object_id.as_str()))
                .map(SourceObjectId::sequence)
        };
        let geometry = &ir.geometry;
        let objects = geometry
            .curves
            .iter()
            .map(|curve| (curve.id.as_str(), object(&curve.source_object)))
            .chain(
                geometry
                    .surfaces
                    .iter()
                    .map(|surface| (surface.id.as_str(), object(&surface.source_object))),
            )
            .chain(
                geometry
                    .points
                    .iter()
                    .map(|point| (point.id.as_str(), object(&point.source_object))),
            )
            .filter_map(|(id, sequence)| Some((id, sequence?)))
            .collect();
        Self {
            records,
            pointers,
            unsequenced,
            objects,
            name_properties: name_property_owners(namespace),
        }
    }

    /// The Directory sequence of the source entity the neutral record `id`
    /// was decoded from.
    fn resolve(&self, id: &str) -> Option<u32> {
        if let Some(sequence) = self.objects.get(id) {
            return Some(*sequence);
        }
        if let Some(sequence) = color_definition_sequence(id) {
            return Some(sequence);
        }
        let id = model_id(id)?;
        if let Some(sequence) = self.objects.get(id.as_str()) {
            return Some(*sequence);
        }
        let (kind, key) = id.strip_prefix("iges:model:")?.split_once('#')?;
        let pieces = key.split(':').collect::<Vec<_>>();
        match (kind, pieces.as_slice()) {
            ("pcurve", [face, boundary, segment, index])
                if sequence_piece(face)
                    .and_then(|face| self.pointers.get(&face))
                    .is_some_and(|(entity_type, _)| matches!(entity_type, 143 | 144)) =>
            {
                let boundary = self.boundary(sequence_piece(face)?, boundary.parse().ok()?)?;
                self.boundary_pcurve(boundary, segment.parse().ok()?, index.parse().ok()?)
            }
            ("pcurve", [.., loop_, use_, index]) => self.loop_pcurve(
                sequence_piece(loop_)?,
                use_.parse().ok()?,
                index.parse().ok()?,
            ),
            ("pcurve", _) => None,
            ("loop", [face, boundary]) if boundary.parse::<usize>().is_ok() => {
                match sequence_piece(face).and_then(|face| self.pointers.get(&face)) {
                    Some((143 | 144, _)) => {
                        self.boundary(sequence_piece(face)?, boundary.parse().ok()?)
                    }
                    _ => last_sequence(key),
                }
            }
            _ if ENDPOINT_SUFFIXES.iter().any(|suffix| key.ends_with(suffix)) => None,
            _ => last_sequence(key),
        }
    }

    /// The entities the analytic surface at `sequence` names.
    fn analytic_pointers(&self, sequence: u32) -> Option<AnalyticPointers> {
        let (entity_type, parameters) = self.pointers.get(&sequence)?;
        analytic_surface_pointers(*entity_type, parameters)
    }

    /// Boundary `index` of the Type 143 or 144 at `face`, in the order the
    /// decoder numbers its loops.
    fn boundary(&self, face: u32, index: usize) -> Option<u32> {
        let (entity_type, record) = self.pointers.get(&face)?;
        let parameter = match entity_type {
            143 => index.checked_add(4)?,
            144 if integer(record, 2) == Some(1) => match index.checked_sub(1) {
                None => 4,
                Some(inner) => inner.checked_add(5)?,
            },
            144 => index.checked_add(5)?,
            _ => return None,
        };
        pointer(record, parameter)
    }

    /// Parameter curve `index` of segment `segment` of the Type 141 or 142
    /// at `boundary`.
    fn boundary_pcurve(&self, boundary: u32, segment: usize, index: usize) -> Option<u32> {
        let (entity_type, record) = self.pointers.get(&boundary)?;
        match entity_type {
            141 => {
                let mut parameter = 5_usize;
                for _ in 0..segment {
                    let count =
                        usize::try_from(integer(record, parameter.checked_add(2)?)?).ok()?;
                    parameter = parameter.checked_add(3)?.checked_add(count)?;
                }
                let count = usize::try_from(integer(record, parameter.checked_add(2)?)?).ok()?;
                (index < count)
                    .then(|| pointer(record, parameter.checked_add(3)?.checked_add(index)?))
                    .flatten()
            }
            142 if segment == 0 && index == 0 => pointer(record, 3),
            _ => None,
        }
    }

    /// Parameter curve `index` of edge use `use_` of the Type 508 at
    /// `loop_`.
    fn loop_pcurve(&self, loop_: u32, use_: usize, index: usize) -> Option<u32> {
        let (508, record) = self.pointers.get(&loop_)? else {
            return None;
        };
        let mut parameter = 2_usize;
        for _ in 0..use_ {
            let count = usize::try_from(integer(record, parameter.checked_add(4)?)?).ok()?;
            parameter = parameter
                .checked_add(5)?
                .checked_add(count.checked_mul(2)?)?;
        }
        let count = usize::try_from(integer(record, parameter.checked_add(4)?)?).ok()?;
        (index < count)
            .then(|| {
                pointer(
                    record,
                    parameter
                        .checked_add(6)?
                        .checked_add(index.checked_mul(2)?)?,
                )
            })
            .flatten()
    }
}

/// The decoder id `iges:model:{kind}#{key}` a neutral id restates. A writer
/// copy `iges:writer:{kind}#{source}@{qualifier}` restates its source when
/// the source is of the same kind.
fn model_id(id: &str) -> Option<String> {
    let mut id = id.to_owned();
    loop {
        if id.starts_with("iges:model:") {
            return Some(id);
        }
        let rest = id.strip_prefix("iges:writer:")?;
        let (kind, copied) = rest.split_once('#')?;
        let copied = copied.rsplit_once('@').map_or(copied, |(copied, _)| copied);
        let source = copied.replacen('/', "#", 1);
        let source_kind = source
            .strip_prefix("iges:model:")
            .or_else(|| source.strip_prefix("iges:writer:"))?
            .split_once('#')?
            .0;
        if source_kind != kind {
            return None;
        }
        id = source;
    }
}

/// The key suffixes of the points and vertices the decoder derives at the
/// ends of a curve. Such a record states the end, and not the curve.
const ENDPOINT_SUFFIXES: [&str; 4] = ["-start", "-end", ":start", ":end"];

/// The sequence a key piece `D{n}` names.
fn sequence_piece(piece: &str) -> Option<u32> {
    SourceObjectId::parse(piece).map(SourceObjectId::sequence)
}

/// The last Directory sequence `D{n}` in a key, which names the entity the
/// keyed record was decoded from: `D{186}:D{514}:D{510}` names the Type 510,
/// and `bounded-plane-D{n}` the Type 108.
fn last_sequence(key: &str) -> Option<u32> {
    let bytes = key.as_bytes();
    let mut found = None;
    for (index, byte) in bytes.iter().enumerate() {
        let starts_piece = index == 0 || matches!(bytes[index - 1], b':' | b'-');
        if *byte != b'D' || !starts_piece {
            continue;
        }
        let digits = bytes[index + 1..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits == 0 {
            continue;
        }
        found = key[index + 1..index + 1 + digits].parse().ok().or(found);
    }
    found
}

/// Each source sequence, with the emitted entities that state it.
struct Held(BTreeMap<u32, BTreeSet<usize>>);

impl Held {
    fn of(sources: &Sources<'_>, entities: &[Entity]) -> Self {
        let mut held = BTreeMap::<u32, BTreeSet<usize>>::new();
        let resolved = |entity: &Entity| {
            entity
                .restates
                .iter()
                .filter_map(|id| sources.resolve(id))
                .collect::<BTreeSet<_>>()
        };
        // A vertex or edge list is stated again only by a written list of its
        // type: a trimmed surface boundary states its curves with no list. A
        // written Type 406 states only the names of the entity it names.
        for (index, entity) in entities.iter().enumerate() {
            if entity.type_code == 406 {
                continue;
            }
            for sequence in resolved(entity) {
                let list = sources
                    .records
                    .get(&sequence)
                    .and_then(|record| entity_type(record))
                    .filter(|entity_type| matches!(entity_type, 502 | 504));
                if list.is_none_or(|list| list == i64::from(entity.type_code)) {
                    held.entry(sequence).or_default().insert(index);
                }
            }
        }
        // A written analytic surface states its location and directions
        // again when its emitted group holds a Type 116 or Type 123.
        let analytic = held
            .iter()
            .filter_map(|(sequence, indices)| {
                let pointers = sources.analytic_pointers(*sequence)?;
                Some((pointers, indices.clone()))
            })
            .collect::<Vec<_>>();
        for (pointers, indices) in analytic {
            let of_type = |code: u32| {
                indices
                    .iter()
                    .copied()
                    .filter(|index| entities[*index].type_code == code)
                    .collect::<BTreeSet<_>>()
            };
            let points = of_type(116);
            let directions = of_type(123);
            if let Some(location) = pointers.location.filter(|_| !points.is_empty()) {
                held.entry(location).or_default().extend(&points);
            }
            if !directions.is_empty() {
                for direction in pointers.directions {
                    held.entry(direction).or_default().extend(&directions);
                }
            }
        }
        // A written Type 406 Form 15 states each source name property of its
        // owner whose value the decoder took as the owner's body name.
        for (index, entity) in entities.iter().enumerate() {
            if entity.type_code != 406 {
                continue;
            }
            let owners = resolved(entity);
            for (property, property_owners) in &sources.name_properties {
                if property_owners.iter().any(|owner| owners.contains(owner)) {
                    held.entry(*property).or_default().insert(index);
                }
            }
        }
        // A transformation is stated again as the placement of each written
        // record that names it (§3.3: it drops with the records that use it).
        let mut pending = held.keys().copied().collect::<Vec<_>>();
        while let Some(sequence) = pending.pop() {
            let Some(record) = sources.records.get(&sequence) else {
                continue;
            };
            let indices = held.get(&sequence).cloned().unwrap_or_default();
            for transformation in transformations_named(record) {
                if sources
                    .records
                    .get(&transformation)
                    .is_none_or(|record| entity_type(record) != Some(124))
                {
                    continue;
                }
                let entry = held.entry(transformation).or_default();
                let before = entry.len();
                entry.extend(&indices);
                if before == 0 || entry.len() != before {
                    pending.push(transformation);
                }
            }
        }
        Self(held)
    }

    fn holds(&self, sequence: u32) -> bool {
        self.0.contains_key(&sequence)
    }
}

/// Why each entity the writer does not state is dropped.
struct Reasons {
    curves: BTreeSet<u32>,
    surfaces: BTreeSet<u32>,
    points: BTreeSet<u32>,
    withheld_locations: BTreeSet<u32>,
    unwritten_locations: BTreeSet<u32>,
    /// Whether the document holds a neutral face.
    faces: bool,
    /// Whether the output writes faces as trimmed surfaces with no shell.
    shells_as_sheets: bool,
}

impl Reasons {
    fn new(ir: &CadIr, sources: &Sources<'_>, held: &Held, entities: &[Entity]) -> Self {
        let geometry = &ir.geometry;
        let sequences = |ids: Vec<&str>| {
            ids.into_iter()
                .filter_map(|id| sources.resolve(id))
                .collect::<BTreeSet<_>>()
        };
        let mut withheld_locations = BTreeSet::new();
        let mut unwritten_locations = BTreeSet::new();
        for sequence in sources.pointers.keys() {
            let Some(location) = sources
                .analytic_pointers(*sequence)
                .and_then(|pointers| pointers.location)
            else {
                continue;
            };
            if held.holds(*sequence) {
                unwritten_locations.insert(location);
            } else {
                withheld_locations.insert(location);
            }
        }
        Self {
            curves: sequences(
                geometry
                    .curves
                    .iter()
                    .map(|curve| curve.id.as_str())
                    .collect(),
            ),
            surfaces: sequences(
                geometry
                    .surfaces
                    .iter()
                    .map(|surface| surface.id.as_str())
                    .collect(),
            ),
            points: sequences(
                geometry
                    .points
                    .iter()
                    .map(|point| point.id.as_str())
                    .collect(),
            ),
            withheld_locations,
            unwritten_locations,
            faces: !geometry.faces.is_empty(),
            shells_as_sheets: entities
                .iter()
                .any(|entity| matches!(entity.type_code, 143 | 144))
                && !entities.iter().any(|entity| entity.type_code == 514),
        }
    }

    /// Whether the entity at `sequence` is a Type 508 loop of more than one
    /// edge use that the writer writes as one trimmed surface boundary.
    fn joins_edges(&self, sequence: u32, sources: &Sources<'_>) -> bool {
        self.shells_as_sheets
            && sources
                .pointers
                .get(&sequence)
                .is_some_and(|(entity_type, parameters)| {
                    *entity_type == 508 && integer(parameters, 1).is_some_and(|uses| uses > 1)
                })
    }

    fn of(&self, record: &NativeRecord, sequence: u32) -> &'static str {
        match entity_type(record) {
            Some(406) if is_name_form(record) => OMITTED_NAME,
            Some(406) => UNENCODED_PROPERTY,
            Some(100 | 102 | 104 | 106 | 110 | 112 | 126 | 130)
                if !(entity_type(record) == Some(106) && is_point_form(record)) =>
            {
                if record.field("use_flag").and_then(|value| value.as_i64()) == Some(5) {
                    UNCARRIED_PCURVE
                } else if degenerate_native_line(record) {
                    DEGENERATE_LINE
                } else if self.curves.contains(&sequence) {
                    WITHHELD_CURVE
                } else {
                    WITHOUT_GEOMETRY
                }
            }
            Some(106 | 116) => {
                if self.withheld_locations.contains(&sequence) {
                    WITHHELD_LOCATION
                } else if self.unwritten_locations.contains(&sequence) {
                    UNWRITTEN_LOCATION
                } else if self.points.contains(&sequence) {
                    WITHHELD_POINT
                } else {
                    WITHOUT_GEOMETRY
                }
            }
            Some(123) => UNREFERENCED_DIRECTION,
            Some(124) => UNUSED_TRANSFORMATION,
            Some(314) => UNUSED_COLOR,
            Some(108 | 114 | 118 | 120 | 122 | 128 | 140 | 190 | 192 | 194 | 196 | 198) => {
                if self.surfaces.contains(&sequence) {
                    WITHHELD_SURFACE
                } else {
                    WITHOUT_GEOMETRY
                }
            }
            Some(141..=144 | 186 | 502 | 504 | 508 | 510 | 514) => {
                if !self.faces {
                    NATIVE_TOPOLOGY
                } else if self.shells_as_sheets && matches!(entity_type(record), Some(186 | 514)) {
                    OPEN_SHELL
                } else if self.shells_as_sheets && matches!(entity_type(record), Some(502 | 504)) {
                    SHEET_TOPOLOGY
                } else {
                    WITHHELD_TOPOLOGY
                }
            }
            _ => UNENCODED_ENTITY,
        }
    }
}

/// The colour definitions of held Type 314 entities whose Directory colour
/// number is not 0: the writer writes 0 there.
fn color_number_drops(
    namespace: &NativeNamespace,
    drops: &BTreeMap<String, &'static str>,
) -> Vec<(String, String)> {
    namespace
        .arenas()
        .get("colors")
        .into_iter()
        .flatten()
        .filter(|record| {
            record
                .field("source_entity")
                .and_then(|value| value.as_str().map(|source| !drops.contains_key(source)))
                .unwrap_or(false)
                && record
                    .field("fallback_color_number")
                    .and_then(|value| value.as_i64())
                    .is_some_and(|number| number != 0)
        })
        .map(|record| (record.id().to_owned(), COLOR_NUMBER.to_owned()))
        .collect()
}

/// What the writer emitted, for the Directory display fields it writes.
struct Written<'a> {
    entities: &'a [Entity],
    presented: &'a BTreeMap<usize, Presented>,
    version: crate::IgesVersion,
}

/// The display fields the writer writes in a Directory entry.
#[derive(PartialEq)]
struct Display {
    line_font: i64,
    color: Option<Color>,
    visible: bool,
}

impl Written<'_> {
    /// The display fields of emitted entity `index`.
    fn display(&self, index: usize) -> Option<Display> {
        let entity = self.entities.get(index)?;
        let presented = self.presented.get(&index);
        Some(Display {
            line_font: super::generated_line_font(self.version, entity.type_code, entity.form),
            color: presented.and_then(|presented| presented.color),
            visible: presented.is_none_or(|presented| presented.visible),
        })
    }
}

/// The display attribute records the writer does not write: each of an
/// entity it drops, with the entity's reason, and each whose fields an
/// emitted entity that states the entity writes with other values. A Type
/// 124 is written as a transformation of its own with no colour, a visible
/// status, and the line font of a Type 124.
fn display_drops(
    namespace: &NativeNamespace,
    sources: &Sources<'_>,
    held: &Held,
    drops: &BTreeMap<String, &'static str>,
    written: &Written<'_>,
) -> Vec<(String, String)> {
    let colors = namespace
        .arenas()
        .get("colors")
        .into_iter()
        .flatten()
        .map(|record| (record.id(), record))
        .collect::<BTreeMap<_, _>>();
    let mut dropped = Vec::new();
    for record in namespace
        .arenas()
        .get("display_attributes")
        .into_iter()
        .flatten()
    {
        let source = record.field("source_entity");
        let source = source.as_ref().and_then(|value| value.as_str());
        let holders = source
            .filter(|source| !drops.contains_key(*source))
            .and_then(entity_sequence)
            .and_then(|sequence| Some((sequence, held.0.get(&sequence)?)));
        let Some((sequence, holders)) = holders else {
            let reason = source
                .and_then(|source| drops.get(source).copied())
                .unwrap_or(UNENCODED_ENTITY);
            dropped.push((record.id().to_owned(), reason.to_owned()));
            continue;
        };
        let source_type = sources
            .records
            .get(&sequence)
            .and_then(|record| entity_type(record));
        let stated = source_display(record, &colors);
        // The entity that carries the body or face of a source entity states
        // its display fields. The curves of its boundary state it with the
        // display fields of a curve.
        let presenting = holders
            .iter()
            .filter(|index| written.presented.contains_key(index))
            .collect::<Vec<_>>();
        let holders = if presenting.is_empty() {
            holders.iter().collect()
        } else {
            presenting
        };
        let writes = |index: &usize| {
            if source_type == Some(124) {
                Some(Display {
                    line_font: super::generated_line_font(written.version, 124, 0),
                    color: None,
                    visible: true,
                })
            } else {
                written.display(*index)
            }
        };
        let reason = match stated {
            Some(stated) if !holders.is_empty() => holders.into_iter().find_map(|index| {
                let display = writes(index)?;
                if display.line_font != stated.line_font {
                    Some(DISPLAY_FIELDS)
                } else if display.color != stated.color {
                    Some(DISPLAY_COLOR)
                } else if display.visible != stated.visible {
                    Some(DISPLAY_VISIBILITY)
                } else {
                    None
                }
            }),
            _ => Some(DISPLAY_FIELDS),
        };
        if let Some(reason) = reason {
            dropped.push((record.id().to_owned(), reason.to_owned()));
        }
    }
    dropped
}

/// The display fields a source display attribute record states, when the
/// writer can write each: a line font number, level 0, view 0, line weight
/// 0, and a colour that is none, a standard colour, or a colour definition.
fn source_display(
    record: &NativeRecord,
    colors: &BTreeMap<&str, &NativeRecord>,
) -> Option<Display> {
    let number = |field: &str| record.field(field).and_then(|value| value.as_i64());
    if number("level_number") != Some(0)
        || number("view") != Some(0)
        || number("line_weight_number") != Some(0)
    {
        return None;
    }
    let line_font = number("line_font_number").filter(|number| *number >= 0)?;
    let color = match number("color_number")? {
        0 => None,
        standard @ 1.. => Some(crate::entities::presentation::standard_color(standard)?),
        _ => {
            let definition = record.field("color_definition");
            let definition = colors.get(definition.as_ref()?.as_str()?)?;
            let percent = |field: &str| {
                definition
                    .field(field)
                    .and_then(|value| value.as_f64())
                    .map(|percent| percent / 100.0)
            };
            Some(Color::new(
                percent("red_percent")?,
                percent("green_percent")?,
                percent("blue_percent")?,
                1.0,
            )?)
        }
    };
    Some(Display {
        line_font,
        color,
        visible: record.field("visible").and_then(|value| value.as_bool())?,
    })
}

/// The physical cards the semantic writer does not write, each with its
/// reason: the Start section, which it writes with its own text; each
/// Directory and Parameter card of a dropped or untyped entity; and each line
/// outside the five sections. The writer writes the Global section again
/// from the source fields it restates and charges each field it does not.
fn card_drops(
    namespace: &NativeNamespace,
    drops: &BTreeMap<String, &'static str>,
) -> Vec<(String, String)> {
    let arenas = namespace.arenas();
    let integer =
        |record: &NativeRecord, field: &str| record.field(field).and_then(|value| value.as_u64());
    let mut directory_owners = BTreeMap::new();
    let mut parameter_owners = BTreeMap::new();
    for record in arenas.get("entities").into_iter().flatten() {
        if let Some(sequence) = integer(record, "directory_sequence") {
            directory_owners.insert(sequence, record.id());
            if let Some(second) = sequence.checked_add(1) {
                directory_owners.insert(second, record.id());
            }
        }
        if let Some((start, end)) = integer(record, "parameter_start").and_then(|start| {
            integer(record, "parameter_line_count")
                .and_then(|count| start.checked_add(count))
                .map(|end| (start, end))
        }) {
            parameter_owners.insert(start, (end, record.id()));
        }
    }
    let parameter_owner = |sequence: u64| {
        parameter_owners
            .range(..=sequence)
            .next_back()
            .filter(|(_, (end, _))| sequence < *end)
            .map(|(_, (_, owner))| *owner)
    };
    let mut dropped = Vec::new();
    for card in arenas.get("cards").into_iter().flatten() {
        let section = card.field("section");
        let sequence = integer(card, "sequence");
        let owner = match (section.as_ref().and_then(|value| value.as_str()), sequence) {
            (Some("terminate" | "global"), _) => continue,
            (Some("start"), _) => {
                dropped.push((card.id().to_owned(), START_CARD.to_owned()));
                continue;
            }
            (Some("directory"), Some(sequence)) => directory_owners.get(&sequence).copied(),
            (Some("parameter"), Some(sequence)) => parameter_owner(sequence),
            (Some("directory" | "parameter"), None) => None,
            _ => {
                dropped.push((card.id().to_owned(), OUTSIDE_SECTIONS.to_owned()));
                continue;
            }
        };
        match owner {
            None => dropped.push((card.id().to_owned(), UNOWNED_CARD.to_owned())),
            Some(owner) => {
                if let Some(reason) = drops.get(owner) {
                    dropped.push((card.id().to_owned(), (*reason).to_owned()));
                }
            }
        }
    }
    dropped
}

fn entity_type(record: &NativeRecord) -> Option<i64> {
    record.field("entity_type").and_then(|value| value.as_i64())
}

fn directory_sequence(record: &NativeRecord) -> Option<u32> {
    record
        .field("directory_sequence")
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
}

/// Parameter `index` of a record's Parameter Data (the entity type is
/// parameter 0), when it is an integer.
fn integer(parameters: &[Value], index: usize) -> Option<i64> {
    let value = parameters.get(index)?.get("value")?;
    (value.get("kind")?.as_str()? == "integer")
        .then(|| value.get("value")?.as_i64())
        .flatten()
}

/// Parameter `index` of a record's Parameter Data as a Directory pointer.
fn pointer(parameters: &[Value], index: usize) -> Option<u32> {
    integer(parameters, index)
        .filter(|sequence| *sequence > 0)
        .and_then(|sequence| u32::try_from(sequence).ok())
}

/// The entities an analytic pointer surface (Types 190 to 198) names, read
/// from its Parameter Data: the decoder records no reference edge for these
/// pointers.
struct AnalyticPointers {
    location: Option<u32>,
    directions: Vec<u32>,
}

fn analytic_surface_pointers(entity_type: i64, record: &[Value]) -> Option<AnalyticPointers> {
    // The parameter index of each Type 123 pointer.
    let directions: &[usize] = match entity_type {
        190 => &[2, 3],
        192 => &[2, 4],
        194 | 198 => &[2, 5],
        196 => &[3, 4],
        _ => return None,
    };
    Some(AnalyticPointers {
        location: pointer(record, 1),
        directions: directions
            .iter()
            .filter_map(|index| pointer(record, *index))
            .collect(),
    })
}

/// The Type 124 sequences `record` names: its Directory transformation
/// pointer and each resolved reference.
fn transformations_named(record: &NativeRecord) -> Vec<u32> {
    let pointer = record
        .field("transform")
        .and_then(|value| value.as_u64())
        .filter(|sequence| *sequence > 0)
        .and_then(|sequence| u32::try_from(sequence).ok());
    let references = record
        .field("references")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|reference| entity_sequence(reference.get("target")?.as_str()?));
    pointer.into_iter().chain(references).collect()
}

/// The sequence of a native entity id `iges:entity:directory#{n}`.
fn entity_sequence(id: &str) -> Option<u32> {
    id.strip_prefix("iges:entity:directory#")?.parse().ok()
}

fn is_name_form(record: &NativeRecord) -> bool {
    record.field("form").and_then(|value| value.as_i64()) == Some(15)
}

/// Whether a Type 106 record is a point form (Forms 1 to 3), whose tuples
/// project to neutral points.
fn is_point_form(record: &NativeRecord) -> bool {
    matches!(
        record.field("form").and_then(|value| value.as_i64()),
        Some(1..=3)
    )
}

/// The owners of each source Type 406 Form 15 whose one value is a name the
/// writer can state: printable ASCII text, the form it writes.
fn name_property_owners(namespace: &NativeNamespace) -> BTreeMap<u32, BTreeSet<u32>> {
    let mut owners = BTreeMap::<u32, BTreeSet<u32>>::new();
    for arena in ["product_properties", "properties"] {
        for record in namespace.arenas().get(arena).into_iter().flatten() {
            let writable = record.field("form").and_then(|value| value.as_i64()) == Some(15)
                && record
                    .field("property_kind")
                    .is_some_and(|value| value.as_str() == Some("name"))
                && record
                    .field("declared_value_count")
                    .is_none_or(|value| value.as_i64() == Some(1))
                && record
                    .field("value")
                    .and_then(|value| {
                        <cadmpeg_ir::native::bytes::NativeBytes as serde::Deserialize>::deserialize(
                            value,
                        )
                        .ok()
                    })
                    .is_some_and(|bytes| {
                        !bytes.is_empty()
                            && bytes
                                .iter()
                                .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
                    });
            let Some(source) = record
                .field("source_entity")
                .and_then(|value| value.as_str().and_then(entity_sequence))
            else {
                continue;
            };
            if !writable {
                continue;
            }
            owners.entry(source).or_default().extend(
                record
                    .field("owners")
                    .and_then(|value| value.as_array().cloned())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|owner| entity_sequence(owner.as_str()?)),
            );
        }
    }
    owners
}

fn degenerate_native_line(record: &NativeRecord) -> bool {
    if entity_type(record) != Some(110) {
        return false;
    }
    let Some(parameters) = record
        .field("parameters")
        .and_then(|value| value.as_array().cloned())
    else {
        return false;
    };
    let mut endpoints = [0.0; 6];
    for (slot, parameter) in parameters.iter().skip(1).take(6).enumerate() {
        let Some(number) = parameter
            .get("value")
            .and_then(|value| value.get("value"))
            .and_then(serde_json::Value::as_f64)
        else {
            return false;
        };
        endpoints[slot] = number;
    }
    endpoints[0] == endpoints[3] && endpoints[1] == endpoints[4] && endpoints[2] == endpoints[5]
}

pub(super) mod appearances;
#[cfg(test)]
mod tests;
