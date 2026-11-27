// SPDX-License-Identifier: Apache-2.0
//! The placed instance tree of a design's occurrences.
//!
//! The decoder builds the tree expanded: the root definition places the
//! top-level occurrences, and a definition that more than one occurrence
//! places lists the child occurrences of each placement. A child's identity
//! extends its parent's ([`crate::ids::nested_occurrence_id`]), or, in an
//! archive, carries the scope of the member instance it belongs to, so the
//! parent of each child is recovered exactly. [`own_once`] then states the
//! tree as CADIR's products do (§8.1).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::selection::{OccurrencePath, PathStep, StepRef};
use cadmpeg_ir::document::{EntityRewrite, Model};
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    ExternalDocument, NestedOverride, Occurrence, ProductDefinition, Storage,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;

use crate::loss::F3dLossCode;

/// The root definition of a decoded file where no one root component record
/// stands for it: the root component whose record is not read, or the
/// document that holds the root components of several Design streams.
pub(crate) fn file_root_id() -> ProductDefinitionId {
    ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "product-definition"),
        cadmpeg_ir::identity_key!("root"),
    )
}

pub(crate) fn state_root_definition(
    ctx: &DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    if ctx.container_only() {
        return Ok(losses);
    }
    let design = match ir.native.namespace("f3d") {
        Some(namespace) => namespace
            .arena_iter_as_for_decode::<crate::records::xref::XrefDesign>(ctx, "xref_designs")
            .next()
            .transpose()?,
        None => None,
    };
    let document = design.as_ref().map(crate::xref::document_identity);
    if let Some(root) =
        ir.state_root_definition(ctx, file_root_id(), ExternalDocument::default())?
    {
        let root_id = root.id.clone();
        retarget_meshes(&mut ir.model, &file_root_id(), &root_id);
        let root = ir
            .model
            .product_definitions
            .iter_mut()
            .find(|definition| definition.id == root_id)
            .ok_or_else(|| CodecError::malformed("F3D root definition was not stated"))?;
        if let (Some(identity), Storage::File { document, .. }) = (document, &mut root.storage) {
            // The manifest terminal supplies only the lineage; the version
            // always comes from design ordinal zero.
            if let (Some(kept), Some(other)) = (&document.document_id, &identity.document_id) {
                if kept != other {
                    losses.push(F3dLossCode::XrefLineageDisagrees.note(format!(
                        "file root lineage {:?} (manifest terminal) disagrees with design ordinal 0 lineage {:?}; the manifest terminal supplies the root lineage",
                        kept.as_str(),
                        other.as_str()
                    )));
                }
            }
            document.document_id = document.document_id.take().or(identity.document_id);
            document.version = document.version.take().or(identity.version);
        }
    }
    Ok(losses)
}

/// Makes the mesh bodies of `from` the mesh bodies of `to`.
pub(crate) fn retarget_meshes(
    model: &mut Model,
    from: &ProductDefinitionId,
    to: &ProductDefinitionId,
) {
    if from == to {
        return;
    }
    for mesh in &mut model.tessellations {
        if let cadmpeg_ir::tessellation::Meshed::Definition { definition, .. } = &mut mesh.of {
            if definition == from {
                *definition = to.clone();
            }
        }
    }
}

/// The file's root definition in `model`, added when it is absent.
pub(crate) fn file_root(model: &mut Model) -> &mut ProductDefinition {
    let id = file_root_id();
    let position = match model
        .product_definitions
        .iter()
        .position(|definition| definition.id == id)
    {
        Some(position) => position,
        None => {
            model
                .product_definitions
                .push(ProductDefinition::root(id, ExternalDocument::default()));
            model.product_definitions.len() - 1
        }
    };
    &mut model.product_definitions[position]
}

/// Places `occurrence` at the top level of the file, after the occurrences
/// already there.
pub(crate) fn place_top_level(model: &mut Model, occurrence: OccurrenceId) {
    let root = file_root(model);
    if !root.occurrences.contains(&occurrence) {
        root.occurrences.push(occurrence);
    }
}

/// Lists `occurrence` among the occurrences of the definition `owner`.
pub(crate) fn place_in(model: &mut Model, owner: &ProductDefinitionId, occurrence: OccurrenceId) {
    if let Some(definition) = model
        .product_definitions
        .iter_mut()
        .find(|definition| &definition.id == owner)
    {
        if !definition.occurrences.contains(&occurrence) {
            definition.occurrences.push(occurrence);
        }
    }
}

/// Lists `child` inside `parent`: among the occurrences of the definition
/// `parent` places, or at the top level when `parent` is `None`.
pub(crate) fn place_under(model: &mut Model, parent: Option<&OccurrenceId>, child: OccurrenceId) {
    let owner = parent.and_then(|parent| {
        model
            .occurrences
            .iter()
            .find(|occurrence| &occurrence.id == parent)
            .map(|occurrence| occurrence.prototype.clone())
    });
    match owner {
        Some(owner) => place_in(model, &owner, child),
        None => place_top_level(model, child),
    }
}

/// Projected product structure: definitions, occurrences, and each
/// occurrence with the occurrence it sits in, `None` at the top level, in
/// their order among their siblings.
pub(crate) struct Projected {
    pub(crate) definitions: Vec<ProductDefinition>,
    pub(crate) occurrences: Vec<Occurrence>,
    pub(crate) placements: Vec<(OccurrenceId, Option<OccurrenceId>)>,
}

