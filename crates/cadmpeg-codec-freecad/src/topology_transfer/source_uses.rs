// SPDX-License-Identifier: Apache-2.0
//! Canonical working source-use plans and scalar native reference rows.

use super::locations::{self, Located, Locations};
use crate::brep::location_identity::LocationIdentityError;
use crate::brep::{ShapePayloadRecord, Tables};
use crate::native::source_use::{SourceUse, SourceUseSelector, SourceUseValue};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::NativeNamespace;
use std::collections::{HashMap, HashSet};

/// Owned working text is fallibly reserved in the actual caller context.
/// This does not admit another semantic entity or retained output home.
pub(super) fn working_text(ctx: &DecodeContext<'_>, value: &str) -> Result<String, CodecError> {
    let mut text = String::new();
    ctx.reserve_text_working(&mut text, value.len(), "FreeCAD topology working identity")?;
    text.push_str(value);
    Ok(text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    root: usize,
    shape: usize,
    location: usize,
}

#[derive(Clone, Copy)]
struct Prefix {
    root: usize,
    key: Key,
    parent: Option<(usize, usize)>,
}

#[derive(Default)]
pub(super) struct Plan {
    nodes: Vec<Prefix>,
    by_key: HashMap<Key, usize>,
    roots: HashMap<usize, usize>,
    children: HashMap<(usize, usize), usize>,
}

impl Plan {
    /// This pass keeps only constant-sized source prefixes. It performs the
    /// original prepass's exact location calls, and never materializes an id
    /// or a complete source route. A local bound can discard the working plan
    /// without charging rows which will not enter the document.
    pub(super) fn discover(
        ctx: &DecodeContext<'_>,
        tables: Tables<'_>,
        locations: &mut Locations,
    ) -> Result<Self, LocationIdentityError> {
        let mut result = Self::default();
        for (root_position, root) in tables.roots.iter().enumerate() {
            let root_location = locations.source(tables, root.location)?;
            let mut visited = HashSet::new();
            let mut stack = Vec::new();
            ctx.reserve_working(&mut stack, 1, "FreeCAD source-use traversal")?;
            stack.push((root.shape, Located::identity(), None::<(usize, usize)>));
            while let Some((shape_index, local, prefix)) = stack.pop() {
                if !locations.visit(ctx, &mut visited, shape_index, local)? {
                    continue;
                }
                let placed = locations.admit(ctx, tables, root_location, local)?;
                let shape = tables.tshapes.resolve(shape_index)?;
                let root_number = root_position
                    .checked_add(1)
                    .ok_or_else(|| CodecError::malformed("source-use root index overflows"))?;
                let key = Key {
                    root: root_number,
                    shape: shape_index,
                    location: placed.identity,
                };
                if result.by_key.contains_key(&key) {
                    return Err(CodecError::malformed(
                        "source-use key disagrees with location visitation",
                    )
                    .into());
                }
                let parent_row = result.nodes.len();
                result
                    .by_key
                    .try_reserve(1)
                    .map_err(|_| locations::allocation_failed(ctx))?;
                ctx.reserve_working(&mut result.nodes, 1, "FreeCAD source-use prefixes")?;
                match prefix {
                    Some((parent, slot)) => {
                        result
                            .children
                            .try_reserve(1)
                            .map_err(|_| locations::allocation_failed(ctx))?;
                        result.children.insert((parent, slot), parent_row);
                    }
                    None => {
                        result
                            .roots
                            .try_reserve(1)
                            .map_err(|_| locations::allocation_failed(ctx))?;
                        result.roots.insert(root_number, parent_row);
                    }
                }
                result.nodes.push(Prefix {
                    root: root_number,
                    key,
                    parent: prefix,
                });
                result.by_key.insert(key, parent_row);
                match &shape.geometry {
                    crate::brep::TextTShapeGeometry::Face { location, .. } => {
                        let support = locations.source(tables, *location)?;
                        locations.admit(ctx, tables, local, support)?;
                    }
                    crate::brep::TextTShapeGeometry::Edge {
                        representations, ..
                    } => {
                        for representation in representations {
                            let carrier = locations.source(tables, representation.location())?;
                            locations.admit(ctx, tables, local, carrier)?;
                        }
                    }
                    _ => {}
                }
                for (slot, child) in shape.children.iter().enumerate() {
                    if child.shape >= shape_index {
                        return Err(CodecError::malformed(
                            "source-use child is not a prior TShape",
                        )
                        .into());
                    }
                    let child_location = locations.source(tables, child.location)?;
                    let child_location = locations.admit(ctx, tables, local, child_location)?;
                    let slot = slot
                        .checked_add(1)
                        .ok_or_else(|| CodecError::malformed("source-use child index overflows"))?;
                    ctx.reserve_working(&mut stack, 1, "FreeCAD source-use traversal")?;
                    stack.push((child.shape, child_location, Some((parent_row, slot))));
                }
            }
        }
        Ok(result)
    }
}

enum Identities {
    Fresh(Vec<SourceUseSelector>),
    Existing(HashMap<usize, String>),
}

pub(super) struct Inventory<'ctx, 'input> {
    ctx: &'ctx DecodeContext<'input>,
    plan: Plan,
    identities: Identities,
}

impl<'ctx, 'input> Inventory<'ctx, 'input> {
    pub(super) fn materialize(
        ctx: &'ctx DecodeContext<'input>,
        owner: &str,
        plan: Plan,
    ) -> Result<Self, LocationIdentityError> {
        let mut rows: Vec<SourceUseSelector> = Vec::new();
        for (position, node) in plan.nodes.iter().enumerate() {
            let ordinal = position
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("source-use identity ordinal overflows"))?;
            let source = match node.parent {
                None => SourceUseValue::Root(node.root),
                Some((parent, slot)) => SourceUseValue::Child {
                    parent: &rows[parent].id,
                    slot,
                },
            };
            let row = SourceUseSelector::admit(ctx, owner, ordinal, source)?;
            ctx.reserve_working(&mut rows, 1, "FreeCAD source-use native inventory")?;
            rows.push(row);
        }
        Ok(Self {
            ctx,
            plan,
            identities: Identities::Fresh(rows),
        })
    }

    pub(super) fn reuse(
        ctx: &'ctx DecodeContext<'input>,
        plan: Plan,
        existing: &[(usize, String)],
    ) -> Result<Self, LocationIdentityError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(CodecError::from(refusal).into());
        }
        let mut identities = HashMap::new();
        identities
            .try_reserve(existing.len())
            .map_err(|_| locations::allocation_failed(ctx))?;
        for (node, id) in existing {
            let mut text = String::new();
            ctx.reserve_text_working(&mut text, id.len(), "FreeCAD existing source-use identity")?;
            text.push_str(id);
            if identities.insert(*node, text).is_some() {
                return Err(CodecError::malformed("duplicate canonical source-use row").into());
            }
        }
        Ok(Self {
            ctx,
            plan,
            identities: Identities::Existing(identities),
        })
    }

    pub(super) fn selector(&self, root: usize, shape: usize, placed: Located) -> Option<&str> {
        let node = self.plan.by_key.get(&Key {
            root,
            shape,
            location: placed.identity,
        })?;
        match &self.identities {
            Identities::Fresh(rows) => rows.get(*node).map(|row| row.id.as_str()),
            Identities::Existing(rows) => rows.get(node).map(String::as_str),
        }
    }

    pub(super) fn is_fresh(&self) -> bool {
        matches!(&self.identities, Identities::Fresh(_))
    }

    pub(super) fn into_records(
        self,
    ) -> Result<Option<Vec<cadmpeg_ir::native::NativeRecord>>, CodecError> {
        let Identities::Fresh(rows) = self.identities else {
            return Ok(None);
        };
        let mut records = Vec::new();
        self.ctx
            .reserve_working(&mut records, rows.len(), "FreeCAD source-use native arena")?;
        for row in rows {
            records.push(row.into_record(self.ctx)?);
        }
        records.sort_unstable_by(|left, right| left.id().cmp(right.id()));
        Ok(Some(records))
    }
}

