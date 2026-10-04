// SPDX-License-Identifier: Apache-2.0
//! Semantic PMI stored in the SWIFT GDT-analysis object graph.
#![warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::ids::{EdgeId, FaceId, PmiId, VertexId};
use cadmpeg_ir::pmi::{
    DatumReference, DimensionKind, DimensionTolerance, GeometricToleranceKind, PmiAnnotation,
    PmiDefinition, PmiQuantity, PmiTarget, PmiValue,
};
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal, PositiveReal};
use cadmpeg_ir::topology::{Body, Edge, Face, Vertex};
use cadmpeg_ir::units::FiniteVector;

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

impl ObjectSection {
    /// Admits distinct references and their matching embedded entity prefix.
    fn new(
        ctx: &DecodeContext<'_>,
        references: Vec<Reference>,
        entities: Vec<Entity>,
    ) -> Result<Option<Self>, CodecError> {
        let mut ids = std::collections::HashSet::new();
        for reference in ctx.admit_iter(&references, "scan SLDPRT new values")? {
            let work = u64_from_index(reference.id.len())
                .checked_mul(2)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "validate SWIFT reference identities",
                        u64::MAX,
                        u64::MAX,
                    )
                })?;
            ctx.charge_work(work, "validate SWIFT reference identities")?;
            if !ctx.insert_hash_set(
                &mut ids,
                reference.id.as_str(),
                "admit distinct SWIFT object references",
            )? {
                let message = ctx.format_retained(
                    format_args!("duplicate SWIFT object reference ID {}", reference.id),
                    "retain SWIFT duplicate reference error",
                )?;
                return Err(CodecError::Malformed(message));
            }
        }
        if entities.len() > references.len() {
            return Ok(None);
        }
        for (reference, entity) in ctx.admit_iter(&references, "scan SWIFT object references")?.zip(ctx.admit_iter(&entities, "scan SWIFT object entities")?) {
            let work = reference
                .class
                .len()
                .checked_add(entity.class.len())
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("bind SWIFT reference classes", u64::MAX, u64::MAX)
                })?;
            ctx.charge_work(u64_from_index(work), "bind SWIFT reference classes")?;
            if !reference_matches_entity(reference, entity) {
                return Ok(None);
            }
        }
        Ok(Some(Self {
            references,
            entities,
        }))
    }
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

#[derive(Clone, Copy)]
pub(crate) struct PrimaryTopology<'a> {
    pub bodies: &'a [Body],
    pub faces: &'a [Face],
    pub edges: &'a [Edge],
    pub vertices: &'a [Vertex],
}

impl TopologyIdentityIndex {
    /// Build an index from the emitted primary topology arenas.
    pub(crate) fn from_model(
        ctx: &DecodeContext<'_>,
        topology: PrimaryTopology<'_>,
        face_bridge_sequences: &[(u32, u16)],
        edge_use_sequences: &[(u32, u16)],
        vertex_use_sequences: &[(u32, u16)],
    ) -> Result<Self, CodecError> {
        let PrimaryTopology {
            bodies,
            faces,
            edges,
            vertices,
        } = topology;
        let mut index = Self::default();
        for body in ctx.admit_iter(bodies, "index SWIFT body identities")? {
            index.insert_primary_id(
                ctx,
                body.id.as_str(),
                PmiTarget::Body {
                    body: copy_topology_id(ctx, body.id.as_str())?,
                },
            )?;
        }
        for edge in ctx.admit_iter(edges, "index SWIFT edge identities")? {
            index.insert_primary_id(
                ctx,
                edge.id.as_str(),
                PmiTarget::Edge {
                    edge: copy_topology_id(ctx, edge.id.as_str())?,
                },
            )?;
        }
        for vertex in ctx.admit_iter(vertices, "index SWIFT vertex identities")? {
            index.insert_primary_id(
                ctx,
                vertex.id.as_str(),
                PmiTarget::Vertex {
                    vertex: copy_topology_id(ctx, vertex.id.as_str())?,
                },
            )?;
        }
        for &(sequence, attr) in face_bridge_sequences {
            ctx.charge_work(1, "index SWIFT face sequence")?;

            let target = face_id_for_attribute(ctx, faces, attr)?
                .map(|face| {
                    copy_topology_id(ctx, face.as_str()).map(|face| PmiTarget::Face { face })
                })
                .transpose()?;
            index.insert_sequence_target(ctx, sequence, target)?;
        }
        for &(sequence, attr) in edge_use_sequences {
            ctx.charge_work(1, "index SWIFT edge sequence")?;

            let target = edge_id_for_attribute(ctx, edges, attr)?
                .map(|edge| {
                    copy_topology_id(ctx, edge.as_str()).map(|edge| PmiTarget::Edge { edge })
                })
                .transpose()?;
            index.insert_sequence_target(ctx, sequence, target)?;
        }
        for &(sequence, attr) in vertex_use_sequences {
            ctx.charge_work(1, "index SWIFT vertex sequence")?;

            let target = vertex_id_for_attribute(ctx, vertices, attr)?
                .map(|vertex| {
                    copy_topology_id(ctx, vertex.as_str())
                        .map(|vertex| PmiTarget::Vertex { vertex })
                })
                .transpose()?;
            index.insert_sequence_target(ctx, sequence, target)?;
        }
        Ok(index)
    }

    fn insert_sequence_target(
        &mut self,
        ctx: &DecodeContext<'_>,
        sequence: u32,
        target: Option<PmiTarget>,
    ) -> Result<(), CodecError> {
        if let Some(existing) = self.sequence_targets.get_mut(&u64::from(sequence)) {
            if existing.as_ref() != target.as_ref() {
                *existing = None;
            }
        } else {
            ctx.insert_btree_map(
                &mut self.sequence_targets,
                u64::from(sequence),
                target,
                "index SWIFT topology sequence",
            )?;
        }
        Ok(())
    }

    fn insert_primary_id(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: &str,
        target: PmiTarget,
    ) -> Result<(), CodecError> {
        let Some(suffix) = id.rsplit_once('#').map(|(_, suffix)| suffix) else {
            return Ok(());
        };
        let Ok(suffix) = suffix.parse::<u64>() else {
            return Ok(());
        };
        ctx.admit_btree_entry(
            &self.entries,
            &suffix,
            "index SWIFT primary topology suffix",
        )?;
        let entries = self.entries.entry(suffix).or_default();
        if !entries.contains(&target) {
            ctx.reserve_vec(entries, 1, "collect SWIFT primary topology targets")?;
            entries.push(target);
        }
        Ok(())
    }

    fn resolve(&self, identifier: &str) -> Option<&PmiTarget> {
        let (lane, suffix) = identifier.rsplit_once(':')?;
        if lane.is_empty() || suffix.is_empty() {
            return None;
        }
        let suffix = suffix.parse::<u64>().ok()?;
        if let Some(target) = self.sequence_targets.get(&suffix) {
            return target.as_ref();
        }
        let targets = self.entries.get(&suffix)?;
        (targets.len() == 1).then(|| targets.first()).flatten()
    }
}

fn face_id_for_attribute<'a>(ctx: &DecodeContext<'_>, faces: &'a [Face], attr: u16) -> Result<Option<&'a FaceId>, CodecError> {
    let prefix = format!("sldprt:brep:face#{attr}");
    Ok(unique_id_for_attribute(ctx.admit_iter(faces, "scan SWIFT face attribute identities")?.map(|face| &face.id), &prefix, FaceId::as_str))
}

fn edge_id_for_attribute<'a>(ctx: &DecodeContext<'_>, edges: &'a [Edge], attr: u16) -> Result<Option<&'a EdgeId>, CodecError> {
    let prefix = format!("sldprt:brep:edge#{attr}");
    Ok(unique_id_for_attribute(ctx.admit_iter(edges, "scan SWIFT edge attribute identities")?.map(|edge| &edge.id), &prefix, EdgeId::as_str))
}

fn vertex_id_for_attribute<'a>(ctx: &DecodeContext<'_>, vertices: &'a [Vertex], attr: u16) -> Result<Option<&'a VertexId>, CodecError> {
    let prefix = format!("sldprt:brep:vertex#{attr}");
    Ok(unique_id_for_attribute(
        ctx.admit_iter(vertices, "scan SWIFT vertex attribute identities")?.map(|vertex| &vertex.id),
        &prefix,
        VertexId::as_str,
    ))
}

