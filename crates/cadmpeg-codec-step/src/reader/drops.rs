// SPDX-License-Identifier: Apache-2.0
//! The §4.5 rule 2 and 3 drop ledger: records a damaged decode drops with
//! each given-up part, so the part's `record.unreadable` loss can name them.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use super::collect_references;
use crate::ids::{self, kind};
use crate::parse::{Exchange, RawRecord};

/// One record a damaged decode drops with a given-up part.
struct DropEntry {
    /// The record id the decode would have held without the damage.
    record: String,
    /// The dead instance whose part names the drop, when attributed.
    dead: Option<u64>,
}

/// Records dropped with the given-up parts of one decode.
pub(crate) struct DropLedger {
    entries: Vec<DropEntry>,
}

impl DropLedger {
    /// An empty ledger for one decode.
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Name `record` as dropped with the part of `dead`, or with the
    /// earliest part when `dead` is unattributed.
    pub(super) fn add(&mut self, _instance: u64, record: String, dead: Option<u64>) {
        if !self.entries.iter().any(|entry| entry.record == record) {
            self.entries.push(DropEntry { record, dead });
        }
    }

    /// Whether the ledger names any drop.
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The sorted dropped ids attributed to one dead instance's part.
    fn records_for(&self, dead: u64, earliest: u64) -> Vec<String> {
        let mut records = BTreeSet::new();
        for entry in &self.entries {
            if entry.dead.unwrap_or(earliest) == dead {
                records.insert(entry.record.clone());
            }
        }
        records.into_iter().collect()
    }
}

/// Name each given-up part's dropped records on its `record.unreadable`
/// loss. `starts` maps each part span start to its dead instance.
pub(super) fn amend_record_unreadable(
    losses: &mut [LossNote],
    ledger: &DropLedger,
    starts: &BTreeMap<u64, u64>,
) {
    if ledger.is_empty() {
        return;
    }
    let earliest = starts.values().copied().min();
    for loss in losses.iter_mut() {
        if loss.code.local_code() != "record.unreadable" {
            continue;
        }
        let Some(span) = loss.provenance.as_ref() else {
            continue;
        };
        let Some(dead) = starts.get(&span.offset).copied() else {
            continue;
        };
        let Some(earliest) = earliest else {
            continue;
        };
        let records = ledger.records_for(dead, earliest);
        if records.is_empty() {
            continue;
        }
        loss.dropped = records;
        let tail = format!("; dropped=[{}]", loss.dropped.join(", "));
        if let Some(prefix) = loss.message.rsplit_once("; dropped=[") {
            loss.message = format!("{}{tail}", prefix.0);
        } else {
            loss.message.push_str(&tail);
        }
    }
}

/// The model record ids one DATA instance would have built, by keyword.
/// Families without a model record mint nothing. Suffix-scoped families
/// (loops, coedges) mint nothing: the comparison's needs closure covers
/// them through the records they need.
fn records_for_keyword(keyword: &str, id: u64) -> Vec<String> {
    match keyword {
        "CARTESIAN_POINT" => vec![ids::data(kind!("point"), id).to_string()],
        "VERTEX_POINT" => vec![ids::data(kind!("vertex"), id).to_string()],
        "EDGE_CURVE" => vec![ids::data(kind!("edge"), id).to_string()],
        "ADVANCED_FACE" | "FACE_SURFACE" => vec![ids::data(kind!("face"), id).to_string()],
        "OPEN_SHELL" | "CLOSED_SHELL" => vec![ids::data(kind!("shell"), id).to_string()],
        "ORIENTED_OPEN_SHELL" | "ORIENTED_CLOSED_SHELL" => {
            vec![ids::data(kind!("shell"), id).to_string()]
        }
        "SHELL_BASED_SURFACE_MODEL" => vec![
            ids::data(kind!("body"), id).to_string(),
            ids::data(kind!("region"), id).to_string(),
        ],
        "PCURVE" | "P_CURVE" => vec![ids::data(kind!("pcurve"), id).to_string()],
        "PRESENTATION_LAYER_ASSIGNMENT" => {
            vec![ids::presentation(kind!("layer"), id).to_string()]
        }
        "LINE"
        | "CIRCLE"
        | "ELLIPSE"
        | "PARABOLA"
        | "HYPERBOLA"
        | "POLYLINE"
        | "B_SPLINE_CURVE"
        | "B_SPLINE_CURVE_WITH_KNOTS"
        | "UNIFORM_CURVE"
        | "QUASI_UNIFORM_CURVE"
        | "BEZIER_CURVE"
        | "TRIMMED_CURVE"
        | "COMPOSITE_CURVE"
        | "BOUNDARY_CURVE"
        | "OUTER_BOUNDARY_CURVE" => {
            vec![ids::data(kind!("curve"), id).to_string()]
        }
        "PLANE"
        | "CYLINDRICAL_SURFACE"
        | "CONICAL_SURFACE"
        | "SPHERICAL_SURFACE"
        | "DEGENERATE_TOROIDAL_SURFACE"
        | "TOROIDAL_SURFACE"
        | "B_SPLINE_SURFACE"
        | "B_SPLINE_SURFACE_WITH_KNOTS"
        | "UNIFORM_SURFACE"
        | "QUASI_UNIFORM_SURFACE"
        | "BEZIER_SURFACE"
        | "OFFSET_SURFACE"
        | "RECTANGULAR_TRIMMED_SURFACE"
        | "CURVE_BOUNDED_SURFACE"
        | "SURFACE_REPLICA" => {
            vec![ids::data(kind!("surface"), id).to_string()]
        }
        _ => Vec::new(),
    }
}