pub(super) struct ResolvedUse {
    pub(super) id: String,
    pub(super) owner: String,
    pub(super) node: usize,
    pub(super) root: Option<usize>,
    key: Key,
}

/// Resolve native parent references once, independent of arena order. The
/// only vectors here are working indexes; serialized selectors contain scalars.
pub(super) fn resolve_namespace(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
    payloads: &[ShapePayloadRecord],
) -> Result<Vec<ResolvedUse>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut owners = HashMap::new();
    owners
        .try_reserve(payloads.len())
        .map_err(|_| locations::allocation_failed(ctx))?;
    for payload in payloads {
        if owners.insert(payload.id.as_str(), payload).is_some() {
            return Err(CodecError::malformed(
                "duplicate source-use payload identity",
            ));
        }
    }
    let mut other = HashSet::new();
    for (arena, records) in namespace.arenas() {
        if arena == "topology_uses" {
            continue;
        }
        other
            .try_reserve(records.len())
            .map_err(|_| locations::allocation_failed(ctx))?;
        for record in records {
            other.insert(record.id());
        }
    }
    let mut rows = Vec::new();
    for row in namespace.arena_iter_as_for_decode::<SourceUseSelector>(ctx, "topology_uses") {
        let row = row?;
        if !cadmpeg_ir::ids::is_valid_identity(&row.id) || other.contains(row.id.as_str()) {
            return Err(CodecError::malformed(
                "source-use native identity is invalid or collides",
            ));
        }
        if !owners.contains_key(row.owner.as_str()) {
            return Err(CodecError::malformed(
                "source-use native owner does not exist",
            ));
        }
        ctx.reserve_working(&mut rows, 1, "FreeCAD current source-use rows")?;
        rows.push(row);
    }
    let mut by_id = HashMap::new();
    by_id
        .try_reserve(rows.len())
        .map_err(|_| locations::allocation_failed(ctx))?;
    for (index, row) in rows.iter().enumerate() {
        if by_id.insert(row.id.as_str(), index).is_some() {
            return Err(CodecError::malformed(
                "duplicate source-use native identity",
            ));
        }
    }
    let mut children: Vec<Vec<usize>> = Vec::new();
    ctx.reserve_working(&mut children, rows.len(), "FreeCAD source-use parent index")?;
    children.resize_with(rows.len(), Vec::new);
    let mut resolved = Vec::new();
    ctx.reserve_working(&mut resolved, rows.len(), "FreeCAD source-use resolution")?;
    resolved.resize(rows.len(), None);
    let mut queue = Vec::new();
    ctx.reserve_working(&mut queue, rows.len(), "FreeCAD source-use parent walk")?;
    for (index, row) in rows.iter().enumerate() {
        match &row.source_use {
            SourceUse::Root { .. } => queue.push(index),
            SourceUse::Child { parent, slot } => {
                if *slot == 0 {
                    return Err(CodecError::malformed("source-use child slot is zero"));
                }
                let parent = by_id.get(parent.as_str()).copied().ok_or_else(|| {
                    CodecError::malformed("source-use native parent does not exist")
                })?;
                if rows[parent].owner != row.owner {
                    return Err(CodecError::malformed(
                        "source-use native parent has a different owner",
                    ));
                }
                ctx.reserve_working(&mut children[parent], 1, "FreeCAD source-use child index")?;
                children[parent].push(index);
            }
        }
    }
    let mut plans = HashMap::new();
    plans
        .try_reserve(payloads.len())
        .map_err(|_| locations::allocation_failed(ctx))?;
    let mut canonical = HashSet::new();
    canonical
        .try_reserve(rows.len())
        .map_err(|_| locations::allocation_failed(ctx))?;
    let mut cursor = 0;
    while let Some(&index) = queue.get(cursor) {
        cursor += 1;
        let row = &rows[index];
        let payload = owners[row.owner.as_str()];
        let tables = Tables::from_payload(payload)
            .ok_or_else(|| CodecError::malformed("source-use owner has no topology"))?;
        if !plans.contains_key(payload.id.as_str()) {
            let mut locations = Locations::new(ctx, tables).map_err(validation_location_error)?;
            let plan =
                Plan::discover(ctx, tables, &mut locations).map_err(validation_location_error)?;
            plans.insert(payload.id.as_str(), plan);
        }
        let plan = &plans[payload.id.as_str()];
        let node = match &row.source_use {
            SourceUse::Root { root } => plan.roots.get(root).copied(),
            SourceUse::Child { parent, slot } => {
                let parent = by_id[parent.as_str()];
                resolved[parent].and_then(|parent| plan.children.get(&(parent, *slot)).copied())
            }
        }
        .ok_or_else(|| {
            CodecError::malformed("source-use selector is not an actual canonical source use")
        })?;
        if !canonical.insert((payload.id.as_str(), node)) {
            return Err(CodecError::malformed(
                "duplicate canonical source-use native row",
            ));
        }
        resolved[index] = Some(node);
        queue.extend_from_slice(&children[index]);
    }
    if cursor != rows.len() {
        return Err(CodecError::malformed(
            "source-use native parent graph is cyclic",
        ));
    }
    drop(by_id);
    let mut result = Vec::new();
    ctx.reserve_working(
        &mut result,
        rows.len(),
        "FreeCAD resolved source-use identities",
    )?;
    for (index, row) in rows.into_iter().enumerate() {
        let node = resolved[index]
            .ok_or_else(|| CodecError::malformed("source-use native row was not resolved"))?;
        let key = plans[row.owner.as_str()].nodes[node].key;
        let root = match row.source_use {
            SourceUse::Root { root } => Some(root),
            SourceUse::Child { .. } => None,
        };
        result.push(ResolvedUse {
            id: row.id,
            owner: row.owner,
            node,
            root,
            key,
        });
    }
    Ok(result)
}