fn copy_topology_id<T>(ctx: &DecodeContext<'_>, id: &str) -> Result<T, CodecError>
where
    T: TryFrom<String, Error = cadmpeg_ir::ids::IdentityError>,
{
    let copy_work = cadmpeg_core::decode::u64_from_index(id.len())
        .checked_mul(4)
        .ok_or_else(|| {
            ctx.refuse_codec_limit("copy SWIFT topology identity", u64::MAX - 1, u64::MAX)
        })?;
    ctx.charge_work(copy_work, "copy SWIFT topology identity")?;
    let text = ctx.format_retained(format_args!("{id}"), "copy SWIFT topology identity")?;
    T::try_from(text).map_err(|_| CodecError::malformed("invalid SWIFT topology identity"))
}

fn unique_id_for_attribute<'a, T: 'a>(
    ids: impl IntoIterator<Item = &'a T>,
    prefix: &str,
    as_str: impl Fn(&T) -> &str,
) -> Option<&'a T> {
    let mut exact = None;
    let mut alternate = None;
    let mut ambiguous = false;
    for id in ids {
        let name = as_str(id);
        if name == prefix {
            exact = Some(id);
        } else if name
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('@'))
        {
            match alternate {
                Some((first_name, _)) if first_name != name => ambiguous = true,
                _ => alternate = Some((name, id)),
            }
        }
    }
    if ambiguous {
        exact
    } else {
        exact.or_else(|| alternate.map(|(_, id)| id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct RenderedDimension {
    kind: RenderedDimensionKind,
    value: PositiveReal,
    decimal_places: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderedDimensionKind {
    Diameter,
    Depth,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ImplicitNominal {
    Exact(PositiveReal),
    Rendered {
        kind: RenderedDimensionKind,
        geometry: PositiveReal,
    },
    RenderedOrExact {
        kind: RenderedDimensionKind,
        geometry: PositiveReal,
        exact: PositiveReal,
    },
}

/// Decode the unique GDT-analysis root carried by a SWIFT schema stream.
pub(crate) fn annotations(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    annotations: &mut Annotations,
    topology: Option<&TopologyIdentityIndex>,
    pattern_hole_nominals: Option<&BTreeMap<String, PositiveReal>>,
) -> Result<Vec<PmiAnnotation>, CodecError> {
    let Some((stream, root, rendered_dimensions)) = scan_root(ctx, scan)? else {
        return Ok(Vec::new());
    };
    let projected = project_with_topology(
        ctx,
        &root,
        topology,
        &rendered_dimensions,
        pattern_hole_nominals,
    )?
    .annotations;
    for (reference, entity) in ctx.admit_iter(&root.annotations.references, "scan SWIFT annotation references")?
        .zip(ctx.admit_iter(&root.annotations.entities, "scan SWIFT annotation entities")?)
    {
        let Some(prefix) = pmi_id_charged(ctx, &reference.id)?.map(PmiId::into_string) else {
            continue;
        };
        for annotation in ctx.admit_iter(&projected, "scan SLDPRT annotations values")?.filter(|annotation| {
            annotation.id.as_str() == prefix
                || annotation
                    .id
                    .as_str()
                    .strip_prefix(&prefix)
                    .is_some_and(|suffix| suffix.starts_with(':'))
        }) {
            crate::annotations::note(
                ctx,
                annotations,
                ctx.format_retained(
                    format_args!("{}", annotation.id.as_str()),
                    "copy SWIFT provenance ID",
                )?,
                &stream,
                u64_from_index(entity.offset),
                "swift_gdt_analysis",
                cadmpeg_ir::Exactness::ByteExact,
            )?;
        }
    }
    Ok(projected)
}

/// Build the native-history join used when a SWIFT hole-pattern graph omits all
/// applied members and CAD identifiers. The map is keyed by the SWIFT object
/// name (`Hole PatternN`) and is populated only by one unambiguous native
/// `LPatternN` whose sole seed is consumed by exactly one later Hole feature.
pub(crate) fn pattern_hole_nominal_context(
    ctx: &DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
) -> Result<BTreeMap<String, PositiveReal>, CodecError> {
    let mut candidates = BTreeMap::<String, Option<PositiveReal>>::new();
    for pattern in features {
        ctx.charge_work(1, "swift pattern hole candidates")?;
        let Some(name) = pattern.name.as_deref() else {
            continue;
        };
        let Some(suffix) = name.strip_prefix("LPattern") else {
            continue;
        };
        if suffix.is_empty() || !ctx.admit_iter(suffix, "scan SLDPRT pattern name digits")?.all(|character| character.is_ascii_digit()) {
            continue;
        }
        if !pattern
            .native_ref
            .as_deref()
            .is_some_and(|native| native.starts_with("sldprt:history:feature#"))
        {
            continue;
        }
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Pattern { seeds, .. },
        ) = pattern.evaluation.definition()
        else {
            continue;
        };
        let [cadmpeg_ir::features::patterns::PatternSeed::Feature(seed)] = seeds.as_slice() else {
            continue;
        };
        let mut preceding_seed = false;
        for candidate in features {
            ctx.charge_work(1, "swift pattern seed search")?;
            if candidate.id == *seed && candidate.ordinal < pattern.ordinal {
                preceding_seed = true;
                break;
            }
        }
        if !preceding_seed {
            continue;
        }
        let mut hole_count = 0u8;
        let mut diameter = None;
        for candidate in features {
            ctx.charge_work(1, "swift pattern hole search")?;
            if candidate.ordinal <= pattern.ordinal {
                continue;
            }
            ctx.charge_work(
                u64_from_index(candidate.dependencies.len()),
                "swift pattern hole dependencies",
            )?;
            if !ctx.admit_iter(candidate.dependencies.as_slice(), "scan SLDPRT pattern_hole_nominal_context values")?
                .any(|dependency| dependency == seed)
                || !candidate
                    .native_ref
                    .as_deref()
                    .is_some_and(|native| native.starts_with("sldprt:history:feature#"))
            {
                continue;
            }
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Hole { shape, .. },
            ) = candidate.evaluation.definition()
            else {
                continue;
            };
            if hole_count == 1 {
                hole_count = 2;
                break;
            }
            hole_count = 1;
            diameter = shape.diameter().map(PositiveReal::from_assigned_length);
        }
        let (1, Some(diameter)) = (hole_count, diameter) else {
            continue;
        };
        let semantic_name = ctx.format_retained(
            format_args!("Hole Pattern{suffix}"),
            "swift pattern semantic name",
        )?;
        if let Some(value) = candidates.get_mut(&semantic_name) {
            *value = None;
        } else {
            ctx.insert_btree_map(
                &mut candidates,
                semantic_name,
                Some(diameter),
                "swift pattern candidate",
            )?;
        }
    }
    let mut nominals = BTreeMap::new();
    for (name, value) in candidates {
        if let Some(value) = value {
            ctx.insert_btree_map(&mut nominals, name, value, "swift pattern nominal")?;
        }
    }
    Ok(nominals)
}

pub(crate) fn unsupported_annotation_classes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<BTreeMap<String, usize>, CodecError> {
    let Some((_, root, rendered)) = scan_root(ctx, scan)? else {
        let mut classes = BTreeMap::new();
        if has_root_marker(ctx, scan)? {
            let key = ctx.format_retained(
                format_args!("GdtAnalysisGraphUnresolved"),
                "retain SLDPRT unsupported SWIFT class",
            )?;
            ctx.insert_btree_map(
                &mut classes,
                key,
                1,
                "collect SLDPRT unsupported SWIFT classes",
            )?;
        }
        return Ok(classes);
    };
    let mut classes = BTreeMap::new();
    if root.annotations.references.len() != root.annotations.entities.len() {
        let key = ctx.format_retained(
            format_args!("GdtAnalysisIncompleteAnnotationRoster"),
            "retain SLDPRT unsupported SWIFT class",
        )?;
        ctx.insert_btree_map(
            &mut classes,
            key,
            root.annotations
                .references
                .len()
                .abs_diff(root.annotations.entities.len()),
            "collect SLDPRT unsupported SWIFT classes",
        )?;
        return Ok(classes);
    }
    Ok(project_with_topology(ctx, &root, None, &rendered, None)?.unsupported)
}

fn has_root_marker(ctx: &DecodeContext<'_>, scan: &ContainerScan<'_>) -> Result<bool, CodecError> {
    Ok(scan.sections(ctx)?.try_fold(false, |found, section| { Ok::<_, cadmpeg_core::CodecError>(found || ( {
        section
            .name()
            .is_some_and(|name| name.starts_with("SWIFT/") && name.contains("Schema"))
            && ctx.admit_iter(section.payload(), "scan SWIFT root class marker")?.windows(std::num::NonZeroUsize::new(ROOT_CLASS.len()).ok_or_else(|| cadmpeg_core::CodecError::malformed("zero scan window width"))?)
                .any(|window| window == ROOT_CLASS.as_bytes())
    } )) })?)
}

fn scan_root(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<Option<(cadmpeg_ir::StreamName, Entity, Vec<RenderedDimension>)>, CodecError> {
    let mut root = None;
    for section in scan.sections(ctx)?.filter(|section| {
        section
            .name()
            .is_some_and(|name| name.starts_with("SWIFT/") && name.contains("Schema"))
    }) {
        ctx.charge_work(1, "scan SWIFT schema sections")?;
        let Some(entity) = parse_unique_root(ctx, section.payload())? else {
            continue;
        };
        let source_name = ctx.format_retained(
            format_args!("{}", section.source_stream().as_str()),
            "copy SWIFT source stream name",
        )?;
        let source_name = cadmpeg_ir::StreamName::try_from(source_name)
            .map_err(|_| CodecError::malformed("invalid SWIFT source stream name"))?;
        let rendered = rendered_dimensions(ctx, section.payload())?;
        if root.replace((source_name, entity, rendered)).is_some() {
            return Ok(None);
        }
    }
    Ok(root)
}

fn parse_unique_root(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<Entity>, CodecError> {
    ctx.charge_work(u64_from_index(payload.len()), "scan SWIFT entity roots")?;
    let mut parsed = None;
    for offset in payload
        .windows(ENTITY_TOKEN.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == ENTITY_TOKEN).then_some(offset))
    {
        let Some(mut cursor) = View::over_retained(payload).child(offset, payload.len()) else {
            continue;
        };
        let Some(entity) = parse_entity(ctx, &mut cursor, 0)? else {
            continue;
        };
        if entity.class != ROOT_CLASS {
            continue;
        }
        if parsed.replace(entity).is_some() {
            return Ok(None);
        }
    }
    Ok(parsed)
}

fn admit_swift_depth(
    ctx: &DecodeContext<'_>,
    depth: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    if depth >= MAX_DEPTH {
        let requested = u64_from_index(depth).checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit(operation, u64_from_index(MAX_DEPTH), u64::MAX)
        })?;
        return Err(ctx.refuse_codec_limit(operation, u64_from_index(MAX_DEPTH), requested));
    }
    Ok(())
}

fn parse_entity(
    ctx: &DecodeContext<'_>,
    cursor: &mut View<'_>,
    depth: usize,
) -> Result<Option<Entity>, CodecError> {
    let offset = cursor.position();
    if pstr(cursor) != Some("Entity") {
        return Ok(None);
    }
    admit_swift_depth(ctx, depth, "parse SWIFT entity")?;
    let _depth_guard = ctx.enter_nested("parse SWIFT entity")?;
    ctx.charge_work(1, "parse SWIFT entity")?;
    let Some(class) = pstr(cursor) else {
        return Ok(None);
    };
    let class = ctx.format_retained(format_args!("{class}"), "copy SWIFT entity class")?;
    if pstr(cursor).is_none() || cursor.u32_le().is_none() {
        return Ok(None);
    }
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
        let Some(section) = pstr(cursor) else {
            return Ok(None);
        };
        match section {
            "Strings" if !seen_strings => {
                seen_strings = true;
                let Some(values) = read_strings(ctx, cursor)? else {
                    return Ok(None);
                };
                entity.strings = values;
            }
            "Integers" if !seen_integers => {
                seen_integers = true;
                let Some(values) = read_integers(ctx, cursor)? else {
                    return Ok(None);
                };
                entity.integers = values;
            }
            "Doubles" if !seen_doubles => {
                seen_doubles = true;
                let Some(values) = read_doubles(ctx, cursor)? else {
                    return Ok(None);
                };
                entity.doubles = values;
            }
            "Features" if !seen_features => {
                seen_features = true;
                let Some(values) = read_objects(ctx, cursor, "EndFeatures", depth)? else {
                    return Ok(None);
                };
                entity.features = values;
            }
            "Annotations" if !seen_annotations => {
                seen_annotations = true;
                let Some(values) = read_objects(ctx, cursor, "EndAnnotations", depth)? else {
                    return Ok(None);
                };
                entity.annotations = values;
            }
            "RelatedObjects" if !seen_related => {
                seen_related = true;
                let Some(values) = read_related(ctx, cursor, depth)? else {
                    return Ok(None);
                };
                entity.related = values;
            }
            "EndEntity" => return Ok(Some(entity)),
            _ => return Ok(None),
        }
    }
}

fn read_strings(
    ctx: &DecodeContext<'_>,
    cursor: &mut View<'_>,
) -> Result<Option<BTreeMap<String, String>>, CodecError> {
    let Some(count) = cursor.u32_le() else {
        return Ok(None);
    };
    let Some(count) = cursor
        .counted(u64::from(count), 2)
        .map(cadmpeg_core::decode::BoundedCount::get)
    else {
        return Ok(None);
    };
    let mut values = BTreeMap::new();
    for _ in 0..count {
        ctx.charge_work(1, "parse SWIFT string properties")?;
        let (Some(name), Some(value)) = (pstr(cursor), pstr(cursor)) else {
            return Ok(None);
        };
        let name = ctx.format_retained(format_args!("{name}"), "copy SWIFT string key")?;
        let value = ctx.format_retained(format_args!("{value}"), "copy SWIFT string value")?;
        if values.contains_key(&name) {
            return Ok(None);
        }
        ctx.insert_btree_map(&mut values, name, value, "collect SWIFT string properties")?;
    }
    Ok((pstr(cursor) == Some("EndStrings")).then_some(values))
}

fn read_integers(
    ctx: &DecodeContext<'_>,
    cursor: &mut View<'_>,
) -> Result<Option<BTreeMap<String, i32>>, CodecError> {
    let Some(count) = cursor.u32_le() else {
        return Ok(None);
    };
    let Some(count) = cursor
        .counted(u64::from(count), 5)
        .map(cadmpeg_core::decode::BoundedCount::get)
    else {
        return Ok(None);
    };
    let mut values = BTreeMap::new();
    for _ in 0..count {
        ctx.charge_work(1, "parse SWIFT integer properties")?;
        let (Some(name), Some(value)) = (pstr(cursor), cursor.i32_le()) else {
            return Ok(None);
        };
        let name = ctx.format_retained(format_args!("{name}"), "copy SWIFT integer key")?;
        if values.contains_key(&name) {
            return Ok(None);
        }
        ctx.insert_btree_map(&mut values, name, value, "collect SWIFT integer properties")?;
    }
    Ok((pstr(cursor) == Some("EndIntegers")).then_some(values))
}

fn read_doubles(
    ctx: &DecodeContext<'_>,
    cursor: &mut View<'_>,
) -> Result<Option<BTreeMap<String, f64>>, CodecError> {
    let Some(count) = cursor.u32_le() else {
        return Ok(None);
    };
    let Some(count) = cursor
        .counted(u64::from(count), 9)
        .map(cadmpeg_core::decode::BoundedCount::get)
    else {
        return Ok(None);
    };
    let mut values = BTreeMap::new();
    for _ in 0..count {
        ctx.charge_work(1, "parse SWIFT double properties")?;
        let (Some(name), Some(value)) = (pstr(cursor), cursor.f64_le()) else {
            return Ok(None);
        };
        let name = ctx.format_retained(format_args!("{name}"), "copy SWIFT double key")?;
        if values.contains_key(&name) {
            return Ok(None);
        }
        ctx.insert_btree_map(&mut values, name, value, "collect SWIFT double properties")?;
    }
    Ok((pstr(cursor) == Some("EndDoubles")).then_some(values))
}

fn read_objects(
    ctx: &DecodeContext<'_>,
    cursor: &mut View<'_>,
    end: &str,
    depth: usize,
) -> Result<Option<ObjectSection>, CodecError> {
    let Some(count) = cursor.u32_le() else {
        return Ok(None);
    };
    let Some(count) = cursor
        .counted(u64::from(count), 2)
        .map(cadmpeg_core::decode::BoundedCount::get)
    else {
        return Ok(None);
    };
    let mut references = Vec::new();
    ctx.reserve_vec(&mut references, count, "collect SWIFT object references")?;
    for _ in 0..count {
        ctx.charge_work(1, "parse SWIFT object references")?;
        let (Some(id), Some(class)) = (pstr(cursor), pstr(cursor)) else {
            return Ok(None);
        };
        references.push(Reference {
            id: ctx.format_retained(format_args!("{id}"), "copy SWIFT object reference ID")?,
            class: ctx
                .format_retained(format_args!("{class}"), "copy SWIFT object reference class")?,
        });
    }
    let mut entities = Vec::new();
    loop {
        let Some(next) = peek_pstr(cursor) else {
            return Ok(None);
        };
        if next != "Entity" {
            break;
        }
        if entities.len() >= references.len() {
            return Ok(None);
        }
        let next_depth = depth.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "advance SWIFT recursion depth",
                u64_from_index(MAX_DEPTH),
                u64::MAX,
            )
        })?;
        let Some(entity) = parse_entity(ctx, cursor, next_depth)? else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut entities, 1, "collect SWIFT object entities")?;
        entities.push(entity);
    }
    if pstr(cursor) != Some(end) {
        return Ok(None);
    }
    ObjectSection::new(ctx, references, entities)
}

