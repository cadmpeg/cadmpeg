// SPDX-License-Identifier: Apache-2.0
//! Nested history bodies of Base and Split features
//! ([spec §13.10](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md#1310-feature-history-states)).
//!
//! The body that a Base feature produces and the body that a Split feature
//! consumes exist only in an earlier history state of the active carrier. The
//! reader materializes that state, decodes it, and keeps the one body with the
//! checked key in a nest of its own. The model arenas never hold its records.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use cadmpeg_asm::brep::annotations::AnnotationRecord;
use cadmpeg_asm::brep::inspect_with_source;
use cadmpeg_asm::brep::transfer::{transfer_inspection, InspectionGeometry};
use cadmpeg_asm::history::walk_entity_versions;
use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::features::{Feature, FeatureId};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::history_body::{BodyTopology, HistoryBody, HistoryStateRef};
use cadmpeg_ir::ids::{BodyId, HistoryBodyId};
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_ir::schema::EntitySchema;
use cadmpeg_ir::topology::{BodyKind, EdgeCarrier};
use cadmpeg_ir::UnknownRecord;

use super::history_states::{
    Binding, Closed, FeatureHistoryState, BASE_TAG, NATIVE_BASE_KIND, SPLIT_TAG,
};
use super::history_views::{with_state_table, Decoded};
use crate::kernel::ActiveCarrier;

/// Cap of nests per part. A nest materializes and decodes a whole history state.
// resource-cap: MAX_HISTORY_BODIES = 4
pub(crate) const MAX_HISTORY_BODIES: usize = 8;
/// Cap of records materialized for all nests of a part.
// resource-cap: MAX_HISTORY_BODY_WORK = 4
pub(crate) const MAX_HISTORY_BODY_WORK: usize = 1 << 21;
/// Scope of every identity in a nest.
const NEST_SCOPE: &str = "history-body";

/// Which side of a feature holds the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// The body is the result of a Base feature, in the state after it.
    Base,
    /// The body is consumed by a Split feature, in the state before it.
    Split,
}

/// One body to extract.
#[derive(Debug, Clone, Copy)]
struct Want {
    /// Ordinal of the request among all requests of the part.
    index: usize,
    /// Index of the feature record.
    record: usize,
    /// Oldest-first ordinal of the state that holds the body.
    ordinal: u32,
    /// State identifier of the `delta_state` header.
    state_id: i64,
    /// Body key.
    key: i64,
    role: Role,
}

/// Counts of the bodies that were asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// Bound Base and Split features.
    pub(crate) requested: usize,
    /// Nests produced.
    pub(crate) built: usize,
    /// States that did not materialize or decode, or bodies absent from them.
    pub(crate) undecoded: usize,
    /// Bodies that are not closed manifold solids, by [`Open::slot`].
    pub(crate) open: [usize; 3],
    /// Bodies skipped under the nest or work cap.
    pub(crate) bounded: usize,
    /// Decoded bodies whose Base or Split feature is not a projected feature.
    pub(crate) unlinked: usize,
}

/// The decoded geometry of a state and the source records it references.
#[derive(Debug, Default)]
pub(crate) struct StateRecords {
    pub(crate) geometry: Topology,
    /// Original body slots of the nests admitted from this state.
    pub(crate) body_slots: BTreeMap<HistoryBodyId, BodyId>,
    pub(crate) unknowns: Vec<UnknownRecord>,
    pub(crate) provenance: Vec<AnnotationRecord>,
}

/// Nests of a part and their links.
#[derive(Debug, Default)]
pub(crate) struct Built {
    /// Nests, sorted by identity.
    pub(crate) bodies: Vec<HistoryBody>,
    /// The states of admitted nests, by oldest-first ordinal.
    pub(crate) states: BTreeMap<u32, StateRecords>,
    /// Identity of the nest of each feature record, by record index.
    pub(crate) links: HashMap<usize, HistoryBodyId>,
    /// The nest that each request whose body was given up would have, and
    /// why the body was given up.
    pub(crate) unbuilt: Vec<(HistoryBodyId, Unbuilt, String)>,
    /// Counts.
    pub(crate) outcome: Outcome,
}

/// Why a request gives no nest for a body that its state holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unbuilt {
    /// The carrier or the state did not materialize or decode.
    Undecoded,
    /// A carrier of the body has no solved form.
    Unsolved,
    /// The state has more than one body with the key.
    Ambiguous,
    /// The body is not a closed manifold solid.
    NotClosed(Open),
    /// The feature of the body is not a projected feature.
    Unlinked,
    /// The request reached the nest cap, a work cap, or a decode resource limit.
    Bounded,
    /// A decode or admission limit refused this body.
    ResourceBounded(cadmpeg_core::decode::ResourceLimit),
}

