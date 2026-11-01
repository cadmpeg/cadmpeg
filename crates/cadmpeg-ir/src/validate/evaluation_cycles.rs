// SPDX-License-Identifier: Apache-2.0
//! Cross-record curve and surface dependencies used by model evaluation.

use std::fmt;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, DepthGuard};
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::geometry::{
    ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
};
use crate::index::{identities::BorrowedIdentities, ModelIndex};
use crate::provenance::SourceProvenance;
use crate::report::{
    check::{Check, Finding},
    loss::LossNote,
    Severity,
};
use crate::schema::rewrite::typed::RewriteIdentities;
use crate::source_fidelity::SourceFidelity;

fn curve_dependencies(definition: &ProceduralCurveDefinition) -> Vec<&str> {
    match definition {
        ProceduralCurveDefinition::Replica { source, .. } => vec![source.as_str()],
        ProceduralCurveDefinition::Subset(payload) => {
            vec![payload.source().as_str()]
        }
        ProceduralCurveDefinition::TolerantIntersection {
            construction,
            parameterization: Some(_),
            ..
        } => construction
            .supports()
            .iter()
            .map(crate::ids::SurfaceId::as_str)
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    }
}

fn surface_dependencies(definition: &ProceduralSurfaceDefinition) -> Vec<&str> {
    match definition {
        ProceduralSurfaceDefinition::AxisRevolution(payload) => {
            vec![payload.directrix().as_str()]
        }
        ProceduralSurfaceDefinition::Extrusion(payload) => {
            vec![payload.directrix().as_str()]
        }
        ProceduralSurfaceDefinition::LinearSweep(payload) => {
            vec![payload.directrix().as_str()]
        }
        ProceduralSurfaceDefinition::Revolution(payload) => {
            vec![payload.directrix().as_str()]
        }
        ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
            vec![first.as_str(), second.as_str()]
        }
        ProceduralSurfaceDefinition::Sum(payload) => {
            vec![payload.first().as_str(), payload.second().as_str()]
        }
        ProceduralSurfaceDefinition::Sweep(payload) if payload.native().is_some() => {
            vec![payload.profile().as_str(), payload.spine().as_str()]
        }
        ProceduralSurfaceDefinition::Blend(payload) => {
            if let Some(native) = payload.native() {
                native
                    .sides
                    .iter()
                    .filter_map(|side| {
                        side.surface
                            .as_ref()
                            .map(|support| support.surface.as_str())
                    })
                    .chain(std::iter::once(native.slice.as_str()))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        }
        ProceduralSurfaceDefinition::VariableBlend(payload) => payload
            .construction()
            .sides
            .iter()
            .filter_map(|side| {
                side.surface
                    .as_ref()
                    .map(|support| support.surface.as_str())
            })
            .collect::<Vec<_>>(),
        ProceduralSurfaceDefinition::CurveBounded { support, .. } => {
            vec![support.as_str()]
        }
        ProceduralSurfaceDefinition::Replica { source, .. } => {
            vec![source.as_str()]
        }
        ProceduralSurfaceDefinition::Subset(payload) => {
            vec![payload.support().as_str()]
        }
        ProceduralSurfaceDefinition::ParallelOffset(payload) => {
            vec![payload.support().as_str()]
        }
        ProceduralSurfaceDefinition::Offset(payload) => {
            vec![payload.support().as_str()]
        }
        _ => Vec::new(),
    }
}

struct Frame<'a, 'session> {
    node: &'a str,
    next_child: usize,
    _depth: DepthGuard<'session>,
}

struct CyclePath<'stack, 'a, 'session> {
    stack: &'stack [Frame<'a, 'session>],
    child: &'a str,
}

impl fmt::Display for CyclePath<'_, '_, '_> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        for frame in self.stack {
            write!(out, "{} -> ", frame.node)?;
        }
        out.write_str(self.child)
    }
}

#[derive(Clone, Copy)]
enum Visit {
    Unseen,
    Active(usize),
    Complete,
}

