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
    unique_id_for_attribute(faces.iter().map(|face| &face.id), &prefix, FaceId::as_str).cloned()
}

fn edge_id_for_attribute(edges: &[Edge], attr: u16) -> Option<EdgeId> {
    let prefix = format!("sldprt:brep:edge#{attr}");
    unique_id_for_attribute(edges.iter().map(|edge| &edge.id), &prefix, EdgeId::as_str).cloned()
}

fn vertex_id_for_attribute(vertices: &[Vertex], attr: u16) -> Option<VertexId> {
    let prefix = format!("sldprt:brep:vertex#{attr}");
    unique_id_for_attribute(
        vertices.iter().map(|vertex| &vertex.id),
        &prefix,
        VertexId::as_str,
    )
    .cloned()
}

fn unique_id_for_attribute<'a, T: 'a>(
    ids: impl IntoIterator<Item = &'a T>,
    prefix: &str,
    as_str: impl Fn(&T) -> &str,
) -> Option<&'a T> {
    let ids = ids
        .into_iter()
        .filter(|id| {
            as_str(id) == prefix
                || as_str(id)
                    .strip_prefix(prefix)
                    .is_some_and(|suffix| suffix.starts_with('@'))
        })
        .map(|id| (as_str(id), id))
        .collect::<BTreeMap<_, _>>();
    if let Some(id) = ids.get(prefix) {
        return Some(*id);
    }
    let mut ids = ids.into_values();
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
        let prefix = pmi_id(&reference.id).into_string();
        for annotation in projected.iter().filter(|annotation| {
            annotation.id.as_str() == prefix
                || annotation.id.as_str().starts_with(&format!("{prefix}:"))
        }) {
            crate::annotations::note(
                annotations,
                annotation.id.as_str().to_owned(),
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
        if let Some(tolerance) = project_tolerance(entity, &datum_ids) {
            let datum_system = if tolerance.references.is_empty() {
                None
            } else if let Some((_, id)) = datum_systems
                .iter()
                .find(|(candidate, _)| *candidate == tolerance.references)
            {
                Some(id.clone())
            } else {
                let id = PmiId::mint(format!(
                    "{}:datum-system",
                    pmi_id(&reference.id).into_string()
                ))
                .expect("identity grammar");
                datum_systems.push((tolerance.references.clone(), id.clone()));
                projected.push(PmiAnnotation {
                    id: id.clone(),
                    name: None,
                    visible: None,
                    targets: Vec::new(),
                    definition: PmiDefinition::DatumSystem {
                        references: tolerance.references,
                    },
                });
                Some(id)
            };
            let (defined_unit, defined_area_unit, defined_area_second_unit) = defined_area(entity);
            projected.push(PmiAnnotation {
                id: pmi_id(&reference.id),
                name: object_name(entity),
                visible: None,
                targets: targets(entity, &feature_index, topology),
                definition: PmiDefinition::GeometricTolerance {
                    tolerance: tolerance.kind,
                    magnitude: tolerance.magnitude,
                    defined_unit,
                    defined_area_unit,
                    defined_area_second_unit,
                    datum_system,
                    modifiers: tolerance_modifiers(entity),
                },
            });
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

struct ProjectedTolerance {
    kind: GeometricToleranceKind,
    magnitude: PmiValue,
    references: Vec<DatumReference>,
}

fn project_tolerance(
    entity: &Entity,
    datum_ids: &BTreeMap<&str, PmiId>,
) -> Option<ProjectedTolerance> {
    let kind = tolerance_kind(short_class(&entity.class))?;
    let magnitude = finite_nonnegative(entity.doubles.get("Tolerance").copied()?)?;
    Some(ProjectedTolerance {
        kind,
        magnitude: length(magnitude),
        references: datum_references(entity, datum_ids),
    })
}

fn project_lower_profile_tier(
    reference: &Reference,
    entity: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    topology: Option<&TopologyIdentityIndex>,
) -> Option<PmiAnnotation> {
    let magnitude = finite_nonnegative(entity.doubles.get("ToleranceLowerTier").copied()?)?;
    Some(PmiAnnotation {
        id: PmiId::mint(format!(
            "{}:lower-tier",
            pmi_id(&reference.id).into_string()
        ))
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
        });
    let tolerance = match (
        deviation(entity, nominal, "LowerLimit", "MinusTolerance"),
        deviation(entity, nominal, "UpperLimit", "PlusTolerance"),
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
            nominal: nominal.map(|value| pmi_value(value, quantity)),
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
mod tests;