impl Built {
    fn give_up(
        &mut self,
        want: &Want,
        why: Unbuilt,
        records: &[FeatureHistoryState],
    ) -> Result<(), CodecError> {
        self.unbuilt.push((
            nest_id(want.index)?,
            why,
            records[want.record].feature.clone(),
        ));
        Ok(())
    }
}

/// Result of the extraction of one body.
enum Extract {
    Nest(Box<BodyTopology>),
    /// The state holds no body with the key.
    Absent,
    /// A carrier of the body has no solved form.
    Unsolved,
    /// The body is not a closed manifold solid.
    NotClosed(Open),
}

/// Why a body is not a closed manifold solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Open {
    /// A reference names no record of the nest.
    Reference,
    /// The body is not a solid body with faces.
    Kind,
    /// An edge with a carrier does not join two coedges.
    Edge,
}

impl Open {
    const fn slot(self) -> usize {
        match self {
            Self::Reference => 0,
            Self::Kind => 1,
            Self::Edge => 2,
        }
    }
}

impl Outcome {
    /// Bodies that are not closed manifold solids.
    pub(crate) fn not_closed(&self) -> usize {
        self.open.iter().sum()
    }
}

fn wants(records: &[FeatureHistoryState], outcome: &mut Outcome) -> Vec<Want> {
    let mut wants = Vec::new();
    for (record, state) in records.iter().enumerate() {
        let role = match state.kind.as_str() {
            SPLIT_TAG => Role::Split,
            BASE_TAG | NATIVE_BASE_KIND => Role::Base,
            _ => continue,
        };
        let (Binding::Bound, Some(key), Some(after)) =
            (state.binding, state.body_key, state.state_after)
        else {
            continue;
        };
        let ordinal = match role {
            Role::Split => state.state_before,
            Role::Base => Some(after.ordinal),
        };
        let Some(ordinal) = ordinal else {
            continue;
        };
        outcome.requested += 1;
        wants.push(Want {
            index: wants.len(),
            record,
            ordinal,
            state_id: 0,
            key,
            role,
        });
    }
    wants
}

/// Gives up the requests of `wants` past the nest cap.
fn bound(
    mut wants: Vec<Want>,
    built: &mut Built,
    records: &[FeatureHistoryState],
) -> Result<Vec<Want>, CodecError> {
    for want in wants.split_off(wants.len().min(MAX_HISTORY_BODIES)) {
        built.outcome.bounded += 1;
        built.give_up(&want, Unbuilt::Bounded, records)?;
    }
    Ok(wants)
}

