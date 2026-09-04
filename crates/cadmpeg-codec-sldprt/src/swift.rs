// SPDX-License-Identifier: Apache-2.0
//! Semantic PMI stored in the SWIFT GDT-analysis object graph.
#![warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use cadmpeg_core::decode::View;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::ids::{EdgeId, FaceId, PmiId, VertexId};
use cadmpeg_ir::pmi::{
    DatumReference, DimensionKind, DimensionTolerance, GeometricToleranceKind, PmiAnnotation,
    PmiDefinition, PmiQuantity, PmiTarget, PmiValue,
};
use cadmpeg_ir::topology::{Body, Edge, Face, Vertex};

use crate::container::ContainerScan;

const EPS_SWIFT_RENDERED_NOMINAL_E7: f64 = 1.0e-7;
const EPS_SWIFT_RENDERED_NOMINAL_E6: f64 = 1.0e-6;
const EPS_SWIFT_APPROXIMATELY_EQUAL_E9: f64 = 1.0e-9;

const ROOT_CLASS: &str = "PrizMetrik.GdtAnalysisSupport.GdtPart";
const ENTITY_TOKEN: &[u8] = b"\x06Entity";
const MAX_DEPTH: usize = 32;
const DIAMETER_EQUIVALENCE_MM: f64 = 1.0e-5;

#[derive(Debug, Clone, PartialEq)]
struct Reference {
    id: String,
    class: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct ObjectSection {
    references: Vec<Reference>,
    entities: Vec<Entity>,
}

#[derive(Debug, Clone, PartialEq)]
struct RelatedObject {
    name: String,
    class: String,
    entity: Entity,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Entity {
    offset: usize,
    class: String,
    strings: BTreeMap<String, String>,
    integers: BTreeMap<String, i32>,
    doubles: BTreeMap<String, f64>,
    features: ObjectSection,
    annotations: ObjectSection,
    related: Vec<RelatedObject>,
}

/// Exact primary topology identities addressable by a SWIFT `CadIdentifier`.
///
/// Sequence suffixes use the active source's bridge, edge-use, and vertex-use
/// sequence fields before any emitted-arena suffix lookup. Supporting geometry
/// and boundary records are intentionally not indexed: a `CadRef` to one of
/// those records remains a source `ShapeAspect` until a primary identity is
/// available.
#[derive(Debug, Default, Clone)]
pub(crate) struct TopologyIdentityIndex {
    entries: BTreeMap<u64, Vec<PmiTarget>>,
    /// `Some(target)` is an unambiguous sequence-to-primary-topology binding.
    /// `None` records a known sequence with no emitted or unique target, so it
    /// cannot fall through to an unrelated primary arena suffix.
    sequence_targets: BTreeMap<u64, Option<PmiTarget>>,
}

impl TopologyIdentityIndex {
    /// Build an index from the emitted primary topology arenas.
    pub(crate) fn from_model(
        bodies: &[Body],
        faces: &[Face],
        edges: &[Edge],
        vertices: &[Vertex],
        face_bridge_sequences: &[(u32, u16)],
        edge_use_sequences: &[(u32, u16)],
        vertex_use_sequences: &[(u32, u16)],
    ) -> Self {
        let mut index = Self::default();
        for body in bodies {
            index.insert_primary_id(
                body.id.as_str(),
                PmiTarget::Body {
                    body: body.id.clone(),
                },
            );
        }
        for edge in edges {
            index.insert_primary_id(
                edge.id.as_str(),
                PmiTarget::Edge {
                    edge: edge.id.clone(),
                },
            );
        }
        for vertex in vertices {
            index.insert_primary_id(
                vertex.id.as_str(),
                PmiTarget::Vertex {
                    vertex: vertex.id.clone(),
                },
            );
        }
        for &(sequence, attr) in face_bridge_sequences {
            let target = face_id_for_attribute(faces, attr).map(|face| PmiTarget::Face { face });
            index.insert_sequence_target(sequence, target.as_ref());
        }
        for &(sequence, attr) in edge_use_sequences {
            let target = edge_id_for_attribute(edges, attr).map(|edge| PmiTarget::Edge { edge });
            index.insert_sequence_target(sequence, target.as_ref());
        }
        for &(sequence, attr) in vertex_use_sequences {
            let target =
                vertex_id_for_attribute(vertices, attr).map(|vertex| PmiTarget::Vertex { vertex });
            index.insert_sequence_target(sequence, target.as_ref());
        }
        index
    }

    fn insert_sequence_target(&mut self, sequence: u32, target: Option<&PmiTarget>) {
        let entry = self
            .sequence_targets
            .entry(u64::from(sequence))
            .or_insert_with(|| target.cloned());
        if entry.as_ref() != target {
            *entry = None;
        }
    }

    fn insert_primary_id(&mut self, id: &str, target: PmiTarget) {
        let Some(suffix) = id.rsplit_once('#').map(|(_, suffix)| suffix) else {
            return;
        };
        let Ok(suffix) = suffix.parse::<u64>() else {
            return;
        };
        let entries = self.entries.entry(suffix).or_default();
        if !entries.contains(&target) {
            entries.push(target);
        }
    }

    fn resolve(&self, identifier: &str) -> Option<PmiTarget> {
        let (lane, suffix) = identifier.rsplit_once(':')?;
        if lane.is_empty() || suffix.is_empty() {
            return None;
        }
        let suffix = suffix.parse::<u64>().ok()?;
        if let Some(target) = self.sequence_targets.get(&suffix) {
            return target.clone();
        }
        let targets = self.entries.get(&suffix)?;
        (targets.len() == 1)
            .then(|| targets.first().cloned())
            .flatten()
    }
}

fn face_id_for_attribute(faces: &[Face], attr: u16) -> Option<FaceId> {
    let prefix = format!("sldprt:brep:face#{attr}");
    unique_id_for_attribute(faces.iter().map(|face| face.id.0.clone()), &prefix).map(FaceId)
}

fn edge_id_for_attribute(edges: &[Edge], attr: u16) -> Option<EdgeId> {
    let prefix = format!("sldprt:brep:edge#{attr}");
    unique_id_for_attribute(edges.iter().map(|edge| edge.id.0.clone()), &prefix).map(EdgeId)
}

fn vertex_id_for_attribute(vertices: &[Vertex], attr: u16) -> Option<VertexId> {
    let prefix = format!("sldprt:brep:vertex#{attr}");
    unique_id_for_attribute(vertices.iter().map(|vertex| vertex.id.0.clone()), &prefix)
        .map(VertexId)
}

fn unique_id_for_attribute<I>(ids: I, prefix: &str) -> Option<String>
where
    I: IntoIterator<Item = String>,
{
    let ids = ids
        .into_iter()
        .filter(|id| {
            id == prefix
                || id
                    .strip_prefix(prefix)
                    .is_some_and(|suffix| suffix.starts_with('@'))
        })
        .collect::<BTreeSet<_>>();
    if let Some(id) = ids.iter().find(|id| id.as_str() == prefix) {
        return Some((*id).clone());
    }
    let mut ids = ids.into_iter();
    let first = ids.next()?;
    ids.next().is_none().then_some(first)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct RenderedDimension {
    kind: RenderedDimensionKind,
    value: f64,
    decimal_places: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderedDimensionKind {
    Diameter,
    Depth,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ImplicitNominal {
    Exact(f64),
    Rendered {
        kind: RenderedDimensionKind,
        geometry: f64,
    },
    RenderedOrExact {
        kind: RenderedDimensionKind,
        geometry: f64,
        exact: f64,
    },
}

/// Decode the unique GDT-analysis root carried by a SWIFT schema stream.
pub(crate) fn annotations(
    scan: &ContainerScan<'_>,
    annotations: &mut Annotations,
    topology: Option<&TopologyIdentityIndex>,
    pattern_hole_nominals: Option<&BTreeMap<String, f64>>,
) -> Vec<PmiAnnotation> {
    let Some((stream, root, rendered_dimensions)) = scan_root(scan) else {
        return Vec::new();
    };
    let projected =
        project_with_topology(&root, topology, &rendered_dimensions, pattern_hole_nominals);
    for (reference, entity) in root
        .annotations
        .references
        .iter()
        .zip(&root.annotations.entities)
    {
        let prefix = pmi_id(&reference.id).0;
        for annotation in projected.iter().filter(|annotation| {
            annotation.id.0 == prefix || annotation.id.as_str().starts_with(&format!("{prefix}:"))
        }) {
            crate::annotations::note(
                annotations,
                annotation.id.0.clone(),
                stream.clone(),
                entity.offset as u64,
                "swift_gdt_analysis",
                cadmpeg_ir::Exactness::ByteExact,
            );
        }
    }
    projected
}

/// Build the native-history join used when a SWIFT hole-pattern graph omits all
/// applied members and CAD identifiers. The map is keyed by the SWIFT object
/// name (`Hole PatternN`) and is populated only by one unambiguous native
/// `LPatternN` whose sole seed is consumed by exactly one later Hole feature.
pub(crate) fn pattern_hole_nominal_context(
    features: &[cadmpeg_ir::features::Feature],
) -> BTreeMap<String, f64> {
    let mut candidates = BTreeMap::<String, Vec<f64>>::new();
    for pattern in features {
        let Some(name) = pattern.name.as_deref() else {
            continue;
        };
        let Some(semantic_name) = semantic_pattern_name(name) else {
            continue;
        };
        if !pattern
            .native_ref
            .as_deref()
            .is_some_and(|native| native.starts_with("sldprt:history:feature#"))
        {
            continue;
        }
        let cadmpeg_ir::features::FeatureDefinition::Pattern { seeds, .. } = &pattern.definition
        else {
            continue;
        };
        let [cadmpeg_ir::features::PatternSeed::Feature(seed)] = seeds.as_slice() else {
            continue;
        };
        if !features
            .iter()
            .any(|candidate| candidate.id == *seed && candidate.ordinal < pattern.ordinal)
        {
            continue;
        }
        let holes = features
            .iter()
            .filter(|candidate| candidate.ordinal > pattern.ordinal)
            .filter(|candidate| {
                candidate
                    .dependencies
                    .iter()
                    .any(|dependency| dependency == seed)
            })
            .filter(|candidate| {
                candidate
                    .native_ref
                    .as_deref()
                    .is_some_and(|native| native.starts_with("sldprt:history:feature#"))
            })
            .filter_map(|candidate| {
                let cadmpeg_ir::features::FeatureDefinition::Hole { diameter, .. } =
                    &candidate.definition
                else {
                    return None;
                };
                Some(
                    diameter
                        .as_ref()
                        .and_then(|cadmpeg_ir::features::Length(diameter)| {
                            diameter
                                .is_finite()
                                .then_some(*diameter)
                                .filter(|diameter| *diameter > 0.0)
                        }),
                )
            })
            .collect::<Vec<_>>();
        let [Some(diameter)] = holes.as_slice() else {
            continue;
        };
        candidates.entry(semantic_name).or_default().push(*diameter);
    }
    candidates
        .into_iter()
        .filter_map(|(name, values)| {
            let [value] = values.as_slice() else {
                return None;
            };
            Some((name, *value))
        })
        .collect()
}

fn semantic_pattern_name(native_name: &str) -> Option<String> {
    let suffix = native_name.strip_prefix("LPattern")?;
    (!suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit()))
        .then(|| format!("Hole Pattern{suffix}"))
}

pub(crate) fn unsupported_annotation_classes(scan: &ContainerScan<'_>) -> BTreeMap<String, usize> {
    let Some((_, root, _)) = scan_root(scan) else {
        return if has_root_marker(scan) {
            BTreeMap::from([("GdtAnalysisGraphUnresolved".into(), 1)])
        } else {
            BTreeMap::new()
        };
    };
    let mut classes = BTreeMap::new();
    if root.annotations.references.len() != root.annotations.entities.len() {
        classes.insert(
            "GdtAnalysisIncompleteAnnotationRoster".into(),
            root.annotations
                .references
                .len()
                .abs_diff(root.annotations.entities.len()),
        );
        return classes;
    }
    for entity in &root.annotations.entities {
        let class = short_class(&entity.class);
        if class != "GdtDatum" && tolerance_kind(class).is_none() && dimension_kind(class).is_none()
        {
            classes
                .entry(class.to_string())
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(1);
        }
    }
    classes
}

fn has_root_marker(scan: &ContainerScan<'_>) -> bool {
    scan.sections().any(|section| {
        section
            .name()
            .is_some_and(|name| name.starts_with("SWIFT/") && name.contains("Schema"))
            && section
                .payload()
                .windows(ROOT_CLASS.len())
                .any(|window| window == ROOT_CLASS.as_bytes())
    })
}

fn scan_root(scan: &ContainerScan<'_>) -> Option<(String, Entity, Vec<RenderedDimension>)> {
    let mut roots = scan
        .sections()
        .filter(|section| {
            section
                .name()
                .is_some_and(|name| name.starts_with("SWIFT/") && name.contains("Schema"))
        })
        .filter_map(|section| {
            parse_unique_root(section.payload()).map(|root| {
                (
                    section.display_name(),
                    root,
                    rendered_dimensions(section.payload()),
                )
            })
        });
    let root = roots.next()?;
    roots.next().is_none().then_some(root)
}

fn parse_unique_root(payload: &[u8]) -> Option<Entity> {
    let mut parsed = None;
    for offset in payload
        .windows(ENTITY_TOKEN.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == ENTITY_TOKEN).then_some(offset))
    {
        let mut cursor = View::over_retained(payload).child(offset, payload.len())?;
        let Some(entity) = parse_entity(&mut cursor, 0) else {
            continue;
        };
        if entity.class != ROOT_CLASS {
            continue;
        }
        if parsed.replace(entity).is_some() {
            return None;
        }
    }
    parsed
}

fn parse_entity(cursor: &mut View<'_>, depth: usize) -> Option<Entity> {
    let offset = cursor.position();
    if depth >= MAX_DEPTH || pstr(cursor)? != "Entity" {
        return None;
    }
    let class = pstr(cursor)?.to_string();
    let _assembly = pstr(cursor)?;
    let _version = cursor.u32_le()?;
    let mut entity = Entity {
        offset,
        class,
        ..Entity::default()
    };
    let mut seen_strings = false;
    let mut seen_integers = false;
    let mut seen_doubles = false;
    let mut seen_features = false;
    let mut seen_annotations = false;
    let mut seen_related = false;
    loop {
        match pstr(cursor)? {
            "Strings" if !seen_strings => {
                seen_strings = true;
                entity.strings = read_strings(cursor)?;
            }
            "Integers" if !seen_integers => {
                seen_integers = true;
                entity.integers = read_integers(cursor)?;
            }
            "Doubles" if !seen_doubles => {
                seen_doubles = true;
                entity.doubles = read_doubles(cursor)?;
            }
            "Features" if !seen_features => {
                seen_features = true;
                entity.features = read_objects(cursor, "EndFeatures", depth)?;
            }
            "Annotations" if !seen_annotations => {
                seen_annotations = true;
                entity.annotations = read_objects(cursor, "EndAnnotations", depth)?;
            }
            "RelatedObjects" if !seen_related => {
                seen_related = true;
                entity.related = read_related(cursor, depth)?;
            }
            "EndEntity" => return Some(entity),
            _ => return None,
        }
    }
}

fn read_strings(cursor: &mut View<'_>) -> Option<BTreeMap<String, String>> {
    let count = cursor.u32_le()?;
    let pairs = cursor.read_counted(u64::from(count), 2, |cursor| {
        Some((pstr(cursor)?.to_string(), pstr(cursor)?.to_string()))
    })?;
    if pstr(cursor)? != "EndStrings" {
        return None;
    }
    unique_map(pairs)
}

fn read_integers(cursor: &mut View<'_>) -> Option<BTreeMap<String, i32>> {
    let count = cursor.u32_le()?;
    let pairs = cursor.read_counted(u64::from(count), 5, |cursor| {
        Some((pstr(cursor)?.to_string(), cursor.i32_le()?))
    })?;
    if pstr(cursor)? != "EndIntegers" {
        return None;
    }
    unique_map(pairs)
}

fn read_doubles(cursor: &mut View<'_>) -> Option<BTreeMap<String, f64>> {
    let count = cursor.u32_le()?;
    let pairs = cursor.read_counted(u64::from(count), 9, |cursor| {
        Some((pstr(cursor)?.to_string(), cursor.f64_le()?))
    })?;
    if pstr(cursor)? != "EndDoubles" {
        return None;
    }
    unique_map(pairs)
}

fn unique_map<T>(pairs: Vec<(String, T)>) -> Option<BTreeMap<String, T>> {
    let mut values = BTreeMap::new();
    for (name, value) in pairs {
        if values.insert(name, value).is_some() {
            return None;
        }
    }
    Some(values)
}

fn read_objects(cursor: &mut View<'_>, end: &str, depth: usize) -> Option<ObjectSection> {
    let count = cursor.u32_le()?;
    let references = cursor.read_counted(u64::from(count), 2, |cursor| {
        Some(Reference {
            id: pstr(cursor)?.to_string(),
            class: pstr(cursor)?.to_string(),
        })
    })?;
    let mut entities = Vec::new();
    while peek_pstr(cursor)? == "Entity" {
        if entities.len() >= references.len() {
            return None;
        }
        entities.push(parse_entity(cursor, depth.checked_add(1)?)?);
    }
    if !references
        .iter()
        .zip(&entities)
        .all(|(reference, entity)| reference_matches_entity(reference, entity))
    {
        return None;
    }
    if pstr(cursor)? != end {
        return None;
    }
    Some(ObjectSection {
        references,
        entities,
    })
}

fn read_related(cursor: &mut View<'_>, depth: usize) -> Option<Vec<RelatedObject>> {
    let count = cursor.u32_le()?;
    let descriptors = cursor.read_counted(u64::from(count), 2, |cursor| {
        Some((pstr(cursor)?.to_string(), pstr(cursor)?.to_string()))
    })?;
    let mut related = Vec::with_capacity(descriptors.len());
    for (name, class) in descriptors {
        let entity = parse_entity(cursor, depth.checked_add(1)?)?;
        if class
            .split_once(',')
            .map_or(class.as_str(), |(name, _)| name)
            != entity.class
        {
            return None;
        }
        related.push(RelatedObject {
            name,
            class,
            entity,
        });
    }
    if pstr(cursor)? != "EndRelatedObjects" {
        return None;
    }
    Some(related)
}

fn reference_matches_entity(reference: &Reference, entity: &Entity) -> bool {
    reference
        .class
        .split_once(',')
        .map_or(reference.class.as_str(), |(class, _)| class)
        == entity.class
}

fn pstr<'a>(cursor: &mut View<'a>) -> Option<&'a str> {
    let len = usize::from(cursor.u8()?);
    std::str::from_utf8(cursor.take(len)?).ok()
}

fn peek_pstr<'a>(cursor: &View<'a>) -> Option<&'a str> {
    let mut probe = *cursor;
    pstr(&mut probe)
}

