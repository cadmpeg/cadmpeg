// SPDX-License-Identifier: Apache-2.0
//! Nested history bodies of Base and Split features
//! ([spec §13.10](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md#1310-feature-history-states)).
//!
//! The body that a Base feature produces and the body that a Split feature
//! consumes exist only in an earlier history state of the active carrier. The
//! reader materializes that state, decodes it, and keeps the one body with the
//! checked key in a nest of its own. The model arenas never hold its records.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use cadmpeg_asm::brep::{decode_with_header, DecodePurpose};
use cadmpeg_asm::history::{
    historical_record_archive, materialize_record_table, walk_entity_versions,
};
use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{CadIr, Model};
use cadmpeg_ir::features::{Feature, FeatureId};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::history_body::{BodyTopology, HistoryBody, HistoryStateRef};
use cadmpeg_ir::ids::HistoryBodyId;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_ir::schema::EntitySchema;
use cadmpeg_ir::topology::{BodyKind, EdgeCarrier};

use super::history_states::{
    Binding, Chain, FeatureHistoryState, BASE_TAG, NATIVE_BASE_KIND, SPLIT_TAG,
};
use crate::kernel::ActiveCarrier;

/// Cap of nests per part. A nest materializes and decodes a whole history state.
pub(crate) const MAX_HISTORY_BODIES: usize = 8;
/// Cap of records materialized for all nests of a part.
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
}

/// Nests of a part and their links.
#[derive(Debug, Default)]
pub(crate) struct Built {
    /// Nests, sorted by identity.
    pub(crate) bodies: Vec<HistoryBody>,
    /// Identity of the nest of each feature record, by record index.
    pub(crate) links: HashMap<usize, HistoryBodyId>,
    /// Counts.
    pub(crate) outcome: Outcome,
}

/// Result of the extraction of one body.
enum Extract {
    Nest(Box<BodyTopology>),
    /// The state holds no body with the key, or a carrier has no solved form.
    Absent,
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
        if wants.len() == MAX_HISTORY_BODIES {
            outcome.bounded += 1;
            continue;
        }
        wants.push(Want {
            record,
            ordinal,
            state_id: 0,
            key,
            role,
        });
    }
    wants
}

/// Materialize the states of the bound Base and Split features of `records`
/// and keep the bodies they name.
pub(crate) fn build(
    ctx: &DecodeContext<'_>,
    carrier: &ActiveCarrier<'_>,
    chain: Chain<'_>,
    records: &[FeatureHistoryState],
    model_features: &[Feature],
) -> Result<Built, CodecError> {
    let mut built = Built::default();
    let mut wants = wants(records, &mut built.outcome);
    let Ok(header) = carrier.header.as_ref() else {
        built.outcome.undecoded += wants.len();
        return Ok(built);
    };
    if wants.is_empty() {
        return Ok(built);
    }
    let Chain {
        views,
        frames,
        archived_ids,
        active_count,
    } = chain;
    let state_count = views.len();
    let Some(archive) = historical_record_archive(&views, frames) else {
        built.outcome.undecoded += wants.len();
        return Ok(built);
    };
    let bytes = carrier.bytes.window();
    let work = WorkBudget::new(MAX_HISTORY_BODY_WORK);
    let mut by_ordinal = BTreeMap::<u32, Vec<usize>>::new();
    for (index, want) in wants.iter().enumerate() {
        by_ordinal.entry(want.ordinal).or_default().push(index);
    }
    let mut extracted = BTreeMap::<usize, Extract>::new();
    let mut failure = None;
    let mut bounded = HashSet::<usize>::new();
    let mut visited = 0_usize;
    walk_entity_versions(&views, active_count, &archived_ids, |slot, versions| {
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
        let state_id = views[slot].state_id;
        for &index in indices {
            wants[index].state_id = state_id;
        }
        if !work.charge_by(work_units(versions.len())) {
            bounded.extend(indices.iter().copied());
            return;
        }
        let Some(table) = materialize_record_table(versions, |revision| archive.get(&revision))
        else {
            return;
        };
        let stream = format!("inventor:history:{ordinal}");
        let brep = match decode_with_header(
            ctx,
            &table,
            bytes,
            Some(&header.metadata),
            &stream,
            cadmpeg_asm::asm_format!("inventor"),
            DecodePurpose::Model,
        ) {
            Ok(brep) => brep,
            Err(error @ CodecError::ResourceLimit(_)) => {
                failure = Some(error);
                return;
            }
            Err(_) => return,
        };
        drop(table);
        let keys = brep
            .body_native_keys
            .iter()
            .filter_map(|record| Some((record.body.as_str().to_owned(), record.asm_body_key?)))
            .collect::<Vec<_>>();
        let mut scratch = CadIr::empty();
        if let Err(error) =
            cadmpeg_asm::brep::transfer::transfer_into_ir(ctx, &mut scratch, "inventor", brep)
        {
            if matches!(error, CodecError::ResourceLimit(_)) {
                failure = Some(error);
            }
            return;
        }
        for &index in indices {
            let key = u64::try_from(wants[index].key).ok();
            let body = keys
                .iter()
                .find(|(_, candidate)| Some(*candidate) == key)
                .map(|(body, _)| body.as_str());
            let result = match body {
                Some(body) => extract(ctx, &scratch.model, body),
                None => Ok(Extract::Absent),
            };
            match result {
                Ok(result) => {
                    extracted.insert(index, result);
                }
                Err(error @ CodecError::ResourceLimit(_)) => {
                    failure = Some(error);
                    return;
                }
                Err(_) => {}
            }
        }
    });
    if let Some(error) = failure {
        return Err(error);
    }
    let features = model_features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, &feature.id)))
        .collect::<HashMap<_, _>>();
    for (index, want) in wants.iter().enumerate() {
        if bounded.contains(&index) {
            built.outcome.bounded += 1;
            continue;
        }
        match extracted.remove(&index) {
            None | Some(Extract::Absent) => built.outcome.undecoded += 1,
            Some(Extract::NotClosed(open)) => built.outcome.open[open.slot()] += 1,
            Some(Extract::Nest(topology)) => {
                let ordinal = built.bodies.len();
                let record = &records[want.record];
                let feature = features
                    .get(record.feature.as_str())
                    .map(|id| (*id).clone());
                let body = nest(ctx, ordinal, want, feature, *topology)?;
                ctx.charge_entities(
                    cadmpeg_core::decode::u64_from_index(body.topology.record_count()),
                    "admit history body records",
                )?;
                built.links.insert(want.record, body.id.clone());
                built.bodies.push(body);
                built.outcome.built += 1;
            }
        }
    }
    built.bodies.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(built)
}