/// Materialize the states of the bound Base and Split features of `records`
/// and keep the bodies they name.
pub(crate) fn build(
    ctx: &DecodeContext<'_>,
    carrier: &ActiveCarrier<'_>,
    closed: &Closed<'_>,
    records: &[FeatureHistoryState],
    model_features: &[Feature],
) -> Result<Built, CodecError> {
    let mut built = Built::default();
    let requested = wants(records, &mut built.outcome);
    let mut wants = bound(requested, &mut built, records)?;
    let Ok(header) = carrier.header.as_ref() else {
        built.outcome.undecoded += wants.len();
        for want in &wants {
            built.give_up(want, Unbuilt::Undecoded, records)?;
        }
        return Ok(built);
    };
    if wants.is_empty() {
        return Ok(built);
    }
    let Closed {
        views,
        archived_ids,
        active_count,
        ..
    } = closed;
    let state_count = views.len();
    let Some(archive) = closed.archive() else {
        built.outcome.undecoded += wants.len();
        for want in &wants {
            built.give_up(want, Unbuilt::Undecoded, records)?;
        }
        return Ok(built);
    };
    let work = WorkBudget::new(MAX_HISTORY_BODY_WORK);
    let mut by_ordinal = BTreeMap::<u32, Vec<usize>>::new();
    for (index, want) in wants.iter().enumerate() {
        by_ordinal.entry(want.ordinal).or_default().push(index);
    }
    let features = model_features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, &feature.id)))
        .collect::<HashMap<_, _>>();
    let mut visited = 0_usize;
    let mut failure = None;
    let mut handled = [false; MAX_HISTORY_BODIES];
    let bytes = carrier.bytes.window();
    let stream = carrier.record_stream();
    walk_entity_versions(views, *active_count, archived_ids, |slot, versions| {
        let visit = visited;
        visited += 1;
        let Some(ordinal) = state_count
            .checked_sub(visit + 1)
            .and_then(|ordinal| u32::try_from(ordinal).ok())
        else {
            return;
        };
        let Some(indices) = by_ordinal.get(&ordinal) else {
            return;
        };
        if failure.is_some() {
            return;
        }
        let mut visit = || -> Result<(), CodecError> {
            for &index in indices {
                wants[index].state_id = views[slot].state_id;
            }
            let refused = |why| indices.iter().map(|_| Err(why)).collect::<Vec<_>>();
            let (state, candidates) = if work.charge_by(work_units(versions.len())) {
                match with_state_table(ctx, archive, versions, |state_ctx, table| {
                    let mut brep = inspect_with_source(
                        state_ctx,
                        table,
                        bytes,
                        Some(&header.metadata),
                        &stream,
                        cadmpeg_asm::asm_format!("inventor"),
                    )?;
                    let unknowns = std::mem::take(&mut brep.unknowns);
                    let ids: HashSet<_> =
                        unknowns.iter().map(|record| record.id().as_str()).collect();
                    let provenance = std::mem::take(&mut brep.annotation_records)
                        .into_iter()
                        .filter(|record| ids.contains(record.id.as_str()))
                        .collect();
                    let state = transfer_inspection(state_ctx, brep)?;
                    let candidates = indices
                        .iter()
                        .map(|&index| {
                            let want = &wants[index];
                            let feature = features.get(records[want.record].feature.as_str());
                            match state_ctx.scratch(bytes.len(), |scratch| {
                                candidate(scratch, &state, want, feature.copied())
                            }) {
                                Ok(Ok(result)) => Ok(result),
                                Ok(Err(limit)) => Ok(Err(Unbuilt::ResourceBounded(limit))),
                                Err(error @ CodecError::ResourceLimit(_)) => Err(error),
                                Err(_) => Ok(Err(Unbuilt::Undecoded)),
                            }
                        })
                        .collect::<Result<Vec<_>, CodecError>>()?;
                    let state = StateRecords {
                        geometry: state.geometry,
                        body_slots: BTreeMap::new(),
                        unknowns,
                        provenance,
                    };
                    Ok((state, candidates))
                })? {
                    Decoded::Scratch((state, candidates)) => (Some(state), candidates),
                    Decoded::Failed => (None, refused(Unbuilt::Undecoded)),
                    Decoded::Bounded(limit) => (None, refused(Unbuilt::ResourceBounded(limit))),
                }
            } else {
                (None, refused(Unbuilt::Bounded))
            };
            let mut body_slots = BTreeMap::new();
            for (&index, candidate) in indices.iter().zip(candidates) {
                let want = &wants[index];
                handled[index] = true;
                let why = match candidate {
                    Ok(Some((body, slot))) => {
                        if let Err(limit) = admit_nest(ctx, &body) {
                            Unbuilt::ResourceBounded(limit)
                        } else {
                            body_slots.insert(body.id.clone(), slot);
                            built.links.insert(want.record, body.id.clone());
                            built.bodies.push(body);
                            built.outcome.built += 1;
                            continue;
                        }
                    }
                    Ok(None) => {
                        built.outcome.undecoded += 1;
                        continue;
                    }
                    Err(why) => why,
                };
                match why {
                    Unbuilt::Bounded | Unbuilt::ResourceBounded(_) => built.outcome.bounded += 1,
                    Unbuilt::Undecoded | Unbuilt::Unsolved | Unbuilt::Ambiguous => {
                        built.outcome.undecoded += 1;
                    }
                    Unbuilt::NotClosed(open) => built.outcome.open[open.slot()] += 1,
                    Unbuilt::Unlinked => built.outcome.unlinked += 1,
                }
                built.give_up(want, why, records)?;
            }
            if let Some(mut state) = state.filter(|_| !body_slots.is_empty()) {
                state.body_slots = body_slots;
                built.states.insert(ordinal, state);
            }
            Ok(())
        };
        failure = visit().err();
    });
    if let Some(error) = failure {
        return Err(error);
    }
    for (index, want) in wants.iter().enumerate() {
        if !handled[index] {
            built.outcome.undecoded += 1;
            built.give_up(want, Unbuilt::Undecoded, records)?;
        }
    }
    built.unbuilt.sort_by(|left, right| left.0.cmp(&right.0));
    built.bodies.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(built)
}