pub(super) fn validate_namespace(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
    payloads: &[ShapePayloadRecord],
) -> Result<(), CodecError> {
    resolve_namespace(ctx, namespace, payloads).map(|_| ())
}

fn validation_location_error(error: LocationIdentityError) -> CodecError {
    match error {
        LocationIdentityError::Decode(error) => error,
        LocationIdentityError::Bounded => {
            CodecError::malformed("source-use canonical validation reached its bound")
        }
    }
}

struct NativeUse {
    root: usize,
    property: String,
    kind: crate::brep::TextShapeKind,
    source_index: Option<usize>,
}
struct CurrentNativeTopology {
    uses: HashMap<String, NativeUse>,
}
impl CurrentNativeTopology {
    fn get(&self, id: &str) -> Option<&NativeUse> {
        self.uses.get(id)
    }
}

fn current_native_topology(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
    payloads: &[ShapePayloadRecord],
    catalog: &SourceCatalog,
) -> Result<CurrentNativeTopology, CodecError> {
    let resolved = resolve_namespace(ctx, namespace, payloads)?;
    let mut owners = HashMap::new();
    owners
        .try_reserve(payloads.len())
        .map_err(|_| locations::allocation_failed(ctx))?;
    for payload in payloads {
        owners.insert(payload.id.as_str(), payload);
    }
    let mut result = CurrentNativeTopology {
        uses: HashMap::new(),
    };
    result
        .uses
        .try_reserve(resolved.len())
        .map_err(|_| locations::allocation_failed(ctx))?;
    for row in resolved {
        let payload = owners[row.owner.as_str()];
        let tables = Tables::from_payload(payload)
            .ok_or_else(|| CodecError::malformed("source-use owner has no topology"))?;
        let kind = tables.tshapes.resolve(row.key.shape)?.kind();
        let source_index = catalog.source_index(ctx, &row.owner, kind, row.key)?;
        result.uses.insert(
            row.id,
            NativeUse {
                root: row.key.root,
                property: working_text(ctx, &payload.property)?,
                kind,
                source_index,
            },
        );
    }
    Ok(result)
}