#[cfg(test)]
fn project(root: &Entity) -> Vec<PmiAnnotation> {
    project_with_topology(root, None, &[], None)
}

#[cfg(test)]
fn enrich_implicit_nominals(
    root: &Entity,
    rendered: &[RenderedDimension],
    annotations: &mut Vec<PmiAnnotation>,
) {
    *annotations = project_with_topology(root, None, rendered, None);
}

#[cfg(test)]
fn enrich_implicit_nominals_with_context(
    root: &Entity,
    rendered: &[RenderedDimension],
    annotations: &mut Vec<PmiAnnotation>,
    pattern_hole_nominals: Option<&BTreeMap<String, f64>>,
) {
    *annotations = project_with_topology(root, None, rendered, pattern_hole_nominals);
}

fn project_with_topology(
    root: &Entity,
    topology: Option<&TopologyIdentityIndex>,
    rendered: &[RenderedDimension],
    pattern_hole_nominals: Option<&BTreeMap<String, f64>>,
) -> Vec<PmiAnnotation> {
    if root.annotations.references.len() != root.annotations.entities.len() {
        return Vec::new();
    }
    let rows = root
        .annotations
        .references
        .iter()
        .zip(&root.annotations.entities)
        .collect::<Vec<_>>();
    let feature_index = feature_index(root);
    let datum_ids = rows
        .iter()
        .filter(|(_, entity)| {
            short_class(&entity.class) == "GdtDatum"
                && !suppressed(entity)
                && entity
                    .strings
                    .get("DatumIdentifier")
                    .is_some_and(|value| !value.is_empty())
        })
        .map(|(reference, _)| (reference.id.as_str(), pmi_id(&reference.id)))
        .collect::<BTreeMap<_, _>>();
    let mut projected = Vec::new();
    let mut datum_systems = Vec::<(Vec<DatumReference>, PmiId)>::new();
    for (reference, entity) in &rows {
        if suppressed(entity) {
            continue;
        }
        if let Some(annotation) = project_datum(reference, entity, &feature_index, topology) {
            projected.push(annotation);
        }
    }
    for (reference, entity) in rows {
        if suppressed(entity) || short_class(&entity.class) == "GdtDatum" {
            continue;
        }
        if let Some((system, mut annotation)) =
            project_tolerance(reference, entity, &datum_ids, &feature_index, topology)
        {
            if let Some(system) = system {
                let PmiDefinition::DatumSystem { references } = &system.definition else {
                    unreachable!("projected datum system definition");
                };
                if let Some((_, id)) = datum_systems
                    .iter()
                    .find(|(candidate, _)| candidate == references)
                {
                    let PmiDefinition::GeometricTolerance { datum_system, .. } =
                        &mut annotation.definition
                    else {
                        unreachable!("projected geometric tolerance definition");
                    };
                    *datum_system = Some(id.clone());
                } else {
                    datum_systems.push((references.clone(), system.id.clone()));
                    projected.push(system);
                }
            }
            projected.push(annotation);
            if short_class(&entity.class) == "GdtCompositeSurfaceProfile" {
                if let Some(lower_tier) =
                    project_lower_profile_tier(reference, entity, &feature_index, topology)
                {
                    projected.push(lower_tier);
                }
            }
        } else if let Some(annotation) = project_dimension(
            root,
            reference,
            entity,
            &feature_index,
            topology,
            rendered,
            pattern_hole_nominals,
        ) {
            projected.push(annotation);
        }
    }
    projected
}

fn project_datum(
    reference: &Reference,
    entity: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    topology: Option<&TopologyIdentityIndex>,
) -> Option<PmiAnnotation> {
    let identification = entity
        .strings
        .get("DatumIdentifier")
        .filter(|value| !value.is_empty())?
        .clone();
    (short_class(&entity.class) == "GdtDatum").then(|| PmiAnnotation {
        id: pmi_id(&reference.id),
        name: object_name(entity),
        visible: None,
        targets: targets(entity, feature_index, topology),
        definition: PmiDefinition::Datum { identification },
    })
}

fn project_tolerance(
    reference: &Reference,
    entity: &Entity,
    datum_ids: &BTreeMap<&str, PmiId>,
    feature_index: &BTreeMap<&str, &Entity>,
    topology: Option<&TopologyIdentityIndex>,
) -> Option<(Option<PmiAnnotation>, PmiAnnotation)> {
    let kind = tolerance_kind(short_class(&entity.class))?;
    let magnitude = finite_nonnegative(entity.doubles.get("Tolerance").copied()?)?;
    let references = datum_references(entity, datum_ids);
    let system = (!references.is_empty()).then(|| {
        let id = PmiId::mint(format!("{}:datum-system", pmi_id(&reference.id).0))
            .expect("identity grammar");
        PmiAnnotation {
            id,
            name: None,
            visible: None,
            targets: Vec::new(),
            definition: PmiDefinition::DatumSystem { references },
        }
    });
    let datum_system = system.as_ref().map(|system| system.id.clone());
    let (defined_unit, defined_area_unit, defined_area_second_unit) = defined_area(entity);
    Some((
        system,
        PmiAnnotation {
            id: pmi_id(&reference.id),
            name: object_name(entity),
            visible: None,
            targets: targets(entity, feature_index, topology),
            definition: PmiDefinition::GeometricTolerance {
                tolerance: kind,
                magnitude: length(magnitude),
                defined_unit,
                defined_area_unit,
                defined_area_second_unit,
                datum_system,
                modifiers: tolerance_modifiers(entity),
            },
        },
    ))
}

fn project_lower_profile_tier(
    reference: &Reference,
    entity: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    topology: Option<&TopologyIdentityIndex>,
) -> Option<PmiAnnotation> {
    let magnitude = finite_nonnegative(entity.doubles.get("ToleranceLowerTier").copied()?)?;
    Some(PmiAnnotation {
        id: PmiId::mint(format!("{}:lower-tier", pmi_id(&reference.id).0))
            .expect("identity grammar"),
        name: object_name(entity).map(|name| format!("{name} lower tier")),
        visible: None,
        targets: targets(entity, feature_index, topology),
        definition: PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::SurfaceProfile,
            magnitude: length(magnitude),
            defined_unit: None,
            defined_area_unit: None,
            defined_area_second_unit: None,
            datum_system: None,
            modifiers: vec!["composite_lower_tier".into()],
        },
    })
}

fn project_dimension(
    root: &Entity,
    reference: &Reference,
    entity: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    topology: Option<&TopologyIdentityIndex>,
    rendered: &[RenderedDimension],
    pattern_hole_nominals: Option<&BTreeMap<String, f64>>,
) -> Option<PmiAnnotation> {
    let dimension = dimension_kind(short_class(&entity.class))?;
    let quantity = dimension_quantity(&dimension);
    let nominal = finite(entity.doubles.get("Nominal").copied()?)
        .filter(|value| {
            *value != 0.0
                || entity
                    .integers
                    .get("Dimension")
                    .is_some_and(|dimension| *dimension != 0)
        })
        .or_else(|| {
            implicit_dimension_nominal(root, entity, feature_index, rendered, pattern_hole_nominals)
        })?;
    let tolerance = match (
        deviation(entity, Some(nominal), "LowerLimit", "MinusTolerance"),
        deviation(entity, Some(nominal), "UpperLimit", "PlusTolerance"),
    ) {
        (Some(lower), Some(upper)) => Some(DimensionTolerance::PlusMinus {
            lower: pmi_value(lower, quantity),
            upper: pmi_value(upper, quantity),
        }),
        _ => None,
    };
    Some(PmiAnnotation {
        id: pmi_id(&reference.id),
        name: object_name(entity),
        visible: None,
        targets: targets(entity, feature_index, topology),
        definition: PmiDefinition::Dimension {
            dimension,
            nominal: pmi_value(nominal, quantity),
            tolerance,
        },
    })
}

fn implicit_dimension_nominal(
    root: &Entity,
    entity: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    rendered: &[RenderedDimension],
    pattern_hole_nominals: Option<&BTreeMap<String, f64>>,
) -> Option<f64> {
    let source = match short_class(&entity.class) {
        "GdtDiameter" => diameter_nominal(root, entity, feature_index, pattern_hole_nominals),
        "GdtDepth" => depth_nominal(root, entity, feature_index),
        "GdtWidth" => {
            width_from_applied_geometry(entity, feature_index).map(ImplicitNominal::Exact)
        }
        "GdtRadius" => {
            radius_from_applied_geometry(entity, feature_index).map(ImplicitNominal::Exact)
        }
        "GdtLength" => {
            length_from_applied_geometry(entity, feature_index).map(ImplicitNominal::Exact)
        }
        "GdtDistanceBetween" => {
            directional_distance(entity, feature_index).map(ImplicitNominal::Exact)
        }
        "GdtCounterBore" => {
            counterbore_from_direct_geometry(entity, feature_index).map(ImplicitNominal::Exact)
        }
        "GdtCounterSinkDiameter" => {
            countersink_diameter_from_direct_geometry(entity, feature_index)
                .map(ImplicitNominal::Exact)
        }
        "GdtCounterSinkAngle" => countersink_angle_from_direct_geometry(entity, feature_index)
            .map(ImplicitNominal::Exact),
        _ => None,
    }?;
    match source {
        ImplicitNominal::Exact(value) => Some(value),
        ImplicitNominal::Rendered { kind, geometry } => entity
            .integers
            .get("BlockToleranceDecimalPlaces")
            .copied()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value <= 9)
            .and_then(|decimal_places| rendered_nominal(geometry, decimal_places, kind, rendered)),
        ImplicitNominal::RenderedOrExact {
            kind,
            geometry,
            exact,
        } => entity
            .integers
            .get("BlockToleranceDecimalPlaces")
            .copied()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value <= 9)
            .and_then(|decimal_places| rendered_nominal(geometry, decimal_places, kind, rendered))
            .or(Some(exact)),
    }
}

fn dimension_quantity(dimension: &DimensionKind) -> PmiQuantity {
    matches!(dimension, DimensionKind::Angular)
        .then_some(PmiQuantity::Angle)
        .unwrap_or(PmiQuantity::Length)
}

fn diameter_from_applied_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    unique_diameter(&diameter_contributors(annotation, feature_index))
}

fn directional_distance(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    if annotation.integers.get("ComputeAnswerBy") != Some(&0)
        || annotation.integers.get("Direction") != Some(&4)
        || annotation.integers.get("NormalTo") != Some(&1)
        || !identity_transform(&unique_related(annotation, "NominalTransform")?.entity)
    {
        return None;
    }
    let direction_entity = &unique_related(annotation, "DirectionVector")?.entity;
    let direction = vector(direction_entity, ["I", "J", "K"])?;
    if !approximately_equal(direction[0].hypot(direction[1]).hypot(direction[2]), 1.0) {
        return None;
    }
    if let Some(length) = closed_slot_feature_size_distance(annotation, feature_index, direction) {
        return Some(length);
    }
    let [first, second] = annotation.features.references.as_slice() else {
        return None;
    };
    let first = location_projection(&first.id, feature_index, direction)?;
    let second = location_projection(&second.id, feature_index, direction)?;
    finite_positive((second - first).abs())
}