type Candidate = Result<Option<(HistoryBody, BodyId)>, Unbuilt>;

fn candidate(
    ctx: &DecodeContext<'_>,
    state: &InspectionGeometry,
    want: &Want,
    feature: Option<&FeatureId>,
) -> Result<Candidate, CodecError> {
    let Ok(key) = u64::try_from(want.key) else {
        return Ok(Ok(None));
    };
    let mut slots = state
        .body_keys
        .iter()
        .filter(|body| body.asm_body_key == Some(key));
    let Some(body) = slots.next() else {
        return Ok(Ok(None));
    };
    if slots.next().is_some() {
        return Ok(Err(Unbuilt::Ambiguous));
    }
    Ok(match extract(ctx, &state.geometry, body.body.as_str())? {
        Extract::Absent => Ok(None),
        Extract::Unsolved => Err(Unbuilt::Unsolved),
        Extract::NotClosed(open) => Err(Unbuilt::NotClosed(open)),
        Extract::Nest(topology) => {
            let Some(feature) = feature else {
                return Ok(Err(Unbuilt::Unlinked));
            };
            let slot = topology.body.id.clone();
            Ok(Some((nest(ctx, want, feature.clone(), *topology)?, slot)))
        }
    })
}

fn admit_nest(
    ctx: &DecodeContext<'_>,
    body: &HistoryBody,
) -> Result<(), cadmpeg_core::decode::ResourceLimit> {
    ctx.admit_optional(
        cadmpeg_core::decode::u64_from_index(body.topology.name_bytes()),
        cadmpeg_core::decode::u64_from_index(body.topology.record_count()),
        "keep Inventor history body records and names",
    )
}

/// The identity of the nest of request `ordinal`.
fn nest_id(ordinal: usize) -> Result<HistoryBodyId, CodecError> {
    HistoryBodyId::mint(format!("inventor:{NEST_SCOPE}:nest#h{ordinal:04}"))
        .map_err(|error| CodecError::malformed(format_args!("{error}")))
}

/// The slot in the ASM state from which this nest's body ID was composed.
pub(crate) fn original_body_slot(body: &HistoryBody) -> Option<BodyId> {
    let request = body
        .id
        .as_str()
        .strip_prefix("inventor:history-body:nest#")?;
    let key = body
        .topology
        .body
        .id
        .as_str()
        .strip_prefix("inventor:history-body:entity#")?
        .strip_prefix(request)?
        .strip_prefix('-')?;
    BodyId::mint(format!("inventor:brep:entity#{key}")).ok()
}

/// The nest in the history-body scope, with the feature link of its role.
fn nest(
    ctx: &DecodeContext<'_>,
    want: &Want,
    feature: FeatureId,
    topology: BodyTopology,
) -> Result<HistoryBody, CodecError> {
    let prefix = format!("h{:04}-", want.index);
    let mut map = IdentityMap::new(|id: &str| {
        let malformed = || CodecError::malformed(format_args!("history body identity {id}"));
        let (format, rest) = id.split_once(':').ok_or_else(malformed)?;
        let (_, rest) = rest.split_once(':').ok_or_else(malformed)?;
        let (kind, key) = rest.split_once('#').ok_or_else(malformed)?;
        Ok(format!("{format}:{NEST_SCOPE}:{kind}#{prefix}{key}"))
    });
    let mut topology = topology.rewrite_identities(ctx, &mut map)?;
    map.finish()?;
    sort_by_identity(&mut topology);
    let (produced_by, consumed_by) = match want.role {
        Role::Base => (Some(feature), None),
        Role::Split => (None, Some(feature)),
    };
    Ok(HistoryBody {
        id: nest_id(want.index)?,
        produced_by,
        consumed_by,
        state_of: None,
        state: HistoryStateRef {
            ordinal: want.ordinal,
            identifier: want.state_id,
        },
        topology: Box::new(topology),
    })
}

fn sort_by_identity(topology: &mut BodyTopology) {
    fn sort<T: EntitySchema>(records: &mut [T]) {
        records.sort_by(|left, right| left.identity().cmp(right.identity()));
    }
    sort(&mut topology.regions);
    sort(&mut topology.shells);
    sort(&mut topology.faces);
    sort(&mut topology.loops);
    sort(&mut topology.coedges);
    sort(&mut topology.edges);
    sort(&mut topology.vertices);
    sort(&mut topology.surfaces);
    sort(&mut topology.curves);
    sort(&mut topology.pcurves);
    sort(&mut topology.points);
}