fn read_related(
    ctx: &DecodeContext<'_>,
    cursor: &mut View<'_>,
    depth: usize,
) -> Result<Option<Vec<RelatedObject>>, CodecError> {
    let Some(count) = cursor.u32_le() else {
        return Ok(None);
    };
    let Some(count) = cursor
        .counted(u64::from(count), 2)
        .map(cadmpeg_core::decode::BoundedCount::get)
    else {
        return Ok(None);
    };
    let mut descriptors = Vec::new();
    ctx.reserve_vec(&mut descriptors, count, "collect SWIFT related descriptors")?;
    for _ in 0..count {
        ctx.charge_work(1, "parse SWIFT related descriptors")?;
        let (Some(name), Some(class)) = (pstr(cursor), pstr(cursor)) else {
            return Ok(None);
        };
        descriptors.push((
            ctx.format_retained(format_args!("{name}"), "copy SWIFT related name")?,
            ctx.format_retained(format_args!("{class}"), "copy SWIFT related class")?,
        ));
    }
    let mut related = Vec::new();
    for (name, class) in descriptors {
        let next_depth = depth.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "advance SWIFT recursion depth",
                u64_from_index(MAX_DEPTH),
                u64::MAX,
            )
        })?;
        let Some(entity) = parse_entity(ctx, cursor, next_depth)? else {
            return Ok(None);
        };
        if class
            .split_once(',')
            .map_or(class.as_str(), |(name, _)| name)
            != entity.class
        {
            return Ok(None);
        }
        ctx.reserve_vec(&mut related, 1, "collect SWIFT related objects")?;
        related.push(RelatedObject {
            name,
            class,
            entity,
        });
    }
    if pstr(cursor) != Some("EndRelatedObjects") {
        return Ok(None);
    }
    Ok(Some(related))
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::service())
            .expect("empty root fits test policy");
    project_with_topology(&ctx, root, None, &[], None)
        .expect("test projection fits policy")
        .annotations
}

#[cfg(test)]
fn enrich_implicit_nominals(
    root: &Entity,
    rendered: &[RenderedDimension],
    annotations: &mut Vec<PmiAnnotation>,
) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::service())
            .expect("empty root fits test policy");
    *annotations = project_with_topology(&ctx, root, None, rendered, None)
        .expect("test projection fits policy")
        .annotations;
}

#[cfg(test)]
fn enrich_implicit_nominals_with_context(
    root: &Entity,
    rendered: &[RenderedDimension],
    annotations: &mut Vec<PmiAnnotation>,
    pattern_hole_nominals: Option<&BTreeMap<String, PositiveReal>>,
) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::service())
            .expect("empty root fits test policy");
    *annotations = project_with_topology(&ctx, root, None, rendered, pattern_hole_nominals)
        .expect("test projection fits policy")
        .annotations;
}

#[derive(Default)]
struct AnnotationProjection {
    annotations: Vec<PmiAnnotation>,
    unsupported: BTreeMap<String, usize>,
}

#[derive(Clone, Copy)]
enum AnnotationDisposition {
    Projected,
    Suppressed,
    Malformed(&'static str),
    Unsupported,
}

fn missing_projection(
    ctx: &DecodeContext<'_>,
    entity: &Entity,
) -> Result<AnnotationDisposition, CodecError> {
    ctx.charge_work(
        u64_from_index(entity.class.len()),
        "classify SWIFT missing projection",
    )?;
    let class = short_class(&entity.class);
    if class == "GdtDatum"
        && entity
            .strings
            .get("DatumIdentifier")
            .is_none_or(String::is_empty)
    {
        return Ok(AnnotationDisposition::Malformed(
            "missing or empty DatumIdentifier",
        ));
    }
    if tolerance_kind(class).is_some()
        && entity
            .doubles
            .get("Tolerance")
            .copied()
            .and_then(NonNegativeReal::new)
            .is_none()
    {
        return Ok(AnnotationDisposition::Malformed(
            "Tolerance must be present, finite and non-negative",
        ));
    }
    Ok(AnnotationDisposition::Unsupported)
}

fn record_projection(
    ctx: &DecodeContext<'_>,
    unsupported: &mut BTreeMap<String, usize>,
    reference: &Reference,
    entity: &Entity,
    disposition: AnnotationDisposition,
) -> Result<(), CodecError> {
    match disposition {
        AnnotationDisposition::Projected | AnnotationDisposition::Suppressed => Ok(()),
        AnnotationDisposition::Malformed(reason) => {
            let message = ctx.format_retained(
                format_args!(
                    "SWIFT annotation {} ({}): {reason}",
                    reference.id,
                    short_class(&entity.class)
                ),
                "retain SWIFT malformed annotation error",
            )?;
            Err(CodecError::Malformed(message))
        }
        AnnotationDisposition::Unsupported => {
            let class = short_class(&entity.class);
            ctx.charge_work(
                u64_from_index(class.len()),
                "count SLDPRT unsupported SWIFT classes",
            )?;
            if let Some(count) = unsupported.get_mut(class) {
                *count = count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "count SLDPRT unsupported SWIFT classes",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?;
            } else {
                let key = ctx.format_retained(
                    format_args!("{class}"),
                    "retain SLDPRT unsupported SWIFT class",
                )?;
                ctx.insert_btree_map(
                    unsupported,
                    key,
                    1,
                    "collect SLDPRT unsupported SWIFT classes",
                )?;
            }
            Ok(())
        }
    }
}

fn project_with_topology(
    ctx: &DecodeContext<'_>,
    root: &Entity,
    topology: Option<&TopologyIdentityIndex>,
    rendered: &[RenderedDimension],
    pattern_hole_nominals: Option<&BTreeMap<String, PositiveReal>>,
) -> Result<AnnotationProjection, CodecError> {
    if root.annotations.references.len() != root.annotations.entities.len() {
        return Ok(AnnotationProjection::default());
    }
    let feature_index = feature_index(ctx, root)?;
    let mut datum_ids = BTreeMap::new();
    for (reference, entity) in ctx.admit_iter(&root.annotations.references, "scan SWIFT annotation references")?
        .zip(ctx.admit_iter(&root.annotations.entities, "scan SWIFT annotation entities")?)
    {
        if !(short_class(&entity.class) == "GdtDatum"
            && !suppressed(entity)
            && entity
                .strings
                .get("DatumIdentifier")
                .is_some_and(|value| !value.is_empty()))
        {
            continue;
        }
        let Some(id) = pmi_id_charged(ctx, &reference.id)? else {
            continue;
        };
        ctx.insert_btree_map(
            &mut datum_ids,
            reference.id.as_str(),
            id,
            "index SWIFT datum IDs",
        )?;
    }
    let mut projected = Vec::new();
    let mut unsupported = BTreeMap::new();
    for (reference, entity) in ctx.admit_iter(&root.annotations.references, "scan SWIFT annotation references")?
        .zip(ctx.admit_iter(&root.annotations.entities, "scan SWIFT annotation entities")?)
    {
        if short_class(&entity.class) != "GdtDatum" {
            continue;
        }
        let disposition = if suppressed(entity) {
            AnnotationDisposition::Suppressed
        } else if pmi_id_charged(ctx, &reference.id)?.is_none() {
            AnnotationDisposition::Unsupported
        } else if let Some(annotation) =
            project_datum(ctx, reference, entity, &feature_index, topology)?
        {
            ctx.reserve_vec(&mut projected, 1, "collect SWIFT datum annotations")?;
            projected.push(annotation);
            AnnotationDisposition::Projected
        } else {
            missing_projection(ctx, entity)?
        };
        record_projection(ctx, &mut unsupported, reference, entity, disposition)?;
    }
    for (reference, entity) in ctx.admit_iter(&root.annotations.references, "scan SWIFT annotation references")?
        .zip(ctx.admit_iter(&root.annotations.entities, "scan SWIFT annotation entities")?)
    {
        if short_class(&entity.class) == "GdtDatum" {
            continue;
        }
        if suppressed(entity) {
            record_projection(
                ctx,
                &mut unsupported,
                reference,
                entity,
                AnnotationDisposition::Suppressed,
            )?;
            continue;
        }
        let Some(id) = pmi_id_charged(ctx, &reference.id)? else {
            record_projection(
                ctx,
                &mut unsupported,
                reference,
                entity,
                AnnotationDisposition::Unsupported,
            )?;
            continue;
        };
        let disposition = if let Some(tolerance) = project_tolerance(ctx, entity, &datum_ids)? {
            let Some(targets) = targets(ctx, entity, &feature_index, topology)? else {
                record_projection(
                    ctx,
                    &mut unsupported,
                    reference,
                    entity,
                    AnnotationDisposition::Unsupported,
                )?;
                continue;
            };
            ctx.charge_work(
                u64_from_index(projected.len()),
                "match SWIFT datum-system references",
            )?;
            let existing_system = ctx.admit_iter(&projected[..], "scan SLDPRT project_with_topology values")?.find(|annotation| {
                matches!(&annotation.definition, PmiDefinition::DatumSystem { references }
                    if *references == tolerance.references)
            });
            let datum_system = if tolerance.references.as_slice().is_empty() {
                None
            } else if let Some(existing) = existing_system {
                Some(copy_pmi_id(ctx, &existing.id)?)
            } else {
                let system_id = ctx.format_retained(
                    format_args!("{}:datum-system", id.as_str()),
                    "format SWIFT datum-system ID",
                )?;
                let system_id = PmiId::mint(system_id)
                    .map_err(|_| CodecError::malformed("invalid SWIFT datum-system ID"))?;
                let datum_system = copy_pmi_id(ctx, &system_id)?;
                ctx.reserve_vec(&mut projected, 1, "collect SWIFT datum systems")?;
                projected.push(PmiAnnotation {
                    id: system_id,
                    name: None,
                    visible: None,
                    targets: Vec::new(),
                    definition: PmiDefinition::DatumSystem {
                        references: tolerance.references,
                    },
                });
                Some(datum_system)
            };
            let (defined_unit, defined_area_unit, defined_area_second_unit) = defined_area(entity);
            ctx.reserve_vec(&mut projected, 1, "collect SWIFT tolerances")?;
            projected.push(PmiAnnotation {
                id,
                name: object_name(ctx, entity)?,
                visible: None,
                targets,
                definition: PmiDefinition::GeometricTolerance {
                    tolerance: tolerance.kind,
                    magnitude: tolerance.magnitude,
                    defined_unit,
                    defined_area_unit,
                    defined_area_second_unit,
                    datum_system,
                    modifiers: tolerance_modifiers(ctx, entity)?,
                },
            });
            if short_class(&entity.class) == "GdtCompositeSurfaceProfile" {
                if let Some(lower_tier) =
                    project_lower_profile_tier(ctx, reference, entity, &feature_index, topology)?
                {
                    ctx.reserve_vec(&mut projected, 1, "collect SWIFT lower tiers")?;
                    projected.push(lower_tier);
                }
            }
            AnnotationDisposition::Projected
        } else if let Some(annotation) = project_dimension(
            ctx,
            DimensionSource {
                root,
                reference,
                entity,
            },
            &feature_index,
            topology,
            rendered,
            pattern_hole_nominals,
        )? {
            ctx.reserve_vec(&mut projected, 1, "collect SWIFT dimensions")?;
            projected.push(annotation);
            AnnotationDisposition::Projected
        } else {
            missing_projection(ctx, entity)?
        };
        record_projection(ctx, &mut unsupported, reference, entity, disposition)?;
    }
    Ok(AnnotationProjection {
        annotations: projected,
        unsupported,
    })
}

fn project_datum(
    ctx: &DecodeContext<'_>,
    reference: &Reference,
    entity: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    topology: Option<&TopologyIdentityIndex>,
) -> Result<Option<PmiAnnotation>, CodecError> {
    let Some(identification) = entity
        .strings
        .get("DatumIdentifier")
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let identification = ctx.format_retained(
        format_args!("{identification}"),
        "copy SWIFT datum identifier",
    )?;
    let Some(targets) = targets(ctx, entity, feature_index, topology)? else {
        return Ok(None);
    };
    let Some(id) = pmi_id_charged(ctx, &reference.id)? else {
        return Ok(None);
    };
    let name = object_name(ctx, entity)?;
    Ok(
        (short_class(&entity.class) == "GdtDatum").then_some(PmiAnnotation {
            id,
            name,
            visible: None,
            targets,
            definition: PmiDefinition::Datum { identification },
        }),
    )
}

struct ProjectedTolerance {
    kind: GeometricToleranceKind,
    magnitude: cadmpeg_ir::pmi::PmiMagnitude,
    references: cadmpeg_ir::pmi::DatumReferences,
}

fn project_tolerance(
    ctx: &DecodeContext<'_>,
    entity: &Entity,
    datum_ids: &BTreeMap<&str, PmiId>,
) -> Result<Option<ProjectedTolerance>, CodecError> {
    let Some(kind) = tolerance_kind(short_class(&entity.class)) else {
        return Ok(None);
    };
    let Some(magnitude) = entity
        .doubles
        .get("Tolerance")
        .copied()
        .and_then(NonNegativeReal::new)
    else {
        return Ok(None);
    };
    let Some(references) = datum_references(ctx, entity, datum_ids)?.try_into().ok() else {
        return Ok(None);
    };
    Ok(Some(ProjectedTolerance {
        kind,
        magnitude: cadmpeg_ir::pmi::PmiMagnitude::from_parts(magnitude, PmiQuantity::Length),
        references,
    }))
}

fn project_lower_profile_tier(
    ctx: &DecodeContext<'_>,
    reference: &Reference,
    entity: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    topology: Option<&TopologyIdentityIndex>,
) -> Result<Option<PmiAnnotation>, CodecError> {
    let Some(magnitude) = entity
        .doubles
        .get("ToleranceLowerTier")
        .copied()
        .and_then(NonNegativeReal::new)
    else {
        return Ok(None);
    };
    let Some(id) = pmi_id_charged(ctx, &reference.id)? else {
        return Ok(None);
    };
    let Some(targets) = targets(ctx, entity, feature_index, topology)? else {
        return Ok(None);
    };
    let id = ctx.format_retained(
        format_args!("{}:lower-tier", id.as_str()),
        "format SWIFT lower-tier ID",
    )?;
    let id = PmiId::mint(id).map_err(|_| CodecError::malformed("invalid SWIFT lower-tier ID"))?;
    let name = entity
        .strings
        .get("ObjectName")
        .filter(|name| !name.is_empty())
        .map(|name| {
            ctx.format_retained(
                format_args!("{name} lower tier"),
                "format SWIFT lower-tier name",
            )
        })
        .transpose()?;
    Ok(Some(PmiAnnotation {
        id,
        name,
        visible: None,
        targets,
        definition: PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::SurfaceProfile,
            magnitude: cadmpeg_ir::pmi::PmiMagnitude::from_parts(magnitude, PmiQuantity::Length),
            defined_unit: None,
            defined_area_unit: None,
            defined_area_second_unit: None,
            datum_system: None,
            modifiers: vec!["composite_lower_tier".into()],
        },
    }))
}

