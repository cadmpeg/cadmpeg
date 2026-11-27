// SPDX-License-Identifier: Apache-2.0
//! Decode sets (part 10, Scope and linkage; §2.2, §3.1, §8.1): the scope
//! qualification of ids, and the keys of `SourceFidelity.file_digests`.

use std::collections::BTreeSet;

use crate::document::CadIr;
use crate::products::{ProductDefinition, Storage};
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::source_fidelity::SourceFidelity;

/// The definition key of an id: the key of a `File` definition's id is the
/// text after its `#`.
fn definition_key(definition: &ProductDefinition) -> Option<&str> {
    definition.id.as_str().split_once('#').map(|(_, key)| key)
}

/// The keys of the document's component files: the `File` definitions with
/// a source. The root definition qualifies no record (§2.2).
pub(in crate::validate) fn file_keys(ir: &CadIr) -> BTreeSet<&str> {
    ir.model
        .product_definitions
        .iter()
        .filter(|definition| is_component_file(definition))
        .filter_map(definition_key)
        .collect()
}

/// Whether `definition` is read from a component file of the decode set.
fn is_component_file(definition: &ProductDefinition) -> bool {
    matches!(
        definition.storage,
        Storage::File {
            source: Some(_),
            ..
        }
    )
}

/// The definition key that qualifies an id's `<scope>`: the text before the
/// scope's `@`, where it holds one.
fn qualifier(id: &str) -> Option<&str> {
    let scope = id.split(':').nth(1)?;
    scope.split_once('@').map(|(key, _)| key)
}

/// Every scope that holds `@` begins with the key of a `File` definition of
/// the document (§2.2).
pub(in crate::validate) fn check_qualified_scope(
    file_keys: &BTreeSet<&str>,
    id: &str,
    findings: &mut Vec<Finding>,
) {
    if let Some(key) = qualifier(id) {
        if !file_keys.contains(key) {
            crate::validate::record_finding(
                findings,
                Check::Identity,
                Severity::Error,
                Some(id),
                crate::rule!("products.entity-id-scope-key-names-no-file",
                    "entity id's scope is qualified by `{key}@`, the key of no `File` definition of the document"
                ),
            );
        }
    }
}

/// The scope of every id of a record read from a component file begins with
/// that file's definition key and `@` (§2.2, §3.1): the records of the
/// file's design scope, and the bodies and records its definition owns. The
/// `File` definition is a record of the placing file.
pub(in crate::validate) fn check_component_records(
    records: &crate::validate::design::records::DesignRecords<'_>,
    findings: &mut Vec<Finding>,
) {
    let ir = records.ir;
    let files = ir
        .model
        .product_definitions
        .iter()
        .filter(|definition| is_component_file(definition))
        .filter_map(|definition| Some((definition, definition_key(definition)?)))
        .collect::<Vec<_>>();
    if files.is_empty() {
        return;
    }
    let topology = TopologyOwners::new(ir);
    for (file, key) in files {
        let mut owned = ComponentRecords::default();
        for spanned in records
            .spanned_definitions(file.id.as_str())
            .iter()
            .copied()
        {
            let Some(&definition) = records.definitions.get(spanned) else {
                continue;
            };
            if definition.id != file.id {
                owned.push(definition.id.as_str());
                // A file the component file places owns its own records.
                if is_component_file(definition) {
                    continue;
                }
            }
            owned.definition(definition);
        }
        owned.design_scope(ir, records, file);
        topology.extend(&mut owned);
        for id in owned.ids {
            if qualifier(id) != Some(key) {
                crate::validate::record_finding(
                    findings,
                    Check::Identity,
                    Severity::Error,
                    Some(id),
                    crate::rule!("products.component-record-scope-unqualified",
                        "id of a record read from the component file of `{}` does not begin its scope with that file's key `{key}@`",
                        file.id
                    ),
                );
            }
        }
    }
}

/// The ids of the records read from one component file.
#[derive(Default)]
struct ComponentRecords<'a> {
    ids: BTreeSet<&'a str>,
    bodies: BTreeSet<&'a str>,
}

impl<'a> ComponentRecords<'a> {
    fn push(&mut self, id: &'a str) {
        self.ids.insert(id);
    }

    /// The bodies, sketches, and occurrences a definition of the file owns.
    fn definition(&mut self, definition: &'a ProductDefinition) {
        for body in &definition.bodies {
            self.push(body.as_str());
            self.bodies.insert(body.as_str());
        }
        for sketch in &definition.sketches {
            self.push(sketch.as_str());
        }
        for occurrence in &definition.occurrences {
            self.push(occurrence.as_str());
        }
    }