/// Every instance a record references, in source order.
fn record_references(
    record: &RawRecord,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut links = BTreeSet::new();
    for partial in &record.partials {
        for value in &partial.parameters {
            collect_references(value, &mut links, ctx)?;
        }
    }
    Ok(links)
}

/// The instances backing the decoded model records.
pub(super) fn model_instances(ir: &CadIr) -> BTreeSet<u64> {
    let mut instances = BTreeSet::new();
    let mut insert = |identity: &str| {
        if let Some(id) = super::step_instance_id(identity) {
            instances.insert(id);
        }
    };
    for point in &ir.model.points {
        insert(point.id.as_str());
    }
    for curve in &ir.model.curves {
        insert(curve.id.as_str());
    }
    for surface in &ir.model.surfaces {
        insert(surface.id.as_str());
    }
    for vertex in &ir.model.vertices {
        insert(vertex.id.as_str());
    }
    for edge in &ir.model.edges {
        insert(edge.id.as_str());
    }
    for coedge in &ir.model.coedges {
        insert(coedge.id.as_str());
    }
    for loop_ in &ir.model.loops {
        insert(loop_.id.as_str());
    }
    for face in &ir.model.faces {
        insert(face.id.as_str());
    }
    for shell in &ir.model.shells {
        insert(shell.id.as_str());
    }
    for region in &ir.model.regions {
        insert(region.id.as_str());
    }
    for body in &ir.model.bodies {
        insert(body.id.as_str());
    }
    for pcurve in &ir.model.pcurves {
        insert(pcurve.id.as_str());
    }
    instances
}

/// Every instance the damage reaches through the undirected instance
/// graph from the dead instances: the records rule 2 drops with the
/// given-up parts. Empty for a clean decode.
pub(super) fn damage_reached(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let dead = exchange.dead_instances();
    if dead.is_empty() {
        return Ok(BTreeSet::new());
    }
    let mut neighbours: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    let mut link = |first: u64, second: u64| {
        neighbours.entry(first).or_default().insert(second);
        neighbours.entry(second).or_default().insert(first);
    };
    for (&id, record) in exchange.records() {
        for target in record_references(record, ctx)? {
            link(id, target);
        }
    }
    for (&id, record) in exchange.dead_records() {
        for target in record_references(record, ctx)? {
            link(id, target);
        }
    }
    let mut reached = dead.clone();
    let mut queue: VecDeque<u64> = dead.iter().copied().collect();
    while let Some(id) = queue.pop_front() {
        if let Some(peers) = neighbours.get(&id).cloned() {
            for peer in peers {
                if reached.insert(peer) {
                    queue.push_back(peer);
                }
            }
        }
    }
    Ok(reached)
}

/// Walk the undirected instance graph from the dead instances and ledger
/// every reached instance's would-be model records, except the records
/// the decode holds. Instances the decode holds are excluded, so a held
/// record is never named; reached instances without a model record name
/// harmless no-ops the comparison ignores.
pub(super) fn ledger_stranded_drops(
    exchange: &Exchange,
    ir: &CadIr,
    ledger: &mut DropLedger,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let reached = damage_reached(exchange, ctx)?;
    if reached.is_empty() {
        return Ok(());
    }
    let dead = exchange.dead_instances();
    let held = model_instances(ir);
    for &id in &reached {
        if held.contains(&id) {
            continue;
        }
        let keyword = if let Some(record) = exchange.records().get(&id) {
            Some(record.partials.first().name.as_str())
        } else {
            exchange.dead_keyword(id)
        };
        let Some(keyword) = keyword else {
            continue;
        };
        let attributed = dead.contains(&id).then_some(id);
        for record in records_for_keyword(keyword, id) {
            ledger.add(id, record, attributed);
        }
    }
    Ok(())
}