#[derive(Clone, Copy)]
struct DimensionSource<'a> {
    root: &'a Entity,
    reference: &'a Reference,
    entity: &'a Entity,
}

fn project_dimension(
    ctx: &DecodeContext<'_>,
    source: DimensionSource<'_>,
    feature_index: &BTreeMap<&str, &Entity>,
    topology: Option<&TopologyIdentityIndex>,
    rendered: &[RenderedDimension],
    pattern_hole_nominals: Option<&BTreeMap<String, PositiveReal>>,
) -> Result<Option<PmiAnnotation>, CodecError> {
    let DimensionSource {
        root,
        reference,
        entity,
    } = source;
    let Some(dimension) = dimension_kind(short_class(&entity.class)) else {
        return Ok(None);
    };
    let quantity = dimension_quantity(&dimension);
    let Some(source_nominal) = entity
        .doubles
        .get("Nominal")
        .copied()
        .and_then(FiniteReal::new)
    else {
        return Ok(None);
    };
    let explicit_nominal = Some(source_nominal).filter(|value| {
        value.get() != 0.0
            || entity
                .integers
                .get("Dimension")
                .is_some_and(|dimension| *dimension != 0)
    });
    let implicit_nominal = if explicit_nominal.is_none() {
        implicit_dimension_nominal(
            ctx,
            root,
            entity,
            feature_index,
            rendered,
            pattern_hole_nominals,
        )?
    } else {
        None
    };
    let nominal = explicit_nominal.or(implicit_nominal);
    let tolerance = match (
        deviation(ctx, entity, nominal, "LowerLimit", "MinusTolerance")?,
        deviation(ctx, entity, nominal, "UpperLimit", "PlusTolerance")?,
    ) {
        (Some(lower), Some(upper)) => Some(DimensionTolerance::PlusMinus {
            lower: PmiValue::from_parts(lower, quantity),
            upper: PmiValue::from_parts(upper, quantity),
        }),
        _ => None,
    };
    let Some(id) = pmi_id_charged(ctx, &reference.id)? else {
        return Ok(None);
    };
    let Some(targets) = targets(ctx, entity, feature_index, topology)? else {
        return Ok(None);
    };
    let Ok(dimension) = cadmpeg_ir::pmi::PmiDimension::new(
        dimension,
        nominal.map(|value| PmiValue::from_parts(value, quantity)),
        tolerance,
    ) else {
        return Ok(None);
    };
    Ok(Some(PmiAnnotation {
        id,
        name: object_name(ctx, entity)?,
        visible: None,
        targets,
        definition: PmiDefinition::Dimension(dimension),
    }))
}

fn implicit_dimension_nominal(
    ctx: &DecodeContext<'_>,
    root: &Entity,
    entity: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    rendered: &[RenderedDimension],
    pattern_hole_nominals: Option<&BTreeMap<String, PositiveReal>>,
) -> Result<Option<FiniteReal>, CodecError> {
    let source = match short_class(&entity.class) {
        "GdtDiameter" => diameter_nominal(ctx, root, entity, feature_index, pattern_hole_nominals)?,
        "GdtDepth" => depth_nominal(ctx, root, entity, feature_index)?,
        "GdtWidth" => {
            width_from_applied_geometry(ctx, entity, feature_index)?.map(ImplicitNominal::Exact)
        }
        "GdtRadius" => {
            radius_from_applied_geometry(ctx, entity, feature_index)?.map(ImplicitNominal::Exact)
        }
        "GdtLength" => {
            length_from_applied_geometry(ctx, entity, feature_index)?.map(ImplicitNominal::Exact)
        }
        "GdtDistanceBetween" => {
            directional_distance(ctx, entity, feature_index)?.map(ImplicitNominal::Exact)
        }
        "GdtCounterBore" => {
            counterbore_from_direct_geometry(ctx, entity, feature_index)?.map(ImplicitNominal::Exact)
        }
        "GdtCounterSinkDiameter" => {
            countersink_diameter_from_direct_geometry(ctx, entity, feature_index)?
                .map(ImplicitNominal::Exact)
        }
        "GdtCounterSinkAngle" => countersink_angle_from_direct_geometry(ctx, entity, feature_index)?
            .map(ImplicitNominal::Exact),
        _ => None,
    };
    let Some(source) = source else {
        return Ok(None);
    };
    Ok(match source {
        ImplicitNominal::Exact(value) => Some(FiniteReal::from(value)),
        ImplicitNominal::Rendered { kind, geometry } => entity
            .integers
            .get("BlockToleranceDecimalPlaces")
            .copied()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value <= 9)
            .map(|decimal_places| rendered_nominal(ctx, geometry, decimal_places, kind, rendered)).transpose()?.flatten(),
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
            .map(|decimal_places| rendered_nominal(ctx, geometry, decimal_places, kind, rendered)).transpose()?.flatten()
            .or(Some(FiniteReal::from(exact))),
    })
}

fn dimension_quantity(dimension: &DimensionKind) -> PmiQuantity {
    matches!(dimension, DimensionKind::Angular)
        .then_some(PmiQuantity::Angle)
        .unwrap_or(PmiQuantity::Length)
}