type SourceKinds = HashMap<&'static str, HashSet<usize>>;

/// Presence is derived from the actual typed payload, independently of which
/// reference-only rows a current document happens to retain.
struct SourceCatalog {
    known: HashMap<String, SourceKinds>,
    unresolved: HashSet<String>,
    indices:
        HashMap<String, HashMap<(crate::brep::TextShapeKind, super::SourceOccurrenceKey), usize>>,
}

impl SourceCatalog {
    fn build(
        ctx: &DecodeContext<'_>,
        payloads: &[crate::brep::ShapePayloadRecord],
    ) -> Result<Self, CodecError> {
        let mut result = Self {
            known: HashMap::new(),
            unresolved: HashSet::new(),
            indices: HashMap::new(),
        };
        result
            .known
            .try_reserve(payloads.len())
            .map_err(|_| locations::allocation_failed(ctx))?;
        for payload in payloads {
            let Some(tables) = Tables::from_payload(payload) else {
                continue;
            };
            let indices = (|| {
                let mut locations = Locations::new(ctx, tables)?;
                Plan::discover(ctx, tables, &mut locations)?;
                super::source_topology_indices(ctx, tables, &mut locations)
            })();
            let indices = match indices {
                Ok(indices) => indices,
                Err(LocationIdentityError::Decode(error)) => return Err(error),
                Err(LocationIdentityError::Bounded) => {
                    // A bounded graph is unknown, never a claim that all of
                    // its names are unlinked historical native data.
                    result
                        .unresolved
                        .try_reserve(1)
                        .map_err(|_| locations::allocation_failed(ctx))?;
                    result
                        .unresolved
                        .insert(working_text(ctx, &payload.property)?);
                    continue;
                }
            };
            result
                .indices
                .try_reserve(1)
                .map_err(|_| locations::allocation_failed(ctx))?;
            let kinds = result
                .known
                .entry(working_text(ctx, &payload.property)?)
                .or_default();
            for ((kind, _), index) in &indices {
                kinds
                    .try_reserve(1)
                    .map_err(|_| locations::allocation_failed(ctx))?;
                let indices = kinds.entry(super::indexed_name(*kind)).or_default();
                indices
                    .try_reserve(1)
                    .map_err(|_| locations::allocation_failed(ctx))?;
                indices.insert(*index);
            }
            result
                .indices
                .insert(working_text(ctx, &payload.id)?, indices);
        }
        Ok(result)
    }