    /// The records of the design scope of `file`: the containers below its
    /// roots and their features, the sketches those features hold with their
    /// entities and constraints, and the records keyed to the scope.
    fn design_scope(
        &mut self,
        ir: &'a CadIr,
        records: &crate::validate::design::records::DesignRecords<'a>,
        file: &ProductDefinition,
    ) {
        use crate::design::container::Container;
        use crate::design::refs::Member;

        let Some(&scope) = records.scopes.get(file.id.as_str()) else {
            return;
        };
        let mut features = BTreeSet::new();
        let mut visited = BTreeSet::new();
        let mut pending = scope
            .roots
            .iter()
            .map(crate::design::ids::ContainerId::as_str)
            .collect::<Vec<_>>();
        while let Some(container) = pending.pop() {
            if !visited.insert(container) {
                continue;
            }
            self.push(container);
            let members: Box<dyn Iterator<Item = &Member>> =
                match records.containers.get(container).copied() {
                    Some(Container::Ordered { members, .. }) => Box::new(members.iter()),
                    Some(Container::Unordered { members, .. }) => Box::new(members.iter()),
                    None => continue,
                };
            for member in members {
                match member {
                    Member::Feature(feature) => {
                        self.push(feature.as_str());
                        features.insert(feature.as_str());
                    }
                    Member::Container(child) => pending.push(child.as_str()),
                }
            }
        }
        let design = &ir.design;
        let sketches = design
            .sketches
            .iter()
            .filter(|sketch| features.contains(sketch.owner.as_str()))
            .map(|sketch| sketch.id.as_str())
            .collect::<BTreeSet<_>>();
        let spatial = design
            .spatial_sketches
            .iter()
            .filter(|sketch| features.contains(sketch.owner.as_str()))
            .map(|sketch| sketch.id.as_str())
            .collect::<BTreeSet<_>>();
        let planar_records = design
            .sketch_entities
            .iter()
            .filter(|entity| sketches.contains(entity.sketch.as_str()))
            .map(|entity| entity.id.as_str())
            .chain(
                design
                    .sketch_constraints
                    .iter()
                    .filter(|constraint| sketches.contains(constraint.sketch.as_str()))
                    .map(|constraint| constraint.id.as_str()),
            );
        let spatial_records = design
            .spatial_sketch_entities
            .iter()
            .filter(|entity| spatial.contains(entity.sketch.as_str()))
            .map(|entity| entity.id.as_str())
            .chain(
                design
                    .spatial_sketch_constraints
                    .iter()
                    .filter(|constraint| spatial.contains(constraint.sketch.as_str()))
                    .map(|constraint| constraint.id.as_str()),
            );
        let held = sketches
            .iter()
            .chain(&spatial)
            .copied()
            .chain(planar_records)
            .chain(spatial_records)
            .collect::<Vec<_>>();
        self.ids.extend(held);
        let definition = &file.id;
        let keyed = design
            .parameters
            .iter()
            .filter(|row| row.scope == *definition)
            .map(|row| row.id.as_str())
            .chain(
                design
                    .evaluations
                    .iter()
                    .filter(|row| row.scope == *definition)
                    .map(|row| row.id.as_str()),
            )
            .chain(
                design
                    .configurations
                    .iter()
                    .filter(|row| row.scope == *definition)
                    .map(|row| row.id.as_str()),
            )
            .chain(
                design
                    .arrangements
                    .iter()
                    .filter(|row| row.scope == *definition)
                    .map(|row| row.id.as_str()),
            )
            .chain(
                design
                    .publications
                    .iter()
                    .filter(|row| row.scope == *definition)
                    .map(|row| row.id.as_str()),
            )
            .chain(
                design
                    .programs
                    .iter()
                    .filter(|row| row.scope == *definition)
                    .map(|row| row.id.as_str()),
            )
            .chain(
                design
                    .sheet_metal_rules
                    .iter()
                    .filter(|row| row.scope == *definition)
                    .map(|row| row.id.as_str()),
            )
            .collect::<Vec<_>>();
        self.ids.extend(keyed);
    }
}