fn closed_slot_feature_size_distance(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    direction: [f64; 3],
) -> Option<f64> {
    if annotation.integers.get("FeatureFosUsage") != Some(&2)
        || annotation.integers.get("OriginFeatureFosUsage") != Some(&2)
    {
        return None;
    }
    let [first_reference, second_reference] = annotation.features.references.as_slice() else {
        return None;
    };
    let first = feature_index.get(first_reference.id.as_str())?;
    let second = feature_index.get(second_reference.id.as_str())?;
    let (cylinder_id, cylinder, slot_id, slot) =
        match (short_class(&first.class), short_class(&second.class)) {
            ("GdtCylinder", "GdtCompoundClosedSlot3D") => (
                first_reference.id.as_str(),
                *first,
                second_reference.id.as_str(),
                *second,
            ),
            ("GdtCompoundClosedSlot3D", "GdtCylinder") => (
                second_reference.id.as_str(),
                *second,
                first_reference.id.as_str(),
                *first,
            ),
            _ => return None,
        };
    if !feature_reaches(slot_id, cylinder_id, feature_index, &mut BTreeSet::new(), 0) {
        return None;
    }
    let slot_geometry = &unique_related(slot, "NomClosedSlot")?.entity;
    let cylinder_geometry = &unique_related(cylinder, "NomCylinder")?.entity;
    let length = finite_positive(slot_geometry.doubles.get("Length").copied()?)?;
    let width = finite_positive(slot_geometry.doubles.get("Width").copied()?)?;
    let radius = finite_positive(cylinder_geometry.doubles.get("R").copied()?)?;
    if length <= width || !diameters_equivalent(radius * 2.0, width) {
        return None;
    }
    let slot_normal = vector(slot_geometry, ["I", "J", "K"])?;
    let longitude = vector(slot_geometry, ["LongitudeI", "LongitudeJ", "LongitudeK"])?;
    let slot_point = vector(slot_geometry, ["X", "Y", "Z"])?;
    let cylinder_axis = vector(cylinder_geometry, ["I", "J", "K"])?;
    let cylinder_point = vector(cylinder_geometry, ["X", "Y", "Z"])?;
    if !approximately_equal(
        slot_normal[0].hypot(slot_normal[1]).hypot(slot_normal[2]),
        1.0,
    ) || !approximately_equal(longitude[0].hypot(longitude[1]).hypot(longitude[2]), 1.0)
        || !approximately_equal(
            cylinder_axis[0]
                .hypot(cylinder_axis[1])
                .hypot(cylinder_axis[2]),
            1.0,
        )
        || !approximately_equal(
            (slot_normal[0] * cylinder_axis[0]
                + slot_normal[1] * cylinder_axis[1]
                + slot_normal[2] * cylinder_axis[2])
                .abs(),
            1.0,
        )
        || !approximately_equal(
            slot_normal[0] * longitude[0]
                + slot_normal[1] * longitude[1]
                + slot_normal[2] * longitude[2],
            0.0,
        )
        || !approximately_equal(
            (longitude[0] * direction[0]
                + longitude[1] * direction[1]
                + longitude[2] * direction[2])
                .abs(),
            1.0,
        )
    {
        return None;
    }
    let displacement = [
        cylinder_point[0] - slot_point[0],
        cylinder_point[1] - slot_point[1],
        cylinder_point[2] - slot_point[2],
    ];
    let displacement_norm = displacement[0]
        .hypot(displacement[1])
        .hypot(displacement[2]);
    let longitudinal = displacement[0] * longitude[0]
        + displacement[1] * longitude[1]
        + displacement[2] * longitude[2];
    (approximately_equal(displacement_norm, longitudinal.abs())
        && approximately_equal(longitudinal.abs(), (length - width) / 2.0))
    .then_some(length)
}

fn feature_reaches(
    id: &str,
    target: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> bool {
    if depth >= MAX_DEPTH || !visited.insert(id.to_string()) {
        return false;
    }
    let Some(feature) = feature_index.get(id) else {
        return false;
    };
    let Some(next_depth) = depth.checked_add(1) else {
        return false;
    };
    child_feature_ids(feature).into_iter().any(|child| {
        child == target
            || feature_reaches(
                child,
                target,
                feature_index,
                &mut visited.clone(),
                next_depth,
            )
    })
}

fn identity_transform(transform: &Entity) -> bool {
    [
        ("R1C1", 1.0),
        ("R1C2", 0.0),
        ("R1C3", 0.0),
        ("R2C1", 0.0),
        ("R2C2", 1.0),
        ("R2C3", 0.0),
        ("R3C1", 0.0),
        ("R3C2", 0.0),
        ("R3C3", 1.0),
        ("X", 0.0),
        ("Y", 0.0),
        ("Z", 0.0),
    ]
    .into_iter()
    .all(|(name, expected)| {
        transform
            .doubles
            .get(name)
            .is_some_and(|value| approximately_equal(*value, expected))
    })
}

fn location_projection(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    direction: [f64; 3],
) -> Option<f64> {
    let feature = feature_index.get(id)?;
    match short_class(&feature.class) {
        "GdtPlane" | "GdtIntersectPlane" => plane_projection(feature, direction),
        "GdtCylinder" => axis_projection(feature, "NomCylinder", direction),
        "GdtCone" => axis_projection(feature, "NomCone", direction),
        "GdtCompoundHole" => {
            let mut projections = Vec::new();
            collect_rotational_projections(
                id,
                feature_index,
                direction,
                &mut BTreeSet::new(),
                0,
                &mut projections,
            );
            unique_measurement(&projections)
        }
        _ => None,
    }
}

fn plane_projection(feature: &Entity, direction: [f64; 3]) -> Option<f64> {
    let plane = &unique_related(feature, "NomPlane")?.entity;
    let normal = vector(plane, ["I", "J", "K"])?;
    let point = vector(plane, ["X", "Y", "Z"])?;
    if !approximately_equal(normal[0].hypot(normal[1]).hypot(normal[2]), 1.0)
        || !approximately_equal(
            (normal[0] * direction[0] + normal[1] * direction[1] + normal[2] * direction[2]).abs(),
            1.0,
        )
    {
        return None;
    }
    finite(point[0] * direction[0] + point[1] * direction[1] + point[2] * direction[2])
}

fn axis_projection(feature: &Entity, geometry: &str, direction: [f64; 3]) -> Option<f64> {
    let axis = &unique_related(feature, geometry)?.entity;
    let axis_direction = vector(axis, ["I", "J", "K"])?;
    let point = vector(axis, ["X", "Y", "Z"])?;
    if !approximately_equal(
        axis_direction[0]
            .hypot(axis_direction[1])
            .hypot(axis_direction[2]),
        1.0,
    ) || !approximately_equal(
        axis_direction[0] * direction[0]
            + axis_direction[1] * direction[1]
            + axis_direction[2] * direction[2],
        0.0,
    ) {
        return None;
    }
    finite(point[0] * direction[0] + point[1] * direction[1] + point[2] * direction[2])
}

fn collect_rotational_projections(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    direction: [f64; 3],
    visited: &mut BTreeSet<String>,
    depth: usize,
    projections: &mut Vec<f64>,
) {
    if depth >= MAX_DEPTH || !visited.insert(id.to_string()) {
        return;
    }
    let Some(feature) = feature_index.get(id) else {
        return;
    };
    let projection = match short_class(&feature.class) {
        "GdtCylinder" => axis_projection(feature, "NomCylinder", direction),
        "GdtCone" => axis_projection(feature, "NomCone", direction),
        _ => None,
    };
    if let Some(projection) = projection {
        projections.push(projection);
        return;
    }
    let Some(next_depth) = depth.checked_add(1) else {
        return;
    };
    for child in child_feature_ids(feature) {
        collect_rotational_projections(
            child,
            feature_index,
            direction,
            &mut visited.clone(),
            next_depth,
            projections,
        );
    }
}

fn diameter_nominal(
    root: &Entity,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    pattern_hole_nominals: Option<&BTreeMap<String, f64>>,
) -> Option<ImplicitNominal> {
    if let Some(geometry) = diameter_from_applied_geometry(annotation, feature_index)
        .or_else(|| hole_diameter_excluding_counterbore(root, annotation, feature_index))
    {
        return Some(ImplicitNominal::RenderedOrExact {
            kind: RenderedDimensionKind::Diameter,
            geometry,
            exact: geometry,
        });
    }
    empty_pattern_hole_nominal(annotation, feature_index, pattern_hole_nominals).map(|geometry| {
        ImplicitNominal::RenderedOrExact {
            kind: RenderedDimensionKind::Diameter,
            geometry,
            exact: geometry,
        }
    })
}

fn empty_pattern_hole_nominal(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    pattern_hole_nominals: Option<&BTreeMap<String, f64>>,
) -> Option<f64> {
    let [reference] = annotation.features.references.as_slice() else {
        return None;
    };
    let pattern = feature_index.get(reference.id.as_str())?;
    if short_class(&pattern.class) != "GdtPattern" || !pattern.features.references.is_empty() {
        return None;
    }
    let collection = unique_related(pattern, "SubFeatures")?;
    if short_class(&collection.class) != "GdtAppliedFeatureCollection"
        || !collection.entity.related.is_empty()
    {
        return None;
    }
    let cad_identifiers = cad_identifiers(pattern);
    if cad_identifiers.is_empty()
        || cad_identifiers
            .iter()
            .any(|identifier| !identifier.is_empty())
    {
        return None;
    }
    let name = object_name(pattern)?;
    pattern_hole_nominals?
        .get(&name)
        .copied()
        .filter(|diameter| diameter.is_finite() && *diameter > 0.0)
}

fn hole_diameter_excluding_counterbore(
    root: &Entity,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    let context = annotation
        .features
        .references
        .iter()
        .map(|reference| reference.id.clone())
        .collect::<BTreeSet<_>>();
    if context.is_empty() {
        return None;
    }
    let counterbore_diameters = root
        .annotations
        .entities
        .iter()
        .filter(|candidate| {
            !suppressed(candidate) && short_class(&candidate.class) == "GdtCounterBore"
        })
        .filter(|candidate| {
            direct_feature_context(candidate, feature_index, "GdtCylinder").as_ref()
                == Some(&context)
        })
        .filter_map(|candidate| counterbore_from_direct_geometry(candidate, feature_index))
        .collect::<Vec<_>>();
    let counterbore_diameter = unique_measurement(&counterbore_diameters)?;
    let contributors = diameter_contributors(annotation, feature_index);
    let remaining = contributors
        .iter()
        .copied()
        .filter(|value| !diameters_equivalent(*value, counterbore_diameter))
        .collect::<Vec<_>>();
    (remaining.len() < contributors.len())
        .then(|| unique_diameter(&remaining))
        .flatten()
}

fn diameter_contributors(annotation: &Entity, feature_index: &BTreeMap<&str, &Entity>) -> Vec<f64> {
    let mut values = Vec::new();
    for reference in &annotation.features.references {
        collect_diameter_contributors(
            &reference.id,
            feature_index,
            &mut BTreeSet::new(),
            0,
            &mut values,
        );
    }
    values
}

fn collect_diameter_contributors(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
    values: &mut Vec<f64>,
) {
    if depth >= MAX_DEPTH || !visited.insert(id.to_string()) {
        return;
    }
    let Some(feature) = feature_index.get(id) else {
        return;
    };
    let radius = match short_class(&feature.class) {
        "GdtCylinder" => nominal_radius(feature, "NomCylinder"),
        "GdtSphere" => nominal_radius(feature, "NomSphere"),
        _ => None,
    };
    if let Some(diameter) = radius.and_then(|radius| finite_positive(radius * 2.0)) {
        values.push(diameter);
        return;
    }
    let Some(next_depth) = depth.checked_add(1) else {
        return;
    };
    for child in child_feature_ids(feature) {
        collect_diameter_contributors(
            child,
            feature_index,
            &mut visited.clone(),
            next_depth,
            values,
        );
    }
}

fn depth_from_applied_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_applied_geometry(annotation, |id| {
        depth_for_feature(id, feature_index, &mut BTreeSet::new(), 0)
    })
}

fn depth_nominal(
    root: &Entity,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<ImplicitNominal> {
    if annotation
        .integers
        .get("IsThreadDepth")
        .is_some_and(|value| *value != 0)
    {
        return thread_depth_from_direct_geometry(annotation, feature_index)
            .map(ImplicitNominal::Exact);
    }
    if let Some(exact) = direct_cylinder_depth(annotation, feature_index) {
        return Some(ImplicitNominal::RenderedOrExact {
            kind: RenderedDimensionKind::Depth,
            geometry: exact,
            exact,
        });
    }
    if let Some(exact) = counterbore_depth_from_sibling(root, annotation, feature_index) {
        return Some(ImplicitNominal::RenderedOrExact {
            kind: RenderedDimensionKind::Depth,
            geometry: exact,
            exact,
        });
    }
    depth_from_applied_geometry(annotation, feature_index).map(|geometry| {
        ImplicitNominal::Rendered {
            kind: RenderedDimensionKind::Depth,
            geometry,
        }
    })
}

fn counterbore_depth_from_sibling(
    root: &Entity,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    let plane = unique_direct_feature(annotation, feature_index, "GdtPlane")?;
    let context = direct_feature_context(annotation, feature_index, "GdtPlane")?;
    let candidates = root
        .annotations
        .entities
        .iter()
        .filter(|candidate| {
            !suppressed(candidate) && short_class(&candidate.class) == "GdtCounterBore"
        })
        .filter(|candidate| {
            direct_feature_context(candidate, feature_index, "GdtCylinder").as_ref()
                == Some(&context)
        })
        .filter_map(|candidate| unique_direct_feature(candidate, feature_index, "GdtCylinder"))
        .filter(|cylinder| plane_terminates_cylinder(plane, cylinder))
        .filter_map(nominal_cylinder_depth)
        .collect::<Vec<_>>();
    unique_measurement(&candidates)
}

fn unique_direct_feature<'a>(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &'a Entity>,
    class: &str,
) -> Option<&'a Entity> {
    let mut candidates = annotation
        .features
        .references
        .iter()
        .filter_map(|reference| feature_index.get(reference.id.as_str()).copied())
        .filter(|feature| short_class(&feature.class) == class);
    let feature = candidates.next()?;
    candidates.next().is_none().then_some(feature)
}

fn direct_feature_context(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    operation_class: &str,
) -> Option<BTreeSet<String>> {
    let context = annotation
        .features
        .references
        .iter()
        .filter(|reference| {
            feature_index
                .get(reference.id.as_str())
                .is_none_or(|feature| short_class(&feature.class) != operation_class)
        })
        .map(|reference| reference.id.clone())
        .collect::<BTreeSet<_>>();
    (!context.is_empty()).then_some(context)
}

fn plane_terminates_cylinder(plane_feature: &Entity, cylinder_feature: &Entity) -> bool {
    let Some(plane) = unique_related(plane_feature, "NomPlane").map(|object| &object.entity) else {
        return false;
    };
    let Some(origin) = unique_related(plane_feature, "NomOrigin").map(|object| &object.entity)
    else {
        return false;
    };
    let Some(cylinder) =
        unique_related(cylinder_feature, "NomCylinder").map(|object| &object.entity)
    else {
        return false;
    };
    let Some(bottom) = unique_related(cylinder_feature, "NomBottom").map(|object| &object.entity)
    else {
        return false;
    };
    let Some([plane_i, plane_j, plane_k]) = vector(plane, ["I", "J", "K"]) else {
        return false;
    };
    let Some([plane_x, plane_y, plane_z]) = vector(plane, ["X", "Y", "Z"]) else {
        return false;
    };
    let Some([origin_x, origin_y, origin_z]) = vector(origin, ["X", "Y", "Z"]) else {
        return false;
    };
    let Some([axis_x, axis_y, axis_z]) = vector(cylinder, ["I", "J", "K"]) else {
        return false;
    };
    let Some([bottom_x, bottom_y, bottom_z]) = vector(bottom, ["X", "Y", "Z"]) else {
        return false;
    };
    approximately_equal(plane_i.hypot(plane_j).hypot(plane_k), 1.0)
        && approximately_equal(axis_x.hypot(axis_y).hypot(axis_z), 1.0)
        && approximately_equal(
            (plane_i * axis_x + plane_j * axis_y + plane_k * axis_z).abs(),
            1.0,
        )
        && approximately_equal(
            (origin_x - plane_x) * plane_i
                + (origin_y - plane_y) * plane_j
                + (origin_z - plane_z) * plane_k,
            0.0,
        )
        && approximately_equal(origin_x, bottom_x)
        && approximately_equal(origin_y, bottom_y)
        && approximately_equal(origin_z, bottom_z)
}

fn direct_cylinder_depth(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_direct_features(
        annotation,
        feature_index,
        "GdtCylinder",
        nominal_cylinder_depth,
    )
}

fn thread_depth_from_direct_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_direct_features(annotation, feature_index, "GdtCylinder", |feature| {
        feature
            .integers
            .get("IsThreaded")
            .is_some_and(|value| *value != 0)
            .then(|| {
                feature
                    .doubles
                    .get("ThreadDepth")
                    .copied()
                    .and_then(finite_positive)
            })
            .flatten()
    })
}

fn width_from_applied_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_applied_geometry(annotation, |id| {
        width_for_feature(id, feature_index, &mut BTreeSet::new(), 0)
    })
}