struct Node<'ir> {
    dependencies: Vec<&'ir str>,
    visit: Visit,
}

/// Only carrier-to-construction associations are needed by the cycle graph.
trait CarrierConstructions<'ir> {
    fn curve(&self, id: &str) -> Option<&'ir ProceduralCurve>;
    fn surface(&self, id: &str) -> Option<&'ir ProceduralSurface>;
}

impl<'ir> CarrierConstructions<'ir> for ModelIndex<'ir> {
    fn curve(&self, id: &str) -> Option<&'ir ProceduralCurve> {
        self.procedural_curves_for_curve(id, crate::index::StandardIndex)
            .and_then(|rows| rows.first().copied())
    }
    fn surface(&self, id: &str) -> Option<&'ir ProceduralSurface> {
        self.procedural_surface_for_surface(id, crate::index::StandardIndex)
    }
}

struct CycleIndex<'ir> {
    curves: BorrowedIdentities<'ir, &'ir ProceduralCurve>,
    surfaces: BorrowedIdentities<'ir, &'ir ProceduralSurface>,
}

impl<'ir> CycleIndex<'ir> {
    fn build(ir: &'ir CadIr) -> Self {
        // The general index selects the first construction with an exact ID.
        // BorrowedIdentities selects the last insertion, so reverse these rows.
        let curves_by_id = BorrowedIdentities::build(|add| {
            for row in ir.model.procedural_curves.iter().rev() {
                add(row.id.as_str(), row);
            }
        });
        let curves = BorrowedIdentities::<&ProceduralCurve>::build(|add| {
            for carrier in ir.model.curves.iter().rev() {
                if let Some(construction) = carrier.geometry.procedural_construction() {
                    if let Some(row) = curves_by_id.get(construction.as_str()) {
                        add(carrier.id.as_str(), row);
                    }
                }
            }
        });
        drop(curves_by_id);
        let surfaces_by_id = BorrowedIdentities::build(|add| {
            for row in ir.model.procedural_surfaces.iter().rev() {
                add(row.id.as_str(), row);
            }
        });
        let surfaces = BorrowedIdentities::<&ProceduralSurface>::build(|add| {
            for carrier in &ir.model.surfaces {
                if let Some(construction) = carrier.geometry.procedural_construction() {
                    if let Some(row) = surfaces_by_id.get(construction.as_str()) {
                        add(carrier.id.as_str(), row);
                    }
                }
            }
        });
        Self { curves, surfaces }
    }
}

impl<'ir> CarrierConstructions<'ir> for CycleIndex<'ir> {
    fn curve(&self, id: &str) -> Option<&'ir ProceduralCurve> {
        self.curves.get(id).copied()
    }
    fn surface(&self, id: &str) -> Option<&'ir ProceduralSurface> {
        self.surfaces.get(id).copied()
    }
}