/// Adds the local and external product structure to `model`, each
/// occurrence listed by the definition its parent places, and returns the
/// external projection's losses. The external placements at the top level
/// come first, as the source numbers them; the others sit in local
/// occurrences.
pub(crate) fn assemble(
    model: &mut Model,
    local: Projected,
    external: Option<crate::xref::ExternalOccurrenceProjection>,
) -> Vec<LossNote> {
    let (external, losses) = match external {
        Some(external) => (
            Projected {
                definitions: external.definitions,
                occurrences: external.occurrences,
                placements: external.placements,
            },
            external.losses,
        ),
        None => (
            Projected {
                definitions: Vec::new(),
                occurrences: Vec::new(),
                placements: Vec::new(),
            },
            Vec::new(),
        ),
    };
    model.product_definitions.extend(external.definitions);
    model.occurrences.extend(external.occurrences);
    model.product_definitions.extend(local.definitions);
    model.occurrences.extend(local.occurrences);
    let (top, nested): (Vec<_>, Vec<_>) = external
        .placements
        .into_iter()
        .partition(|(_, parent)| parent.is_none());
    for (occurrence, parent) in top.into_iter().chain(local.placements).chain(nested) {
        place_under(model, parent.as_ref(), occurrence);
    }
    losses
}

/// Whether `occurrence` sits at the top level: the file's root definition,
/// or no definition, lists it.
pub(crate) fn is_top_level(definitions: &[ProductDefinition], occurrence: &OccurrenceId) -> bool {
    cadmpeg_ir::products::owner(definitions, occurrence).is_none_or(ProductDefinition::is_root)
}

/// The occurrence `occurrence` sits in, or `None` at the top level. Where
/// the owning definition has several placements, the placement is the one
/// whose identity `occurrence` extends, or whose archive member instance
/// scope (`instance_scopes`) `occurrence` is in.
fn parent<'m>(
    definitions: &[ProductDefinition],
    occurrences: &'m [Occurrence],
    occurrence: &OccurrenceId,
    instance_scopes: &BTreeMap<OccurrenceId, String>,
) -> Option<&'m Occurrence> {
    let owner = cadmpeg_ir::products::owner(definitions, occurrence)?;
    if owner.is_root() {
        return None;
    }
    let mut placements = occurrences
        .iter()
        .filter(|candidate| candidate.prototype == owner.id);
    let first = placements.next()?;
    let Some(second) = placements.next() else {
        return Some(first);
    };
    let candidates = std::iter::once(first)
        .chain(std::iter::once(second))
        .chain(placements)
        .collect::<Vec<_>>();
    candidates
        .iter()
        .copied()
        .find(|candidate| extends(occurrence, &candidate.id))
        .or_else(|| {
            let scope = instance_scope(occurrence.as_str())?;
            let mut matching = candidates
                .iter()
                .copied()
                .filter(|candidate| match instance_scopes.get(&candidate.id) {
                    Some(instance) => instance == scope,
                    None => instance_scope(candidate.id.as_str()) == Some(scope),
                });
            match (matching.next(), matching.next()) {
                (Some(placement), None) => Some(placement),
                _ => None,
            }
        })
}

/// The occurrence each nested occurrence sits in.
pub(crate) fn parent_ids(
    definitions: &[ProductDefinition],
    occurrences: &[Occurrence],
) -> std::collections::BTreeMap<OccurrenceId, OccurrenceId> {
    parent_ids_in_members(definitions, occurrences, &BTreeMap::new())
}

/// [`parent_ids`] for a model that places archive member instances:
/// `instance_scopes` maps each occurrence that places one to the identity
/// scope of that instance, which names the placement of a member's own
/// occurrences below a definition several occurrences place.
pub(crate) fn parent_ids_in_members(
    definitions: &[ProductDefinition],
    occurrences: &[Occurrence],
    instance_scopes: &BTreeMap<OccurrenceId, String>,
) -> std::collections::BTreeMap<OccurrenceId, OccurrenceId> {
    occurrences
        .iter()
        .filter_map(|occurrence| {
            parent(definitions, occurrences, &occurrence.id, instance_scopes)
                .map(|parent| (occurrence.id.clone(), parent.id.clone()))
        })
        .collect()
}

/// The final segment of an occurrence identity: the element that names the
/// occurrence in an operand `occurrence_path`.
pub(crate) fn identity_segment(identity: &OccurrenceId) -> &str {
    identity
        .as_str()
        .rsplit(['#', ':'])
        .next()
        .unwrap_or_default()
}

/// Whether the identity of `child` extends that of `parent`.
fn extends(child: &OccurrenceId, parent: &OccurrenceId) -> bool {
    child
        .as_str()
        .strip_prefix(parent.as_str())
        .is_some_and(|rest| rest.starts_with(':'))
}

fn belongs_to_placement<T: PartialEq + Copy>(
    assigned: Option<T>,
    placement: T,
    first: Option<T>,
) -> bool {
    assigned.or(first).is_none_or(|parent| parent == placement)
}

/// The archive member instance scope of an identity rescoped into it
/// (`f3d:<scope>` of `f3d:<scope>@<identity>`), or `None` outside an archive
/// member.
fn instance_scope(id: &str) -> Option<&str> {
    let key = crate::ids::member_key(id)?;
    Some(&id[..crate::ids::SCHEME_PREFIX.len() + key.len()])
}

/// Whether `occurrence` is the identity occurrence the component projection
/// gives a Design stream's root component.
fn is_root_component_occurrence(occurrence: &Occurrence) -> bool {
    let Some((_, key)) = occurrence.id.as_str().split_once('#') else {
        return false;
    };
    let root_key = key.starts_with("root-")
        || key.split_once(':').is_some_and(|(stream, rest)| {
            stream.starts_with("stream") && rest.starts_with("root-")
        });
    root_key
        && occurrence.native_ref.is_none()
        && occurrence
            .transform
            .is_none_or(|transform| transform.rows() == Transform::identity().rows())
}