fn radius_from_applied_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_applied_geometry(annotation, |id| {
        radius_for_feature(id, feature_index, &mut BTreeSet::new(), 0)
    })
}

fn length_from_applied_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_applied_geometry(annotation, |id| {
        length_for_feature(id, feature_index, &mut BTreeSet::new(), 0)
    })
}

fn counterbore_from_direct_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_direct_features(annotation, feature_index, "GdtCylinder", |feature| {
        finite_positive(nominal_radius(feature, "NomCylinder")? * 2.0)
    })
}

fn countersink_diameter_from_direct_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_direct_features(
        annotation,
        feature_index,
        "GdtCone",
        nominal_cone_top_diameter,
    )
}

fn countersink_angle_from_direct_geometry(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Option<f64> {
    measurement_from_direct_features(annotation, feature_index, "GdtCone", nominal_cone_angle)
}

fn measurement_from_applied_geometry(
    annotation: &Entity,
    mut measurement: impl FnMut(&str) -> Option<f64>,
) -> Option<f64> {
    let candidates = annotation
        .features
        .references
        .iter()
        .filter_map(|reference| measurement(&reference.id))
        .collect::<Vec<_>>();
    unique_measurement(&candidates)
}

fn measurement_from_direct_features(
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    class: &str,
    measurement: impl Fn(&Entity) -> Option<f64>,
) -> Option<f64> {
    let candidates = annotation
        .features
        .references
        .iter()
        .filter_map(|reference| feature_index.get(reference.id.as_str()).copied())
        .filter(|feature| short_class(&feature.class) == class)
        .filter_map(measurement)
        .collect::<Vec<_>>();
    unique_measurement(&candidates)
}

fn rendered_nominal(
    raw_mm: f64,
    decimal_places: u32,
    kind: RenderedDimensionKind,
    rendered_dimensions: &[RenderedDimension],
) -> Option<f64> {
    const LENGTH_SCALES_MM: &[f64] = &[
        EPS_SWIFT_RENDERED_NOMINAL_E7,
        EPS_SWIFT_RENDERED_NOMINAL_E6,
        1.0e-3,
        0.0254,
        1.0,
        10.0,
        25.4,
        304.8,
        1000.0,
    ];
    let exponent = i32::try_from(decimal_places).ok()?;
    let precision = 10.0_f64.powi(exponent);
    let mut candidates = Vec::new();
    for scale in LENGTH_SCALES_MM {
        let rendered = (raw_mm / scale * precision).round() / precision;
        for value in rendered_dimensions
            .iter()
            .filter(|value| value.kind == kind && value.decimal_places == decimal_places)
        {
            if approximately_equal(value.value, rendered) {
                candidates.push(value.value * scale);
            }
        }
    }
    unique_measurement(&candidates)
}

fn rendered_dimensions(payload: &[u8]) -> Vec<RenderedDimension> {
    const STRING_MARKER: &[u8] = &[0xff, 0xfe, 0xff];
    payload
        .windows(STRING_MARKER.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == STRING_MARKER).then_some(offset))
        .filter_map(|offset| {
            let length_offset = offset.checked_add(STRING_MARKER.len())?;
            let units = usize::from(*payload.get(length_offset)?);
            if !(1..=128).contains(&units) {
                return None;
            }
            let start = length_offset.checked_add(1)?;
            let text = View::utf16le_at(payload, start, units)?.0;
            Some(rendered_dimension_literals(&text))
        })
        .flatten()
        .collect()
}

fn rendered_dimension_literals(text: &str) -> Vec<RenderedDimension> {
    const TOKENS: &[(&str, RenderedDimensionKind)] = &[
        ("<MOD-DIAM>", RenderedDimensionKind::Diameter),
        ("&lt;MOD-DIAM&gt;", RenderedDimensionKind::Diameter),
        ("<HOLE-DEPTH>", RenderedDimensionKind::Depth),
        ("&lt;HOLE-DEPTH&gt;", RenderedDimensionKind::Depth),
    ];
    let mut values = Vec::new();
    for (token, kind) in TOKENS {
        let mut remainder = text;
        while let Some((_, tail)) = remainder.split_once(token) {
            let literal = tail.trim_start();
            let end = literal
                .bytes()
                .position(|byte| !byte.is_ascii_digit() && !matches!(byte, b'.' | b'+' | b'-'))
                .unwrap_or(literal.len());
            let literal = literal.get(..end).unwrap_or_default();
            if let Some((_, fractional)) = literal.split_once('.') {
                let parsed = literal.parse::<f64>().ok();
                let places = u32::try_from(fractional.len()).ok();
                if !fractional.is_empty() && fractional.bytes().all(|byte| byte.is_ascii_digit()) {
                    if let (Some(value), Some(decimal_places)) = (parsed, places) {
                        if value.is_finite() && value > 0.0 {
                            values.push(RenderedDimension {
                                kind: *kind,
                                value,
                                decimal_places,
                            });
                        }
                    }
                }
            }
            remainder = tail;
        }
    }
    values
}