/// Walk the evaluator graph, stopping when the finding consumer asks to stop.
fn walk_cycles<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir CadIr,
    index: &impl CarrierConstructions<'ir>,
    mut emit: impl FnMut(Finding) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    let mut graph = BorrowedIdentities::build(|add| {
        for curve in &ir.model.curves {
            if let Some(procedural) = index.curve(curve.id.as_str()) {
                let dependencies = curve_dependencies(procedural.definition());
                if !dependencies.is_empty() {
                    add(
                        curve.id.as_str(),
                        Node {
                            dependencies,
                            visit: Visit::Unseen,
                        },
                    );
                }
            }
        }
        for surface in &ir.model.surfaces {
            if let Some(procedural) = index.surface(surface.id.as_str()) {
                let dependencies = surface_dependencies(procedural.definition());
                if !dependencies.is_empty() {
                    add(
                        surface.id.as_str(),
                        Node {
                            dependencies,
                            visit: Visit::Unseen,
                        },
                    );
                }
            }
        }
    });
    let mut starts = graph.identities().collect::<Vec<_>>();
    starts.sort_by(Ord::cmp);
    let mut stack = Vec::new();
    for start in starts {
        let Some(node) = graph.get_mut(start) else {
            continue;
        };
        if matches!(node.visit, Visit::Complete) {
            continue;
        }
        node.visit = Visit::Active(0);
        let depth = ctx.enter_nested("cycle traversal depth")?;
        stack.push(Frame {
            node: start,
            next_child: 0,
            _depth: depth,
        });
        while let Some(frame) = stack.last_mut() {
            let child = graph
                .get(frame.node)
                .and_then(|node| node.dependencies.get(frame.next_child))
                .copied();
            let Some(child) = child else {
                if let Some(finished) = stack.pop() {
                    if let Some(node) = graph.get_mut(finished.node) {
                        node.visit = Visit::Complete;
                    }
                }
                continue;
            };
            frame.next_child += 1;
            let Some(node) = graph.get_mut(child) else {
                continue;
            };
            match node.visit {
                Visit::Complete => {}
                Visit::Active(start_index) => {
                    let finding = Finding {
                        related: stack[start_index..]
                            .iter()
                            .map(|frame| frame.node)
                            .filter(|id| *id != child)
                            .map(str::to_owned)
                            .collect(),
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: format!(
                            "malformed curve/surface reference cycle: {}",
                            CyclePath {
                                stack: &stack[start_index..],
                                child
                            }
                        ),
                        entity: Some(child.to_owned()),
                    };
                    if !emit(finding)? {
                        return Ok(());
                    }
                }
                Visit::Unseen => {
                    node.visit = Visit::Active(stack.len());
                    let depth = ctx.enter_nested("cycle traversal depth")?;
                    stack.push(Frame {
                        node: child,
                        next_child: 0,
                        _depth: depth,
                    });
                }
            }
        }
    }
    Ok(())
}

/// Report cycles in the dependencies an evaluator can follow from a carrier.
pub(super) fn check_evaluation_cycles<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir CadIr,
    index: &ModelIndex<'ir>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    walk_cycles(ctx, ir, index, |finding| {
        findings.push(finding);
        Ok(true)
    })
}

/// The part kind of a carrier dropped from an evaluation cycle.
const CYCLE_PART_KIND: &str = "procedural_carrier";

/// Give up every carrier on an evaluation cycle as one part.
///
/// Each carrier that still stands when its turn comes is given up: the stub
/// keeps an empty extent, since a carrier has no isolatable source extent
/// after decoding, and one `record.unreadable` loss names the carrier, the
/// cycle and every record dropped with it. Records that need a dropped record
/// are dropped too, transitively; records that only list one lose that entry
/// through typed list pruning.
pub(crate) fn give_up_evaluation_cycles(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    fidelity: &mut SourceFidelity,
    format: &str,
) -> Result<(), CodecError> {
    if ir.model.procedural_curves.is_empty() && ir.model.procedural_surfaces.is_empty() {
        return Ok(());
    }
    let index = CycleIndex::build(ir);
    let mut members: Vec<(String, String)> = Vec::new();
    walk_cycles(ctx, ir, &index, |finding| {
        let message = finding.message.clone();
        if let Some(entity) = finding.entity.clone() {
            members.push((entity, message.clone()));
        }
        for related in &finding.related {
            members.push((related.clone(), message.clone()));
        }
        Ok(true)
    })?;
    members.sort();
    members.dedup();
    let mut messages: BTreeMap<String, String> = BTreeMap::new();
    for (member, message) in members {
        messages.entry(member).or_insert(message);
    }
    let mut dead: BTreeSet<String> = BTreeSet::new();
    for (seed, message) in &messages {
        if dead.contains(seed) {
            continue;
        }
        let native_link = carrier_native_ref(ir, seed);
        let before: BTreeSet<String> = dead.clone();
        drop_carrier_closure(ctx, ir, seed, &mut dead)?;
        let dropped: Vec<String> = dead.difference(&before).cloned().collect();
        if dropped.is_empty() {
            continue;
        }
        give_up_carrier(
            ctx,
            ir,
            losses,
            fidelity,
            format,
            CarrierGiveUp {
                seed,
                message,
                dropped: &dropped,
                native_link,
            },
        )?;
    }
    Ok(())
}