/// The topology records below each body, by the parent pointers of the
/// records (a region's body, a shell's region, a face's shell, a loop's
/// face, a coedge's loop), with the edges, vertices, and carriers they name.
struct TopologyOwners<'a> {
    ir: &'a CadIr,
    children: std::collections::HashMap<&'a str, Vec<&'a str>>,
    edges: std::collections::HashMap<&'a str, &'a crate::topology::Edge>,
    vertices: std::collections::HashMap<&'a str, &'a crate::topology::Vertex>,
}

impl<'a> TopologyOwners<'a> {
    fn new(ir: &'a CadIr) -> Self {
        let geometry = &ir.geometry;
        let mut children = std::collections::HashMap::<&str, Vec<&str>>::new();
        let mut child = |parent: &'a str, id: &'a str| children.entry(parent).or_default().push(id);
        for region in &geometry.regions {
            child(region.body.as_str(), region.id.as_str());
        }
        for shell in &geometry.shells {
            child(shell.region.as_str(), shell.id.as_str());
        }
        for face in &geometry.faces {
            child(face.shell.as_str(), face.id.as_str());
        }
        for item in &geometry.loops {
            child(item.face.as_str(), item.id.as_str());
        }
        for coedge in &geometry.coedges {
            child(coedge.owner_loop.as_str(), coedge.id.as_str());
        }
        Self {
            ir,
            children,
            edges: geometry
                .edges
                .iter()
                .map(|edge| (edge.id.as_str(), edge))
                .collect(),
            vertices: geometry
                .vertices
                .iter()
                .map(|vertex| (vertex.id.as_str(), vertex))
                .collect(),
        }
    }

    /// Adds to `owned` the topology records below its bodies.
    fn extend(&self, owned: &mut ComponentRecords<'a>) {
        let geometry = &self.ir.geometry;
        let mut below = BTreeSet::new();
        let mut pending = owned.bodies.iter().copied().collect::<Vec<_>>();
        while let Some(id) = pending.pop() {
            if below.insert(id) {
                pending.extend(self.children.get(id).into_iter().flatten().copied());
            }
        }
        let mut edges = BTreeSet::new();
        let mut vertices = BTreeSet::new();
        for shell in geometry
            .shells
            .iter()
            .filter(|row| below.contains(row.id.as_str()))
        {
            edges.extend(shell.wire_edges().map(crate::ids::EdgeId::as_str));
            vertices.extend(shell.free_vertices().map(crate::ids::VertexId::as_str));
        }
        for face in geometry
            .faces
            .iter()
            .filter(|row| below.contains(row.id.as_str()))
        {
            owned.push(face.surface.as_str());
        }
        for coedge in geometry
            .coedges
            .iter()
            .filter(|row| below.contains(row.id.as_str()))
        {
            edges.insert(coedge.edge.as_str());
            for pcurve in &coedge.pcurves {
                owned.push(pcurve.pcurve.as_str());
            }
        }
        for edge in edges.iter().filter_map(|id| self.edges.get(id)) {
            owned.push(edge.id.as_str());
            if let Some(curve) = edge.carrier.curve() {
                owned.push(curve.as_str());
            }
            vertices.extend(edge.vertices().map(crate::ids::VertexId::as_str));
        }
        for vertex in vertices.iter().filter_map(|id| self.vertices.get(id)) {
            owned.push(vertex.id.as_str());
            owned.push(vertex.point.as_str());
        }
        owned.ids.extend(below);
    }
}

/// Every key of `SourceFidelity.file_digests` names a `File` definition
/// read from a component file; the root definition has none (§8.1).
pub(in crate::validate) fn check_file_digests(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    findings: &mut Vec<Finding>,
) {
    for key in fidelity.file_digests.keys() {
        let storage = ir
            .model
            .product_definitions
            .iter()
            .find(|definition| definition.id == *key)
            .map(|definition| &definition.storage);
        let mut report = |ruled: crate::report::check::Ruled<'_>| {
            crate::validate::record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(key.as_str()),
                ruled,
            );
        };
        match storage {
            Some(Storage::File {
                source: Some(_), ..
            }) => {}
            Some(Storage::File { source: None, .. }) => report(crate::rule!(
                "products.file-digest-of-root",
                "the sidecar states a file digest for the root definition, whose file `decode_digest` covers"
            )),
            Some(Storage::Embedded | Storage::External(_)) => report(crate::rule!(
                "products.file-digest-of-no-file",
                "the sidecar states a file digest for a definition read from no file of its own"
            )),
            None => report(crate::rule!(
                "products.file-digest-of-no-definition",
                "the sidecar states a file digest for no product definition of the document"
            )),
        }
    }
}

#[cfg(test)]
mod tests;