fn depth_for_feature(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Option<f64> {
    measurement_for_feature(id, feature_index, visited, depth, |feature| {
        (short_class(&feature.class) == "GdtCylinder")
            .then(|| nominal_cylinder_depth(feature))
            .flatten()
    })
}

fn width_for_feature(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Option<f64> {
    measurement_for_feature(
        id,
        feature_index,
        visited,
        depth,
        |feature| match short_class(&feature.class) {
            "GdtCompoundWidth" => nominal_measurement(feature, "NomCompoundWidth", "Width"),
            "GdtCompoundClosedSlot3D" => nominal_measurement(feature, "NomClosedSlot", "Width"),
            _ => None,
        },
    )
}

fn radius_for_feature(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Option<f64> {
    measurement_for_feature(
        id,
        feature_index,
        visited,
        depth,
        |feature| match short_class(&feature.class) {
            "GdtFillet" => feature
                .doubles
                .get("Radius")
                .copied()
                .and_then(finite_positive),
            "GdtCylinder" => nominal_radius(feature, "NomCylinder"),
            "GdtSphere" => nominal_radius(feature, "NomSphere"),
            _ => None,
        },
    )
}

fn length_for_feature(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Option<f64> {
    measurement_for_feature(
        id,
        feature_index,
        visited,
        depth,
        |feature| match short_class(&feature.class) {
            "GdtCompoundClosedSlot3D" => nominal_measurement(feature, "NomClosedSlot", "Length"),
            _ => None,
        },
    )
}

fn measurement_for_feature(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
    direct_measurement: impl Copy + Fn(&Entity) -> Option<f64>,
) -> Option<f64> {
    if depth >= MAX_DEPTH || !visited.insert(id.to_string()) {
        return None;
    }
    let feature = feature_index.get(id)?;
    if let Some(measurement) = direct_measurement(feature) {
        return Some(measurement);
    }
    let next_depth = depth.checked_add(1)?;
    let candidates = child_feature_ids(feature)
        .into_iter()
        .filter_map(|child| {
            measurement_for_feature(
                child,
                feature_index,
                &mut visited.clone(),
                next_depth,
                direct_measurement,
            )
        })
        .collect::<Vec<_>>();
    unique_measurement(&candidates)
}

fn child_feature_ids(feature: &Entity) -> Vec<&str> {
    let mut ids = feature
        .features
        .references
        .iter()
        .map(|reference| reference.id.as_str())
        .collect::<Vec<_>>();
    if let Some(subfeatures) = direct_subfeature_ids(feature) {
        ids.extend(subfeatures);
    }
    ids
}

fn nominal_radius(feature: &Entity, name: &str) -> Option<f64> {
    nominal_measurement(feature, name, "R")
}

fn nominal_measurement(feature: &Entity, object: &str, field: &str) -> Option<f64> {
    finite_positive(
        unique_related(feature, object)?
            .entity
            .doubles
            .get(field)
            .copied()?,
    )
}

fn nominal_cylinder_depth(feature: &Entity) -> Option<f64> {
    let cylinder = &unique_related(feature, "NomCylinder")?.entity;
    let top = &unique_related(feature, "NomTop")?.entity;
    let bottom = &unique_related(feature, "NomBottom")?.entity;
    let [i, j, k] = vector(cylinder, ["I", "J", "K"])?;
    let [top_x, top_y, top_z] = vector(top, ["X", "Y", "Z"])?;
    let [bottom_x, bottom_y, bottom_z] = vector(bottom, ["X", "Y", "Z"])?;
    let axis_norm = i.hypot(j).hypot(k);
    if !approximately_equal(axis_norm, 1.0) {
        return None;
    }
    let dx = top_x - bottom_x;
    let dy = top_y - bottom_y;
    let dz = top_z - bottom_z;
    let displacement = dx.hypot(dy).hypot(dz);
    let axial = (dx * i + dy * j + dz * k).abs();
    approximately_equal(displacement, axial)
        .then_some(axial)
        .and_then(finite_positive)
}

fn nominal_cone_angle(feature: &Entity) -> Option<f64> {
    let angle = unique_related(feature, "NomCone")?
        .entity
        .doubles
        .get("FullAngle")
        .copied()
        .and_then(finite_positive)?;
    (angle < std::f64::consts::PI).then_some(angle)
}

fn nominal_cone_top_diameter(feature: &Entity) -> Option<f64> {
    let cone = &unique_related(feature, "NomCone")?.entity;
    let top = &unique_related(feature, "NomTop")?.entity;
    let angle = nominal_cone_angle(feature)?;
    let [axis_x, axis_y, axis_z] = vector(cone, ["I", "J", "K"])?;
    let [apex_x, apex_y, apex_z] = vector(cone, ["X", "Y", "Z"])?;
    let [top_i, top_j, top_k] = vector(top, ["I", "J", "K"])?;
    let [top_x, top_y, top_z] = vector(top, ["X", "Y", "Z"])?;
    if !approximately_equal(axis_x.hypot(axis_y).hypot(axis_z), 1.0)
        || !approximately_equal(top_i.hypot(top_j).hypot(top_k), 1.0)
        || !approximately_equal(
            (axis_x * top_i + axis_y * top_j + axis_z * top_k).abs(),
            1.0,
        )
    {
        return None;
    }
    let dx = top_x - apex_x;
    let dy = top_y - apex_y;
    let dz = top_z - apex_z;
    let displacement = dx.hypot(dy).hypot(dz);
    let axial = (dx * axis_x + dy * axis_y + dz * axis_z).abs();
    if !approximately_equal(displacement, axial) {
        return None;
    }
    finite_positive(axial * (angle / 2.0).tan() * 2.0)
}

fn vector<const N: usize>(entity: &Entity, names: [&str; N]) -> Option<[f64; N]> {
    let values = names.map(|name| entity.doubles.get(name).copied().and_then(finite));
    values
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn unique_measurement(values: &[f64]) -> Option<f64> {
    let first = *values.first()?;
    values
        .iter()
        .all(|value| approximately_equal(*value, first))
        .then_some(first)
}

fn unique_diameter(values: &[f64]) -> Option<f64> {
    let first = *values.first()?;
    values
        .iter()
        .all(|value| diameters_equivalent(*value, first))
        .then_some(first)
}

fn diameters_equivalent(left: f64, right: f64) -> bool {
    approximately_equal(left, right) || (left - right).abs() <= DIAMETER_EQUIVALENCE_MM
}

fn approximately_equal(left: f64, right: f64) -> bool {
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= scale * EPS_SWIFT_APPROXIMATELY_EQUAL_E9
}

fn deviation(
    entity: &Entity,
    nominal: Option<f64>,
    limit_key: &str,
    tolerance_key: &str,
) -> Option<f64> {
    let tolerance = finite(entity.doubles.get(tolerance_key).copied()?);
    if tolerance != Some(0.0) {
        return tolerance;
    }
    if let (Some(nominal), Some(limit)) = (
        nominal,
        entity
            .doubles
            .get(limit_key)
            .copied()
            .and_then(finite)
            .filter(|limit| *limit != 0.0),
    ) {
        return finite(limit - nominal);
    }
    tolerance
}

fn datum_references(entity: &Entity, datum_ids: &BTreeMap<&str, PmiId>) -> Vec<DatumReference> {
    let mut result = Vec::new();
    for (name, precedence) in [
        ("PrimaryDatums", NonZeroU32::MIN),
        ("SecondaryDatums", NonZeroU32::MIN.saturating_add(1)),
        ("TertiaryDatums", NonZeroU32::MIN.saturating_add(2)),
    ] {
        let Some(collection) = unique_related(entity, name) else {
            continue;
        };
        let applied = collection
            .entity
            .related
            .iter()
            .filter(|object| object.class.ends_with(".GdtAppliedDatum"))
            .collect::<Vec<_>>();
        for datum in &applied {
            let [reference] = datum.entity.annotations.references.as_slice() else {
                continue;
            };
            let Some(id) = datum_ids.get(reference.id.as_str()) else {
                continue;
            };
            result.push(DatumReference {
                datum: (*id).clone(),
                precedence,
                common_group: (applied.len() > 1).then_some(precedence.get()),
                modifiers: integer_modifier(datum.entity.integers.get("Modifier").copied()),
            });
        }
    }
    result
}

fn unique_related<'a>(entity: &'a Entity, name: &str) -> Option<&'a RelatedObject> {
    let mut matches = entity.related.iter().filter(|object| object.name == name);
    let object = matches.next()?;
    matches.next().is_none().then_some(object)
}

fn feature_index(root: &Entity) -> BTreeMap<&str, &Entity> {
    if root.features.references.len() != root.features.entities.len() {
        return BTreeMap::new();
    }
    root.features
        .references
        .iter()
        .zip(&root.features.entities)
        .map(|(reference, entity)| (reference.id.as_str(), entity))
        .collect()
}

fn targets(
    entity: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    topology: Option<&TopologyIdentityIndex>,
) -> Vec<PmiTarget> {
    let mut ids = Vec::new();
    for reference in &entity.features.references {
        ids.extend(expanded_feature_ids(&reference.id, feature_index, 0));
    }
    let mut seen = BTreeSet::new();
    let mut targets = Vec::new();
    for source_id in ids.into_iter().filter(|id| seen.insert(id.clone())) {
        let Some(feature) = feature_index.get(source_id.as_str()) else {
            targets.push(PmiTarget::ShapeAspect { source_id });
            continue;
        };
        let mut had_identifier = false;
        let mut unresolved_identifier = false;
        let mut resolved = Vec::new();
        for identifier in cad_identifiers(feature) {
            had_identifier = true;
            let Some(topology) = topology else {
                unresolved_identifier = true;
                continue;
            };
            if let Some(target) = topology.resolve(identifier) {
                if !resolved.contains(&target) {
                    resolved.push(target);
                }
            } else {
                unresolved_identifier = true;
            }
        }
        let has_resolved = !resolved.is_empty();
        targets.extend(resolved);
        if !had_identifier || unresolved_identifier || !has_resolved {
            targets.push(PmiTarget::ShapeAspect { source_id });
        }
    }
    targets
}

fn cad_identifiers(feature: &Entity) -> Vec<&str> {
    fn visit<'a>(entity: &'a Entity, identifiers: &mut Vec<&'a str>) {
        if short_class(&entity.class) == "CadRef" {
            if let Some(identifier) = entity.strings.get("CadIdentifier") {
                identifiers.push(identifier.as_str());
            }
        }
        for child in &entity.features.entities {
            visit(child, identifiers);
        }
        for child in &entity.annotations.entities {
            visit(child, identifiers);
        }
        for related in &entity.related {
            visit(&related.entity, identifiers);
        }
    }

    let mut identifiers = Vec::new();
    visit(feature, &mut identifiers);
    identifiers
}

fn expanded_feature_ids(
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    depth: usize,
) -> Vec<String> {
    let Some(feature) = feature_index.get(id) else {
        return vec![id.to_string()];
    };
    if short_class(&feature.class) != "GdtPattern" || depth >= MAX_DEPTH {
        return vec![id.to_string()];
    }
    let Some(subfeatures) = direct_subfeature_ids(feature) else {
        return vec![id.to_string()];
    };
    let mut members = Vec::new();
    for subfeature in subfeatures {
        let Some(next_depth) = depth.checked_add(1) else {
            return vec![id.to_string()];
        };
        members.extend(expanded_feature_ids(subfeature, feature_index, next_depth));
    }
    if members.is_empty() {
        vec![id.to_string()]
    } else {
        members
    }
}

fn direct_subfeature_ids(feature: &Entity) -> Option<Vec<&str>> {
    let collection = unique_related(feature, "SubFeatures")?;
    let mut ids = Vec::new();
    for applied in &collection.entity.related {
        if !applied.class.ends_with(".GdtAppliedFeature") {
            return None;
        }
        let [reference] = applied.entity.features.references.as_slice() else {
            return None;
        };
        ids.push(reference.id.as_str());
    }
    (!ids.is_empty()).then_some(ids)
}

fn tolerance_modifiers(entity: &Entity) -> Vec<String> {
    let mut values = integer_modifier(entity.integers.get("Modifier").copied());
    for (key, name) in [
        ("IsFreeState", "free_state"),
        ("IsStatistical", "statistical"),
        ("IsToBeInspected", "inspection"),
        ("IsTangentPlane", "tangent_plane"),
    ] {
        if entity.integers.get(key).is_some_and(|value| *value != 0) {
            values.push(name.into());
        }
    }
    if entity
        .integers
        .get("ProjectedZoneEnabled")
        .is_some_and(|value| *value != 0)
    {
        if let Some(value) = entity
            .doubles
            .get("ProjectedZoneValue")
            .and_then(|v| finite_nonnegative(*v))
        {
            values.push(format!("projected_zone:{value}_mm"));
        } else {
            values.push("projected_zone".into());
        }
    }
    if entity
        .integers
        .get("IsMaxTolerance")
        .is_some_and(|value| *value != 0)
    {
        if let Some(value) = entity
            .doubles
            .get("MaxTolerance")
            .and_then(|v| finite_nonnegative(*v))
        {
            values.push(format!("maximum_tolerance:{value}_mm"));
        } else {
            values.push("maximum_tolerance".into());
        }
    }
    values
}

fn integer_modifier(value: Option<i32>) -> Vec<String> {
    match value {
        Some(1) => vec!["maximum_material_requirement".into()],
        Some(2) => vec!["least_material_requirement".into()],
        Some(value) if value != 0 => vec![format!("sldprt:{value}")],
        _ => Vec::new(),
    }
}

fn defined_area(entity: &Entity) -> (Option<PmiValue>, Option<String>, Option<PmiValue>) {
    if entity
        .integers
        .get("PerUnitArea")
        .is_none_or(|value| *value == 0)
    {
        return (None, None, None);
    }
    match entity.integers.get("PerUnitAreaType").copied() {
        Some(0) => (
            entity
                .doubles
                .get("PerUnitAreaLength")
                .copied()
                .and_then(finite_positive)
                .map(length),
            Some("rectangular".into()),
            entity
                .doubles
                .get("PerUnitAreaWidth")
                .copied()
                .and_then(finite_positive)
                .map(length),
        ),
        Some(1) => (
            entity
                .doubles
                .get("PerUnitAreaDiameter")
                .copied()
                .and_then(finite_positive)
                .map(length),
            Some("circular".into()),
            None,
        ),
        Some(kind) => (None, Some(format!("sldprt:{kind}")), None),
        None => (None, Some("sldprt:unspecified".into()), None),
    }
}

fn tolerance_kind(class: &str) -> Option<GeometricToleranceKind> {
    use GeometricToleranceKind as Kind;
    Some(match class {
        "GdtStraightness" => Kind::Straightness,
        "GdtFlatness" => Kind::Flatness,
        "GdtRoundness" | "GdtCircularity" => Kind::Roundness,
        "GdtCylindricity" => Kind::Cylindricity,
        "GdtCoaxiality" => Kind::Coaxiality,
        "GdtLineProfile" => Kind::LineProfile,
        "GdtSurfaceProfile" => Kind::SurfaceProfile,
        "GdtCompositeSurfaceProfile" => Kind::SurfaceProfile,
        "GdtAngularity" => Kind::Angularity,
        "GdtPerpendicularity" => Kind::Perpendicularity,
        "GdtParallelism" => Kind::Parallelism,
        "GdtPosition" => Kind::Position,
        "GdtConcentricity" => Kind::Concentricity,
        "GdtSymmetry" => Kind::Symmetry,
        "GdtCircularRunout" => Kind::CircularRunout,
        "GdtTotalRunout" => Kind::TotalRunout,
        _ => return None,
    })
}

fn dimension_kind(class: &str) -> Option<DimensionKind> {
    Some(match class {
        "GdtDiameter" => DimensionKind::Diameter,
        "GdtRadius" => DimensionKind::Radius,
        "GdtAngle" | "GdtAngleBetween" | "GdtCounterSinkAngle" => DimensionKind::Angular,
        "GdtDistanceBetween" => DimensionKind::Location,
        "GdtWidth" | "GdtLength" | "GdtDepth" | "GdtCounterBore" | "GdtCounterSinkDiameter" => {
            DimensionKind::Size
        }
        _ => return None,
    })
}

fn short_class(class: &str) -> &str {
    class.rsplit('.').next().unwrap_or(class)
}

fn object_name(entity: &Entity) -> Option<String> {
    entity
        .strings
        .get("ObjectName")
        .filter(|name| !name.is_empty())
        .cloned()
}

fn pmi_id(source_id: &str) -> PmiId {
    PmiId::mint(format!("sldprt:model:pmi#{source_id}")).expect("identity grammar")
}

fn suppressed(entity: &Entity) -> bool {
    entity
        .integers
        .get("IsSuppressed")
        .is_some_and(|value| *value != 0)
}

fn finite(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

fn finite_nonnegative(value: f64) -> Option<f64> {
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn finite_positive(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

fn length(value: f64) -> PmiValue {
    pmi_value(value, PmiQuantity::Length)
}

fn pmi_value(value: f64, quantity: PmiQuantity) -> PmiValue {
    PmiValue { value, quantity }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(id: &str, class: &str) -> Reference {
        Reference {
            id: id.into(),
            class: format!("PrizMetrik.GdtAnalysis.{class},gdtanalysis.net"),
        }
    }

    fn entity(class: &str) -> Entity {
        Entity {
            class: format!("PrizMetrik.GdtAnalysis.{class}"),
            ..Entity::default()
        }
    }

    fn semantic_root() -> Entity {
        let mut datum = entity("GdtDatum");
        datum.strings.insert("ObjectName".into(), "Datum A".into());
        datum.strings.insert("DatumIdentifier".into(), "A".into());
        datum.features.references.push(reference("F10", "GdtPlane"));

        let mut applied = entity("GdtAppliedDatum");
        applied.integers.insert("Modifier".into(), 2);
        applied
            .annotations
            .references
            .push(reference("A10", "GdtDatum"));
        let mut collection = entity("GdtAppliedDatumCollection");
        collection.related.push(RelatedObject {
            name: "SubAnnotation0".into(),
            class: "PrizMetrik.GdtAnalysis.GdtAppliedDatum".into(),
            entity: applied,
        });

        let mut position = entity("GdtPosition");
        position
            .strings
            .insert("ObjectName".into(), "Position 1".into());
        position.integers.insert("Modifier".into(), 1);
        position.integers.insert("ProjectedZoneEnabled".into(), 1);
        position.doubles.insert("Tolerance".into(), 0.25);
        position.doubles.insert("ProjectedZoneValue".into(), 4.0);
        position
            .features
            .references
            .push(reference("FP", "GdtPattern"));
        position.related.push(RelatedObject {
            name: "PrimaryDatums".into(),
            class: "PrizMetrik.GdtAnalysis.GdtAppliedDatumCollection".into(),
            entity: collection,
        });

        let mut diameter = entity("GdtDiameter");
        diameter
            .strings
            .insert("ObjectName".into(), "Diameter 1".into());
        diameter.doubles.insert("Nominal".into(), 0.0);
        diameter.doubles.insert("MinusTolerance".into(), -0.1);
        diameter.doubles.insert("PlusTolerance".into(), 0.2);
        diameter.doubles.insert("LowerLimit".into(), 0.0);
        diameter.doubles.insert("UpperLimit".into(), 0.0);
        diameter
            .features
            .references
            .push(reference("F20", "GdtCylinder"));

        let mut angle = entity("GdtAngleBetween");
        angle.integers.insert("Dimension".into(), 1);
        angle.doubles.insert("Nominal".into(), 0.0);
        angle.doubles.insert("MinusTolerance".into(), -0.01);
        angle.doubles.insert("PlusTolerance".into(), 0.01);
        angle.doubles.insert("LowerLimit".into(), 0.0);
        angle.doubles.insert("UpperLimit".into(), 0.0);

        let mut root = Entity {
            class: ROOT_CLASS.into(),
            ..Entity::default()
        };
        let cylinder = entity("GdtCylinder");
        let second_cylinder = cylinder.clone();
        let mut subfeatures = entity("GdtAppliedFeatureCollection");
        for (ordinal, id) in ["F20", "F21"].into_iter().enumerate() {
            let mut applied = entity("GdtAppliedFeature");
            applied
                .features
                .references
                .push(reference(id, "GdtCylinder"));
            subfeatures.related.push(RelatedObject {
                name: format!("SubFeature{ordinal}"),
                class: "PrizMetrik.GdtAnalysis.GdtAppliedFeature".into(),
                entity: applied,
            });
        }
        let mut pattern = entity("GdtPattern");
        pattern.related.push(RelatedObject {
            name: "SubFeatures".into(),
            class: "PrizMetrik.GdtAnalysis.GdtAppliedFeatureCollection".into(),
            entity: subfeatures,
        });
        root.features.references = vec![
            reference("FP", "GdtPattern"),
            reference("F20", "GdtCylinder"),
            reference("F21", "GdtCylinder"),
        ];
        root.features.entities = vec![pattern, cylinder, second_cylinder];
        root.annotations.references = vec![
            reference("A10", "GdtDatum"),
            reference("A20", "GdtPosition"),
            reference("A30", "GdtDiameter"),
            reference("A40", "GdtAngleBetween"),
        ];
        root.annotations.entities = vec![datum, position, diameter, angle];
        root
    }

    fn neutral_feature(
        id: &str,
        name: &str,
        ordinal: u64,
        dependencies: Vec<cadmpeg_ir::features::FeatureId>,
        definition: cadmpeg_ir::features::FeatureDefinition,
    ) -> cadmpeg_ir::features::Feature {
        cadmpeg_ir::features::Feature {
            id: cadmpeg_ir::features::FeatureId(format!("sldprt:model:feature#{id}")),
            ordinal,
            name: Some(name.into()),
            suppressed: None,
            dependencies,
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: Some(format!("sldprt:history:feature#{id}")),
        }
    }

    fn simple_hole_definition(diameter: f64) -> cadmpeg_ir::features::FeatureDefinition {
        use cadmpeg_ir::features::{FeatureDefinition, HoleKind, Length};

        FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            placements: None,
            construction: cadmpeg_ir::features::HoleConstruction::form(HoleKind::Simple),
            exit_kind: None,
            diameter: Some(Length(diameter)),
            extent: None,
            bottom: None,
            taper_angle: None,
            allow_multi_profile_faces: None,
        }
    }

    #[test]
    fn empty_swift_pattern_uses_one_native_hole_join() {
        use cadmpeg_ir::features::{FeatureDefinition, FeatureId, PatternKind, PatternSeed};

        let seed = FeatureId("sldprt:model:feature#seed".into());
        let pattern_definition = FeatureDefinition::Pattern {
            seeds: vec![PatternSeed::Feature(seed.clone())],
            pattern: PatternKind::Unresolved,
        };
        let features = vec![
            neutral_feature(
                "seed",
                "Sketch20",
                1,
                Vec::new(),
                FeatureDefinition::Native {
                    kind: "Sketch".into(),
                    parameters: BTreeMap::new(),
                },
            ),
            neutral_feature("pattern", "LPattern6", 2, Vec::new(), pattern_definition),
            neutral_feature(
                "hole",
                "Hole5",
                3,
                vec![seed],
                simple_hole_definition(6.1468),
            ),
        ];
        let context = pattern_hole_nominal_context(&features);
        assert_eq!(context.get("Hole Pattern6"), Some(&6.1468));

        let mut pattern = cad_feature("GdtPattern", "");
        pattern
            .strings
            .insert("ObjectName".into(), "Hole Pattern6".into());
        pattern.related.push(RelatedObject {
            name: "SubFeatures".into(),
            class: "PrizMetrik.GdtAnalysis.GdtAppliedFeatureCollection".into(),
            entity: entity("GdtAppliedFeatureCollection"),
        });
        let mut diameter = entity("GdtDiameter");
        diameter
            .strings
            .insert("ObjectName".into(), "Diameter 8".into());
        diameter.doubles.insert("Nominal".into(), 0.0);
        diameter.features.references = vec![reference("FP", "GdtPattern")];
        let root = Entity {
            class: ROOT_CLASS.into(),
            features: ObjectSection {
                references: vec![reference("FP", "GdtPattern")],
                entities: vec![pattern],
            },
            annotations: ObjectSection {
                references: vec![reference("A8", "GdtDiameter")],
                entities: vec![diameter],
            },
            ..Entity::default()
        };
        let mut projected = project(&root);
        enrich_implicit_nominals_with_context(&root, &[], &mut projected, Some(&context));
        let PmiDefinition::Dimension { nominal, .. } =
            &projected.first().expect("diameter annotation").definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, length(6.1468));

        let mut ambiguous = features.clone();
        ambiguous.push(neutral_feature(
            "hole2",
            "Hole6",
            4,
            vec![FeatureId("sldprt:model:feature#seed".into())],
            simple_hole_definition(6.1468),
        ));
        assert!(pattern_hole_nominal_context(&ambiguous).is_empty());

        let mut unresolved = features;
        let mut unresolved_hole = neutral_feature(
            "hole2",
            "Hole6",
            4,
            vec![FeatureId("sldprt:model:feature#seed".into())],
            simple_hole_definition(6.1468),
        );
        let FeatureDefinition::Hole { diameter, .. } = &mut unresolved_hole.definition else {
            panic!("expected hole definition");
        };
        *diameter = None;
        unresolved.push(unresolved_hole);
        assert!(pattern_hole_nominal_context(&unresolved).is_empty());
    }

    fn cad_feature(class: &str, identifier: &str) -> Entity {
        let mut cad_ref = entity("CadRef");
        cad_ref
            .strings
            .insert("CadIdentifier".into(), identifier.into());
        let mut references = entity("CadRefCollection");
        references.related.push(RelatedObject {
            name: "CadRef0".into(),
            class: "PrizMetrik.GdtAnalysis.CadRef".into(),
            entity: cad_ref,
        });
        let mut feature = entity(class);
        feature.related.push(RelatedObject {
            name: "CadReferences".into(),
            class: "PrizMetrik.GdtAnalysis.CadRefCollection".into(),
            entity: references,
        });
        feature
    }

    #[test]
    fn cad_identifier_binds_unique_primary_topology_and_preserves_fallback() {
        let mut datum = entity("GdtDatum");
        datum.strings.insert("DatumIdentifier".into(), "A".into());
        datum.features.references.push(reference("F10", "GdtPlane"));
        let mut root = Entity {
            class: ROOT_CLASS.into(),
            ..Entity::default()
        };
        root.features.references.push(reference("F10", "GdtPlane"));
        root.features
            .entities
            .push(cad_feature("GdtPlane", "125:42"));
        root.annotations
            .references
            .push(reference("A10", "GdtDatum"));
        root.annotations.entities.push(datum);

        let mut index = TopologyIdentityIndex::default();
        index.sequence_targets.insert(
            42,
            Some(PmiTarget::Face {
                face: FaceId::mint("sldprt:brep:face#42").expect("identity grammar"),
            }),
        );
        let projected = project_with_topology(&root, Some(&index), &[], None);
        let first = projected.first().expect("projected datum");
        assert_eq!(
            first.targets,
            [PmiTarget::Face {
                face: "sldprt:brep:face#42".into()
            }]
        );

        root.features
            .entities
            .first_mut()
            .expect("GdtPlane")
            .related
            .first_mut()
            .expect("CadReferences")
            .entity
            .related
            .first_mut()
            .expect("CadRef0")
            .entity
            .strings
            .insert("CadIdentifier".into(), "125:99".into());
        let projected = project_with_topology(&root, Some(&index), &[], None);
        let first = projected.first().expect("projected datum");
        assert_eq!(
            first.targets,
            [PmiTarget::ShapeAspect {
                source_id: "F10".into()
            }]
        );
    }

    #[test]
    fn cad_identifier_resolves_each_primary_topology_kind_and_rejects_collisions() {
        use cadmpeg_ir::ids::{BodyId, EdgeId, FaceId, PointId, ShellId, SurfaceId, VertexId};
        use cadmpeg_ir::topology::{Body, BodyKind, Edge, Face, Sense, Vertex};

        let body = Body {
            id: BodyId::mint("sldprt:brep:body#11").expect("identity grammar"),
            kind: BodyKind::default(),
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        };
        let face = Face {
            id: FaceId::mint("sldprt:brep:face#22").expect("identity grammar"),
            shell: ShellId::mint("sldprt:brep:shell#1").expect("identity grammar"),
            surface: SurfaceId::mint("sldprt:brep:surf#22").expect("identity grammar"),
            sense: Sense::Forward,
            loops: Vec::new().into(),
            name: None,
            color: None,
            tolerance: None,
        };
        let edge = Edge {
            id: EdgeId::mint("sldprt:brep:edge#33").expect("identity grammar"),
            curve: None,
            start: VertexId::mint("sldprt:brep:vertex#1").expect("identity grammar"),
            end: VertexId::mint("sldprt:brep:vertex#2").expect("identity grammar"),
            param_range: None,
            tolerance: None,
        };
        let vertex = Vertex {
            id: VertexId::mint("sldprt:brep:vertex#44").expect("identity grammar"),
            point: PointId::mint("sldprt:brep:point#44").expect("identity grammar"),
            tolerance: None,
        };
        let index = TopologyIdentityIndex::from_model(
            std::slice::from_ref(&body),
            std::slice::from_ref(&face),
            std::slice::from_ref(&edge),
            std::slice::from_ref(&vertex),
            &[(222, 22)],
            &[(333, 33)],
            &[(444, 44)],
        );

        assert_eq!(
            index.resolve("schema-a:11"),
            Some(PmiTarget::Body {
                body: body.id.clone(),
            })
        );
        assert_eq!(
            index.resolve("schema-b:222"),
            Some(PmiTarget::Face {
                face: face.id.clone(),
            })
        );
        assert!(index.resolve("schema-b:22").is_none());
        assert_eq!(
            index.resolve("schema-c:33"),
            Some(PmiTarget::Edge {
                edge: edge.id.clone(),
            })
        );
        assert_eq!(
            index.resolve("schema-d:44"),
            Some(PmiTarget::Vertex {
                vertex: vertex.id.clone(),
            })
        );
        assert_eq!(
            index.resolve("schema-e:333"),
            Some(PmiTarget::Edge {
                edge: edge.id.clone(),
            })
        );
        assert_eq!(
            index.resolve("schema-f:444"),
            Some(PmiTarget::Vertex {
                vertex: vertex.id.clone(),
            })
        );

        let qualified_alternate = Face {
            id: FaceId::mint("sldprt:brep:face#22@alternate").expect("identity grammar"),
            ..face.clone()
        };
        let active_with_alternate = TopologyIdentityIndex::from_model(
            std::slice::from_ref(&body),
            &[face.clone(), qualified_alternate],
            std::slice::from_ref(&edge),
            std::slice::from_ref(&vertex),
            &[(222, 22)],
            &[],
            &[],
        );
        assert_eq!(
            active_with_alternate.resolve("schema-g:222"),
            Some(PmiTarget::Face {
                face: face.id.clone(),
            })
        );
        assert!(index.resolve("schema-e:not-a-number").is_none());
        assert!(index.resolve("11").is_none());

        let collision = Face {
            id: FaceId::mint("sldprt:brep:face#11").expect("identity grammar"),
            ..face.clone()
        };
        let index = TopologyIdentityIndex::from_model(
            std::slice::from_ref(&body),
            &[collision],
            std::slice::from_ref(&edge),
            std::slice::from_ref(&vertex),
            &[],
            &[],
            &[],
        );
        assert_eq!(
            index.resolve("schema-g:11"),
            Some(PmiTarget::Body {
                body: body.id.clone(),
            })
        );

        let sequence_wins = TopologyIdentityIndex::from_model(
            std::slice::from_ref(&body),
            std::slice::from_ref(&face),
            std::slice::from_ref(&edge),
            std::slice::from_ref(&vertex),
            &[],
            &[(11, 33)],
            &[],
        );
        assert_eq!(
            sequence_wins.resolve("schema-h:11"),
            Some(PmiTarget::Edge {
                edge: edge.id.clone(),
            })
        );

        let unresolved = TopologyIdentityIndex::from_model(
            std::slice::from_ref(&body),
            std::slice::from_ref(&face),
            std::slice::from_ref(&edge),
            std::slice::from_ref(&vertex),
            &[(11, 999)],
            &[],
            &[],
        );
        assert!(unresolved.resolve("schema-i:11").is_none());

        let conflicting = TopologyIdentityIndex::from_model(
            std::slice::from_ref(&body),
            &[
                face.clone(),
                Face {
                    id: FaceId::mint("sldprt:brep:face#23").expect("identity grammar"),
                    ..face.clone()
                },
            ],
            std::slice::from_ref(&edge),
            std::slice::from_ref(&vertex),
            &[(77, 22), (77, 23)],
            &[],
            &[],
        );
        assert!(conflicting.resolve("schema-j:77").is_none());

        let conflicting_families = TopologyIdentityIndex::from_model(
            std::slice::from_ref(&body),
            std::slice::from_ref(&face),
            std::slice::from_ref(&edge),
            std::slice::from_ref(&vertex),
            &[(88, 22)],
            &[(88, 33)],
            &[],
        );
        assert!(conflicting_families.resolve("schema-k:88").is_none());
    }

    fn put_pstr(bytes: &mut Vec<u8>, value: &str) {
        bytes.push(u8::try_from(value.len()).expect("fixture Pascal string"));
        bytes.extend_from_slice(value.as_bytes());
    }

    fn encode_entity(entity: &Entity, bytes: &mut Vec<u8>) {
        put_pstr(bytes, "Entity");
        put_pstr(bytes, &entity.class);
        put_pstr(bytes, "gdtanalysis.net");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        if !entity.strings.is_empty() {
            put_pstr(bytes, "Strings");
            bytes.extend_from_slice(&(entity.strings.len() as u32).to_le_bytes());
            for (key, value) in &entity.strings {
                put_pstr(bytes, key);
                put_pstr(bytes, value);
            }
            put_pstr(bytes, "EndStrings");
        }
        if !entity.integers.is_empty() {
            put_pstr(bytes, "Integers");
            bytes.extend_from_slice(&(entity.integers.len() as u32).to_le_bytes());
            for (key, value) in &entity.integers {
                put_pstr(bytes, key);
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            put_pstr(bytes, "EndIntegers");
        }
        if !entity.doubles.is_empty() {
            put_pstr(bytes, "Doubles");
            bytes.extend_from_slice(&(entity.doubles.len() as u32).to_le_bytes());
            for (key, value) in &entity.doubles {
                put_pstr(bytes, key);
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            put_pstr(bytes, "EndDoubles");
        }
        encode_objects("Features", "EndFeatures", &entity.features, bytes);
        encode_objects("Annotations", "EndAnnotations", &entity.annotations, bytes);
        if !entity.related.is_empty() {
            put_pstr(bytes, "RelatedObjects");
            bytes.extend_from_slice(&(entity.related.len() as u32).to_le_bytes());
            for object in &entity.related {
                put_pstr(bytes, &object.name);
                put_pstr(bytes, &object.class);
            }
            for object in &entity.related {
                encode_entity(&object.entity, bytes);
            }
            put_pstr(bytes, "EndRelatedObjects");
        }
        put_pstr(bytes, "EndEntity");
    }

    fn encode_objects(name: &str, end: &str, section: &ObjectSection, bytes: &mut Vec<u8>) {
        if section.references.is_empty() && section.entities.is_empty() {
            return;
        }
        put_pstr(bytes, name);
        bytes.extend_from_slice(&(section.references.len() as u32).to_le_bytes());
        for reference in &section.references {
            put_pstr(bytes, &reference.id);
            put_pstr(bytes, &reference.class);
        }
        for entity in &section.entities {
            encode_entity(entity, bytes);
        }
        put_pstr(bytes, end);
    }

    fn encoded_root() -> Vec<u8> {
        let mut bytes = vec![0x11, 0x22, 0x33];
        encode_entity(&semantic_root(), &mut bytes);
        bytes
    }

    #[test]
    fn parses_and_projects_semantic_graph() {
        let parsed = parse_unique_root(&encoded_root()).expect("synthetic SWIFT root");
        let annotations = project(&parsed);
        assert_eq!(annotations.len(), 5);

        let position = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Position 1"))
            .expect("position annotation");
        let PmiDefinition::GeometricTolerance {
            magnitude,
            datum_system,
            modifiers,
            ..
        } = &position.definition
        else {
            panic!("position definition");
        };
        assert_eq!(*magnitude, length(0.25));
        assert_eq!(
            datum_system.as_ref().map(|id| id.0.as_str()),
            Some("sldprt:model:pmi#A20:datum-system")
        );
        assert_eq!(
            modifiers,
            &["maximum_material_requirement", "projected_zone:4_mm"]
        );
        assert_eq!(
            position.targets,
            [
                PmiTarget::ShapeAspect {
                    source_id: "F20".into()
                },
                PmiTarget::ShapeAspect {
                    source_id: "F21".into()
                }
            ]
        );

        let system = annotations
            .iter()
            .find(|annotation| annotation.id.as_str().ends_with(":datum-system"))
            .expect("datum system");
        let PmiDefinition::DatumSystem { references } = &system.definition else {
            panic!("datum-system definition");
        };
        assert_eq!(references.len(), 1);
        let datum_reference = references.first().expect("primary datum reference");
        assert_eq!(datum_reference.precedence.get(), 1);
        assert_eq!(datum_reference.modifiers, ["least_material_requirement"]);

        assert!(!annotations
            .iter()
            .any(|annotation| annotation.name.as_deref() == Some("Diameter 1")));

        let angle = annotations
            .iter()
            .find(|annotation| annotation.id.as_str().ends_with("#A40"))
            .expect("angular annotation");
        let PmiDefinition::Dimension { nominal, .. } = &angle.definition else {
            panic!("angular definition");
        };
        assert_eq!(
            *nominal,
            PmiValue {
                value: 0.0,
                quantity: PmiQuantity::Angle,
            }
        );
    }

    #[test]
    fn rejects_ambiguous_root_and_impossible_count() {
        let encoded = encoded_root();
        let mut duplicate = encoded.clone();
        duplicate.extend_from_slice(&encoded);
        assert_eq!(parse_unique_root(&duplicate), None);

        let mut malformed = encoded;
        let marker = b"\x0bAnnotations";
        let offset = malformed
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("root annotations marker")
            + marker.len();
        malformed
            .get_mut(offset..offset + 4)
            .expect("count field")
            .copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_unique_root(&malformed), None);
    }

    fn cylinder_with_radius(radius: f64) -> Entity {
        let mut cylinder = entity("GdtCylinder");
        let mut geometry = Entity {
            class: "PrizMetrik.Geometry.GeoCylinder".into(),
            ..Entity::default()
        };
        geometry.doubles.insert("R".into(), radius);
        geometry.doubles.insert("I".into(), 0.0);
        geometry.doubles.insert("J".into(), 0.0);
        geometry.doubles.insert("K".into(), 1.0);
        cylinder.related.push(RelatedObject {
            name: "NomCylinder".into(),
            class: geometry.class.clone(),
            entity: geometry,
        });
        cylinder
    }

    fn cylinder_with_radius_and_depth(radius: f64, depth: f64) -> Entity {
        let mut cylinder = cylinder_with_radius(radius);
        for (name, z) in [("NomTop", depth), ("NomBottom", 0.0)] {
            let mut plane = Entity {
                class: "PrizMetrik.Geometry.GeoPlane".into(),
                ..Entity::default()
            };
            plane.doubles.insert("X".into(), 0.0);
            plane.doubles.insert("Y".into(), 0.0);
            plane.doubles.insert("Z".into(), z);
            cylinder.related.push(RelatedObject {
                name: name.into(),
                class: plane.class.clone(),
                entity: plane,
            });
        }
        cylinder
    }

    fn cone_with_angle_and_top(angle: f64, top: f64) -> Entity {
        let mut cone = entity("GdtCone");
        let mut geometry = Entity {
            class: "PrizMetrik.Geometry.GeoCone".into(),
            ..Entity::default()
        };
        for (name, value) in [
            ("FullAngle", angle),
            ("I", 0.0),
            ("J", 0.0),
            ("K", 1.0),
            ("X", 0.0),
            ("Y", 0.0),
            ("Z", 0.0),
        ] {
            geometry.doubles.insert(name.into(), value);
        }
        cone.related.push(RelatedObject {
            name: "NomCone".into(),
            class: geometry.class.clone(),
            entity: geometry,
        });
        let mut plane = Entity {
            class: "PrizMetrik.Geometry.GeoPlane".into(),
            ..Entity::default()
        };
        for (name, value) in [
            ("I", 0.0),
            ("J", 0.0),
            ("K", -1.0),
            ("X", 0.0),
            ("Y", 0.0),
            ("Z", top),
        ] {
            plane.doubles.insert(name.into(), value);
        }
        cone.related.push(RelatedObject {
            name: "NomTop".into(),
            class: plane.class.clone(),
            entity: plane,
        });
        cone
    }

    fn plane_with_origin(origin_z: f64) -> Entity {
        let mut feature = entity("GdtPlane");
        let mut plane = Entity {
            class: "PrizMetrik.Geometry.GeoPlane".into(),
            ..Entity::default()
        };
        for (name, value) in [
            ("I", 0.0),
            ("J", 0.0),
            ("K", 1.0),
            ("X", 5.0),
            ("Y", 0.0),
            ("Z", 0.0),
        ] {
            plane.doubles.insert(name.into(), value);
        }
        feature.related.push(RelatedObject {
            name: "NomPlane".into(),
            class: plane.class.clone(),
            entity: plane,
        });
        let mut origin = Entity {
            class: "PrizMetrik.Geometry.GeoPoint".into(),
            ..Entity::default()
        };
        for (name, value) in [("X", 0.0), ("Y", 0.0), ("Z", origin_z)] {
            origin.doubles.insert(name.into(), value);
        }
        feature.related.push(RelatedObject {
            name: "NomOrigin".into(),
            class: origin.class.clone(),
            entity: origin,
        });
        feature
    }

    fn plane_at(point: [f64; 3], normal: [f64; 3]) -> Entity {
        let mut feature = entity("GdtPlane");
        let mut plane = Entity {
            class: "PrizMetrik.Geometry.GeoPlane".into(),
            ..Entity::default()
        };
        for (name, value) in ["X", "Y", "Z"].into_iter().zip(point) {
            plane.doubles.insert(name.into(), value);
        }
        for (name, value) in ["I", "J", "K"].into_iter().zip(normal) {
            plane.doubles.insert(name.into(), value);
        }
        feature.related.push(RelatedObject {
            name: "NomPlane".into(),
            class: plane.class.clone(),
            entity: plane,
        });
        feature
    }

    fn cylinder_at(point: [f64; 3], axis: [f64; 3]) -> Entity {
        let mut feature = cylinder_with_radius(3.0);
        let cylinder = &mut feature
            .related
            .first_mut()
            .expect("nominal cylinder")
            .entity;
        for (name, value) in ["X", "Y", "Z"].into_iter().zip(point) {
            cylinder.doubles.insert(name.into(), value);
        }
        for (name, value) in ["I", "J", "K"].into_iter().zip(axis) {
            cylinder.doubles.insert(name.into(), value);
        }
        feature
    }

    fn distance_annotation(first: Reference, second: Reference, direction: [f64; 3]) -> Entity {
        let mut annotation = entity("GdtDistanceBetween");
        annotation.doubles.insert("Nominal".into(), 0.0);
        annotation.doubles.insert("MinusTolerance".into(), -0.5);
        annotation.doubles.insert("PlusTolerance".into(), 0.5);
        annotation.doubles.insert("LowerLimit".into(), 0.0);
        annotation.doubles.insert("UpperLimit".into(), 0.0);
        annotation.integers.insert("ComputeAnswerBy".into(), 0);
        annotation.integers.insert("Dimension".into(), 0);
        annotation.integers.insert("Direction".into(), 4);
        annotation.integers.insert("NormalTo".into(), 1);
        annotation.features.references = vec![first, second];
        let mut transform = Entity {
            class: "PrizMetrik.Geometry.GeoTransform".into(),
            ..Entity::default()
        };
        for (name, value) in [
            ("R1C1", 1.0),
            ("R1C2", 0.0),
            ("R1C3", 0.0),
            ("R2C1", 0.0),
            ("R2C2", 1.0),
            ("R2C3", 0.0),
            ("R3C1", 0.0),
            ("R3C2", 0.0),
            ("R3C3", 1.0),
            ("X", 0.0),
            ("Y", 0.0),
            ("Z", 0.0),
        ] {
            transform.doubles.insert(name.into(), value);
        }
        annotation.related.push(RelatedObject {
            name: "NominalTransform".into(),
            class: transform.class.clone(),
            entity: transform,
        });
        let mut vector = Entity {
            class: "PrizMetrik.Geometry.GeoUnitVector".into(),
            ..Entity::default()
        };
        for (name, value) in ["I", "J", "K"].into_iter().zip(direction) {
            vector.doubles.insert(name.into(), value);
        }
        annotation.related.push(RelatedObject {
            name: "DirectionVector".into(),
            class: vector.class.clone(),
            entity: vector,
        });
        annotation
    }

    fn feature_with_nominal_measurement(
        feature_class: &str,
        object_name: &str,
        geometry_class: &str,
        field: &str,
        value: f64,
    ) -> Entity {
        let mut feature = entity(feature_class);
        let mut geometry = Entity {
            class: format!("PrizMetrik.Geometry.{geometry_class}"),
            ..Entity::default()
        };
        geometry.doubles.insert(field.into(), value);
        feature.related.push(RelatedObject {
            name: object_name.into(),
            class: geometry.class.clone(),
            entity: geometry,
        });
        feature
    }

    #[test]
    fn rendered_diameter_resolves_rounded_applied_geometry() {
        let mut root = semantic_root();
        *root.features.entities.get_mut(1).expect("first cylinder") =
            cylinder_with_radius(1.984_375);
        *root.features.entities.get_mut(2).expect("second cylinder") =
            cylinder_with_radius(1.984_375);
        let diameter = root.annotations.entities.get_mut(2).expect("diameter");
        diameter
            .integers
            .insert("BlockToleranceDecimalPlaces".into(), 3);
        diameter.doubles.insert("MinusTolerance".into(), 0.0);
        diameter.doubles.insert("PlusTolerance".into(), 0.0);
        diameter.doubles.insert("LowerLimit".into(), 3.8);
        diameter.doubles.insert("UpperLimit".into(), 4.1);
        let displayed = [RenderedDimension {
            kind: RenderedDimensionKind::Diameter,
            value: 0.156,
            decimal_places: 3,
        }];
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &displayed, &mut annotations);
        let diameter = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Diameter 1"))
            .expect("diameter annotation");
        let PmiDefinition::Dimension {
            nominal,
            tolerance:
                Some(DimensionTolerance::PlusMinus {
                    lower: lower_deviation,
                    upper: upper_deviation,
                }),
            ..
        } = &diameter.definition
        else {
            panic!("dimension definition");
        };
        assert!(approximately_equal(nominal.value, 3.962_4));
        assert!(approximately_equal(lower_deviation.value, -0.162_4));
        assert!(approximately_equal(upper_deviation.value, 0.137_6));

        root.annotations
            .entities
            .get_mut(2)
            .expect("diameter")
            .features
            .references = vec![reference("FP", "GdtPattern")];
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &displayed, &mut annotations);
        let diameter = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Diameter 1"))
            .expect("diameter annotation");
        let PmiDefinition::Dimension { nominal, .. } = &diameter.definition else {
            panic!("dimension definition");
        };
        assert!(approximately_equal(nominal.value, 3.962_4));
    }

    #[test]
    fn conflicting_pattern_sizes_do_not_resolve_a_nominal() {
        let mut root = semantic_root();
        *root.features.entities.get_mut(1).expect("first cylinder") = cylinder_with_radius(2.5);
        *root.features.entities.get_mut(2).expect("second cylinder") = cylinder_with_radius(3.0);
        let diameter = root.annotations.entities.get_mut(2).expect("diameter");
        diameter
            .integers
            .insert("BlockToleranceDecimalPlaces".into(), 1);
        diameter.features.references = vec![reference("FP", "GdtPattern")];
        let mut annotations = project(&root);
        enrich_implicit_nominals(
            &root,
            &[RenderedDimension {
                kind: RenderedDimensionKind::Diameter,
                value: 5.0,
                decimal_places: 1,
            }],
            &mut annotations,
        );
        assert!(!annotations
            .iter()
            .any(|annotation| annotation.name.as_deref() == Some("Diameter 1")));
    }

    #[test]
    fn numerically_equivalent_pattern_sizes_supply_diameter_without_rendered_text() {
        let mut root = semantic_root();
        *root.features.entities.get_mut(1).expect("first cylinder") = cylinder_with_radius(2.5);
        *root.features.entities.get_mut(2).expect("second cylinder") =
            cylinder_with_radius(2.500_002_5);
        root.annotations
            .entities
            .get_mut(2)
            .expect("diameter")
            .features
            .references = vec![reference("FP", "GdtPattern")];
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A30"))
            .expect("pattern diameter")
            .definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, length(5.0));
    }

    #[test]
    fn diameter_equivalence_does_not_merge_distinct_sizes() {
        assert_eq!(unique_diameter(&[10.0, 10.000_005]), Some(10.0));
        assert_eq!(unique_diameter(&[10.0, 10.000_02]), None);
    }

    #[test]
    fn empty_pattern_does_not_bind_an_unrelated_rendered_diameter() {
        let mut root = semantic_root();
        root.features
            .entities
            .first_mut()
            .and_then(|pattern| pattern.related.first_mut())
            .expect("pattern members")
            .entity
            .related
            .clear();
        root.annotations
            .entities
            .get_mut(2)
            .expect("diameter")
            .features
            .references = vec![reference("FP", "GdtPattern")];

        let mut annotations = project(&root);
        enrich_implicit_nominals(
            &root,
            &[RenderedDimension {
                kind: RenderedDimensionKind::Diameter,
                value: 0.25,
                decimal_places: 3,
            }],
            &mut annotations,
        );
        assert!(!annotations
            .iter()
            .any(|annotation| annotation.id == pmi_id("A30")));
    }

    #[test]
    fn counterbore_pattern_supplies_distinct_hole_diameter() {
        let mut root = semantic_root();
        *root
            .features
            .entities
            .get_mut(1)
            .expect("counterbore cylinder") = cylinder_with_radius(5.0);
        *root.features.entities.get_mut(2).expect("hole cylinder") = cylinder_with_radius(3.0);
        root.annotations
            .entities
            .get_mut(2)
            .expect("diameter")
            .features
            .references = vec![reference("FP", "GdtPattern")];

        let mut counterbore = entity("GdtCounterBore");
        counterbore.doubles.insert("Nominal".into(), 0.0);
        counterbore.features.references = vec![
            reference("FP", "GdtPattern"),
            reference("F20", "GdtCylinder"),
        ];
        root.annotations
            .references
            .push(reference("A50", "GdtCounterBore"));
        root.annotations.entities.push(counterbore);

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A30"))
            .expect("pattern diameter")
            .definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, length(6.0));
    }

    #[test]
    fn unrelated_counterbore_size_does_not_select_a_pattern_diameter() {
        let mut root = semantic_root();
        *root.features.entities.get_mut(1).expect("first cylinder") = cylinder_with_radius(5.0);
        *root.features.entities.get_mut(2).expect("second cylinder") = cylinder_with_radius(3.0);
        root.annotations
            .entities
            .get_mut(2)
            .expect("diameter")
            .features
            .references = vec![reference("FP", "GdtPattern")];
        root.features
            .references
            .push(reference("FCB", "GdtCylinder"));
        root.features.entities.push(cylinder_with_radius(4.0));

        let mut counterbore = entity("GdtCounterBore");
        counterbore.doubles.insert("Nominal".into(), 0.0);
        counterbore.features.references = vec![
            reference("FP", "GdtPattern"),
            reference("FCB", "GdtCylinder"),
        ];
        root.annotations
            .references
            .push(reference("A50", "GdtCounterBore"));
        root.annotations.entities.push(counterbore);

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        assert!(!annotations
            .iter()
            .any(|annotation| annotation.id == pmi_id("A30")));
    }

    #[test]
    fn direct_cylinder_and_sphere_supply_diameter_without_rendered_text() {
        let mut root = semantic_root();
        *root.features.entities.get_mut(1).expect("direct cylinder") = cylinder_with_radius(17.5);
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A30"))
            .expect("direct diameter")
            .definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, length(35.0));

        *root.features.entities.get_mut(1).expect("direct sphere") =
            feature_with_nominal_measurement("GdtSphere", "NomSphere", "GeoSphere", "R", 15.875);
        root.features
            .references
            .get_mut(1)
            .expect("direct feature reference")
            .class = "PrizMetrik.GdtAnalysis.GdtSphere,gdtanalysis.net".into();
        root.annotations
            .entities
            .get_mut(2)
            .and_then(|diameter| diameter.features.references.first_mut())
            .expect("diameter feature reference")
            .class = "PrizMetrik.GdtAnalysis.GdtSphere,gdtanalysis.net".into();
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A30"))
            .expect("direct diameter")
            .definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, length(31.75));
    }

    #[test]
    fn conflicting_rendered_units_do_not_resolve_a_nominal() {
        assert_eq!(
            rendered_nominal(
                5.0,
                1,
                RenderedDimensionKind::Diameter,
                &[
                    RenderedDimension {
                        kind: RenderedDimensionKind::Diameter,
                        value: 5.0,
                        decimal_places: 1,
                    },
                    RenderedDimension {
                        kind: RenderedDimensionKind::Diameter,
                        value: 0.2,
                        decimal_places: 1,
                    },
                ],
            ),
            None
        );
    }

    #[test]
    fn directional_plane_distance_supplies_location_nominal() {
        let mut root = semantic_root();
        root.features.references.push(reference("FL1", "GdtPlane"));
        root.features
            .entities
            .push(plane_at([3.0, 4.0, 5.0], [0.0, 0.0, 1.0]));
        root.features.references.push(reference("FL2", "GdtPlane"));
        root.features
            .entities
            .push(plane_at([8.0, 9.0, 25.0], [0.0, 0.0, 1.0]));
        root.annotations
            .references
            .push(reference("A50", "GdtDistanceBetween"));
        root.annotations.entities.push(distance_annotation(
            reference("FL1", "GdtPlane"),
            reference("FL2", "GdtPlane"),
            [0.0, 0.0, -1.0],
        ));

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension {
            nominal,
            tolerance:
                Some(DimensionTolerance::PlusMinus {
                    lower: lower_deviation,
                    upper: upper_deviation,
                }),
            ..
        } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A50"))
            .expect("location dimension")
            .definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, length(20.0));
        assert_eq!(*lower_deviation, length(-0.5));
        assert_eq!(*upper_deviation, length(0.5));

        *root.features.entities.last_mut().expect("second plane") =
            plane_at([8.0, 9.0, 25.0], [1.0, 0.0, 0.0]);
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        assert!(!annotations
            .iter()
            .any(|annotation| annotation.id == pmi_id("A50")));
    }

    #[test]
    fn directional_compound_hole_axes_supply_location_nominal() {
        let mut root = semantic_root();
        for (hole_id, cylinder_id, y) in [("FH1", "FC1", 210.0), ("FH2", "FC2", 285.0)] {
            let mut hole = entity("GdtCompoundHole");
            hole.features
                .references
                .push(reference(cylinder_id, "GdtCylinder"));
            root.features
                .references
                .push(reference(hole_id, "GdtCompoundHole"));
            root.features.entities.push(hole);
            root.features
                .references
                .push(reference(cylinder_id, "GdtCylinder"));
            root.features
                .entities
                .push(cylinder_at([230.0, y, 27.0], [0.0, 0.0, 1.0]));
        }
        root.annotations
            .references
            .push(reference("A50", "GdtDistanceBetween"));
        root.annotations.entities.push(distance_annotation(
            reference("FH1", "GdtCompoundHole"),
            reference("FH2", "GdtCompoundHole"),
            [0.0, 1.0, 0.0],
        ));

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A50"))
            .expect("hole-axis location")
            .definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, length(75.0));
    }

    #[test]
    fn closed_slot_end_feature_supplies_length_location_nominal() {
        let mut root = semantic_root();
        root.features
            .references
            .push(reference("FSC", "GdtCylinder"));
        root.features
            .entities
            .push(cylinder_at([38.1, 3.037_84, -95.25], [0.0, 1.0, 0.0]));
        root.features
            .entities
            .last_mut()
            .and_then(|cylinder| cylinder.related.first_mut())
            .expect("nominal cylinder")
            .entity
            .doubles
            .insert("R".into(), 3.175);
        let mut slot = entity("GdtCompoundClosedSlot3D");
        slot.features
            .references
            .push(reference("FSC", "GdtCylinder"));
        let mut geometry = Entity {
            class: "PrizMetrik.Geometry.GeoClosedSlot".into(),
            ..Entity::default()
        };
        for (name, value) in [
            ("I", 0.0),
            ("J", -1.0),
            ("K", 0.0),
            ("LongitudeI", -1.0),
            ("LongitudeJ", 0.0),
            ("LongitudeK", 0.0),
            ("X", 47.625),
            ("Y", 3.037_84),
            ("Z", -95.25),
            ("Length", 25.4),
            ("Width", 6.35),
        ] {
            geometry.doubles.insert(name.into(), value);
        }
        slot.related.push(RelatedObject {
            name: "NomClosedSlot".into(),
            class: geometry.class.clone(),
            entity: geometry,
        });
        root.features
            .references
            .push(reference("FS", "GdtCompoundClosedSlot3D"));
        root.features.entities.push(slot);
        root.annotations
            .references
            .push(reference("A50", "GdtDistanceBetween"));
        let mut distance = distance_annotation(
            reference("FSC", "GdtCylinder"),
            reference("FS", "GdtCompoundClosedSlot3D"),
            [-1.0, 0.0, 0.0],
        );
        distance.integers.insert("FeatureFosUsage".into(), 2);
        distance.integers.insert("OriginFeatureFosUsage".into(), 2);
        root.annotations.entities.push(distance);

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A50"))
            .expect("slot length location")
            .definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, length(25.4));

        root.features
            .entities
            .last_mut()
            .and_then(|slot| slot.related.first_mut())
            .expect("nominal slot")
            .entity
            .doubles
            .insert("Width".into(), 7.0);
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        assert!(!annotations
            .iter()
            .any(|annotation| annotation.id == pmi_id("A50")));
    }

    #[test]
    fn rendered_depth_resolves_axial_nominal_planes() {
        let mut root = semantic_root();
        *root.features.entities.get_mut(1).expect("first cylinder") =
            cylinder_with_radius_and_depth(2.5, 7.625);
        let mut depth = entity("GdtDepth");
        depth.strings.insert("ObjectName".into(), "Depth 1".into());
        depth
            .integers
            .insert("BlockToleranceDecimalPlaces".into(), 2);
        depth.doubles.insert("Nominal".into(), 0.0);
        depth.doubles.insert("MinusTolerance".into(), -0.254);
        depth.doubles.insert("PlusTolerance".into(), 0.254);
        depth.doubles.insert("LowerLimit".into(), 0.0);
        depth.doubles.insert("UpperLimit".into(), 0.0);
        depth
            .features
            .references
            .push(reference("F20", "GdtCylinder"));
        root.annotations
            .references
            .push(reference("A50", "GdtDepth"));
        root.annotations.entities.push(depth);

        let mut annotations = project(&root);
        enrich_implicit_nominals(
            &root,
            &[RenderedDimension {
                kind: RenderedDimensionKind::Depth,
                value: 0.3,
                decimal_places: 2,
            }],
            &mut annotations,
        );
        let depth = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Depth 1"))
            .expect("depth annotation");
        let PmiDefinition::Dimension { nominal, .. } = &depth.definition else {
            panic!("dimension definition");
        };
        assert!(approximately_equal(nominal.value, 7.62));
    }

    #[test]
    fn direct_and_thread_cylinders_supply_depth_without_rendered_text() {
        let mut root = semantic_root();
        *root.features.entities.get_mut(1).expect("direct cylinder") =
            cylinder_with_radius_and_depth(5.0, 14.2875);
        let mut depth = entity("GdtDepth");
        depth.doubles.insert("Nominal".into(), 0.0);
        depth
            .features
            .references
            .push(reference("F20", "GdtCylinder"));
        root.annotations
            .references
            .push(reference("A50", "GdtDepth"));
        root.annotations.entities.push(depth);

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A50"))
            .expect("direct depth annotation")
            .definition
        else {
            panic!("dimension definition");
        };
        assert!(approximately_equal(nominal.value, 14.2875));

        root.annotations
            .entities
            .last_mut()
            .expect("thread-depth annotation")
            .integers
            .insert("IsThreadDepth".into(), 1);
        let cylinder = root
            .features
            .entities
            .get_mut(1)
            .expect("threaded cylinder");
        cylinder.integers.insert("IsThreaded".into(), 1);
        cylinder.doubles.insert("ThreadDepth".into(), 12.0);
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A50"))
            .expect("thread depth annotation")
            .definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, length(12.0));
    }

    #[test]
    fn counterbore_bottom_plane_resolves_sibling_cylinder_depth() {
        let mut root = semantic_root();
        root.features
            .references
            .push(reference("FCB", "GdtCylinder"));
        root.features
            .entities
            .push(cylinder_with_radius_and_depth(5.0, 12.7));
        root.features.references.push(reference("FDP", "GdtPlane"));
        root.features.entities.push(plane_with_origin(0.0));

        let mut counterbore = entity("GdtCounterBore");
        counterbore.doubles.insert("Nominal".into(), 0.0);
        counterbore.features.references = vec![
            reference("FP", "GdtPattern"),
            reference("FCB", "GdtCylinder"),
        ];
        root.annotations
            .references
            .push(reference("ACB", "GdtCounterBore"));
        root.annotations.entities.push(counterbore);
        let mut depth = entity("GdtDepth");
        depth.doubles.insert("Nominal".into(), 0.0);
        depth.features.references =
            vec![reference("FP", "GdtPattern"), reference("FDP", "GdtPlane")];
        root.annotations
            .references
            .push(reference("AD", "GdtDepth"));
        root.annotations.entities.push(depth);

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("AD"))
            .expect("counterbore depth")
            .definition
        else {
            panic!("dimension definition");
        };
        assert_eq!(*nominal, length(12.7));

        root.features
            .entities
            .last_mut()
            .and_then(|plane| plane.related.get_mut(1))
            .expect("depth-plane origin")
            .entity
            .doubles
            .insert("Z".into(), 1.0);
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        assert!(!annotations
            .iter()
            .any(|annotation| annotation.id == pmi_id("AD")));
    }

    #[test]
    fn semantic_slot_dimensions_resolve_exact_nominals() {
        let mut root = semantic_root();
        root.features
            .references
            .push(reference("FW", "GdtCompoundWidth"));
        root.features
            .entities
            .push(feature_with_nominal_measurement(
                "GdtCompoundWidth",
                "NomCompoundWidth",
                "GeoOpenSlot",
                "Width",
                12.7,
            ));
        root.features
            .references
            .push(reference("FL", "GdtCompoundClosedSlot3D"));
        root.features
            .entities
            .push(feature_with_nominal_measurement(
                "GdtCompoundClosedSlot3D",
                "NomClosedSlot",
                "GeoClosedSlot",
                "Length",
                38.1,
            ));
        let mut width = entity("GdtWidth");
        width.strings.insert("ObjectName".into(), "Width 1".into());
        width.doubles.insert("Nominal".into(), 0.0);
        width.doubles.insert("MinusTolerance".into(), 0.0);
        width.doubles.insert("PlusTolerance".into(), 0.0);
        width.doubles.insert("LowerLimit".into(), 12.5);
        width.doubles.insert("UpperLimit".into(), 12.9);
        width
            .features
            .references
            .push(reference("FW", "GdtCompoundWidth"));
        root.annotations
            .references
            .push(reference("A50", "GdtWidth"));
        root.annotations.entities.push(width);
        let mut length = entity("GdtLength");
        length
            .strings
            .insert("ObjectName".into(), "Length 1".into());
        length.doubles.insert("Nominal".into(), 0.0);
        length
            .features
            .references
            .push(reference("FL", "GdtCompoundClosedSlot3D"));
        root.annotations
            .references
            .push(reference("A60", "GdtLength"));
        root.annotations.entities.push(length);

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let width = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Width 1"))
            .expect("width annotation");
        let PmiDefinition::Dimension {
            nominal,
            tolerance:
                Some(DimensionTolerance::PlusMinus {
                    lower: lower_deviation,
                    upper: upper_deviation,
                }),
            ..
        } = &width.definition
        else {
            panic!("dimension definition");
        };
        assert!(approximately_equal(nominal.value, 12.7));
        assert!(approximately_equal(lower_deviation.value, -0.2));
        assert!(approximately_equal(upper_deviation.value, 0.2));
        let length = annotations
            .iter()
            .find(|annotation| annotation.name.as_deref() == Some("Length 1"))
            .expect("length annotation");
        let PmiDefinition::Dimension { nominal, .. } = &length.definition else {
            panic!("dimension definition");
        };
        assert!(approximately_equal(nominal.value, 38.1));
    }

    #[test]
    fn compound_hole_dimensions_use_direct_operation_geometry() {
        let mut root = semantic_root();
        *root
            .features
            .entities
            .get_mut(1)
            .expect("first pattern member") = cylinder_with_radius(3.0);
        *root
            .features
            .entities
            .get_mut(2)
            .expect("second pattern member") = cylinder_with_radius(3.0);
        root.features
            .references
            .push(reference("FCB", "GdtCylinder"));
        root.features.entities.push(cylinder_with_radius(7.9375));
        root.features.references.push(reference("FCS", "GdtCone"));
        root.features
            .entities
            .push(cone_with_angle_and_top(std::f64::consts::FRAC_PI_2, 10.0));

        for (id, class, feature) in [
            ("ACB", "GdtCounterBore", "FCB"),
            ("ACSD", "GdtCounterSinkDiameter", "FCS"),
            ("ACSA", "GdtCounterSinkAngle", "FCS"),
        ] {
            let mut annotation = entity(class);
            annotation.doubles.insert("Nominal".into(), 0.0);
            annotation
                .features
                .references
                .push(reference("FP", "GdtPattern"));
            annotation.features.references.push(reference(
                feature,
                if feature == "FCB" {
                    "GdtCylinder"
                } else {
                    "GdtCone"
                },
            ));
            root.annotations.references.push(reference(id, class));
            root.annotations.entities.push(annotation);
        }

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        for (id, expected, quantity) in [
            ("ACB", 15.875, PmiQuantity::Length),
            ("ACSD", 20.0, PmiQuantity::Length),
            ("ACSA", std::f64::consts::FRAC_PI_2, PmiQuantity::Angle),
        ] {
            let PmiDefinition::Dimension { nominal, .. } = &annotations
                .iter()
                .find(|annotation| annotation.id == pmi_id(id))
                .expect("compound-hole annotation")
                .definition
            else {
                panic!("dimension definition");
            };
            assert_eq!(nominal.quantity, quantity);
            assert!(approximately_equal(nominal.value, expected));
        }
    }

    #[test]
    fn semantic_slot_width_traverses_patterns_and_rejects_disagreement() {
        let mut root = semantic_root();
        for (index, value) in [(1, 9.525), (2, 9.525)] {
            *root
                .features
                .entities
                .get_mut(index)
                .expect("pattern member") = feature_with_nominal_measurement(
                "GdtCompoundClosedSlot3D",
                "NomClosedSlot",
                "GeoClosedSlot",
                "Width",
                value,
            );
        }
        let pattern_members = root
            .features
            .entities
            .first_mut()
            .and_then(|pattern| pattern.related.first_mut())
            .expect("pattern members");
        for applied in &mut pattern_members.entity.related {
            applied
                .entity
                .features
                .references
                .first_mut()
                .expect("pattern member reference")
                .class = "PrizMetrik.GdtAnalysis.GdtCompoundClosedSlot3D,gdtanalysis.net".into();
        }
        let mut width = entity("GdtWidth");
        width.doubles.insert("Nominal".into(), 0.0);
        width
            .features
            .references
            .push(reference("FP", "GdtPattern"));
        root.annotations
            .references
            .push(reference("A50", "GdtWidth"));
        root.annotations.entities.push(width);

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension { nominal, .. } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A50"))
            .expect("width annotation")
            .definition
        else {
            panic!("dimension definition");
        };
        assert!(approximately_equal(nominal.value, 9.525));

        root.features
            .entities
            .get_mut(2)
            .and_then(|feature| feature.related.first_mut())
            .expect("second nominal slot")
            .entity
            .doubles
            .insert("Width".into(), 6.35);
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        assert!(!annotations
            .iter()
            .any(|annotation| annotation.id == pmi_id("A50")));
    }

    #[test]
    fn semantic_radius_resolves_fillets_cylinders_and_spheres() {
        let mut root = semantic_root();
        let mut fillet = entity("GdtFillet");
        fillet.doubles.insert("Radius".into(), 3.175);
        *root.features.entities.get_mut(1).expect("first member") = fillet;
        *root.features.entities.get_mut(2).expect("second member") = cylinder_with_radius(3.175);
        root.features
            .references
            .get_mut(1)
            .expect("first feature reference")
            .class = "PrizMetrik.GdtAnalysis.GdtFillet,gdtanalysis.net".into();
        let pattern_members = root
            .features
            .entities
            .first_mut()
            .and_then(|pattern| pattern.related.first_mut())
            .expect("pattern members");
        for (applied, class) in pattern_members
            .entity
            .related
            .iter_mut()
            .zip(["GdtFillet", "GdtCylinder"])
        {
            applied
                .entity
                .features
                .references
                .first_mut()
                .expect("pattern member reference")
                .class = format!("PrizMetrik.GdtAnalysis.{class},gdtanalysis.net");
        }
        let mut radius = entity("GdtRadius");
        radius.doubles.insert("Nominal".into(), 0.0);
        radius.doubles.insert("MinusTolerance".into(), -0.1);
        radius.doubles.insert("PlusTolerance".into(), 0.0);
        radius.doubles.insert("UpperLimit".into(), 0.0);
        radius
            .features
            .references
            .push(reference("FP", "GdtPattern"));
        root.annotations
            .references
            .push(reference("A50", "GdtRadius"));
        root.annotations.entities.push(radius);

        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        let PmiDefinition::Dimension {
            nominal,
            tolerance:
                Some(DimensionTolerance::PlusMinus {
                    upper: upper_deviation,
                    ..
                }),
            ..
        } = &annotations
            .iter()
            .find(|annotation| annotation.id == pmi_id("A50"))
            .expect("radius annotation")
            .definition
        else {
            panic!("dimension definition");
        };
        assert!(approximately_equal(nominal.value, 3.175));
        assert_eq!(*upper_deviation, length(0.0));

        *root.features.entities.get_mut(2).expect("second member") =
            feature_with_nominal_measurement("GdtSphere", "NomSphere", "GeoSphere", "R", 4.0);
        root.features
            .references
            .get_mut(2)
            .expect("second feature reference")
            .class = "PrizMetrik.GdtAnalysis.GdtSphere,gdtanalysis.net".into();
        root.features
            .entities
            .first_mut()
            .and_then(|pattern| pattern.related.first_mut())
            .and_then(|members| members.entity.related.get_mut(1))
            .and_then(|applied| applied.entity.features.references.first_mut())
            .expect("second pattern member reference")
            .class = "PrizMetrik.GdtAnalysis.GdtSphere,gdtanalysis.net".into();
        let mut annotations = project(&root);
        enrich_implicit_nominals(&root, &[], &mut annotations);
        assert!(!annotations
            .iter()
            .any(|annotation| annotation.id == pmi_id("A50")));
    }

    #[test]
    fn scans_explicit_rendered_diameter_literals() {
        let mut payload = Vec::new();
        for text in [
            "<COUNT=#X ><MOD-DIAM> .156",
            "<MOD-DIAM> <sft_holeDia>",
            "<MOD-DIAM> .281<HOLE-SPOT><MOD-DIAM> .438",
            "<MOD-DIAM> .250 <HOLE-DEPTH> .30",
        ] {
            payload.extend_from_slice(&[0xff, 0xfe, 0xff]);
            payload.push(u8::try_from(text.encode_utf16().count()).expect("fixture length"));
            for unit in text.encode_utf16() {
                payload.extend_from_slice(&unit.to_le_bytes());
            }
        }
        assert_eq!(
            rendered_dimensions(&payload),
            [
                RenderedDimension {
                    kind: RenderedDimensionKind::Diameter,
                    value: 0.156,
                    decimal_places: 3,
                },
                RenderedDimension {
                    kind: RenderedDimensionKind::Diameter,
                    value: 0.281,
                    decimal_places: 3,
                },
                RenderedDimension {
                    kind: RenderedDimensionKind::Diameter,
                    value: 0.438,
                    decimal_places: 3,
                },
                RenderedDimension {
                    kind: RenderedDimensionKind::Diameter,
                    value: 0.25,
                    decimal_places: 3,
                },
                RenderedDimension {
                    kind: RenderedDimensionKind::Depth,
                    value: 0.3,
                    decimal_places: 2,
                },
            ]
        );
    }
}