/// Drop `seed` with the carriers, constructions and dependents that need it.
fn drop_carrier_closure(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    seed: &str,
    dead: &mut BTreeSet<String>,
) -> Result<(), CodecError> {
    dead.insert(seed.to_owned());
    loop {
        let mut grown = false;
        grown |= drop_cycle_constructions(ir, dead);
        grown |= drop_cycle_carriers(ir, dead);
        grown |= drop_required_dependents(ctx, ir, dead)?;
        if !grown {
            break;
        }
    }
    sweep_dead(ir, dead);
    Ok(())
}

/// Drop constructions whose carrier is dead or whose dependencies are dead.
fn drop_cycle_constructions(ir: &CadIr, dead: &mut BTreeSet<String>) -> bool {
    let mut grown = false;
    for carrier in &ir.model.curves {
        if dead.contains(carrier.id.as_str()) {
            if let Some(construction) = carrier.geometry.procedural_construction() {
                grown |= dead.insert(construction.as_str().to_owned());
            }
        }
    }
    for carrier in &ir.model.surfaces {
        if dead.contains(carrier.id.as_str()) {
            if let Some(construction) = carrier.geometry.procedural_construction() {
                grown |= dead.insert(construction.as_str().to_owned());
            }
        }
    }
    for row in &ir.model.procedural_curves {
        if curve_dependencies(row.definition())
            .iter()
            .any(|dependency| dead.contains(*dependency))
        {
            grown |= dead.insert(row.id.as_str().to_owned());
        }
    }
    for row in &ir.model.procedural_surfaces {
        if surface_dependencies(row.definition())
            .iter()
            .any(|dependency| dead.contains(*dependency))
        {
            grown |= dead.insert(row.id.as_str().to_owned());
        }
    }
    grown
}

/// Drop carriers whose construction is dead.
fn drop_cycle_carriers(ir: &CadIr, dead: &mut BTreeSet<String>) -> bool {
    let mut grown = false;
    for carrier in &ir.model.curves {
        if let Some(construction) = carrier.geometry.procedural_construction() {
            if dead.contains(construction.as_str()) {
                grown |= dead.insert(carrier.id.as_str().to_owned());
            }
        }
    }
    for carrier in &ir.model.surfaces {
        if let Some(construction) = carrier.geometry.procedural_construction() {
            if dead.contains(construction.as_str()) {
                grown |= dead.insert(carrier.id.as_str().to_owned());
            }
        }
    }
    grown
}

/// One cycle carrier given up as a part, with its dependents.
struct CarrierGiveUp<'a> {
    seed: &'a str,
    message: &'a str,
    dropped: &'a [String],
    native_link: Option<String>,
}

/// Give up one cycle carrier: stub, retained bytes and one loss.
fn give_up_carrier(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    fidelity: &mut SourceFidelity,
    format: &str,
    give_up: CarrierGiveUp<'_>,
) -> Result<(), CodecError> {
    use crate::source_fidelity::{RetainedBytes, RetainedSourceRecord};
    let CarrierGiveUp {
        seed,
        message,
        dropped,
        native_link,
    } = give_up;
    let id = eval_part_id(format, seed)?;
    let mut links = Vec::new();
    if let Some(link) = native_link {
        if let Ok(link) = crate::ids::Identity::new(link) {
            if native_contains(&ir.native, link.as_str()) {
                links.push(link);
            }
        }
    }
    let classes = ctx.depth().required_scope().classes().collect();
    let record = RetainedSourceRecord::from_bytes(
        crate::provenance::SourceOwner::Root,
        0,
        RetainedBytes::Inline { data: Vec::new() },
    )?
    .with_untyped(classes);
    ctx.charge_entities(1, "evaluation cycle give-up")?;
    let stub = crate::NativeUnknownRecord {
        id: id.clone(),
        links,
    };
    let stored = crate::native::NativeRecord::from(&stub);
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(stored.text_len()),
        "evaluation cycle give-up",
    )?;
    fidelity.insert_retained_record(id, record)?;
    ir.native
        .namespace_mut(format)
        .arenas_mut()
        .entry("unknowns".to_owned())
        .or_default()
        .push(stored);
    let loss_message = format!(
        "part {CYCLE_PART_KIND} stream=\"\" offset=0 length=0 record={seed}: {message}; dropped=[{}]",
        dropped.join(", ")
    );
    let mut loss = crate::codec::DecodeLossCode::RecordUnreadable
        .note(loss_message)
        .with_provenance(SourceProvenance::root(format, 0));
    loss.dropped = dropped.to_vec();
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(loss.message.len()),
        "evaluation cycle give-up",
    )?;
    losses.push(loss);
    Ok(())
}