/// The nest in the history-body scope, with the feature link of its role.
fn nest(
    ctx: &DecodeContext<'_>,
    ordinal: usize,
    want: &Want,
    feature: Option<FeatureId>,
    topology: BodyTopology,
) -> Result<HistoryBody, CodecError> {
    let prefix = format!("h{ordinal:04}-");
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
        Role::Base => (feature, None),
        Role::Split => (None, feature),
    };
    Ok(HistoryBody {
        id: HistoryBodyId::mint(format!("inventor:{NEST_SCOPE}:nest#h{ordinal:04}"))
            .map_err(|error| CodecError::malformed(format_args!("{error}")))?,
        produced_by,
        consumed_by,
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
    model: &'a Model,
    body: &str,
) -> Result<(HashSet<&'a str>, usize), CodecError> {
    let mut table = HashMap::<&str, Record<'_>>::new();
    macro_rules! index {
        ($arena:ident, $variant:ident) => {
            for record in &model.$arena {
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
    for record in &model.surfaces {
        table.insert(record.identity(), Record::Surface);
    }
    for record in &model.curves {
        table.insert(record.identity(), Record::Curve);
    }
    index!(pcurves, Pcurve);
    index!(points, Point);
    let Some((&start, _)) = table.get_key_value(body) else {
        return Ok((HashSet::new(), 0));
    };
    let mut seen = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    let mut missing = 0;
    while let Some(id) = queue.pop_front() {
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
    let model = topology.to_model();
    let mut inside = HashSet::<&str>::new();
    macro_rules! collect {
        ($($arena:ident),*) => {$(
            for record in &model.$arena {
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
            for record in &model.$arena {
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
fn extract(ctx: &DecodeContext<'_>, model: &Model, body: &str) -> Result<Extract, CodecError> {
    let (reached, missing) = reach(ctx, model, body)?;
    let Some(root) = model
        .bodies
        .iter()
        .find(|candidate| candidate.id.as_str() == body)
    else {
        return Ok(Extract::Absent);
    };
    let mut topology = BodyTopology::new(root.clone());
    topology.regions = retain(&model.regions, &reached);
    topology.shells = retain(&model.shells, &reached);
    topology.faces = retain(&model.faces, &reached);
    topology.loops = retain(&model.loops, &reached);
    topology.coedges = retain(&model.coedges, &reached);
    topology.edges = retain(&model.edges, &reached);
    topology.vertices = retain(&model.vertices, &reached);
    topology.surfaces = retain(&model.surfaces, &reached);
    topology.curves = retain(&model.curves, &reached);
    topology.pcurves = retain(&model.pcurves, &reached);
    topology.points = retain(&model.points, &reached);
    if !solve(&mut topology) {
        return Ok(Extract::Absent);
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