    fn source_index(
        &self,
        ctx: &DecodeContext<'_>,
        owner: &str,
        kind: crate::brep::TextShapeKind,
        key: Key,
    ) -> Result<Option<usize>, CodecError> {
        let key = super::SourceOccurrenceKey::from_identity(ctx, key.shape, key.location)?;
        Ok(self
            .indices
            .get(owner)
            .and_then(|indices| indices.get(&(kind, key)))
            .copied())
    }

    fn needs_inverse(&self, property: &str, kind: &str, index: usize) -> bool {
        self.unresolved.contains(property)
            || self
                .known
                .get(property)
                .and_then(|kinds| kinds.get(kind))
                .is_some_and(|indices| indices.contains(&index))
    }
}

/// Current neutral inverse rows for the roles historically named by each
/// kernel kind. Wrapper Body/Region/Shell rows keep native_ref too, but are
/// not mistaken for an Edge/Face/etc selection of the same source TShape.
pub(crate) struct CurrentTopologyBindings {
    topology: CurrentNativeTopology,
    catalog: SourceCatalog,
    records: HashMap<String, Vec<String>>,
    record_native: HashMap<String, String>,
    selections: HashMap<String, HashMap<&'static str, HashMap<usize, Vec<String>>>>,
}

impl CurrentTopologyBindings {
    pub(crate) fn source_kind(&self, native: &str) -> Option<crate::brep::TextShapeKind> {
        self.topology.get(native).map(|source| source.kind)
    }

    pub(crate) fn selected_records<'a>(
        &'a self,
        property: &str,
        indexed_name: &str,
        index: usize,
    ) -> (bool, impl Iterator<Item = &'a str> + 'a) {
        let sources = self
            .selections
            .get(property)
            .and_then(|kinds| kinds.get(indexed_name))
            .and_then(|indices| indices.get(&index));
        (
            self.catalog.needs_inverse(property, indexed_name, index),
            sources
                .into_iter()
                .flatten()
                .filter_map(move |native| self.records.get(native))
                .flatten()
                .map(String::as_str),
        )
    }

    pub(crate) fn native_for_record(&self, record: &str) -> Option<&str> {
        self.record_native.get(record).map(String::as_str)
    }
}