/// The retained-record identity of a carrier given up from an evaluation
/// cycle. Decoder parts hash their source extent; a carrier has none after
/// decoding, so its record identity discriminator takes the extent's place.
fn eval_part_id(format: &str, carrier: &str) -> Result<crate::ids::UnknownId, CodecError> {
    use cadmpeg_core::decode::u64_from_index;
    let mut address = Vec::new();
    for field in [CYCLE_PART_KIND.as_bytes(), carrier.as_bytes()] {
        address.extend_from_slice(&u64_from_index(field.len()).to_be_bytes());
        address.extend_from_slice(field);
    }
    crate::ids::UnknownId::try_from(format!(
        "{format}:source:part#{}",
        crate::hash::sha256_hex(&address)
    ))
    .map_err(CodecError::malformed)
}

/// The carrier's native record link from the model, before the drop.
fn carrier_native_ref(ir: &CadIr, seed: &str) -> Option<String> {
    ir.model
        .curves
        .iter()
        .find(|carrier| carrier.id.as_str() == seed)
        .and_then(|carrier| carrier.native_ref.clone())
        .or_else(|| {
            ir.model
                .surfaces
                .iter()
                .find(|carrier| carrier.id.as_str() == seed)
                .and_then(|carrier| carrier.native_ref.clone())
        })
}

/// True when `native` holds a record with `id` in any namespace and arena.
fn native_contains(native: &crate::native::Native, id: &str) -> bool {
    native.0.values().any(|namespace| {
        namespace
            .arenas()
            .values()
            .any(|records| records.iter().any(|record| record.id() == id))
    })
}

/// Drop records that need a dead record, pruning lists that only name one.
/// Returns true when another record died.
fn drop_required_dependents(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    dead: &mut BTreeSet<String>,
) -> Result<bool, CodecError> {
    use crate::schema::rewrite::typed::prune::ReferencePruneEffect;
    let mut newly = Vec::new();
    macro_rules! scan_dependents {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
            $(for record in &mut ir.model.$field {
                if dead.contains(crate::schema::EntitySchema::identity(record)) {
                    continue;
                }
                match record.prune_reference_lists(ctx, dead)? {
                    ReferencePruneEffect::Unchanged | ReferencePruneEffect::ListsChanged => {}
                    _ => newly.push(
                        crate::schema::EntitySchema::identity(record).to_owned(),
                    ),
                }
            })*
        };
    }
    crate::document::arena_registry!(scan_dependents);
    let grown = !newly.is_empty();
    dead.extend(newly);
    Ok(grown)
}

/// Remove dead records and the regeneration edges that touch them.
fn sweep_dead(ir: &mut CadIr, dead: &BTreeSet<String>) {
    macro_rules! sweep_arena {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
            $(ir.model.$field.retain(|record| {
                !dead.contains(crate::schema::EntitySchema::identity(record))
            });)*
        };
    }
    crate::document::arena_registry!(sweep_arena);
    ir.model.feature_regeneration_parents.drop_dead(dead);
}

#[cfg(test)]
mod tests;
