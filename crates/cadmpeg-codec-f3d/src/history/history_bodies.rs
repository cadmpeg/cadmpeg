// SPDX-License-Identifier: Apache-2.0
//! Base Feature bodies at their producing state.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{CadIr, Model};
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::history_body::{BodyTopology, HistoryBody, HistoryStateRef};
use cadmpeg_ir::ids::HistoryBodyId;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_ir::schema::EntitySchema;

use crate::container::{self, ContainerScan};
use crate::history_records::AsmHistory;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;

const MAX_HISTORY_BODIES: usize = 64;
const MAX_HISTORY_BODY_WORK: usize = 1 << 21;

struct Want<'a> {
    feature: &'a Feature,
    state: i64,
    slot: i64,
}

#[derive(Clone, Copy)]
pub(crate) struct Inputs<'a> {
    pub(crate) features: &'a [Feature],
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) histories: &'a [AsmHistory],
    pub(crate) bindings: &'a HashMap<String, String>,
    pub(crate) primary_brep: Option<&'a str>,
}

pub(crate) fn fill(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    inputs: Inputs<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Vec<HistoryBody>, CodecError> {
    let Inputs {
        features,
        scopes,
        histories,
        bindings,
        primary_brep,
    } = inputs;
    let mut bodies_out = Vec::new();
    let mut wants = BTreeMap::<usize, Vec<Want<'_>>>::new();
    for feature in features {
        let Some(bodies) = super::stored_bodies::bound_result_bodies(feature) else {
            continue;
        };
        let Some(current) = feature
            .native_ref
            .as_deref()
            .and_then(|id| scopes.iter().find(|scope| scope.id == id))
            .and_then(DesignParameterScope::history_state_id)
        else {
            continue;
        };
        let bound = feature
            .native_ref
            .as_deref()
            .and_then(|id| bindings.get(id))
            .map(String::as_str);
        let output_bound = output_history(
            bodies.iter().map(cadmpeg_ir::ids::BodyId::as_str),
            histories,
            primary_brep,
        );
        let Some(bound) = bound.or(output_bound) else {
            continue;
        };
        let mut matches = histories
            .iter()
            .enumerate()
            .filter(|(_, history)| history.id == bound);
        let Some((history_index, history)) = matches.next() else {
            continue;
        };
        if matches.next().is_some()
            || history
                .states()
                .iter()
                .filter(|state| state.state_id == current)
                .count()
                != 1
        {
            continue;
        }
        for body in bodies {
            if let Some(slot) = super::stable_ref(body.as_str()) {
                wants.entry(history_index).or_default().push(Want {
                    feature,
                    state: current,
                    slot,
                });
            }
        }
    }
    let work = WorkBudget::new(MAX_HISTORY_BODY_WORK);
    for (index, wants) in wants {
        let history = &histories[index];
        let Some(facts) = container::history_breps(scan).find(|facts| {
            history
                .id
                .starts_with(&format!("f3d:{}:asm-history#", facts.name))
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&facts.name)?;
        fill_history(ctx, bytes, history, &wants, &mut bodies_out, &work, losses)?;
    }
    bodies_out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(bodies_out)
}

fn output_history<'a>(
    bodies: impl Iterator<Item = &'a str>,
    histories: &'a [AsmHistory],
    primary_brep: Option<&str>,
) -> Option<&'a str> {
    let mut source = None;
    for body in bodies {
        let candidate = if body.starts_with("f3d:brep:entity#") {
            primary_brep?.rsplit('/').next()?.strip_prefix("BREP.")?
        } else {
            body.strip_prefix("f3d:brep/")?.split_once('/')?.0
        };
        if source.is_some_and(|source| source != candidate) {
            return None;
        }
        source = Some(candidate);
    }
    let source = source?;
    let mut matches = histories
        .iter()
        .filter(|history| super::historical_brep_source(&history.id) == Some(source));
    let history = matches.next()?;
    matches.next().is_none().then_some(history.id.as_str())
}