fn diameter_from_applied_geometry(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<PositiveReal>, CodecError> {
    let contributors = diameter_contributors(ctx, annotation, feature_index)?;
    Ok(unique_diameter(ctx.admit_iter(&contributors, "scan SLDPRT applied hole diameters")?.copied()))
}

fn directional_distance(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<PositiveReal>, CodecError> {
    if annotation.integers.get("ComputeAnswerBy") != Some(&0)
        || annotation.integers.get("Direction") != Some(&4)
        || annotation.integers.get("NormalTo") != Some(&1)
    {
        return Ok(None);
    }
    let Some(transform) = unique_related(annotation, "NominalTransform") else {
        return Ok(None);
    };
    if !identity_transform(ctx, &transform.entity)? {
        return Ok(None);
    }
    let Some(direction_entity) = unique_related(annotation, "DirectionVector") else {
        return Ok(None);
    };
    let Some(direction) = vector(ctx, &direction_entity.entity, ["I", "J", "K"])? else {
        return Ok(None);
    };
    let [direction_i, direction_j, direction_k] = direction.get();
    if !approximately_equal(direction_i.hypot(direction_j).hypot(direction_k), 1.0) {
        return Ok(None);
    }
    if let Some(length) =
        closed_slot_feature_size_distance(ctx, annotation, feature_index, direction.get())?
    {
        return Ok(Some(length));
    }
    let [first, second] = annotation.features.references.as_slice() else {
        return Ok(None);
    };
    let Some(first) = location_projection(ctx, &first.id, feature_index, direction.get())? else {
        return Ok(None);
    };
    let Some(second) = location_projection(ctx, &second.id, feature_index, direction.get())? else {
        return Ok(None);
    };
    Ok(PositiveReal::new((second.get() - first.get()).abs()))
}

fn closed_slot_feature_size_distance(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    direction: [f64; 3],
) -> Result<Option<PositiveReal>, CodecError> {
    if annotation.integers.get("FeatureFosUsage") != Some(&2)
        || annotation.integers.get("OriginFeatureFosUsage") != Some(&2)
    {
        return Ok(None);
    }
    let Some((cylinder_id, cylinder, slot_id, slot)) = (|| {
        let [first_reference, second_reference] = annotation.features.references.as_slice() else {
            return Ok::<_, cadmpeg_core::CodecError>(None);
        };
        let first = match ctx.get_btree_map(&(feature_index), first_reference.id.as_str(), "look up SLDPRT ordered key")? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) };
        let second = match ctx.get_btree_map(&(feature_index), second_reference.id.as_str(), "look up SLDPRT ordered key")? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) };
        Ok::<_, cadmpeg_core::CodecError>(Some(
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
                _ => return Ok::<_, cadmpeg_core::CodecError>(None),
            },
        ))
    })()? else {
        return Ok(None);
    };
    if !feature_reaches(
        ctx,
        slot_id,
        cylinder_id,
        feature_index,
        &mut BTreeSet::new(),
        0,
    )? {
        return Ok(None);
    }
    (|| {
        let slot_geometry = &match unique_related(slot, "NomClosedSlot") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.entity;
        let cylinder_geometry = &match unique_related(cylinder, "NomCylinder") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.entity;
        let length = match PositiveReal::new(match slot_geometry.doubles.get("Length").copied() { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }) { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) };
        let width = match PositiveReal::new(match slot_geometry.doubles.get("Width").copied() { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }) { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) };
        let radius = match PositiveReal::new(match cylinder_geometry.doubles.get("R").copied() { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }) { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) };
        if length <= width || !diameters_equivalent(radius.get() * 2.0, width.get()) {
            return Ok::<_, cadmpeg_core::CodecError>(None);
        }
        let [slot_normal_i, slot_normal_j, slot_normal_k] =
            match vector(ctx, slot_geometry, ["I", "J", "K"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
        let [longitude_i, longitude_j, longitude_k] =
            match vector(ctx, slot_geometry, ["LongitudeI", "LongitudeJ", "LongitudeK"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
        let [slot_x, slot_y, slot_z] = match vector(ctx, slot_geometry, ["X", "Y", "Z"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
        let [axis_i, axis_j, axis_k] = match vector(ctx, cylinder_geometry, ["I", "J", "K"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
        let [cylinder_x, cylinder_y, cylinder_z] =
            match vector(ctx, cylinder_geometry, ["X", "Y", "Z"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
        let [direction_i, direction_j, direction_k] = direction;
        if !approximately_equal(slot_normal_i.hypot(slot_normal_j).hypot(slot_normal_k), 1.0)
            || !approximately_equal(longitude_i.hypot(longitude_j).hypot(longitude_k), 1.0)
            || !approximately_equal(axis_i.hypot(axis_j).hypot(axis_k), 1.0)
            || !approximately_equal(
                (slot_normal_i * axis_i + slot_normal_j * axis_j + slot_normal_k * axis_k).abs(),
                1.0,
            )
            || !approximately_equal(
                slot_normal_i * longitude_i
                    + slot_normal_j * longitude_j
                    + slot_normal_k * longitude_k,
                0.0,
            )
            || !approximately_equal(
                (longitude_i * direction_i + longitude_j * direction_j + longitude_k * direction_k)
                    .abs(),
                1.0,
            )
        {
            return Ok::<_, cadmpeg_core::CodecError>(None);
        }
        let displacement_x = cylinder_x - slot_x;
        let displacement_y = cylinder_y - slot_y;
        let displacement_z = cylinder_z - slot_z;
        let displacement_norm = displacement_x.hypot(displacement_y).hypot(displacement_z);
        let longitudinal = displacement_x * longitude_i
            + displacement_y * longitude_j
            + displacement_z * longitude_k;
        Ok::<_, cadmpeg_core::CodecError>((approximately_equal(displacement_norm, longitudinal.abs())
            && approximately_equal(longitudinal.abs(), (length.get() - width.get()) / 2.0))
        .then_some(length))
    })()
}

fn feature_reaches(
    ctx: &DecodeContext<'_>,
    id: &str,
    target: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Result<bool, CodecError> {
    ctx.charge_work(1, "traverse SWIFT feature reachability")?;
    if visited.contains(id) {
        return Ok(false);
    }
    admit_swift_depth(ctx, depth, "traverse SWIFT feature reachability")?;
    let _depth = ctx.enter_nested("traverse SWIFT feature reachability")?;
    let owned_id =
        ctx.format_retained(format_args!("{id}"), "retain SWIFT reachability path ID")?;
    ctx.insert_btree_set(visited, owned_id, "track SWIFT reachability path")?;
    let result = (|| {
        let Some(feature) = ctx.get_btree_map(&(feature_index), id, "look up SLDPRT ordered key")? else {
            return Ok(false);
        };
        let next_depth = depth.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "advance SWIFT recursion depth",
                u64_from_index(MAX_DEPTH),
                u64::MAX,
            )
        })?;
        for child in child_feature_ids(ctx, feature)? {
            if child == target
                || feature_reaches(ctx, child, target, feature_index, visited, next_depth)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    })();
    visited.remove(id);
    result
}

fn identity_transform(ctx: &cadmpeg_core::decode::DecodeContext<'_>, transform: &Entity) -> Result<bool, cadmpeg_core::CodecError> {
    Ok::<_, cadmpeg_core::CodecError>([
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
    .into_iter().try_fold(true, |found, (name, expected)| { Ok::<_, cadmpeg_core::CodecError>(found && ( {
        ctx.get_btree_map(&(transform
            .doubles), name, "look up SLDPRT ordered key")?
            .is_some_and(|value| approximately_equal(*value, expected))
    } )) })?)
}

fn location_projection(
    ctx: &DecodeContext<'_>,
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    direction: [f64; 3],
) -> Result<Option<FiniteReal>, CodecError> {
    let Some(feature) = ctx.get_btree_map(&(feature_index), id, "look up SLDPRT ordered key")? else {
        return Ok(None);
    };
    Ok(match short_class(&feature.class) {
        "GdtPlane" | "GdtIntersectPlane" => plane_projection(ctx, feature, direction)?,
        "GdtCylinder" => axis_projection(ctx, feature, "NomCylinder", direction)?,
        "GdtCone" => axis_projection(ctx, feature, "NomCone", direction)?,
        "GdtCompoundHole" => {
            let mut projections = Vec::new();
            collect_rotational_projections(
                ctx,
                id,
                feature_index,
                direction,
                &mut BTreeSet::new(),
                0,
                &mut projections,
            )?;
            unique_measurement(ctx.admit_iter(&projections, "scan SLDPRT rotational projections")?.copied())
        }
        _ => None,
    })
}

fn plane_projection(ctx: &cadmpeg_core::decode::DecodeContext<'_>, feature: &Entity, direction: [f64; 3]) -> Result<Option<FiniteReal>, cadmpeg_core::CodecError> {
    let plane = &match unique_related(feature, "NomPlane") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.entity;
    let [normal_i, normal_j, normal_k] = match vector(ctx, plane, ["I", "J", "K"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
    let [point_x, point_y, point_z] = match vector(ctx, plane, ["X", "Y", "Z"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
    let [direction_i, direction_j, direction_k] = direction;
    if !approximately_equal(normal_i.hypot(normal_j).hypot(normal_k), 1.0)
        || !approximately_equal(
            (normal_i * direction_i + normal_j * direction_j + normal_k * direction_k).abs(),
            1.0,
        )
    {
        return Ok::<_, cadmpeg_core::CodecError>(None);
    }
    Ok::<_, cadmpeg_core::CodecError>(FiniteReal::new(point_x * direction_i + point_y * direction_j + point_z * direction_k))
}

fn axis_projection(ctx: &cadmpeg_core::decode::DecodeContext<'_>, feature: &Entity, geometry: &str, direction: [f64; 3]) -> Result<Option<FiniteReal>, cadmpeg_core::CodecError> {
    let axis = &match unique_related(feature, geometry) { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.entity;
    let [axis_i, axis_j, axis_k] = match vector(ctx, axis, ["I", "J", "K"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
    let [point_x, point_y, point_z] = match vector(ctx, axis, ["X", "Y", "Z"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
    let [direction_i, direction_j, direction_k] = direction;
    if !approximately_equal(axis_i.hypot(axis_j).hypot(axis_k), 1.0)
        || !approximately_equal(
            axis_i * direction_i + axis_j * direction_j + axis_k * direction_k,
            0.0,
        )
    {
        return Ok::<_, cadmpeg_core::CodecError>(None);
    }
    Ok::<_, cadmpeg_core::CodecError>(FiniteReal::new(point_x * direction_i + point_y * direction_j + point_z * direction_k))
}

fn collect_rotational_projections(
    ctx: &DecodeContext<'_>,
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    direction: [f64; 3],
    visited: &mut BTreeSet<String>,
    depth: usize,
    projections: &mut Vec<FiniteReal>,
) -> Result<(), CodecError> {
    ctx.charge_work(1, "scan SWIFT rotational features")?;
    if visited.contains(id) {
        return Ok(());
    }
    admit_swift_depth(ctx, depth, "scan SWIFT rotational features")?;
    let _depth = ctx.enter_nested("scan SWIFT rotational features")?;
    let owned_id = ctx.format_retained(format_args!("{id}"), "retain SWIFT rotational path ID")?;
    ctx.insert_btree_set(visited, owned_id, "track SWIFT rotational path")?;
    let result = (|| {
        let Some(feature) = ctx.get_btree_map(&(feature_index), id, "look up SLDPRT ordered key")? else {
            return Ok(());
        };
        let projection = match short_class(&feature.class) {
            "GdtCylinder" => axis_projection(ctx, feature, "NomCylinder", direction)?,
            "GdtCone" => axis_projection(ctx, feature, "NomCone", direction)?,
            _ => None,
        };
        if let Some(projection) = projection {
            ctx.reserve_vec(projections, 1, "collect SWIFT rotational projections")?;
            projections.push(projection);
            return Ok(());
        }
        let next_depth = depth.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "advance SWIFT recursion depth",
                u64_from_index(MAX_DEPTH),
                u64::MAX,
            )
        })?;
        for child in child_feature_ids(ctx, feature)? {
            collect_rotational_projections(
                ctx,
                child,
                feature_index,
                direction,
                visited,
                next_depth,
                projections,
            )?;
        }
        Ok(())
    })();
    visited.remove(id);
    result
}

fn diameter_nominal(
    ctx: &DecodeContext<'_>,
    root: &Entity,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    pattern_hole_nominals: Option<&BTreeMap<String, PositiveReal>>,
) -> Result<Option<ImplicitNominal>, CodecError> {
    let direct = diameter_from_applied_geometry(ctx, annotation, feature_index)?;
    let geometry = if direct.is_some() {
        direct
    } else {
        hole_diameter_excluding_counterbore(ctx, root, annotation, feature_index)?
    };
    if let Some(geometry) = geometry {
        return Ok(Some(ImplicitNominal::RenderedOrExact {
            kind: RenderedDimensionKind::Diameter,
            geometry,
            exact: geometry,
        }));
    }
    Ok(
        empty_pattern_hole_nominal(ctx, annotation, feature_index, pattern_hole_nominals)?.map(
            |geometry| ImplicitNominal::RenderedOrExact {
                kind: RenderedDimensionKind::Diameter,
                geometry,
                exact: geometry,
            },
        ),
    )
}

fn empty_pattern_hole_nominal(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    pattern_hole_nominals: Option<&BTreeMap<String, PositiveReal>>,
) -> Result<Option<PositiveReal>, CodecError> {
    let [reference] = annotation.features.references.as_slice() else {
        return Ok(None);
    };
    let Some(pattern) = ctx.get_btree_map(&(feature_index), reference.id.as_str(), "look up SLDPRT ordered key")? else {
        return Ok(None);
    };
    if short_class(&pattern.class) != "GdtPattern" || !pattern.features.references.is_empty() {
        return Ok(None);
    }
    let Some(collection) = unique_related(pattern, "SubFeatures") else {
        return Ok(None);
    };
    if short_class(&collection.class) != "GdtAppliedFeatureCollection"
        || !collection.entity.related.is_empty()
    {
        return Ok(None);
    }
    let mut has_identifier = false;
    let mut has_nonempty_identifier = false;
    visit_cad_identifiers(ctx, pattern, &mut |identifier| {
        has_identifier = true;
        has_nonempty_identifier |= !identifier.is_empty();
        Ok(())
    })?;
    if !has_identifier || has_nonempty_identifier {
        return Ok(None);
    }
    let Some(name) = pattern
        .strings
        .get("ObjectName")
        .filter(|name| !name.is_empty())
    else {
        return Ok(None);
    };
    Ok(pattern_hole_nominals.map(|nominals| {Ok::<_, cadmpeg_core::CodecError>(ctx.get_btree_map(&(nominals), name, "look up SLDPRT ordered key")?.copied())}).transpose()?.flatten())
}

fn hole_diameter_excluding_counterbore(
    ctx: &DecodeContext<'_>,
    root: &Entity,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<PositiveReal>, CodecError> {
    let mut context = BTreeSet::new();
    for reference in &annotation.features.references {
        ctx.charge_work(1, "collect SWIFT diameter context")?;
        ctx.insert_btree_set(
            &mut context,
            reference.id.as_str(),
            "collect SWIFT diameter context",
        )?;
    }
    if context.is_empty() {
        return Ok(None);
    }
    let mut counterbore_diameter: Option<PositiveReal> = None;
    for candidate in &root.annotations.entities {
        ctx.charge_work(1, "scan SWIFT counterbore diameters")?;
        if suppressed(candidate) || short_class(&candidate.class) != "GdtCounterBore" {
            continue;
        }
        if direct_feature_context(ctx, candidate, feature_index, "GdtCylinder")?.as_ref()
            != Some(&context)
        {
            continue;
        }
        let Some(value) = counterbore_from_direct_geometry(ctx, candidate, feature_index)? else {
            continue;
        };
        if let Some(first) = counterbore_diameter {
            if !approximately_equal(value.get(), first.get()) {
                return Ok(None);
            }
        } else {
            counterbore_diameter = Some(value);
        }
    }
    let Some(counterbore_diameter) = counterbore_diameter else {
        return Ok(None);
    };
    let contributors = diameter_contributors(ctx, annotation, feature_index)?;
    let mut removed_counterbore = false;
    let remaining = ctx.admit_iter(&contributors, "scan SLDPRT hole diameter contributors")?.copied().filter(|value| {
        if diameters_equivalent(value.get(), counterbore_diameter.get()) {
            removed_counterbore = true;
            false
        } else {
            true
        }
    });
    let remaining_diameter = unique_diameter(remaining);
    Ok(removed_counterbore.then_some(remaining_diameter).flatten())
}

fn diameter_contributors(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Vec<PositiveReal>, CodecError> {
    let mut values = Vec::new();
    for reference in ctx.admit_iter(&annotation.features.references, "scan SLDPRT diameter_contributors values")? {
        collect_diameter_contributors(
            ctx,
            &reference.id,
            feature_index,
            &mut BTreeSet::new(),
            0,
            &mut values,
        )?;
    }
    Ok(values)
}

fn collect_diameter_contributors(
    ctx: &DecodeContext<'_>,
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
    values: &mut Vec<PositiveReal>,
) -> Result<(), CodecError> {
    ctx.charge_work(1, "scan SWIFT diameter features")?;
    if visited.contains(id) {
        return Ok(());
    }
    admit_swift_depth(ctx, depth, "scan SWIFT diameter features")?;
    let _depth = ctx.enter_nested("scan SWIFT diameter features")?;
    let owned_id = ctx.format_retained(format_args!("{id}"), "retain SWIFT diameter path ID")?;
    ctx.insert_btree_set(visited, owned_id, "track SWIFT diameter path")?;
    let result = (|| {
        let Some(feature) = ctx.get_btree_map(&(feature_index), id, "look up SLDPRT ordered key")? else {
            return Ok(());
        };
        let radius = match short_class(&feature.class) {
            "GdtCylinder" => nominal_radius(ctx, feature, "NomCylinder")?,
            "GdtSphere" => nominal_radius(ctx, feature, "NomSphere")?,
            _ => None,
        };
        if let Some(diameter) = radius.and_then(|radius| PositiveReal::new(radius.get() * 2.0)) {
            ctx.reserve_vec(values, 1, "collect SWIFT diameter contributors")?;
            values.push(diameter);
            return Ok(());
        }
        let next_depth = depth.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "advance SWIFT recursion depth",
                u64_from_index(MAX_DEPTH),
                u64::MAX,
            )
        })?;
        for child in child_feature_ids(ctx, feature)? {
            collect_diameter_contributors(ctx, child, feature_index, visited, next_depth, values)?;
        }
        Ok(())
    })();
    visited.remove(id);
    result
}

fn depth_from_applied_geometry(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<PositiveReal>, CodecError> {
    measurement_from_applied_geometry(ctx, annotation, |id| {
        depth_for_feature(ctx, id, feature_index, &mut BTreeSet::new(), 0)
    })
}

fn depth_nominal(
    ctx: &DecodeContext<'_>,
    root: &Entity,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<ImplicitNominal>, CodecError> {
    if annotation
        .integers
        .get("IsThreadDepth")
        .is_some_and(|value| *value != 0)
    {
        return Ok(thread_depth_from_direct_geometry(ctx, annotation, feature_index)?
            .map(ImplicitNominal::Exact));
    }
    if let Some(exact) = direct_cylinder_depth(ctx, annotation, feature_index)? {
        return Ok(Some(ImplicitNominal::RenderedOrExact {
            kind: RenderedDimensionKind::Depth,
            geometry: exact,
            exact,
        }));
    }
    if let Some(exact) = counterbore_depth_from_sibling(ctx, root, annotation, feature_index)? {
        return Ok(Some(ImplicitNominal::RenderedOrExact {
            kind: RenderedDimensionKind::Depth,
            geometry: exact,
            exact,
        }));
    }
    Ok(
        depth_from_applied_geometry(ctx, annotation, feature_index)?.map(|geometry| {
            ImplicitNominal::Rendered {
                kind: RenderedDimensionKind::Depth,
                geometry,
            }
        }),
    )
}

fn counterbore_depth_from_sibling(
    ctx: &DecodeContext<'_>,
    root: &Entity,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<PositiveReal>, CodecError> {
    let Some(plane) = unique_direct_feature(ctx, annotation, feature_index, "GdtPlane")? else {
        return Ok(None);
    };
    let Some(context) = direct_feature_context(ctx, annotation, feature_index, "GdtPlane")? else {
        return Ok(None);
    };
    let mut first: Option<PositiveReal> = None;
    for candidate in &root.annotations.entities {
        ctx.charge_work(1, "scan SWIFT counterbore depths")?;
        if suppressed(candidate) || short_class(&candidate.class) != "GdtCounterBore" {
            continue;
        }
        if direct_feature_context(ctx, candidate, feature_index, "GdtCylinder")?.as_ref()
            != Some(&context)
        {
            continue;
        }
        let Some(cylinder) = unique_direct_feature(ctx, candidate, feature_index, "GdtCylinder")? else {
            continue;
        };
        if !plane_terminates_cylinder(ctx, plane, cylinder)? {
            continue;
        }
        let Some(value) = nominal_cylinder_depth(ctx, cylinder)? else {
            continue;
        };
        if let Some(prior) = first {
            if !approximately_equal(value.get(), prior.get()) {
                return Ok(None);
            }
        } else {
            first = Some(value);
        }
    }
    Ok(first)
}

fn unique_direct_feature<'a>(
    ctx: &DecodeContext<'_>, annotation: &Entity,
    feature_index: &BTreeMap<&str, &'a Entity>, class: &str,
) -> Result<Option<&'a Entity>, CodecError> {
    let mut selected = None;
    for reference in ctx.admit_iter(&annotation.features.references, "scan SWIFT direct feature candidates")? {
        let Some(feature) = ctx.get_btree_map(feature_index, reference.id.as_str(), "look up SLDPRT ordered key")? else { continue; };
        if short_class(&feature.class) != class { continue; }
        if selected.is_some() { return Ok(None); }
        selected = Some(*feature);
    }
    Ok(selected)
}

fn direct_feature_context<'a>(
    ctx: &DecodeContext<'_>,
    annotation: &'a Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    operation_class: &str,
) -> Result<Option<BTreeSet<&'a str>>, CodecError> {
    let mut context = BTreeSet::new();
    for reference in &annotation.features.references {
        ctx.charge_work(1, "scan SWIFT feature context")?;
        if ctx.get_btree_map(&(feature_index), reference.id.as_str(), "look up SLDPRT ordered key")?
            .is_some_and(|feature| short_class(&feature.class) == operation_class)
        {
            continue;
        }
        ctx.insert_btree_set(
            &mut context,
            reference.id.as_str(),
            "collect SWIFT feature context",
        )?;
    }
    Ok((!context.is_empty()).then_some(context))
}

fn plane_terminates_cylinder(ctx: &cadmpeg_core::decode::DecodeContext<'_>, plane_feature: &Entity, cylinder_feature: &Entity) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(plane) = unique_related(plane_feature, "NomPlane").map(|object| &object.entity) else {
        return Ok::<_, cadmpeg_core::CodecError>(false);
    };
    let Some(origin) = unique_related(plane_feature, "NomOrigin").map(|object| &object.entity)
    else {
        return Ok::<_, cadmpeg_core::CodecError>(false);
    };
    let Some(cylinder) =
        unique_related(cylinder_feature, "NomCylinder").map(|object| &object.entity)
    else {
        return Ok::<_, cadmpeg_core::CodecError>(false);
    };
    let Some(bottom) = unique_related(cylinder_feature, "NomBottom").map(|object| &object.entity)
    else {
        return Ok::<_, cadmpeg_core::CodecError>(false);
    };
    let Some([plane_i, plane_j, plane_k]) = vector(ctx, plane, ["I", "J", "K"])?.map(FiniteVector::get)
    else {
        return Ok::<_, cadmpeg_core::CodecError>(false);
    };
    let Some([plane_x, plane_y, plane_z]) = vector(ctx, plane, ["X", "Y", "Z"])?.map(FiniteVector::get)
    else {
        return Ok::<_, cadmpeg_core::CodecError>(false);
    };
    let Some([origin_x, origin_y, origin_z]) =
        vector(ctx, origin, ["X", "Y", "Z"])?.map(FiniteVector::get)
    else {
        return Ok::<_, cadmpeg_core::CodecError>(false);
    };
    let Some([axis_x, axis_y, axis_z]) = vector(ctx, cylinder, ["I", "J", "K"])?.map(FiniteVector::get)
    else {
        return Ok::<_, cadmpeg_core::CodecError>(false);
    };
    let Some([bottom_x, bottom_y, bottom_z]) =
        vector(ctx, bottom, ["X", "Y", "Z"])?.map(FiniteVector::get)
    else {
        return Ok::<_, cadmpeg_core::CodecError>(false);
    };
    Ok::<_, cadmpeg_core::CodecError>(approximately_equal(plane_i.hypot(plane_j).hypot(plane_k), 1.0)
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
        && approximately_equal(origin_z, bottom_z))
}

fn direct_cylinder_depth(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<PositiveReal>, CodecError> {
    measurement_from_direct_features(
        ctx,
        annotation,
        feature_index,
        "GdtCylinder",
        |feature| nominal_cylinder_depth(ctx, feature),
    )
}

fn thread_depth_from_direct_geometry(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<PositiveReal>, CodecError> {
    measurement_from_direct_features(ctx, annotation, feature_index, "GdtCylinder", |feature| {
        Ok::<_, cadmpeg_core::CodecError>(feature
            .integers
            .get("IsThreaded")
            .is_some_and(|value| *value != 0)
            .then(|| {
                feature
                    .doubles
                    .get("ThreadDepth")
                    .copied()
                    .and_then(PositiveReal::new)
            })
            .flatten())
    })
}

fn width_from_applied_geometry(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<PositiveReal>, CodecError> {
    measurement_from_applied_geometry(ctx, annotation, |id| {
        width_for_feature(ctx, id, feature_index, &mut BTreeSet::new(), 0)
    })
}

fn radius_from_applied_geometry(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<PositiveReal>, CodecError> {
    measurement_from_applied_geometry(ctx, annotation, |id| {
        radius_for_feature(ctx, id, feature_index, &mut BTreeSet::new(), 0)
    })
}

fn length_from_applied_geometry(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<PositiveReal>, CodecError> {
    measurement_from_applied_geometry(ctx, annotation, |id| {
        length_for_feature(ctx, id, feature_index, &mut BTreeSet::new(), 0)
    })
}

fn counterbore_from_direct_geometry(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<PositiveReal>, CodecError> {
    measurement_from_direct_features(ctx, annotation, feature_index, "GdtCylinder", |feature| {
        Ok::<_, cadmpeg_core::CodecError>(PositiveReal::new(match nominal_radius(ctx, feature, "NomCylinder")? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get() * 2.0))
    })
}

fn countersink_diameter_from_direct_geometry(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<PositiveReal>, CodecError> {
    measurement_from_direct_features(
        ctx,
        annotation,
        feature_index,
        "GdtCone",
        |feature| nominal_cone_top_diameter(ctx, feature),
    )
}

fn countersink_angle_from_direct_geometry(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
) -> Result<Option<PositiveReal>, CodecError> {
    measurement_from_direct_features(ctx, annotation, feature_index, "GdtCone", |feature| Ok(nominal_cone_angle(feature)))
}

fn measurement_from_applied_geometry(
    ctx: &DecodeContext<'_>,
    annotation: &Entity,
    mut measurement: impl FnMut(&str) -> Result<Option<PositiveReal>, CodecError>,
) -> Result<Option<PositiveReal>, CodecError> {
    let mut first: Option<PositiveReal> = None;
    for reference in ctx.admit_iter(&annotation.features.references, "scan SWIFT applied measurement features")? {
        let Some(value) = measurement(&reference.id)? else {
            continue;
        };
        if let Some(prior) = first {
            if !approximately_equal(value.get(), prior.get()) {
                return Ok(None);
            }
        } else {
            first = Some(value);
        }
    }
    Ok(first)
}

fn measurement_from_direct_features(
    ctx: &DecodeContext<'_>, annotation: &Entity,
    feature_index: &BTreeMap<&str, &Entity>, class: &str,
    measurement: impl Fn(&Entity) -> Result<Option<PositiveReal>, CodecError>,
) -> Result<Option<PositiveReal>, CodecError> {
    let mut first: Option<PositiveReal> = None;
    for reference in ctx.admit_iter(&annotation.features.references, "scan SWIFT direct measurement features")? {
        let Some(feature) = ctx.get_btree_map(feature_index, reference.id.as_str(), "look up SLDPRT ordered key")? else { continue; };
        if short_class(&feature.class) != class { continue; }
        let Some(value) = measurement(feature)? else { continue; };
        if let Some(prior) = first {
            if !approximately_equal(value.get(), prior.get()) { return Ok(None); }
        } else { first = Some(value); }
    }
    Ok(first)
}

fn rendered_nominal(
    ctx: &DecodeContext<'_>,
    raw_mm: PositiveReal,
    decimal_places: u32,
    kind: RenderedDimensionKind,
    rendered_dimensions: &[RenderedDimension],
) -> Result<Option<FiniteReal>, CodecError> {
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
    let Ok(exponent) = i32::try_from(decimal_places) else { return Ok(None); };
    let precision = 10.0_f64.powi(exponent);
    let mut candidate: Option<FiniteReal> = None;
    let mut ambiguous = false;
    for scale in ctx.admit_iter(LENGTH_SCALES_MM, "scan SWIFT rendered unit scales")? {
        let rendered = (raw_mm.get() / scale * precision).round() / precision;
        for value in ctx.admit_iter(rendered_dimensions, "scan SWIFT rendered dimension values")?
            .filter(|value| value.kind == kind && value.decimal_places == decimal_places)
        {
            if approximately_equal(value.value.get(), rendered) {
                let Some(measured) = FiniteReal::new(value.value.get() * scale) else { return Ok(None); };
                if let Some(first) = candidate {
                    ambiguous |= !approximately_equal(measured.get(), first.get());
                } else {
                    candidate = Some(measured);
                }
            }
        }
    }
    Ok(if ambiguous { None } else { candidate })
}

fn rendered_dimensions(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<RenderedDimension>, CodecError> {
    const STRING_MARKER: &[u8] = &[0xff, 0xfe, 0xff];
    ctx.charge_work(
        u64_from_index(payload.len()),
        "scan SWIFT rendered literals",
    )?;
    let mut dimensions = Vec::new();
    for (offset, bytes) in payload.windows(STRING_MARKER.len()).enumerate() {
        if bytes != STRING_MARKER {
            continue;
        }
        let Some(length_offset) = offset.checked_add(STRING_MARKER.len()) else {
            continue;
        };
        let Some(units) = payload.get(length_offset).map(|count| usize::from(*count)) else {
            continue;
        };
        if !(1..=128).contains(&units) {
            continue;
        }
        let Some(start) = length_offset.checked_add(1) else {
            continue;
        };
        let Some(bytes) = units.checked_mul(4) else {
            continue;
        };
        let (mut text, _text_reservation) =
            ctx.scoped_string(bytes, "decode SWIFT rendered literal text")?;
        if decode_rendered_utf16(payload, start, units, &mut text).is_none() {
            continue;
        }
        let parsed = rendered_dimension_literals(ctx, &text)?;
        ctx.reserve_vec(
            &mut dimensions,
            parsed.len(),
            "collect SWIFT rendered dimensions",
        )?;
        dimensions.extend(parsed);
    }
    Ok(dimensions)
}

fn decode_rendered_utf16(
    payload: &[u8],
    start: usize,
    units: usize,
    text: &mut String,
) -> Option<()> {
    let mut view = View::over_retained(payload);
    view.seek(start)?;
    let mut remaining = units;
    while remaining > 0 {
        let unit = view.u16_le()?;
        remaining = remaining.checked_sub(1)?;
        let codepoint = if (0xd800..=0xdbff).contains(&unit) {
            if remaining == 0 {
                return None;
            }
            let low = view.u16_le()?;
            remaining = remaining.checked_sub(1)?;
            if !(0xdc00..=0xdfff).contains(&low) {
                return None;
            }
            let upper = u32::from(unit & 0x03ff).checked_shl(10)?;
            0x10000u32
                .checked_add(upper)?
                .checked_add(u32::from(low & 0x03ff))?
        } else if (0xdc00..=0xdfff).contains(&unit) {
            return None;
        } else {
            u32::from(unit)
        };
        text.push(char::from_u32(codepoint)?);
    }
    Some(())
}

fn rendered_dimension_literals(
    ctx: &DecodeContext<'_>,
    text: &str,
) -> Result<Vec<RenderedDimension>, CodecError> {
    const TOKENS: &[(&str, RenderedDimensionKind)] = &[
        ("<MOD-DIAM>", RenderedDimensionKind::Diameter),
        ("&lt;MOD-DIAM&gt;", RenderedDimensionKind::Diameter),
        ("<HOLE-DEPTH>", RenderedDimensionKind::Depth),
        ("&lt;HOLE-DEPTH&gt;", RenderedDimensionKind::Depth),
    ];
    let mut values = Vec::new();
    for (token, kind) in TOKENS {
        ctx.charge_work(
            u64_from_index(text.len()),
            "scan SWIFT rendered dimension tokens",
        )?;
        let mut remainder = text;
        while let Some((_, tail)) = remainder.split_once(token) {
            let literal = tail.trim_start();
            let end = ctx.admit_iter(literal.as_bytes(), "scan SWIFT rendered literal bytes")?
                .copied()
                .position(|byte| !byte.is_ascii_digit() && !matches!(byte, b'.' | b'+' | b'-'))
                .unwrap_or(literal.len());
            let literal = literal.get(..end).unwrap_or_default();
            if let Some((_, fractional)) = literal.split_once('.') {
                let parsed = literal.parse::<f64>().ok();
                let places = u32::try_from(fractional.len()).ok();
                if !fractional.is_empty() && ctx.admit_iter(fractional.as_bytes(), "scan SWIFT rendered fraction bytes")?.all(|byte| byte.is_ascii_digit()) {
                    if let (Some(value), Some(decimal_places)) = (parsed, places) {
                        if let Some(value) = PositiveReal::new(value) {
                            ctx.reserve_vec(
                                &mut values,
                                1,
                                "collect SWIFT rendered literal values",
                            )?;
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
    Ok(values)
}

fn depth_for_feature(
    ctx: &DecodeContext<'_>,
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Result<Option<PositiveReal>, CodecError> {
    measurement_for_feature(ctx, id, feature_index, visited, depth, |feature| { if short_class(&feature.class) == "GdtCylinder" { nominal_cylinder_depth(ctx, feature) } else { Ok(None) } })
}

fn width_for_feature(
    ctx: &DecodeContext<'_>,
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Result<Option<PositiveReal>, CodecError> {
    measurement_for_feature(
        ctx,
        id,
        feature_index,
        visited,
        depth,
        |feature| {Ok::<_, cadmpeg_core::CodecError>(match short_class(&feature.class) {
            "GdtCompoundWidth" => nominal_measurement(ctx, feature, "NomCompoundWidth", "Width")?,
            "GdtCompoundClosedSlot3D" => nominal_measurement(ctx, feature, "NomClosedSlot", "Width")?,
            _ => None,
        })},
    )
}

fn radius_for_feature(
    ctx: &DecodeContext<'_>,
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Result<Option<PositiveReal>, CodecError> {
    measurement_for_feature(
        ctx,
        id,
        feature_index,
        visited,
        depth,
        |feature| {Ok::<_, cadmpeg_core::CodecError>(match short_class(&feature.class) {
            "GdtFillet" => feature
                .doubles
                .get("Radius")
                .copied()
                .and_then(PositiveReal::new),
            "GdtCylinder" => nominal_radius(ctx, feature, "NomCylinder")?,
            "GdtSphere" => nominal_radius(ctx, feature, "NomSphere")?,
            _ => None,
        })},
    )
}

fn length_for_feature(
    ctx: &DecodeContext<'_>,
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Result<Option<PositiveReal>, CodecError> {
    measurement_for_feature(
        ctx,
        id,
        feature_index,
        visited,
        depth,
        |feature| {Ok::<_, cadmpeg_core::CodecError>(match short_class(&feature.class) {
            "GdtCompoundClosedSlot3D" => nominal_measurement(ctx, feature, "NomClosedSlot", "Length")?,
            _ => None,
        })},
    )
}

fn measurement_for_feature(
    ctx: &DecodeContext<'_>,
    id: &str,
    feature_index: &BTreeMap<&str, &Entity>,
    visited: &mut BTreeSet<String>,
    depth: usize,
    direct_measurement: impl Copy + Fn(&Entity) -> Result<Option<PositiveReal>, CodecError>,
) -> Result<Option<PositiveReal>, CodecError> {
    ctx.charge_work(1, "measure SWIFT feature geometry")?;
    if visited.contains(id) {
        return Ok(None);
    }
    admit_swift_depth(ctx, depth, "measure SWIFT feature geometry")?;
    let _depth = ctx.enter_nested("measure SWIFT feature geometry")?;
    let owned_id = ctx.format_retained(format_args!("{id}"), "retain SWIFT measurement path ID")?;
    ctx.insert_btree_set(visited, owned_id, "track SWIFT measurement path")?;
    let result = (|| {
        let Some(feature) = ctx.get_btree_map(&(feature_index), id, "look up SLDPRT ordered key")? else {
            return Ok(None);
        };
        if let Some(measurement) = direct_measurement(feature)? {
            return Ok(Some(measurement));
        }
        let next_depth = depth.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "advance SWIFT recursion depth",
                u64_from_index(MAX_DEPTH),
                u64::MAX,
            )
        })?;
        let mut first: Option<PositiveReal> = None;
        for child in child_feature_ids(ctx, feature)? {
            let Some(value) = measurement_for_feature(
                ctx,
                child,
                feature_index,
                visited,
                next_depth,
                direct_measurement,
            )?
            else {
                continue;
            };
            if let Some(prior) = first {
                if !approximately_equal(value.get(), prior.get()) {
                    return Ok(None);
                }
            } else {
                first = Some(value);
            }
        }
        Ok(first)
    })();
    visited.remove(id);
    result
}

fn child_feature_ids<'a>(ctx: &DecodeContext<'_>, feature: &'a Entity) -> Result<impl Iterator<Item = &'a str> + 'a, CodecError> {
    Ok(ctx.admit_iter(&feature.features.references, "scan SWIFT child feature references")?
        .map(|reference| reference.id.as_str())
        .chain(direct_subfeature_ids(ctx, feature)?.into_iter().flatten()))
}

fn nominal_radius(ctx: &cadmpeg_core::decode::DecodeContext<'_>, feature: &Entity, name: &str) -> Result<Option<PositiveReal>, cadmpeg_core::CodecError> {
    nominal_measurement(ctx, feature, name, "R")
}

fn nominal_measurement(ctx: &cadmpeg_core::decode::DecodeContext<'_>, feature: &Entity, object: &str, field: &str) -> Result<Option<PositiveReal>, cadmpeg_core::CodecError> {
    Ok::<_, cadmpeg_core::CodecError>(PositiveReal::new(
        match ctx.get_btree_map(&(match unique_related(feature, object) { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }
            .entity
            .doubles), field, "look up SLDPRT ordered key")?
            .copied() { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) },
    ))
}

fn nominal_cylinder_depth(ctx: &cadmpeg_core::decode::DecodeContext<'_>, feature: &Entity) -> Result<Option<PositiveReal>, cadmpeg_core::CodecError> {
    let cylinder = &match unique_related(feature, "NomCylinder") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.entity;
    let top = &match unique_related(feature, "NomTop") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.entity;
    let bottom = &match unique_related(feature, "NomBottom") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.entity;
    let [i, j, k] = match vector(ctx, cylinder, ["I", "J", "K"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
    let [top_x, top_y, top_z] = match vector(ctx, top, ["X", "Y", "Z"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
    let [bottom_x, bottom_y, bottom_z] = match vector(ctx, bottom, ["X", "Y", "Z"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
    let axis_norm = i.hypot(j).hypot(k);
    if !approximately_equal(axis_norm, 1.0) {
        return Ok::<_, cadmpeg_core::CodecError>(None);
    }
    let dx = top_x - bottom_x;
    let dy = top_y - bottom_y;
    let dz = top_z - bottom_z;
    let displacement = dx.hypot(dy).hypot(dz);
    let axial = (dx * i + dy * j + dz * k).abs();
    Ok::<_, cadmpeg_core::CodecError>(approximately_equal(displacement, axial)
        .then_some(axial)
        .and_then(PositiveReal::new))
}

fn nominal_cone_angle(feature: &Entity) -> Option<PositiveReal> {
    let angle = unique_related(feature, "NomCone")?
        .entity
        .doubles
        .get("FullAngle")
        .copied()
        .and_then(PositiveReal::new)?;
    (angle.get() < std::f64::consts::PI).then_some(angle)
}

fn nominal_cone_top_diameter(ctx: &cadmpeg_core::decode::DecodeContext<'_>, feature: &Entity) -> Result<Option<PositiveReal>, cadmpeg_core::CodecError> {
    let cone = &match unique_related(feature, "NomCone") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.entity;
    let top = &match unique_related(feature, "NomTop") { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.entity;
    let angle = match nominal_cone_angle(feature) { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
    let [axis_x, axis_y, axis_z] = match vector(ctx, cone, ["I", "J", "K"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
    let [apex_x, apex_y, apex_z] = match vector(ctx, cone, ["X", "Y", "Z"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
    let [top_i, top_j, top_k] = match vector(ctx, top, ["I", "J", "K"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
    let [top_x, top_y, top_z] = match vector(ctx, top, ["X", "Y", "Z"])? { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) }.get();
    if !approximately_equal(axis_x.hypot(axis_y).hypot(axis_z), 1.0)
        || !approximately_equal(top_i.hypot(top_j).hypot(top_k), 1.0)
        || !approximately_equal(
            (axis_x * top_i + axis_y * top_j + axis_z * top_k).abs(),
            1.0,
        )
    {
        return Ok::<_, cadmpeg_core::CodecError>(None);
    }
    let dx = top_x - apex_x;
    let dy = top_y - apex_y;
    let dz = top_z - apex_z;
    let displacement = dx.hypot(dy).hypot(dz);
    let axial = (dx * axis_x + dy * axis_y + dz * axis_z).abs();
    if !approximately_equal(displacement, axial) {
        return Ok::<_, cadmpeg_core::CodecError>(None);
    }
    Ok::<_, cadmpeg_core::CodecError>(PositiveReal::new(axial * (angle / 2.0).tan() * 2.0))
}

fn vector<const N: usize>(ctx: &DecodeContext<'_>, entity: &Entity, names: [&str; N]) -> Result<Option<FiniteVector<N>>, CodecError> {
    let mut components = [0.0; N];
    let mut missing = false;
    for (component, name) in components.iter_mut().zip(names) {
        match ctx.get_btree_map(&entity.doubles, name, "look up SLDPRT ordered key")? {
            Some(value) => *component = *value,
            None => missing = true,
        }
    }
    Ok(if missing { None } else { FiniteVector::new(components) })
}

fn unique_measurement<T: Copy + Into<f64>>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values
        .all(|value| approximately_equal(value.into(), first.into()))
        .then_some(first)
}

fn unique_diameter<T: Copy + Into<f64>>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values
        .all(|value| diameters_equivalent(value.into(), first.into()))
        .then_some(first)
}

fn diameters_equivalent(left: f64, right: f64) -> bool {
    approximately_equal(left, right) || (left - right).abs() <= DIAMETER_EQUIVALENCE_MM
}

fn approximately_equal(left: f64, right: f64) -> bool {
    if !left.is_finite() || !right.is_finite() {
        return false;
    }
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= scale * EPS_SWIFT_APPROXIMATELY_EQUAL_E9
}

fn deviation(ctx: &cadmpeg_core::decode::DecodeContext<'_>, 
    entity: &Entity,
    nominal: Option<FiniteReal>,
    limit_key: &str,
    tolerance_key: &str,
) -> Result<Option<FiniteReal>, cadmpeg_core::CodecError> {
    let tolerance = FiniteReal::new(match ctx.get_btree_map(&(entity.doubles), tolerance_key, "look up SLDPRT ordered key")?.copied() { Some(value) => value, None => return Ok::<_, cadmpeg_core::CodecError>(None) });
    if tolerance.map(FiniteReal::get) != Some(0.0) {
        return Ok::<_, cadmpeg_core::CodecError>(tolerance);
    }
    if let (Some(nominal), Some(limit)) = (
        nominal,
        ctx.get_btree_map(&(entity
            .doubles), limit_key, "look up SLDPRT ordered key")?
            .copied()
            .and_then(FiniteReal::new)
            .filter(|limit| limit.get() != 0.0),
    ) {
        return Ok::<_, cadmpeg_core::CodecError>(FiniteReal::new(limit.get() - nominal.get()));
    }
    Ok::<_, cadmpeg_core::CodecError>(tolerance)
}

const PRIMARY_DATUM_PRECEDENCE: NonZeroU32 = NonZeroU32::MIN;
const SECONDARY_DATUM_PRECEDENCE: Option<NonZeroU32> = NonZeroU32::new(2);
const TERTIARY_DATUM_PRECEDENCE: Option<NonZeroU32> = NonZeroU32::new(3);

fn datum_references(
    ctx: &DecodeContext<'_>,
    entity: &Entity,
    datum_ids: &BTreeMap<&str, PmiId>,
) -> Result<Vec<DatumReference>, CodecError> {
    let mut result = Vec::new();
    for (name, precedence) in [
        ("PrimaryDatums", Some(PRIMARY_DATUM_PRECEDENCE)),
        ("SecondaryDatums", SECONDARY_DATUM_PRECEDENCE),
        ("TertiaryDatums", TERTIARY_DATUM_PRECEDENCE),
    ] {
        let precedence = precedence.ok_or_else(|| {
            CodecError::InvalidInput("SWIFT datum precedence must be nonzero".into())
        })?;
        let Some(collection) = unique_related(entity, name) else {
            continue;
        };
        let applied_count = ctx.admit_iter(&collection.entity.related[..], "scan SLDPRT datum_references values")?
            .filter(|object| object.class.ends_with(".GdtAppliedDatum"))
            .count();
        for datum in ctx.admit_iter(&collection.entity.related, "project SWIFT datum references")?
            .filter(|object| object.class.ends_with(".GdtAppliedDatum"))
        {
            let [reference] = datum.entity.annotations.references.as_slice() else {
                continue;
            };
            let Some(id) = ctx.get_btree_map(&(datum_ids), reference.id.as_str(), "look up SLDPRT ordered key")? else {
                continue;
            };
            ctx.reserve_vec(&mut result, 1, "collect SWIFT datum references")?;
            result.push(DatumReference {
                datum: copy_pmi_id(ctx, id)?,
                precedence,
                common_group: (applied_count > 1).then_some(precedence.get()),
                modifiers: integer_modifier(datum.entity.integers.get("Modifier").copied()),
            });
        }
    }
    Ok(result)
}

fn unique_related<'a>(entity: &'a Entity, name: &str) -> Option<&'a RelatedObject> {
    let mut matches = entity.related.iter().filter(|object| object.name == name);
    let object = matches.next()?;
    matches.next().is_none().then_some(object)
}

fn feature_index<'a>(
    ctx: &DecodeContext<'_>,
    root: &'a Entity,
) -> Result<BTreeMap<&'a str, &'a Entity>, CodecError> {
    if root.features.references.len() != root.features.entities.len() {
        return Ok(BTreeMap::new());
    }
    let mut index = BTreeMap::new();
    for (reference, entity) in root.features.references.iter().zip(&root.features.entities) {
        ctx.charge_work(1, "index SWIFT features")?;
        ctx.insert_btree_map(
            &mut index,
            reference.id.as_str(),
            entity,
            "index SWIFT feature references",
        )?;
    }
    Ok(index)
}

fn targets(
    ctx: &DecodeContext<'_>,
    entity: &Entity,
    feature_index: &BTreeMap<&str, &Entity>,
    topology: Option<&TopologyIdentityIndex>,
) -> Result<Option<Vec<PmiTarget>>, CodecError> {
    let mut seen = BTreeSet::new();
    let mut targets = Vec::new();
    for reference in ctx.admit_iter(&entity.features.references, "scan SLDPRT targets values")? {
        let valid =
            visit_expanded_feature_ids(ctx, &reference.id, feature_index, 0, &mut |source_id| {
                if !ctx.insert_btree_set(
                    &mut seen,
                    source_id,
                    "deduplicate SWIFT target feature IDs",
                )? {
                    return Ok(true);
                }
                if !ctx.admit_iter(source_id, "scan SWIFT target source characters")?
                    .any(|character| !character.is_whitespace())
                {
                    return Ok(false);
                }
                let Some(feature) = ctx.get_btree_map(&(feature_index), source_id, "look up SLDPRT ordered key")? else {
                    ctx.reserve_vec(&mut targets, 1, "collect SWIFT shape-aspect targets")?;
                    targets.push(shape_aspect_target(ctx, source_id)?);
                    return Ok(true);
                };
                let first_target = targets.len();
                let mut had_identifier = false;
                let mut unresolved_identifier = false;
                visit_cad_identifiers(ctx, feature, &mut |identifier| {
                    had_identifier = true;
                    let Some(topology) = topology else {
                        unresolved_identifier = true;
                        return Ok(());
                    };
                    if let Some(target) = topology.resolve(identifier) {
                        ctx.charge_work(
                            u64_from_index((first_target..targets.len()).len()),
                            "deduplicate SWIFT topology targets",
                        )?;
                        if !targets
                            .iter()
                            .skip(first_target)
                            .any(|existing| existing == target)
                        {
                            ctx.reserve_vec(&mut targets, 1, "collect SWIFT topology targets")?;
                            targets.push(copy_pmi_target(ctx, target)?);
                        }
                    } else {
                        unresolved_identifier = true;
                    }
                    Ok(())
                })?;
                if !had_identifier || unresolved_identifier || targets.len() == first_target {
                    ctx.reserve_vec(&mut targets, 1, "collect SWIFT shape-aspect targets")?;
                    targets.push(shape_aspect_target(ctx, source_id)?);
                }
                Ok(true)
            })?;
        if !valid {
            return Ok(None);
        }
    }
    Ok(Some(targets))
}

fn shape_aspect_target(ctx: &DecodeContext<'_>, source_id: &str) -> Result<PmiTarget, CodecError> {
    let source_id =
        ctx.format_retained(format_args!("{source_id}"), "copy SWIFT shape-aspect ID")?;
    let source_id =
        cadmpeg_core::text::NonBlankString::for_decode(ctx, source_id, "validate nonblank text")?
            .ok_or_else(|| CodecError::malformed("invalid SWIFT shape-aspect ID"))?;
    Ok(PmiTarget::ShapeAspect { source_id })
}

fn copy_pmi_target(ctx: &DecodeContext<'_>, target: &PmiTarget) -> Result<PmiTarget, CodecError> {
    Ok(match target {
        PmiTarget::Body { body } => PmiTarget::Body {
            body: copy_topology_id(ctx, body.as_str())?,
        },
        PmiTarget::Face { face } => PmiTarget::Face {
            face: copy_topology_id(ctx, face.as_str())?,
        },
        PmiTarget::Edge { edge } => PmiTarget::Edge {
            edge: copy_topology_id(ctx, edge.as_str())?,
        },
        PmiTarget::Vertex { vertex } => PmiTarget::Vertex {
            vertex: copy_topology_id(ctx, vertex.as_str())?,
        },
        PmiTarget::Point { point } => PmiTarget::Point {
            point: copy_topology_id(ctx, point.as_str())?,
        },
        PmiTarget::Curve { curve } => PmiTarget::Curve {
            curve: copy_topology_id(ctx, curve.as_str())?,
        },
        PmiTarget::Product { product } => PmiTarget::Product {
            product: copy_topology_id(ctx, product.as_str())?,
        },
        PmiTarget::Occurrence { occurrence } => PmiTarget::Occurrence {
            occurrence: copy_topology_id(ctx, occurrence.as_str())?,
        },
        PmiTarget::ShapeAspect { source_id } => shape_aspect_target(ctx, source_id.as_str())?,
    })
}

fn visit_cad_identifiers(
    ctx: &DecodeContext<'_>,
    entity: &Entity,
    visit: &mut impl FnMut(&str) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("scan SWIFT CAD identifiers")?;
    ctx.charge_work(1, "scan SWIFT CAD identifiers")?;
    if short_class(&entity.class) == "CadRef" {
        if let Some(identifier) = entity.strings.get("CadIdentifier") {
            visit(identifier)?;
        }
    }
    for child in ctx.admit_iter(&entity.features.entities, "scan SLDPRT visit_cad_identifiers values")? {
        visit_cad_identifiers(ctx, child, visit)?;
    }
    for child in ctx.admit_iter(&entity.annotations.entities, "scan SLDPRT visit_cad_identifiers values")? {
        visit_cad_identifiers(ctx, child, visit)?;
    }
    for related in ctx.admit_iter(&entity.related, "scan SLDPRT visit_cad_identifiers values")? {
        visit_cad_identifiers(ctx, &related.entity, visit)?;
    }
    Ok(())
}

fn visit_expanded_feature_ids<'a>(
    ctx: &DecodeContext<'_>,
    id: &'a str,
    feature_index: &BTreeMap<&str, &'a Entity>,
    depth: usize,
    visit: &mut impl FnMut(&'a str) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    let _depth = ctx.enter_nested("expand SWIFT target features")?;
    ctx.charge_work(1, "expand SWIFT target features")?;
    let Some(feature) = ctx.get_btree_map(&(feature_index), id, "look up SLDPRT ordered key")? else {
        return visit(id);
    };
    if short_class(&feature.class) != "GdtPattern" {
        return visit(id);
    }
    admit_swift_depth(ctx, depth, "expand SWIFT target features")?;
    let Some(subfeatures) = direct_subfeature_ids(ctx, feature)? else {
        return visit(id);
    };
    for subfeature in subfeatures {
        let next_depth = depth.checked_add(1).ok_or_else(|| {
            ctx.refuse_codec_limit(
                "advance SWIFT recursion depth",
                u64_from_index(MAX_DEPTH),
                u64::MAX,
            )
        })?;
        if !visit_expanded_feature_ids(ctx, subfeature, feature_index, next_depth, visit)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn direct_subfeature_ids<'a>(ctx: &DecodeContext<'_>, feature: &'a Entity) -> Result<Option<impl Iterator<Item = &'a str> + 'a>, CodecError> {
    let Some(collection) = unique_related(feature, "SubFeatures") else { return Ok(None); };
    if collection.entity.related.is_empty() {
        return Ok(None);
    }
    for applied in ctx.admit_iter(&collection.entity.related, "validate SWIFT direct subfeatures")? {
        if !applied.class.ends_with(".GdtAppliedFeature") {
            return Ok(None);
        }
        let [_] = applied.entity.features.references.as_slice() else {
            return Ok(None);
        };
    }
    Ok(Some(ctx.admit_iter(&collection.entity.related, "scan SWIFT direct subfeatures")?.filter_map(|applied| {
        applied
            .entity
            .features
            .references
            .first()
            .map(|reference| reference.id.as_str())
    })))
}

fn tolerance_modifiers(ctx: &cadmpeg_core::decode::DecodeContext<'_>, entity: &Entity) -> Result<Vec<String>, cadmpeg_core::CodecError> {
    let mut values = integer_modifier(entity.integers.get("Modifier").copied());
    for (key, name) in [
        ("IsFreeState", "free_state"),
        ("IsStatistical", "statistical"),
        ("IsToBeInspected", "inspection"),
        ("IsTangentPlane", "tangent_plane"),
    ] {
        if ctx.get_btree_map(&(entity.integers), key, "look up SLDPRT ordered key")?.is_some_and(|value| *value != 0) {
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
            .and_then(|v| NonNegativeReal::new(*v))
        {
            values.push(format!("projected_zone:{}_mm", value.get()));
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
            .and_then(|v| NonNegativeReal::new(*v))
        {
            values.push(format!("maximum_tolerance:{}_mm", value.get()));
        } else {
            values.push("maximum_tolerance".into());
        }
    }
    Ok::<_, cadmpeg_core::CodecError>(values)
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
                .and_then(PositiveReal::new)
                .map(|value| PmiValue::from_parts(value.into(), PmiQuantity::Length)),
            Some("rectangular".into()),
            entity
                .doubles
                .get("PerUnitAreaWidth")
                .copied()
                .and_then(PositiveReal::new)
                .map(|value| PmiValue::from_parts(value.into(), PmiQuantity::Length)),
        ),
        Some(1) => (
            entity
                .doubles
                .get("PerUnitAreaDiameter")
                .copied()
                .and_then(PositiveReal::new)
                .map(|value| PmiValue::from_parts(value.into(), PmiQuantity::Length)),
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

fn object_name(ctx: &DecodeContext<'_>, entity: &Entity) -> Result<Option<String>, CodecError> {
    entity
        .strings
        .get("ObjectName")
        .filter(|name| !name.is_empty())
        .map(|name| ctx.format_retained(format_args!("{name}"), "copy SWIFT object name"))
        .transpose()
}

#[cfg(test)]
fn pmi_id(source_id: &str) -> Option<PmiId> {
    PmiId::mint(format!("sldprt:model:pmi#{source_id}")).ok()
}

fn pmi_id_charged(ctx: &DecodeContext<'_>, source_id: &str) -> Result<Option<PmiId>, CodecError> {
    let text = ctx.format_retained(
        format_args!("sldprt:model:pmi#{source_id}"),
        "format SWIFT PMI identity",
    )?;
    Ok(PmiId::mint(text).ok())
}

fn copy_pmi_id(ctx: &DecodeContext<'_>, id: &PmiId) -> Result<PmiId, CodecError> {
    let copy_work = cadmpeg_core::decode::u64_from_index(id.as_str().len())
        .checked_mul(4)
        .ok_or_else(|| ctx.refuse_codec_limit("copy SWIFT PMI identity", u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(copy_work, "copy SWIFT PMI identity")?;
    let text = ctx.format_retained(format_args!("{}", id.as_str()), "copy SWIFT PMI identity")?;
    PmiId::mint(text).map_err(|_| CodecError::malformed("invalid SWIFT PMI identity"))
}

fn suppressed(entity: &Entity) -> bool {
    entity
        .integers
        .get("IsSuppressed")
        .is_some_and(|value| *value != 0)
}

#[cfg(test)]
mod tests;