/// One placed instance of the expanded tree.
struct Node {
    /// The expanded occurrence the node is.
    occurrence: usize,
    parent: Option<usize>,
    /// The definition whose child the occurrence is.
    owner: usize,
    /// The node of the occurrence it is an instance of once the definition
    /// owns its children once; itself for that occurrence's first instance.
    canonical: usize,
    /// A root component's identity occurrence, folded into the definition
    /// that places it.
    transparent: bool,
}

/// What [`own_once`] could not state exactly.
#[derive(Default)]
struct OwnLosses {
    /// Children that only some placements of their definition hold.
    partial: usize,
    /// References to one instance of an occurrence that its definition's
    /// placements share.
    shared_references: usize,
}

/// States the expanded instance tree as CADIR's products do (§8.1):
///
/// - The root component of the file, and of each archive member, is the
///   definition its file holds at its top: the definition that places it
///   only through its identity occurrence is folded into it, so it takes
///   that definition's storage, label, bodies, and other occurrences.
/// - A definition owns its child occurrences once. The first placement of a
///   definition, in instance order, gives each child its base values; a
///   placement whose instance differs states the difference as a nested
///   override on the top-level occurrence of its path. A nested occurrence
///   keeps the identity of its own carrier.
///
/// `instance_scopes` maps each occurrence that places an archive member
/// instance to that instance's identity scope. Every reference to an
/// expanded occurrence names its occurrence afterwards.
pub(crate) fn own_once(
    ctx: &DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    instance_scopes: &BTreeMap<OccurrenceId, String>,
) -> Result<Vec<LossNote>, CodecError> {
    use crate::records::feature::assembly_features::DesignComponentOccurrence;
    use cadmpeg_ir::products::OperandContainer;

    let model = &ir.model;
    let roots = model
        .product_definitions
        .iter()
        .enumerate()
        .filter(|(_, definition)| definition.is_root())
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let definitions = &model.product_definitions;
    let occurrences = &model.occurrences;
    let occurrence_at = occurrences
        .iter()
        .enumerate()
        .map(|(index, occurrence)| (occurrence.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let definition_at = definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| (definition.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut placements = vec![Vec::new(); definitions.len()];
    for (index, occurrence) in occurrences.iter().enumerate() {
        if let Some(&definition) = definition_at.get(occurrence.prototype.as_str()) {
            placements[definition].push(index);
        }
    }

    let folds = folds(
        definitions,
        occurrences,
        &occurrence_at,
        &definition_at,
        &placements,
    );
    let transparent = folds
        .values()
        .flat_map(|(_, rooted)| rooted)
        .copied()
        .collect::<BTreeSet<_>>();

    // The placement each child of a definition with several placements
    // belongs to.
    let mut assigned = HashMap::<usize, usize>::new();
    for (definition, row) in definitions.iter().enumerate() {
        if placements[definition].len() < 2 {
            continue;
        }
        for child in &row.occurrences {
            let Some(&child) = occurrence_at.get(child.as_str()) else {
                continue;
            };
            if let Some(placement) = assignment(
                &occurrences[child],
                &placements[definition],
                occurrences,
                instance_scopes,
            ) {
                assigned.insert(child, placement);
            }
        }
    }

    let mut nodes = Vec::<Node>::new();
    let mut visited = vec![false; occurrences.len()];
    let mut first_placement = HashMap::<usize, Option<usize>>::new();
    let mut canonical_children = BTreeMap::<usize, Vec<usize>>::new();
    let mut by_relative = HashMap::<(usize, String), usize>::new();
    let mut identities = HashMap::<usize, OccurrenceId>::new();
    let mut used = BTreeSet::<OccurrenceId>::new();
    let mut losses = OwnLosses::default();
    let mut stack = Vec::new();
    for &root in roots.iter().rev() {
        for child in definitions[root].occurrences.iter().rev() {
            if let Some(&child) = occurrence_at.get(child.as_str()) {
                stack.push((child, None, root));
            }
        }
    }
    while let Some((occurrence, parent, owner)) = stack.pop() {
        if std::mem::replace(&mut visited[occurrence], true) {
            continue;
        }
        let node = nodes.len();
        let placement = parent.map(|parent: usize| nodes[parent].occurrence);
        let relative = relative_key(&occurrences[occurrence], placement.map(|p| &occurrences[p]));
        let first = *first_placement.entry(owner).or_insert(placement);
        let canonical = if first == placement {
            None
        } else {
            let found = by_relative.get(&(owner, relative.clone())).copied();
            if found.is_none() {
                losses.partial += 1;
            }
            found
        };
        let canonical = match canonical {
            Some(canonical) => canonical,
            None => {
                by_relative.insert((owner, relative), node);
                canonical_children.entry(owner).or_default().push(node);
                let mut identity =
                    own_identity(&occurrences[occurrence], placement.map(|p| &occurrences[p]))?;
                if !used.insert(identity.clone()) {
                    identity = occurrences[occurrence].id.clone();
                    used.insert(identity.clone());
                }
                identities.insert(node, identity);
                node
            }
        };
        nodes.push(Node {
            occurrence,
            parent,
            owner,
            canonical,
            transparent: transparent.contains(&occurrence),
        });
        let Some(&prototype) = definition_at.get(occurrences[occurrence].prototype.as_str()) else {
            continue;
        };
        let several = placements[prototype].len() > 1;
        let first = first_placement.get(&prototype).copied().flatten();
        for child in definitions[prototype].occurrences.iter().rev() {
            let Some(&child) = occurrence_at.get(child.as_str()) else {
                continue;
            };
            let belongs =
                !several || belongs_to_placement(assigned.get(&child).copied(), occurrence, first);
            if belongs && !visited[child] {
                stack.push((child, Some(node), prototype));
            }
        }
    }

    // Each canonical node's instances.
    let mut instances = BTreeMap::<usize, Vec<usize>>::new();
    for (index, node) in nodes.iter().enumerate() {
        instances.entry(node.canonical).or_default().push(index);
    }
    let folded_into = folds
        .iter()
        .map(|(&from, (into, _))| (from, *into))
        .collect::<BTreeMap<_, _>>();
    // A fold states the component's id, except where the file definition is
    // an archive member's: its key qualifies every record of the member (§2.2).
    let folded_id = folds
        .iter()
        .flat_map(|(&from, &(into, _))| {
            let kept = if crate::ids::member_definition_key(definitions[from].id.as_str()).is_some()
            {
                from
            } else {
                into
            };
            [(from, kept), (into, kept)]
        })
        .collect::<BTreeMap<_, _>>();
    let definition_id = |index: usize| -> ProductDefinitionId {
        definitions[folded_id.get(&index).copied().unwrap_or(index)]
            .id
            .clone()
    };

    let mut stated = Vec::<Occurrence>::new();
    let mut stated_at = HashMap::<usize, usize>::new();
    for (&canonical, members) in &instances {
        let node = &nodes[canonical];
        if node.transparent {
            continue;
        }
        let source = &occurrences[node.occurrence];
        let mut occurrence = source.clone();
        occurrence.id = identities[&canonical].clone();
        if let Some(&prototype) = definition_at.get(source.prototype.as_str()) {
            occurrence.prototype = definition_id(prototype);
        }
        let values = members
            .iter()
            .map(|member| &occurrences[nodes[*member].occurrence])
            .collect::<Vec<_>>();
        occurrence.visible = common(values.iter().map(|value| value.visible));
        occurrence.suppressed = common(values.iter().map(|value| value.suppressed));
        occurrence.representation = common(values.iter().map(|value| value.representation.clone()));
        occurrence.overrides.clear();
        stated_at.insert(canonical, stated.len());
        stated.push(occurrence);
    }

    // Each instance that differs from its occurrence's base values.
    let mut overrides = BTreeMap::<usize, Vec<NestedOverride>>::new();
    for (index, node) in nodes.iter().enumerate() {
        if node.transparent {
            continue;
        }
        let Some(&base) = stated_at.get(&node.canonical) else {
            continue;
        };
        let base = &stated[base];
        let value = &occurrences[node.occurrence];
        let transform = value.transform.filter(|transform| {
            index != node.canonical && !same_placement(base.transform, *transform)
        });
        let visible = value.visible.filter(|_| value.visible != base.visible);
        let suppressed = value
            .suppressed
            .filter(|_| value.suppressed != base.suppressed);
        let representation = value
            .representation
            .clone()
            .filter(|_| value.representation != base.representation);
        if transform.is_none()
            && visible.is_none()
            && suppressed.is_none()
            && representation.is_none()
        {
            continue;
        }
        let mut chain = Vec::new();
        let mut current = Some(index);
        while let Some(at) = current {
            if !nodes[at].transparent {
                chain.push(nodes[at].canonical);
            }
            current = nodes[at].parent;
        }
        let Some((&top, path)) = chain.split_last() else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        let path = OccurrencePath(
            path.iter()
                .rev()
                .map(|step| PathStep {
                    occurrence: StepRef::Occurrence(identities[step].clone()),
                    configuration: None,
                })
                .collect(),
        );
        let entries = overrides.entry(top).or_default();
        let position = match entries.iter().position(|entry| entry.path == path) {
            Some(position) => position,
            None => {
                let position = entries.len();
                entries.push(NestedOverride {
                    path,
                    transform: None,
                    suppressed: None,
                    visible: None,
                    representation: None,
                });
                position
            }
        };
        let entry = &mut entries[position];
        entry.transform = entry.transform.or(transform);
        entry.visible = entry.visible.or(visible);
        entry.suppressed = entry.suppressed.or(suppressed);
        entry.representation = entry.representation.take().or(representation);
    }
    for (top, entries) in overrides {
        if let Some(&at) = stated_at.get(&top) {
            stated[at].overrides = entries;
        }
    }

    // The occurrences of each definition the tree reaches.
    let listed = |owner: usize| -> Vec<OccurrenceId> {
        let mut out = Vec::new();
        let mut pending = canonical_children
            .get(&owner)
            .into_iter()
            .flatten()
            .rev()
            .copied()
            .collect::<Vec<_>>();
        while let Some(node) = pending.pop() {
            if nodes[node].transparent {
                let prototype = definition_at
                    .get(occurrences[nodes[node].occurrence].prototype.as_str())
                    .copied();
                pending.extend(
                    prototype
                        .and_then(|prototype| canonical_children.get(&prototype))
                        .into_iter()
                        .flatten()
                        .rev()
                        .copied(),
                );
            } else {
                out.push(identities[&node].clone());
            }
        }
        out
    };
    let reached_owners = nodes.iter().map(|node| node.owner).collect::<BTreeSet<_>>();
    let mut identity_map = HashMap::<String, String>::new();
    let mut shared = BTreeSet::<String>::new();
    for node in nodes.iter().filter(|node| !node.transparent) {
        let from = occurrences[node.occurrence].id.as_str();
        let to = identities[&node.canonical].as_str();
        if instances[&node.canonical].len() > 1 {
            shared.insert(from.to_owned());
        }
        if from != to {
            identity_map.insert(from.to_owned(), to.to_owned());
        }
    }
    for (&from, &(into, _)) in &folds {
        let (lost, kept) = if folded_id[&from] == from {
            (into, from)
        } else {
            (from, into)
        };
        identity_map.insert(
            definitions[lost].id.as_str().to_owned(),
            definitions[kept].id.as_str().to_owned(),
        );
    }
    let renamed = |id: &str| identity_map.get(id).map_or(id, String::as_str).to_owned();

    let mut product_definitions = Vec::with_capacity(definitions.len());
    for (index, definition) in definitions.iter().enumerate() {
        if folded_into.contains_key(&index) {
            continue;
        }
        let mut definition = definition.clone();
        if let Some((&from, _)) = folds.iter().find(|(_, (into, _))| *into == index) {
            let placing = &definitions[from];
            definition.id = definition_id(from);
            definition.storage = placing.storage.clone();
            if definition.label.is_none() {
                definition.label.clone_from(&placing.label);
            }
            if definition.native_ref.is_none() {
                definition.native_ref.clone_from(&placing.native_ref);
            }
            for body in &placing.bodies {
                if !definition.bodies.contains(body) {
                    definition.bodies.push(body.clone());
                }
            }
            for sketch in &placing.sketches {
                if !definition.sketches.contains(sketch) {
                    definition.sketches.push(sketch.clone());
                }
            }
            definition.occurrences = listed(from);
        } else if reached_owners.contains(&index) {
            definition.occurrences = listed(index);
        } else {
            for occurrence in &mut definition.occurrences {
                *occurrence = mint(renamed(occurrence.as_str()))?;
            }
        }
        product_definitions.push(definition);
    }
    let reached = nodes
        .iter()
        .map(|node| node.occurrence)
        .collect::<BTreeSet<_>>();
    let listed_by_reached = definitions
        .iter()
        .enumerate()
        .filter(|(index, _)| reached_owners.contains(index) || folded_into.contains_key(index))
        .flat_map(|(_, definition)| &definition.occurrences)
        .map(OccurrenceId::as_str)
        .collect::<BTreeSet<_>>();
    for (index, occurrence) in occurrences.iter().enumerate() {
        if reached.contains(&index) || listed_by_reached.contains(occurrence.id.as_str()) {
            continue;
        }
        let mut occurrence = occurrence.clone();
        occurrence.prototype = ProductDefinitionId::mint(renamed(occurrence.prototype.as_str()))
            .map_err(|error| CodecError::malformed(error.to_string()))?;
        stated.push(occurrence);
    }

    if !ir.model.assembly_joints.is_empty() {
        let mut native_keys = BTreeMap::new();
        if let Some(namespace) = ir.native.namespace("f3d") {
            for carrier in namespace.arena_iter_as_for_decode::<DesignComponentOccurrence>(
                ctx,
                "design_component_occurrences",
            ) {
                let carrier = carrier?;
                native_keys.insert(carrier.id, carrier.occurrence_guid.as_str().to_owned());
            }
            for reference in namespace
                .arena_iter_as_for_decode::<crate::records::xref::XrefReference>(
                    ctx,
                    "xref_references",
                )
            {
                let reference = reference?;
                if let Some(key) = reference
                    .placement_source
                    .and_then(|source| source.occurrence_guid)
                {
                    native_keys.insert(reference.id, key.as_str().to_owned());
                }
            }
            for scope in namespace
                .arena_iter_as_for_decode::<crate::records::feature::scope::DesignParameterScope>(
                    ctx,
                    "design_parameter_scopes",
                )
            {
                let scope = scope?;
                if let Some(key) = scope
                    .component_insert_construction()
                    .and_then(|construction| construction.occurrence_key.as_ref())
                {
                    native_keys.insert(scope.id.clone(), key.value.as_str().to_owned());
                }
            }
        }
        let native_key = |at: usize| {
            let source = &occurrences[nodes[at].occurrence];
            source
                .native_ref
                .as_ref()
                .and_then(|id| native_keys.get(id))
                .cloned()
                .or_else(|| {
                    let parent = nodes[at].parent.map(|p| &occurrences[nodes[p].occurrence]);
                    let relative = relative_key(source, parent);
                    let key = relative.strip_prefix(':').unwrap_or(&relative);
                    (source.native_ref.is_none() && !key.contains([':', '#']))
                        .then(|| key.to_owned())
                })
        };
        let mut keyed = BTreeMap::new();
        for &at in instances.keys().filter(|&&at| !nodes[at].transparent) {
            if let Some(key) = native_key(at) {
                keyed
                    .entry((definition_id(nodes[at].owner), key.to_ascii_lowercase()))
                    .and_modify(|target| *target = None)
                    .or_insert_with(|| Some(identities[&at].clone()));
            }
        }
        let mut selections = HashMap::new();
        for (index, node) in nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| !node.transparent)
        {
            let mut chain = Vec::new();
            let mut current = Some(index);
            while let Some(at) = current {
                if !nodes[at].transparent {
                    chain.push(at);
                }
                current = nodes[at].parent;
            }
            let Some(top) = chain.pop() else { continue };
            if chain.is_empty() || instances[&nodes[top].canonical].len() != 1 {
                continue;
            }
            // The frame of an operand that names the occurrence is in the
            // space of its prototype; the operand is restated in the space of
            // the outer insertion, so the frame takes each nested placement,
            // innermost first.
            let mut nested_pose = Some(Transform::identity());
            for &at in &chain {
                nested_pose = nested_pose.and_then(|pose| {
                    occurrences[nodes[at].occurrence]
                        .transform
                        .unwrap_or_else(Transform::identity)
                        .compose(pose)
                        .ok()
                });
            }
            let Some(nested_pose) = nested_pose else {
                continue;
            };
            let keys = chain
                .into_iter()
                .rev()
                .map(|at| {
                    let key = native_key(nodes[at].canonical)?;
                    let identity = &identities[&nodes[at].canonical];
                    let target = keyed
                        .get(&(definition_id(nodes[at].owner), key.to_ascii_lowercase()))
                        .and_then(Option::as_ref);
                    (target == Some(identity)).then(|| identity_segment(identity).to_owned())
                })
                .collect::<Option<Vec<_>>>();
            if let Some(keys) = keys {
                selections.insert(
                    occurrences[node.occurrence].id.clone(),
                    (
                        occurrences[nodes[top].occurrence].id.clone(),
                        keys,
                        nested_pose,
                    ),
                );
            }
        }
        for connector in ir
            .model
            .assembly_joints
            .iter_mut()
            .flat_map(cadmpeg_ir::products::AssemblyJoint::connectors_mut)
        {
            if let OperandContainer::Occurrence {
                occurrence,
                occurrence_path,
            } = &mut connector.operand.container
            {
                if let Some((top, keys, nested_pose)) = selections.get(occurrence) {
                    let Ok(frame) = nested_pose.compose(connector.frame) else {
                        continue;
                    };
                    let mut path = keys.clone();
                    path.append(occurrence_path);
                    *occurrence = top.clone();
                    *occurrence_path = path;
                    connector.frame = frame;
                }
            }
        }
    }

    let mut rewrite = SharedIdentities {
        ctx,
        map: &identity_map,
        shared: &shared,
        count: 0,
    };
    let mut old = std::mem::take(&mut ir.model);
    old.product_definitions.clear();
    old.occurrences.clear();
    ir.model.extend_rewritten(old, &mut rewrite)?;
    losses.shared_references = rewrite.count;
    crate::scene::rename_bound(ctx, ir, &identity_map)?;
    ir.model.product_definitions = product_definitions;
    ir.model.occurrences = stated;

    let mut notes = Vec::new();
    if losses.partial != 0 {
        notes.push(F3dLossCode::PlacementChildrenDiffer.note(format!(
            "{} nested occurrence(s) are held by only some placements of the definition that owns them; the definition owns each once, so every placement shows it",
            losses.partial
        )));
    }
    if losses.shared_references != 0 {
        notes.push(F3dLossCode::InstanceReferenceShared.note(format!(
            "{} reference(s) name one placed instance of a nested occurrence whose definition is placed more than once; each names the occurrence, which every placement shows",
            losses.shared_references
        )));
    }
    Ok(notes)
}

/// The namespace and key of an occurrence identity.
fn identity_parts(id: &OccurrenceId) -> Result<(&str, &str), CodecError> {
    id.as_str()
        .split_once('#')
        .ok_or_else(|| CodecError::malformed("F3D occurrence identity has no key"))
}

fn mint(id: String) -> Result<OccurrenceId, CodecError> {
    OccurrenceId::mint(id).map_err(|error| CodecError::malformed(error.to_string()))
}

/// The value every instance states, or `None` where they differ.
fn common<T: PartialEq>(mut values: impl Iterator<Item = Option<T>>) -> Option<T> {
    let first = values.next().flatten()?;
    values
        .all(|value| value.as_ref() == Some(&first))
        .then_some(first)
}

/// A placement that moves less than this many millimetres, with a linear
/// block within [`EPS_SAME_LINEAR`], is the same placement.
const EPS_SAME_SHIFT_MM: f64 = 1.0e-4;
const EPS_SAME_LINEAR: f64 = 1.0e-6;

fn same_placement(base: Option<Transform>, other: Transform) -> bool {
    let Some(base) = base else {
        return false;
    };
    let (base, other) = (base.affine_rows(), other.affine_rows());
    let shift = (0..3)
        .map(|row| (base[row][3] - other[row][3]).powi(2))
        .sum::<f64>()
        .sqrt();
    shift <= EPS_SAME_SHIFT_MM
        && (0..3)
            .flat_map(|row| (0..3).map(move |column| (row, column)))
            .all(|(row, column)| (base[row][column] - other[row][column]).abs() <= EPS_SAME_LINEAR)
}

/// The definitions that place a root component only through its identity
/// occurrence, each with the component it folds into and those occurrences.
fn folds(
    definitions: &[ProductDefinition],
    occurrences: &[Occurrence],
    occurrence_at: &HashMap<&str, usize>,
    definition_at: &HashMap<&str, usize>,
    placements: &[Vec<usize>],
) -> BTreeMap<usize, (usize, Vec<usize>)> {
    let mut out = BTreeMap::new();
    for (index, definition) in definitions.iter().enumerate() {
        if !matches!(definition.storage, Storage::File { .. }) {
            continue;
        }
        let rooted = definition
            .occurrences
            .iter()
            .filter_map(|id| occurrence_at.get(id.as_str()).copied())
            .filter(|&occurrence| is_root_component_occurrence(&occurrences[occurrence]))
            .collect::<Vec<_>>();
        let components = rooted
            .iter()
            .map(|&occurrence| {
                definition_at
                    .get(occurrences[occurrence].prototype.as_str())
                    .copied()
            })
            .collect::<Option<BTreeSet<_>>>();
        let Some(components) = components else {
            continue;
        };
        let mut components = components.into_iter();
        let (Some(component), None) = (components.next(), components.next()) else {
            continue;
        };
        if component == index
            || !matches!(definitions[component].storage, Storage::Embedded)
            || !placements[component]
                .iter()
                .all(|placement| rooted.contains(placement))
            || rooted.len() != placements[index].len().max(1)
        {
            continue;
        }
        out.insert(index, (component, rooted));
    }
    out
}

/// The placement among `candidates` that `child` belongs to: the one its
/// identity extends, or the one whose archive member instance it is in.
fn assignment(
    child: &Occurrence,
    candidates: &[usize],
    occurrences: &[Occurrence],
    instance_scopes: &BTreeMap<OccurrenceId, String>,
) -> Option<usize> {
    if let Some(placement) = candidates
        .iter()
        .copied()
        .filter(|&candidate| extends(&child.id, &occurrences[candidate].id))
        .max_by_key(|&candidate| occurrences[candidate].id.as_str().len())
    {
        return Some(placement);
    }
    let scope = instance_scope(child.id.as_str())?;
    let mut matching = candidates.iter().copied().filter(|&candidate| {
        let placing = &occurrences[candidate].id;
        match instance_scopes.get(placing) {
            Some(instance) => instance == scope,
            None => instance_scope(placing.as_str()) == Some(scope),
        }
    });
    match (matching.next(), matching.next()) {
        (Some(placement), None) => Some(placement),
        _ => None,
    }
}

/// The identity of `occurrence` relative to the placement it sits in, the
/// same for its instance under every placement of its definition.
fn relative_key(occurrence: &Occurrence, placement: Option<&Occurrence>) -> String {
    let id = occurrence.id.as_str();
    if let Some(tail) = placement
        .and_then(|placement| id.strip_prefix(placement.id.as_str()))
        .filter(|tail| tail.starts_with(':'))
    {
        return tail.to_owned();
    }
    match instance_scope(id) {
        Some(scope) => id[scope.len()..].to_owned(),
        None => id.to_owned(),
    }
}

/// The identity of an occurrence once its definition owns it once: the
/// identity of its own carrier, without the path of the placement it sits in.
fn own_identity(
    occurrence: &Occurrence,
    placement: Option<&Occurrence>,
) -> Result<OccurrenceId, CodecError> {
    let id = occurrence.id.as_str();
    let own = placement.and_then(|placement| {
        let (namespace, key) = id.split_once('#')?;
        let (_, parent) = placement.id.as_str().split_once('#')?;
        key.strip_prefix(parent)?
            .strip_prefix(':')
            .map(|tail| format!("{namespace}#{tail}"))
    });
    own.map_or_else(|| Ok(occurrence.id.clone()), mint)
}

/// Renames the expanded occurrences a model references, counting the
/// references to an instance of an occurrence that several placements share.
struct SharedIdentities<'a, 'c> {
    ctx: &'a DecodeContext<'c>,
    map: &'a HashMap<String, String>,
    shared: &'a BTreeSet<String>,
    count: usize,
}

impl EntityRewrite for SharedIdentities<'_, '_> {
    type Error = CodecError;

    fn rewrite<T: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities>(
        &mut self,
        entity: T,
    ) -> Result<T, CodecError> {
        let (map, shared) = (self.map, self.shared);
        let mut count = 0;
        let rewritten = cadmpeg_ir::schema::rewrite::identities(self.ctx, entity, |id| {
            if shared.contains(id) {
                count += 1;
            }
            Ok(map.get(id).cloned().unwrap_or_else(|| id.to_owned()))
        })?;
        self.count += count;
        Ok(rewritten)
    }
}

/// The instance tree of a model whose definitions own their children once,
/// expanded as the decoder builds it: each placed instance is an occurrence
/// whose identity extends that of the instance it sits in.
pub(crate) struct Expanded {
    pub(crate) definitions: Vec<ProductDefinition>,
    pub(crate) occurrences: Vec<Occurrence>,
    /// The occurrence each expanded instance is an instance of.
    pub(crate) occurrence_of: BTreeMap<OccurrenceId, OccurrenceId>,
    pub(crate) parents: BTreeMap<OccurrenceId, OccurrenceId>,
}

/// Expands the instance tree of `definitions` and `occurrences` as
/// [`own_once`] found it, each instance at the values its top-level
/// occurrence's overrides give it. The decode policy bounds the instance count.
pub(crate) fn expand(
    limits: &cadmpeg_core::decode::ResourceLimits,
    definitions: &[ProductDefinition],
    occurrences: &[Occurrence],
    instance_scopes: &BTreeMap<OccurrenceId, String>,
) -> Result<Expanded, CodecError> {
    /// One instance to expand: its occurrence, the instance it sits in, and
    /// its path below its top-level occurrence.
    struct Pending<'a> {
        occurrence: &'a OccurrenceId,
        parent: Option<OccurrenceId>,
        top: Option<&'a Occurrence>,
        path: Vec<OccurrenceId>,
    }
    let occurrence_at = occurrences
        .iter()
        .map(|occurrence| (occurrence.id.as_str(), occurrence))
        .collect::<HashMap<_, _>>();
    let definition_at = definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| (definition.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut placements = vec![Vec::new(); definitions.len()];
    for (index, occurrence) in occurrences.iter().enumerate() {
        if let Some(&definition) = definition_at.get(occurrence.prototype.as_str()) {
            placements[definition].push(index);
        }
    }
    let mut assigned = BTreeMap::new();
    for (definition, row) in definitions.iter().enumerate() {
        if placements[definition].len() < 2 {
            continue;
        }
        for child in &row.occurrences {
            let Some(&child) = occurrence_at.get(child.as_str()) else {
                continue;
            };
            if let Some(placement) =
                assignment(child, &placements[definition], occurrences, instance_scopes)
            {
                assigned.insert(&child.id, &occurrences[placement].id);
            }
        }
    }
    let mut expanded = Expanded {
        definitions: definitions.to_vec(),
        occurrences: Vec::new(),
        occurrence_of: BTreeMap::new(),
        parents: BTreeMap::new(),
    };
    for definition in &mut expanded.definitions {
        if !definition.is_root() {
            definition.occurrences.clear();
        }
    }
    let mut stack = definitions
        .iter()
        .filter(|definition| definition.is_root())
        .flat_map(|definition| &definition.occurrences)
        .map(|occurrence| Pending {
            occurrence,
            parent: None,
            top: None,
            path: Vec::new(),
        })
        .collect::<Vec<_>>();
    stack.reverse();
    let mut seen = BTreeSet::new();
    while let Some(pending) = stack.pop() {
        let Some(&occurrence) = occurrence_at.get(pending.occurrence.as_str()) else {
            continue;
        };
        let instance = match &pending.parent {
            None => occurrence.id.clone(),
            Some(parent) => {
                let ((_, own), (namespace, parent)) =
                    (identity_parts(&occurrence.id)?, identity_parts(parent)?);
                mint(format!("{namespace}#{parent}:{own}"))?
            }
        };
        if !seen.insert(instance.clone()) {
            continue;
        }
        if cadmpeg_core::decode::u64_from_index(seen.len()) > limits.max_entities {
            return Err(CodecError::malformed(
                "the expanded F3D instance tree exceeds the entity limit",
            ));
        }
        let mut copy = occurrence.clone();
        copy.id = instance.clone();
        copy.overrides.clear();
        if let Some(entry) = pending.top.and_then(|top| {
            top.overrides.iter().find(|entry| {
                entry.path.0.len() == pending.path.len()
                    && entry.path.0.iter().zip(&pending.path).all(|(step, id)| {
                        matches!(&step.occurrence, StepRef::Occurrence(step) if step == id)
                    })
            })
        }) {
            copy.transform = entry.transform.or(copy.transform);
            copy.visible = entry.visible.or(copy.visible);
            copy.suppressed = entry.suppressed.or(copy.suppressed);
            copy.representation = entry.representation.clone().or(copy.representation.take());
        }
        if pending.parent.is_some() {
            if let Some(&owner) = cadmpeg_ir::products::owner(definitions, &occurrence.id)
                .and_then(|owner| definition_at.get(owner.id.as_str()))
            {
                expanded.definitions[owner]
                    .occurrences
                    .push(instance.clone());
            }
        }
        expanded
            .occurrence_of
            .insert(instance.clone(), occurrence.id.clone());
        if let Some(parent) = pending.parent {
            expanded.parents.insert(instance.clone(), parent);
        }
        expanded.occurrences.push(copy);
        let top = pending.top.unwrap_or(occurrence);
        if let Some(&prototype) = definition_at.get(occurrence.prototype.as_str()) {
            for child in definitions[prototype].occurrences.iter().rev() {
                if !belongs_to_placement(assigned.get(child).copied(), &occurrence.id, None) {
                    continue;
                }
                let mut path = pending.path.clone();
                path.push(child.clone());
                stack.push(Pending {
                    occurrence: child,
                    parent: Some(instance.clone()),
                    top: Some(top),
                    path,
                });
            }
        }
    }
    Ok(expanded)
}

/// The products a decode states from projected local and external structure.
#[cfg(test)]
pub(crate) fn stated(
    local: Projected,
    external: Option<crate::xref::ExternalOccurrenceProjection>,
) -> Model {
    let mut ir = cadmpeg_ir::CadIr::empty();
    assemble(&mut ir.model, local, external);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    own_once(&ctx, &mut ir, &BTreeMap::new()).expect("own once");
    ir.model
}

/// The world placement of each placed instance of `model`, by the identity
/// [`expand`] gives it.
#[cfg(test)]
pub(crate) fn world_poses(model: &Model) -> BTreeMap<OccurrenceId, Transform> {
    let expanded = expand(
        &cadmpeg_core::decode::ResourceLimits::default(),
        &model.product_definitions,
        &model.occurrences,
        &std::collections::BTreeMap::new(),
    )
    .expect("expand");
    let parents = &expanded.parents;
    let local = expanded
        .occurrences
        .iter()
        .map(|occurrence| (&occurrence.id, occurrence.transform.unwrap_or_default()))
        .collect::<BTreeMap<_, _>>();
    local
        .keys()
        .map(|&id| {
            let mut world = local[id];
            let mut current = id;
            while let Some(parent) = parents.get(current) {
                world = local[parent].compose(world).expect("compose");
                current = parent;
            }
            (id.clone(), world)
        })
        .collect()
}

/// The definitions that state `parents` (the position of the occurrence each
/// occurrence sits in, `None` at the top level) as the decoder builds the
/// expanded tree: each occurrence places a definition of its own, which
/// lists the occurrences that sit in it.
#[cfg(test)]
pub(crate) fn owned_by(
    occurrences: &mut [Occurrence],
    parents: &[Option<usize>],
) -> Vec<ProductDefinition> {
    let mut model = Model::default();
    for occurrence in occurrences.iter_mut() {
        occurrence.prototype =
            ProductDefinitionId::mint(format!("{}/definition", occurrence.id)).expect("identity");
        model.product_definitions.push(ProductDefinition::new(
            occurrence.prototype.clone(),
            Storage::Embedded,
        ));
    }
    model.occurrences = occurrences.to_vec();
    for (index, parent) in parents.iter().enumerate() {
        let parent = parent.map(|parent| occurrences[parent].id.clone());
        place_under(&mut model, parent.as_ref(), occurrences[index].id.clone());
    }
    model.product_definitions
}

/// The occurrences the definition `owner` lists.
#[cfg(test)]
pub(crate) fn listed<'m>(model: &'m Model, owner: &ProductDefinitionId) -> Vec<&'m OccurrenceId> {
    model
        .product_definitions
        .iter()
        .find(|definition| &definition.id == owner)
        .map(|definition| definition.occurrences.iter().collect())
        .unwrap_or_default()
}

/// The definition that lists `occurrence`.
#[cfg(test)]
pub(crate) fn owner_of<'m>(
    model: &'m Model,
    occurrence: &OccurrenceId,
) -> Option<&'m ProductDefinition> {
    cadmpeg_ir::products::owner(&model.product_definitions, occurrence)
}

#[cfg(test)]
impl From<crate::design::components::LocalComponentProjection> for Projected {
    fn from(local: crate::design::components::LocalComponentProjection) -> Self {
        Self {
            definitions: local.definitions,
            occurrences: local.occurrences,
            placements: local.placements,
        }
    }
}

#[cfg(test)]
mod tests;