pub(crate) fn current_bindings(
    ctx: &DecodeContext<'_>,
    ir: &cadmpeg_ir::CadIr,
) -> Result<CurrentTopologyBindings, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    use crate::brep::TextShapeKind;
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(CurrentTopologyBindings {
            topology: CurrentNativeTopology {
                uses: HashMap::new(),
            },
            catalog: SourceCatalog {
                known: HashMap::new(),
                unresolved: HashSet::new(),
                indices: HashMap::new(),
            },
            records: HashMap::new(),
            record_native: HashMap::new(),
            selections: HashMap::new(),
        });
    };
    let payloads =
        namespace.arena_as_for_decode::<crate::brep::ShapePayloadRecord>(ctx, "shape_payloads")?;
    let catalog = SourceCatalog::build(ctx, &payloads)?;
    let topology = current_native_topology(ctx, namespace, &payloads, &catalog)?;
    #[derive(Clone, Copy)]
    enum NamedRole {
        Body,
        Region,
        Shell,
        Face,
        Loop,
        Edge,
        Vertex,
    }
    let mut selections =
        HashMap::<String, HashMap<&'static str, HashMap<usize, Vec<String>>>>::new();
    for (native, source) in &topology.uses {
        let Some(index) = source.source_index else {
            continue;
        };
        selections
            .try_reserve(1)
            .map_err(|_| locations::allocation_failed(ctx))?;
        let kinds = selections
            .entry(working_text(ctx, &source.property)?)
            .or_default();
        kinds
            .try_reserve(1)
            .map_err(|_| locations::allocation_failed(ctx))?;
        let indices = kinds.entry(super::indexed_name(source.kind)).or_default();
        indices
            .try_reserve(1)
            .map_err(|_| locations::allocation_failed(ctx))?;
        let sources = indices.entry(index).or_default();
        ctx.reserve_working(sources, 1, "FreeCAD source selection inverse")?;
        sources.push(working_text(ctx, native)?);
    }
    // Source root order is stable independently of opaque row spelling and arena order.
    for kinds in selections.values_mut() {
        for indices in kinds.values_mut() {
            for sources in indices.values_mut() {
                sources.sort_unstable_by_key(|id| topology.uses[id].root);
            }
        }
    }
    let mut record_native = HashMap::<String, String>::new();
    let mut records = HashMap::<String, Vec<String>>::new();
    let mut insert = |id: &str, native: Option<&str>, role: NamedRole| -> Result<(), CodecError> {
        let Some(native) = native else {
            return Ok(());
        };
        let Some(source) = topology.get(native) else {
            return Ok(());
        };
        record_native
            .try_reserve(1)
            .map_err(|_| locations::allocation_failed(ctx))?;
        record_native.insert(working_text(ctx, id)?, working_text(ctx, native)?);
        let names_this_kind = matches!(
            (role, source.kind),
            (
                NamedRole::Body,
                TextShapeKind::Compound | TextShapeKind::CompSolid
            ) | (NamedRole::Region, TextShapeKind::Solid)
                | (NamedRole::Shell, TextShapeKind::Shell | TextShapeKind::Wire)
                | (NamedRole::Face, TextShapeKind::Face)
                | (NamedRole::Loop, TextShapeKind::Wire)
                | (NamedRole::Edge, TextShapeKind::Edge)
                | (NamedRole::Vertex, TextShapeKind::Vertex)
        );
        if !names_this_kind {
            return Ok(());
        }
        records
            .try_reserve(1)
            .map_err(|_| locations::allocation_failed(ctx))?;
        let rows = records.entry(working_text(ctx, native)?).or_default();
        ctx.reserve_working(rows, 1, "FreeCAD current native topology inverse")?;
        rows.push(working_text(ctx, id)?);
        Ok(())
    };
    for body in &ir.model.bodies {
        insert(
            body.id.as_str(),
            body.native_ref.as_deref(),
            NamedRole::Body,
        )?;
    }
    for region in &ir.model.regions {
        insert(
            region.id.as_str(),
            region.native_ref.as_deref(),
            NamedRole::Region,
        )?;
    }
    for shell in &ir.model.shells {
        insert(
            shell.id.as_str(),
            shell.native_ref.as_deref(),
            NamedRole::Shell,
        )?;
    }
    for face in &ir.model.faces {
        insert(
            face.id.as_str(),
            face.native_ref.as_deref(),
            NamedRole::Face,
        )?;
    }
    for wire in &ir.model.loops {
        insert(
            wire.id.as_str(),
            wire.native_ref.as_deref(),
            NamedRole::Loop,
        )?;
    }
    for edge in &ir.model.edges {
        insert(
            edge.id.as_str(),
            edge.native_ref.as_deref(),
            NamedRole::Edge,
        )?;
    }
    for vertex in &ir.model.vertices {
        insert(
            vertex.id.as_str(),
            vertex.native_ref.as_deref(),
            NamedRole::Vertex,
        )?;
    }
    Ok(CurrentTopologyBindings {
        topology,
        catalog,
        records,
        record_native,
        selections,
    })
}

#[cfg(test)]
mod tests;