/// One record of a decoded state, by identity.
#[derive(Clone, Copy)]
enum Record<'a> {
    Body(&'a cadmpeg_ir::topology::Body),
    Region(&'a cadmpeg_ir::topology::Region),
    Shell(&'a cadmpeg_ir::topology::Shell),
    Face(&'a cadmpeg_ir::topology::Face),
    Loop(&'a cadmpeg_ir::topology::Loop),
    Coedge(&'a cadmpeg_ir::topology::Coedge),
    Edge(&'a cadmpeg_ir::topology::Edge),
    Vertex(&'a cadmpeg_ir::topology::Vertex),
    /// A carrier, which refers to no record of the state.
    Surface,
    /// A carrier, which refers to no record of the state.
    Curve,
    Pcurve(&'a cadmpeg_ir::geometry::pcurve::Pcurve),
    Point(&'a cadmpeg_ir::topology::Point),
}

impl Record<'_> {
    fn references(
        self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        match self {
            // A carrier names a procedural construction or a source object at
            // most; the nest keeps its solved geometry only.
            Self::Surface | Self::Curve => Ok(()),
            Self::Body(record) => record.visit_references(ctx, visitor),
            Self::Region(record) => record.visit_references(ctx, visitor),
            Self::Shell(record) => record.visit_references(ctx, visitor),
            Self::Face(record) => record.visit_references(ctx, visitor),
            Self::Loop(record) => record.visit_references(ctx, visitor),
            Self::Coedge(record) => record.visit_references(ctx, visitor),
            Self::Edge(record) => record.visit_references(ctx, visitor),
            Self::Vertex(record) => record.visit_references(ctx, visitor),
            Self::Pcurve(record) => record.visit_references(ctx, visitor),
            Self::Point(record) => record.visit_references(ctx, visitor),
        }
    }
}

/// The records reachable from `body`, with the count of references that name
/// no record of the model.
fn reach<'a>(
    ctx: &DecodeContext<'_>,
    geometry: &'a Topology,
    body: &str,
) -> Result<(HashSet<&'a str>, usize), CodecError> {
    let mut table = HashMap::<&str, Record<'_>>::new();
    macro_rules! index {
        ($arena:ident, $variant:ident) => {
            for record in &geometry.$arena {
                table.insert(record.identity(), Record::$variant(record));
            }
        };
    }
    index!(bodies, Body);
    index!(regions, Region);
    index!(shells, Shell);
    index!(faces, Face);
    index!(loops, Loop);
    index!(coedges, Coedge);
    index!(edges, Edge);
    index!(vertices, Vertex);
    for record in &geometry.surfaces {
        table.insert(record.identity(), Record::Surface);
    }
    for record in &geometry.curves {
        table.insert(record.identity(), Record::Curve);
    }
    index!(pcurves, Pcurve);
    index!(points, Point);
    let Some((&start, _)) = table.get_key_value(body) else {
        return Ok((HashSet::new(), 0));
    };
    let mut children = HashMap::<&str, Vec<&str>>::new();
    for region in &geometry.regions {
        children
            .entry(region.body.as_str())
            .or_default()
            .push(region.id.as_str());
    }
    for shell in &geometry.shells {
        children
            .entry(shell.region.as_str())
            .or_default()
            .push(shell.id.as_str());
    }
    for face in &geometry.faces {
        children
            .entry(face.shell.as_str())
            .or_default()
            .push(face.id.as_str());
    }
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    let mut missing = 0;
    while let Some(id) = queue.pop_front() {
        for &child in children.get(id).into_iter().flatten() {
            if seen.insert(child) {
                queue.push_back(child);
            }
        }
        let Some(record) = table.get(id).copied() else {
            continue;
        };
        let mut targets = Vec::new();
        record.references(ctx, &mut |target| {
            targets.push(target.to_owned());
            Ok(())
        })?;
        for target in targets {
            match table.get_key_value(target.as_str()) {
                Some((&key, _)) => {
                    if seen.insert(key) {
                        queue.push_back(key);
                    }
                }
                None => missing += 1,
            }
        }
    }
    Ok((seen, missing))
}

/// Keep the records that `reached` names, in model order.
fn retain<T: EntitySchema + Clone>(records: &[T], reached: &HashSet<&str>) -> Vec<T> {
    records
        .iter()
        .filter(|record| reached.contains(record.identity()))
        .cloned()
        .collect()
}

/// Whether the nest is a solid body whose every edge joins exactly two
/// coedges, apart from edges that have no carrier.
fn closed(topology: &BodyTopology) -> Result<(), Open> {
    if topology.body.kind != BodyKind::Solid || topology.faces.is_empty() {
        return Err(Open::Kind);
    }
    let mut uses = HashMap::<&str, usize>::new();
    for coedge in &topology.coedges {
        *uses.entry(coedge.edge.as_str()).or_default() += 1;
    }
    let sound = topology.edges.iter().all(|edge| {
        let count = uses.get(edge.id.as_str()).copied().unwrap_or(0);
        count == 2 || (count < 2 && matches!(edge.carrier, EdgeCarrier::Free))
    });
    if sound {
        Ok(())
    } else {
        Err(Open::Edge)
    }
}

/// A carrier that a procedural construction produced keeps its solved cache as
/// its geometry. Returns false when a carrier has no solved geometry: a
/// procedural carrier without a cache, or a carrier of unknown shape.
fn solve(topology: &mut BodyTopology) -> bool {
    for surface in &mut topology.surfaces {
        surface.source_object = None;
        if let SurfaceGeometry::Procedural { cache, .. } = &mut surface.geometry {
            let Some(cache) = cache.take() else {
                return false;
            };
            surface.geometry = SurfaceGeometry::Solved(cache);
        }
        if matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
        ) {
            return false;
        }
    }
    for curve in &mut topology.curves {
        curve.source_object = None;
        if let CurveGeometry::Procedural { cache, .. } = &mut curve.geometry {
            let Some(cache) = cache.take() else {
                return false;
            };
            curve.geometry = CurveGeometry::Solved(cache);
        }
        if matches!(
            curve.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
        ) {
            return false;
        }
    }
    true
}

/// Whether a record of the nest refers to an identity outside the nest.
fn escapes(ctx: &DecodeContext<'_>, topology: &BodyTopology) -> Result<bool, CodecError> {
    let geometry = topology.to_topology();
    let mut inside = HashSet::<&str>::new();
    macro_rules! collect {
        ($($arena:ident),*) => {$(
            for record in &geometry.$arena {
                inside.insert(record.identity());
            }
        )*};
    }
    collect!(
        bodies, regions, shells, faces, loops, coedges, edges, vertices, surfaces, curves, pcurves,
        points
    );
    let mut escaped = false;
    macro_rules! visit {
        ($($arena:ident),*) => {$(
            for record in &geometry.$arena {
                record.visit_references(ctx, &mut |target| {
                    escaped |= !inside.contains(target);
                    Ok(())
                })?;
            }
        )*};
    }
    visit!(
        bodies, regions, shells, faces, loops, coedges, edges, vertices, surfaces, curves, pcurves,
        points
    );
    Ok(escaped)
}

/// Copy the records of one body of a decoded state into a nest.
fn extract(
    ctx: &DecodeContext<'_>,
    geometry: &Topology,
    body: &str,
) -> Result<Extract, CodecError> {
    let (reached, missing) = reach(ctx, geometry, body)?;
    let Some(root) = geometry
        .bodies
        .iter()
        .find(|candidate| candidate.id.as_str() == body)
    else {
        return Ok(Extract::Absent);
    };
    let mut topology = BodyTopology::new(root.clone());
    topology.regions = retain(&geometry.regions, &reached);
    topology.shells = retain(&geometry.shells, &reached);
    topology.faces = retain(&geometry.faces, &reached);
    topology.loops = retain(&geometry.loops, &reached);
    topology.coedges = retain(&geometry.coedges, &reached);
    topology.edges = retain(&geometry.edges, &reached);
    topology.vertices = retain(&geometry.vertices, &reached);
    topology.surfaces = retain(&geometry.surfaces, &reached);
    topology.curves = retain(&geometry.curves, &reached);
    topology.pcurves = retain(&geometry.pcurves, &reached);
    topology.points = retain(&geometry.points, &reached);
    if !solve(&mut topology) {
        return Ok(Extract::Unsolved);
    }
    if missing != 0 || escapes(ctx, &topology)? {
        return Ok(Extract::NotClosed(Open::Reference));
    }
    if let Err(open) = closed(&topology) {
        return Ok(Extract::NotClosed(open));
    }
    Ok(Extract::Nest(Box::new(topology)))
}

#[cfg(test)]
mod tests;