fn fill_history(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    history: &AsmHistory,
    wants: &[Want<'_>],
    bodies: &mut Vec<HistoryBody>,
    work: &WorkBudget,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let Some(header) = cadmpeg_asm::asm_header::parse(bytes) else {
        return Ok(());
    };
    let states = history.states();
    let mut archive = None;
    let mut archive_attempted = false;
    let Some(active_count) = super::archived_active_record_count(states)?
        .or_else(|| super::insert_only_active_record_count(states))
        .and_then(|count| i64::try_from(count).ok())
    else {
        return Ok(());
    };
    let archived_ids = states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut subtypes = None;
    let mut newer = HashMap::<i64, HashSet<Option<i64>>>::new();
    let mut visited = 0;
    let mut failure = None;
    super::walk_entity_versions(
        states,
        active_count,
        &archived_ids,
        |state_index, versions| {
            let ordinal = states.len() - visited - 1;
            visited += 1;
            if failure.is_some() {
                return;
            }
            let state_id = states[state_index].state_id;
            for want in wants.iter().filter(|want| want.state == state_id) {
                let revision = versions.get(&want.slot).copied();
                if !newer
                    .get(&want.slot)
                    .is_some_and(|seen| seen.iter().any(|new| *new != revision))
                {
                    continue;
                }
                if !archive_attempted {
                    archive_attempted = true;
                    match super::decode_record_archive(ctx, states, bytes, header.width) {
                        Ok(records) => archive = records,
                        Err(error) => {
                            failure = Some(error);
                            break;
                        }
                    }
                }
                let Some(archive) = archive.as_ref() else {
                    continue;
                };
                if bodies.len() >= MAX_HISTORY_BODIES
                    || !work.charge_by(work_units(versions.len()))
                    || (subtypes.is_none() && !work.charge_by(work_units(archive.len())))
                {
                    losses.push(F3dLossCode::FeatureHistoryBodyBounded.note(format!(
                        "{} body slot {} state {state_id} exceeds the history body cap",
                        want.feature.id, want.slot
                    )));
                    continue;
                }
                let subtypes = subtypes.get_or_insert_with(|| {
                    archive_subtypes(archive, header.metadata.save_format_version)
                });
                match build(
                    ctx,
                    archive,
                    versions,
                    bytes,
                    (&header.metadata, subtypes),
                    want,
                    (ordinal, bodies.len()),
                ) {
                    Ok(Some(body)) => bodies.push(body),
                    Ok(None) => {
                        losses.push(F3dLossCode::FeatureHistoryBodyUndecoded.note(format!(
                            "{} body slot {} state {state_id} has an incomplete geometry closure",
                            want.feature.id, want.slot
                        )));
                    }
                    Err(error) => failure = Some(error),
                }
            }
            for want in wants {
                newer
                    .entry(want.slot)
                    .or_default()
                    .insert(versions.get(&want.slot).copied());
            }
        },
    );
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(())
}

fn archive_subtypes(
    archive: &super::HistoricalRecordArchive,
    version: Option<u32>,
) -> cadmpeg_asm::nurbs::toks::SubtypeTable {
    let mut records = archive.values().cloned().collect::<Vec<_>>();
    records.sort_by_key(|record| record.offset);
    cadmpeg_asm::nurbs::toks::SubtypeTable::from_records(&records).with_save_format_version(version)
}

fn build(
    ctx: &DecodeContext<'_>,
    archive: &super::HistoricalRecordArchive,
    versions: &BTreeMap<i64, i64>,
    bytes: &[u8],
    tables: (
        &cadmpeg_asm::kernel_header::KernelHeader,
        &cadmpeg_asm::nurbs::toks::SubtypeTable,
    ),
    want: &Want<'_>,
    position: (usize, usize),
) -> Result<Option<HistoryBody>, CodecError> {
    let (ordinal, nest_index) = position;
    let Some(table) =
        cadmpeg_asm::history::materialize_record_table(versions, |revision| archive.get(&revision))
    else {
        return Ok(None);
    };
    let brep = match cadmpeg_asm::brep::decode_with_table(
        ctx,
        &table,
        bytes,
        (Some(tables.0), tables.1),
        "f3d:history",
        crate::ids::ID_FORMAT,
        cadmpeg_asm::brep::DecodePurpose::Model,
    ) {
        Ok(brep) => brep,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    let Some(body) = brep
        .bodies
        .iter()
        .find(|body| super::stable_ref(body.id.as_str()) == Some(want.slot))
        .map(|body| body.id.clone())
    else {
        return Ok(None);
    };
    let mut scratch = CadIr::empty();
    cadmpeg_asm::brep::transfer::transfer_into_ir(ctx, &mut scratch, "f3d", brep)?;
    let Extract::Nest(topology) = extract(ctx, &scratch.model, body.as_str())? else {
        return Ok(None);
    };
    let mut map = IdentityMap::new(|id: &str| {
        let (_, rest) = id
            .split_once(':')
            .ok_or_else(|| CodecError::malformed("history body identity"))?;
        let (_, rest) = rest
            .split_once(':')
            .ok_or_else(|| CodecError::malformed("history body identity"))?;
        let (kind, key) = rest
            .split_once('#')
            .ok_or_else(|| CodecError::malformed("history body identity"))?;
        Ok(format!("f3d:history-body:{kind}#h{nest_index:04}-{key}"))
    });
    let mut topology = topology.rewrite_identities(ctx, &mut map)?;
    map.finish()?;
    sort_by_identity(&mut topology);
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(topology.record_count()),
        "admit history body records",
    )?;
    Ok(Some(HistoryBody {
        id: HistoryBodyId::mint(format!("f3d:history-body:nest#h{nest_index:04}"))
            .map_err(|error| CodecError::malformed(format_args!("{error}")))?,
        produced_by: Some(want.feature.id.clone()),
        consumed_by: None,
        state: HistoryStateRef {
            ordinal: u32::try_from(ordinal)
                .map_err(|_| CodecError::malformed("history ordinal"))?,
            identifier: want.state,
        },
        topology,
    }))
}

enum Extract {
    Nest(Box<BodyTopology>),
    Absent,
    NotClosed,
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
    Surface,
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

fn retain<T: EntitySchema + Clone>(records: &[T], reached: &HashSet<&str>) -> Vec<T> {
    records
        .iter()
        .filter(|record| reached.contains(record.identity()))
        .cloned()
        .collect()
}

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
        return Ok(Extract::NotClosed);
    }
    Ok(Extract::Nest(Box::new(topology)))
}

#[cfg(test)]
mod tests;
