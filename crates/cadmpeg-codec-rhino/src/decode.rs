// SPDX-License-Identifier: Apache-2.0
//! Decode Rhino metadata and retain object records for later geometry phases.

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_ir::annotations::{ExactnessNote, StreamProvenance};
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::draft::{ModelCheckpoint, ModelDraft};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::{DecodeReport, LossNote, Severity};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Color, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::{NativeUnknownRecord, UnknownRecord};
use cadmpeg_ir::SourceProvenance;
use cadmpeg_ir::{Exactness, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

use crate::chunks::ArchiveVersion;
use crate::container::{OpaqueRecord, Scan};
use crate::loss::RhinoLossCode;
use crate::objects::ObjectDescriptor;

/// Maximum bytes retained for one Rhino object record.
pub(crate) const RETAINED_RECORD_CAP: usize = 16 * 1024 * 1024;
/// Maximum bytes retained across all Rhino object records in one document.
pub(crate) const RETAINED_DOCUMENT_CAP: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
struct ClassOutcome {
    decoded: usize,
    retained: usize,
    native_retained: usize,
    native_code: Option<RhinoLossCode>,
    attribute_degraded: usize,
    failed_framed: usize,
    first_offset: u64,
    first_object_type: u32,
}

/// Outcome of resolving one foreign object UUID against the object table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjectReference {
    /// Exactly one record owns the UUID; the value is its source order.
    Resolved(usize),
    /// No record owns the UUID.
    Missing,
    /// Several records own the UUID, so no single record can be selected.
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeometryStatus {
    Retained,
    NativeRetained,
    Decoded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArenaLengths {
    bodies: usize,
    regions: usize,
    shells: usize,
    faces: usize,
    loops: usize,
    coedges: usize,
    edges: usize,
    vertices: usize,
    points: usize,
    curves: usize,
    pcurves: usize,
    surfaces: usize,
    subds: usize,
    tessellations: usize,
    procedural_curves: usize,
    procedural_surfaces: usize,
    features: usize,
    parameters: usize,
    semantic_annotations: usize,
}

#[derive(Debug)]
struct AnnotationCheckpoint {
    stream_count: usize,
    provenance: BTreeMap<String, StreamProvenance>,
    exactness: BTreeMap<String, ExactnessNote>,
}

impl AnnotationCheckpoint {
    fn capture(annotations: &cadmpeg_ir::Annotations) -> Self {
        Self {
            stream_count: annotations.streams.len(),
            provenance: annotations.provenance.clone(),
            exactness: annotations.exactness.clone(),
        }
    }

    fn rollback(self, annotations: &mut cadmpeg_ir::Annotations) {
        annotations.streams.truncate(self.stream_count);
        annotations.provenance = self.provenance;
        annotations.exactness = self.exactness;
    }
}

impl ArenaLengths {
    const EMPTY: Self = Self {
        bodies: 0,
        regions: 0,
        shells: 0,
        faces: 0,
        loops: 0,
        coedges: 0,
        edges: 0,
        vertices: 0,
        points: 0,
        curves: 0,
        pcurves: 0,
        surfaces: 0,
        subds: 0,
        tessellations: 0,
        procedural_curves: 0,
        procedural_surfaces: 0,
        features: 0,
        parameters: 0,
        semantic_annotations: 0,
    };

    fn capture(ir: &CadIr) -> Self {
        Self {
            bodies: ir.model.bodies.len(),
            regions: ir.model.regions.len(),
            shells: ir.model.shells.len(),
            faces: ir.model.faces.len(),
            loops: ir.model.loops.len(),
            coedges: ir.model.coedges.len(),
            edges: ir.model.edges.len(),
            vertices: ir.model.vertices.len(),
            points: ir.model.points.len(),
            curves: ir.model.curves.len(),
            pcurves: ir.model.pcurves.len(),
            surfaces: ir.model.surfaces.len(),
            subds: ir.model.subds.len(),
            tessellations: ir.model.tessellations.len(),
            procedural_curves: ir.model.procedural_curves.len(),
            procedural_surfaces: ir.model.procedural_surfaces.len(),
            features: ir.model.features.len(),
            parameters: ir.model.parameters.len(),
            semantic_annotations: ir.model.semantic_annotations.len(),
        }
    }

    fn truncate(self, ir: &mut CadIr) {
        ir.model.bodies.truncate(self.bodies);
        ir.model.regions.truncate(self.regions);
        ir.model.shells.truncate(self.shells);
        ir.model.faces.truncate(self.faces);
        ir.model.loops.truncate(self.loops);
        ir.model.coedges.truncate(self.coedges);
        ir.model.edges.truncate(self.edges);
        ir.model.vertices.truncate(self.vertices);
        ir.model.points.truncate(self.points);
        ir.model.curves.truncate(self.curves);
        ir.model.pcurves.truncate(self.pcurves);
        ir.model.surfaces.truncate(self.surfaces);
        ir.model.subds.truncate(self.subds);
        ir.model.tessellations.truncate(self.tessellations);
        ir.model.procedural_curves.truncate(self.procedural_curves);
        ir.model
            .procedural_surfaces
            .truncate(self.procedural_surfaces);
        ir.model.features.truncate(self.features);
        ir.model.parameters.truncate(self.parameters);
        ir.model
            .semantic_annotations
            .truncate(self.semantic_annotations);
    }

    fn added_since(self, before: Self) -> Option<usize> {
        [
            (self.bodies, before.bodies),
            (self.regions, before.regions),
            (self.shells, before.shells),
            (self.faces, before.faces),
            (self.loops, before.loops),
            (self.coedges, before.coedges),
            (self.edges, before.edges),
            (self.vertices, before.vertices),
            (self.points, before.points),
            (self.curves, before.curves),
            (self.pcurves, before.pcurves),
            (self.surfaces, before.surfaces),
            (self.subds, before.subds),
            (self.tessellations, before.tessellations),
            (self.procedural_curves, before.procedural_curves),
            (self.procedural_surfaces, before.procedural_surfaces),
            (self.features, before.features),
            (self.parameters, before.parameters),
            (self.semantic_annotations, before.semantic_annotations),
        ]
        .into_iter()
        .try_fold(0_usize, |total, (after, before)| {
            total.checked_add(after.checked_sub(before)?)
        })
    }

    fn appended_ids(self, ir: &CadIr) -> Option<BTreeSet<String>> {
        let after = Self::capture(ir);
        after.added_since(self)?;
        let mut ids = BTreeSet::new();
        ids.extend(
            ir.model.bodies[self.bodies..]
                .iter()
                .map(|entity| entity.id.0.clone()),
        );
        ids.extend(
            ir.model.regions[self.regions..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.shells[self.shells..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.faces[self.faces..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.loops[self.loops..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.coedges[self.coedges..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.edges[self.edges..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.vertices[self.vertices..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.points[self.points..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.curves[self.curves..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.pcurves[self.pcurves..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.surfaces[self.surfaces..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.subds[self.subds..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.tessellations[self.tessellations..]
                .iter()
                .map(|entity| entity.id.clone()),
        );
        ids.extend(
            ir.model.procedural_curves[self.procedural_curves..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.procedural_surfaces[self.procedural_surfaces..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.features[self.features..]
                .iter()
                .map(|entity| entity.id.to_string()),
        );
        ids.extend(
            ir.model.parameters[self.parameters..]
                .iter()
                .map(|entity| entity.id.0.clone()),
        );
        ids.extend(
            ir.model.semantic_annotations[self.semantic_annotations..]
                .iter()
                .map(|entity| entity.id.0.clone()),
        );
        Some(ids)
    }

    fn remove_ids(ir: &mut CadIr, ids: &BTreeSet<String>) {
        ir.model.bodies.retain(|entity| !ids.contains(&entity.id.0));
        ir.model
            .regions
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .shells
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .faces
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .loops
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .coedges
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .edges
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .vertices
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .points
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .curves
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .pcurves
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .surfaces
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .subds
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .tessellations
            .retain(|entity| !ids.contains(&entity.id));
        ir.model
            .procedural_curves
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .procedural_surfaces
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .features
            .retain(|entity| !ids.contains(&entity.id.to_string()));
        ir.model
            .parameters
            .retain(|entity| !ids.contains(&entity.id.0));
        ir.model
            .semantic_annotations
            .retain(|entity| !ids.contains(&entity.id.0));
    }
}

const MAX_INSTANCE_REFERENCES: usize = 1 << 20;
const MAX_INSTANCE_MEMBERS: usize = 1 << 20;
const MAX_INSTANCE_ENTITIES: usize = 1 << 20;

#[derive(Debug, Clone, Copy)]
struct ExpansionBudget {
    references: usize,
    members: usize,
    entities: usize,
    limits: [usize; 3],
}

impl ExpansionBudget {
    fn new() -> Self {
        Self {
            references: 0,
            members: 0,
            entities: 0,
            limits: [
                MAX_INSTANCE_REFERENCES,
                MAX_INSTANCE_MEMBERS,
                MAX_INSTANCE_ENTITIES,
            ],
        }
    }

    fn from_session(ctx: &cadmpeg_core::decode::DecodeContext<'_>) -> Self {
        let collections =
            usize::try_from(ctx.policy().limits.max_collection_items).unwrap_or(usize::MAX);
        let entities = usize::try_from(ctx.policy().limits.max_entities).unwrap_or(usize::MAX);
        let mut budget = Self::new();
        budget.limits = [
            collections.min(MAX_INSTANCE_REFERENCES),
            collections.min(MAX_INSTANCE_MEMBERS),
            entities.min(MAX_INSTANCE_ENTITIES),
        ];
        budget
    }

    fn charge(value: &mut usize, amount: usize, limit: usize, label: &str) -> Result<(), String> {
        *value = value
            .checked_add(amount)
            .filter(|value| *value <= limit)
            .ok_or_else(|| format!("document instance {label} budget exceeded"))?;
        Ok(())
    }

    fn reference(&mut self) -> Result<(), String> {
        Self::charge(&mut self.references, 1, self.limits[0], "reference")
    }

    fn member(&mut self) -> Result<(), String> {
        Self::charge(&mut self.members, 1, self.limits[1], "member")
    }

    fn entities(&mut self, amount: usize) -> Result<(), String> {
        Self::charge(&mut self.entities, amount, self.limits[2], "entity")
    }
}

/// Mutable decode state shared by metadata and geometry phases.
#[derive(Clone)]
pub(crate) struct DecodeContext<'a> {
    scan: &'a Scan<'a>,
    expand: crate::mesh::MeshExpand<'a>,
    ir: CadIr,
    annotations: cadmpeg_ir::Annotations,
    unknowns: Vec<UnknownRecord>,
    opaque_records: Vec<UnknownRecord>,
    statuses: Vec<GeometryStatus>,
    outcomes: BTreeMap<String, ClassOutcome>,
    retained_bytes: usize,
    retention_limits: [usize; 2],
    mesh_budget: crate::mesh::MeshBudget,
    geometry_transferred: bool,
    phase_warnings: Vec<String>,
    /// Typed loss notes from semantic conversion; drain into report `losses`.
    typed_losses: Vec<LossNote>,
    selected_object: Option<usize>,
    instance_key: Option<String>,
    instance_path: Vec<String>,
    instance_color: Option<Color>,
    instance_visible: Option<bool>,
    object_candidates: BTreeMap<crate::wire::Uuid, Vec<usize>>,
    definition_candidates: BTreeMap<crate::wire::Uuid, usize>,
    expansion_budget: ExpansionBudget,
}

impl<'a> DecodeContext<'a> {
    /// Starts a transaction from a completed Rhino scan.
    pub(crate) fn new(scan: &'a Scan<'a>, expand: crate::mesh::MeshExpand<'a>) -> Self {
        let mut object_candidates = BTreeMap::new();
        for (source_order, object) in scan.objects.iter().enumerate() {
            if let Some(identity) = &object.identity {
                object_candidates
                    .entry(identity.object_id)
                    .or_insert_with(Vec::new)
                    .push(source_order);
            }
        }
        let mut context = Self {
            scan,
            expand,
            ir: build_ir(scan),
            annotations: cadmpeg_ir::Annotations::default(),
            unknowns: Vec::with_capacity(scan.objects.len()),
            opaque_records: Vec::new(),
            statuses: Vec::with_capacity(scan.objects.len()),
            outcomes: BTreeMap::new(),
            retained_bytes: 0,
            retention_limits: [RETAINED_RECORD_CAP, RETAINED_DOCUMENT_CAP],
            mesh_budget: crate::mesh::MeshBudget::from_session(expand.ctx()),
            geometry_transferred: false,
            phase_warnings: Vec::new(),
            typed_losses: Vec::new(),
            selected_object: None,
            instance_key: None,
            instance_path: Vec::new(),
            instance_color: None,
            instance_visible: None,
            object_candidates,
            definition_candidates: scan
                .definitions
                .definitions
                .iter()
                .enumerate()
                .filter(|(_, definition)| !scan.definitions.ambiguous_ids.contains(&definition.id))
                .map(|(index, definition)| (definition.id, index))
                .collect(),
            expansion_budget: ExpansionBudget::from_session(expand.ctx()),
        };
        context.retain_object_records();
        context.retain_opaque_records();
        context
    }

    #[cfg(test)]
    pub(crate) fn set_expansion_limits(&mut self, limits: [usize; 3]) {
        self.expansion_budget.limits = limits;
    }

    #[cfg(test)]
    pub(crate) fn set_retention_limits(&mut self, record: usize, document: usize) {
        self.retention_limits = [record, document];
        self.unknowns.clear();
        self.opaque_records.clear();
        self.statuses.clear();
        self.outcomes.clear();
        self.retained_bytes = 0;
        self.retain_object_records();
        self.retain_opaque_records();
    }

    /// Returns the document mesh budget's retained-byte count.
    #[cfg(test)]
    pub(crate) fn mesh_budget_used(&self) -> usize {
        self.mesh_budget.used()
    }

    /// Returns the source archive version.
    pub(crate) fn archive(&self) -> ArchiveVersion {
        self.scan.archive
    }

    /// Returns the native-to-millimeter scale when the source declares one.
    pub(crate) fn unit_scale(&self) -> Option<f64> {
        let units = self.scan.metadata.settings.units.as_ref()?;
        match units.unit {
            crate::settings::UnitSystem::None => Some(1.0),
            _ => units.millimeters_per_unit,
        }
    }

    /// Looks up a scanned object by deterministic source order.
    #[cfg(test)]
    pub(crate) fn object(&self, source_order: usize) -> Option<&ObjectDescriptor> {
        self.scan.objects.get(source_order)
    }

    /// Looks up the retained unknown record for a source-order object.
    #[cfg(test)]
    pub(crate) fn unknown(&self, source_order: usize) -> Option<&UnknownRecord> {
        self.unknowns.get(source_order)
    }

    #[cfg(test)]
    pub(crate) fn unknown_mut(&mut self, source_order: usize) -> Option<&mut UnknownRecord> {
        self.unknowns.get_mut(source_order)
    }

    #[cfg(test)]
    pub(crate) fn unknown_count(&self) -> usize {
        self.unknowns.len()
    }

    /// Appends a later geometry-phase link to an object record.
    pub(crate) fn append_link(&mut self, source_order: usize, link: String) -> bool {
        let Some(record) = self.unknowns.get_mut(source_order) else {
            return false;
        };
        if link == record.id.to_string() {
            return false;
        }
        if let Err(index) = record.links.binary_search(&link) {
            record.links.insert(index, link);
        }
        true
    }

    fn append_links(&mut self, source_order: usize, links: &[String]) -> bool {
        let Some(record) = self.unknowns.get_mut(source_order) else {
            return false;
        };
        append_links_to_record(record, links);
        true
    }

    fn validate_candidate<T>(
        &mut self,
        apply: impl FnOnce(&mut CadIr, &mut cadmpeg_ir::Annotations) -> T,
    ) -> Result<T, String> {
        self.validate_candidate_fallible(|ir, annotations| Ok(apply(ir, annotations)))
    }

    fn validate_candidate_fallible<T>(
        &mut self,
        apply: impl FnOnce(&mut CadIr, &mut cadmpeg_ir::Annotations) -> Result<T, String>,
    ) -> Result<T, String> {
        let before = ArenaLengths::capture(&self.ir);
        let annotation_checkpoint = AnnotationCheckpoint::capture(&self.annotations);
        let value = match apply(&mut self.ir, &mut self.annotations) {
            Ok(value) => value,
            Err(error) => {
                before.truncate(&mut self.ir);
                annotation_checkpoint.rollback(&mut self.annotations);
                return Err(error);
            }
        };
        self.ir
            .set_native_unknowns_from("rhino", self.unknowns.iter().map(NativeUnknownRecord::from))
            .expect("Rhino unknown records serialize");
        let appended = before
            .appended_ids(&self.ir)
            .expect("Rhino candidate builders only append IR entities");
        let validation = cadmpeg_ir::admit_with_annotations(
            &self.ir,
            &self.annotations,
            cadmpeg_ir::RHINO_DRAFT_CHECKS,
            Vec::new(),
        );
        if validation.is_ok() {
            let unknowns = match self.ir.native_unknowns("rhino") {
                Ok(unknowns) => unknowns,
                Err(error) => {
                    before.truncate(&mut self.ir);
                    annotation_checkpoint.rollback(&mut self.annotations);
                    return Err(error.to_string());
                }
            };
            let mut link_updates = Vec::with_capacity(unknowns.len());
            for reference in unknowns {
                let Some(index) = self
                    .unknowns
                    .iter()
                    .position(|record| record.id == reference.id)
                else {
                    before.truncate(&mut self.ir);
                    annotation_checkpoint.rollback(&mut self.annotations);
                    return Err(format!("candidate introduced unknown {}", reference.id));
                };
                link_updates.push((index, reference.links));
            }
            if let Err(error) = self.expansion_budget.entities(appended.len()) {
                before.truncate(&mut self.ir);
                annotation_checkpoint.rollback(&mut self.annotations);
                return Err(error);
            }
            if let Err(error) = self.charge_session_entities(appended.len()) {
                before.truncate(&mut self.ir);
                annotation_checkpoint.rollback(&mut self.annotations);
                return Err(error);
            }
            for (index, links) in link_updates {
                self.unknowns[index].links = links;
            }
            self.ir.model.finalize();
            Ok(value)
        } else {
            before.truncate(&mut self.ir);
            annotation_checkpoint.rollback(&mut self.annotations);
            Err(validation_findings(&validation))
        }
    }

    /// Returns mutable IR for the current decode transaction.
    #[cfg(test)]
    pub(crate) fn ir_mut(&mut self) -> &mut CadIr {
        &mut self.ir
    }

    #[cfg(test)]
    pub(crate) fn reject_duplicate_entity_candidate(&mut self) -> String {
        self.ir.model.points.push(Point {
            id: "rhino:test:duplicate-point".into(),
            position: Point3::new(1.0, 2.0, 3.0),
            source_object: None,
        });
        let result = self.validate_candidate(|candidate, _annotations| {
            let point = Point {
                id: "rhino:test:duplicate-point".into(),
                position: Point3::new(0.0, 0.0, 0.0),
                source_object: None,
            };
            candidate.model.points.push(point);
        });
        result.expect_err("duplicate entity ID must fail validation")
    }

    /// Marks one retained object as successfully decoded.
    pub(crate) fn mark_decoded(&mut self, source_order: usize) -> bool {
        self.transition(source_order, GeometryStatus::Decoded)
    }

    /// Marks one framed object as failed after a skippable payload error.
    pub(crate) fn mark_failed(&mut self, source_order: usize) -> bool {
        self.transition(source_order, GeometryStatus::Failed)
    }

    /// Marks one object as read but retained as native passthrough.
    ///
    /// Record the native-retention loss code once per Rhino class.
    fn mark_native_retained(&mut self, source_order: usize, code: RhinoLossCode) -> bool {
        if !self.transition(source_order, GeometryStatus::NativeRetained) {
            return false;
        }
        let class = self.scan.objects[source_order].class_uuid.to_string();
        let outcome = self.outcomes.get_mut(&class).expect("status class exists");
        outcome.native_code = Some(code);
        true
    }

    /// Resolves one foreign object UUID to the single record that owns it.
    fn resolve_object(&self, id: crate::wire::Uuid) -> ObjectReference {
        match self
            .object_candidates
            .get(&id)
            .map_or(&[][..], Vec::as_slice)
        {
            [order] => ObjectReference::Resolved(*order),
            [] => ObjectReference::Missing,
            _ => ObjectReference::Ambiguous,
        }
    }

    /// Resolves foreign object UUIDs to the native identities of their records.
    ///
    /// The result is positional: index `i` holds the identity for `ids[i]`, or
    /// `None` when that UUID is nil, names no record, or names several. Every
    /// non-nil UUID that does not resolve is charged against `role`.
    fn resolve_object_records(
        &mut self,
        source_order: usize,
        role: &str,
        ids: &[crate::wire::Uuid],
    ) -> Vec<Option<String>> {
        let mut resolved = Vec::new();
        let mut charges = Vec::new();
        for id in ids {
            if id.is_nil() {
                resolved.push(None);
                continue;
            }
            match self.resolve_object(*id) {
                ObjectReference::Resolved(order) => {
                    resolved.push(Some(Self::mint_unknown_id(order).to_string()));
                }
                ObjectReference::Missing => {
                    resolved.push(None);
                    charges.push((RhinoLossCode::ReferenceMemberUnresolved, *id));
                }
                ObjectReference::Ambiguous => {
                    resolved.push(None);
                    charges.push((RhinoLossCode::ReferenceMemberAmbiguous, *id));
                }
            }
        }
        for (code, id) in charges {
            let note = code.note(format!(
                "{role} in object record {source_order} references object {id}"
            ));
            self.typed_losses.push(note);
        }
        resolved
    }

    /// Decode and atomically commit supported simple geometry.
    pub(crate) fn decode_geometry(&mut self) {
        if !object_geometry_archive(self.archive()) {
            return;
        }
        for source_order in 0..self.scan.objects.len() {
            if self
                .selected_object
                .is_some_and(|selected| selected != source_order)
            {
                continue;
            }
            let object = &self.scan.objects[source_order];
            if self.selected_object.is_none() && self.is_definition_member(object) {
                continue;
            }
            if crate::instances::is_reference_class(object.class_uuid) {
                self.expand_reference(source_order);
                continue;
            }
            if crate::subd::supported_class(object.class_uuid) {
                self.decode_subd(source_order, object);
                continue;
            }
            if crate::brep::supported_class(object.class_uuid) {
                self.decode_brep(source_order, object);
                continue;
            }
            if crate::extrusion::supported_class(object.class_uuid) {
                self.decode_extrusion(source_order, object);
                continue;
            }
            if object.class_uuid == crate::hatch::CLASS {
                self.decode_hatch(source_order, object);
                continue;
            }
            if object.class_uuid == crate::detail::CLASS {
                self.decode_detail(source_order, object);
                continue;
            }
            if object.class_uuid == crate::cage::CLASS {
                self.decode_cage(source_order, object);
                continue;
            }
            if object.class_uuid == crate::morph::CLASS {
                self.decode_morph(source_order, object);
                continue;
            }
            if object.class_uuid == crate::curve_on_surface::CLASS {
                self.decode_curve_on_surface(source_order, object);
                continue;
            }
            if object.class_uuid == crate::polyedge::CURVE_CLASS {
                self.decode_polyedge(source_order, object);
                continue;
            }
            if !crate::curves::supported_class(object.class_uuid)
                && !crate::mesh::supported_class(object.class_uuid)
            {
                continue;
            }
            let Some(scale) = self.unit_scale() else {
                self.scan_warning(
                    source_order,
                    "simple geometry retained because document units are unavailable",
                );
                continue;
            };
            if crate::mesh::supported_class(object.class_uuid) {
                let Some(identity) = object.identity.as_ref() else {
                    self.scan_warning(
                        source_order,
                        "mesh retained because identity is unavailable",
                    );
                    continue;
                };
                let key = self.object_key(identity, source_order);
                let decoded = crate::mesh::decode(
                    self.expand,
                    self.scan.data,
                    object.class_data_range.clone(),
                    self.archive(),
                    crate::mesh::MeshDecodeOptions {
                        writer_version: self.scan.metadata.properties.writer_version,
                        association: Some(self.source_association(identity)),
                        id: format!("rhino:object:tessellation#{key}"),
                        scale,
                        userdata: &object.userdata,
                    },
                    &mut self.mesh_budget,
                );
                match decoded {
                    Ok(mesh) => {
                        let proxy = object
                            .userdata
                            .iter()
                            .find(|extra| {
                                extra.class_uuid == crate::subd::SUBD_MESH_PROXY_USERDATA
                                    && extra.item_uuid == crate::subd::SUBD_MESH_PROXY_USERDATA
                            })
                            .cloned();
                        let mut proxy_transferred = false;
                        if let Some(extra) = proxy {
                            let subd_id: cadmpeg_ir::ids::SubdId =
                                format!("rhino:object:subd#{key}").into();
                            match crate::subd::decode_mesh_proxy(
                                self.scan.data,
                                &extra,
                                self.archive(),
                                scale,
                                subd_id,
                                mesh.proxy_fingerprint,
                            ) {
                                Ok(Some(crate::subd::DecodedSubd::Surface {
                                    surface,
                                    neutral_metadata,
                                    enum_diagnostics,
                                    warnings,
                                })) => {
                                    proxy_transferred = self.commit_subd_surface(
                                        source_order,
                                        *surface,
                                        neutral_metadata,
                                        enum_diagnostics,
                                        warnings,
                                        scale != 1.0,
                                    );
                                    if proxy_transferred {
                                        self.mark_decoded(source_order);
                                    } else {
                                        self.scan_warning(
                                            source_order,
                                            "valid SubD mesh proxy rejected by IR validation; parent mesh retained",
                                        );
                                    }
                                }
                                Ok(None) => self.scan_warning(
                                    source_order,
                                    "SubD mesh proxy failed its validity or parent-mesh identity checks; parent mesh retained",
                                ),
                                Err(error) => self.scan_warning(
                                    source_order,
                                    &format!("SubD mesh proxy dropped: {error}; parent mesh retained"),
                                ),
                                Ok(Some(crate::subd::DecodedSubd::Empty)) => unreachable!(
                                    "mesh proxy decoder does not admit an empty SubD"
                                ),
                            }
                        }
                        if !proxy_transferred && self.commit_mesh(source_order, mesh) {
                            self.mark_decoded(source_order);
                        } else if !proxy_transferred {
                            self.mark_failed(source_order);
                        }
                    }
                    Err(error) => {
                        let future = matches!(
                            error,
                            crate::curves::GeometryError::UnsupportedVersion { .. }
                        );
                        self.scan_warning(
                            source_order,
                            &format!(
                                "mesh {}: {error}",
                                if future { "retained" } else { "failed" }
                            ),
                        );
                        if !future {
                            self.mark_failed(source_order);
                        }
                    }
                }
                continue;
            }
            let decoded = crate::curves::decode(
                self.scan.data,
                object.class_uuid,
                object.class_data_range.clone(),
                scale,
                self.archive(),
            );
            let procedural_surface = crate::surfaces::is_procedural_class(object.class_uuid);
            match decoded {
                Ok(value) => {
                    if self.commit_geometry(source_order, value) {
                        self.mark_decoded(source_order);
                    } else if procedural_surface {
                        self.scan_warning(
                            source_order,
                            "procedural surface candidate rejected by IR validation",
                        );
                        self.commit_unknown_surface(source_order);
                    } else {
                        self.mark_failed(source_order);
                    }
                }
                Err(error) => {
                    let future = matches!(
                        error,
                        crate::curves::GeometryError::UnsupportedVersion { .. }
                    );
                    self.scan_warning(
                        source_order,
                        &format!(
                            "simple geometry {}: {error}",
                            if procedural_surface {
                                "degraded and retained"
                            } else if future {
                                "retained"
                            } else {
                                "failed"
                            }
                        ),
                    );
                    if procedural_surface {
                        self.commit_unknown_surface(source_order);
                    } else if !future {
                        self.mark_failed(source_order);
                    }
                }
            }
        }
    }

    /// Decode semantic dimensions independently of shape carriers.
    pub(crate) fn decode_dimensions(&mut self) {
        if !matches!(
            self.archive(),
            ArchiveVersion::V2
                | ArchiveVersion::V3
                | ArchiveVersion::V4
                | ArchiveVersion::V5
                | ArchiveVersion::V6
                | ArchiveVersion::V7
                | ArchiveVersion::V8
                | ArchiveVersion::V9
        ) {
            return;
        }
        for source_order in 0..self.scan.objects.len() {
            let object = &self.scan.objects[source_order];
            if !crate::dimensions::supported_class(object.class_uuid) {
                continue;
            }
            if self.is_definition_member(object) {
                self.scan_warning(
                    source_order,
                    "definition-member dimension retained because annotation instance expansion is unsupported",
                );
                continue;
            }
            let Some(scale) = self.unit_scale() else {
                self.scan_warning(
                    source_order,
                    "dimension retained because document units are unavailable",
                );
                continue;
            };
            let Some(identity) = object.identity.as_ref() else {
                self.scan_warning(
                    source_order,
                    "dimension retained because identity is unavailable",
                );
                continue;
            };
            let key = self.object_key(identity, source_order);
            match crate::dimensions::decode(
                self.scan.data,
                object.class_uuid,
                object.class_data_range.clone(),
                scale,
                self.archive(),
            ) {
                Ok(mut dimension) => {
                    if matches!(
                        object.class_uuid,
                        crate::dimensions::V5_LINEAR
                            | crate::dimensions::V5_ANGULAR
                            | crate::dimensions::V5_RADIAL
                            | crate::dimensions::V5_ORDINATE
                    ) {
                        for (class, label) in [
                            (crate::dimensions::V5_DIM_EXTRA, "dimension"),
                            (crate::dimensions::V5_ANGULAR_EXTRA, "angular dimension"),
                        ] {
                            let count = duplicate_userdata_count(&object.userdata, class);
                            if count > 1 {
                                self.typed_losses.push(
                                    RhinoLossCode::DuplicateRecordResolved.note(format!(
                                        "{label} object at offset {} has {count} matching userdata records; first serialized record wins",
                                        object.range.start
                                    )),
                                );
                            }
                        }
                        if let Err(error) = crate::dimensions::apply_userdata(
                            self.scan.data,
                            &object.userdata,
                            self.archive(),
                            scale,
                            &mut dimension,
                        ) {
                            self.scan_warning(
                                source_order,
                                &format!("dimension extension retained: {error}"),
                            );
                            continue;
                        }
                    }
                    // `SemanticAnnotation::order` must be globally unique and
                    // is a `u32`. The arena length is the dense next index and
                    // rolls back with the arena, unlike a standalone counter.
                    let Ok(order) = u32::try_from(self.ir.model.semantic_annotations.len()) else {
                        self.scan_warning(
                            source_order,
                            "dimension retained because the annotation arena exceeds u32 ordinals",
                        );
                        continue;
                    };
                    let object = Self::mint_unknown_id(source_order).to_string();
                    let (annotation, unresolved) = crate::dimensions::project(
                        &dimension,
                        &key,
                        (!identity.name.is_empty()).then(|| identity.name.clone()),
                        &object,
                        order,
                    );
                    if dimension.override_present {
                        self.typed_losses
                            .push(RhinoLossCode::DimensionOverrideDropped.note(format!(
                                "dimension object at offset {} has an unapplied style override",
                                dimension.source_range.start
                            )));
                    }
                    let links = [annotation.id.0.clone()];
                    let result = self.validate_candidate(|candidate, _annotations| {
                        candidate.model.semantic_annotations.push(annotation);
                    });
                    match result {
                        Ok(()) => {
                            self.append_links(source_order, &links);
                            self.mark_decoded(source_order);
                            for code in unresolved {
                                self.typed_losses.push(code.note(format!(
                                    "dimension record {source_order} reference is not resolved to a \
                                     decoded record"
                                )));
                            }
                        }
                        Err(error) => self.scan_warning(
                            source_order,
                            &format!("dimension candidate rejected: {error}"),
                        ),
                    }
                }
                Err(error) => {
                    self.scan_warning(source_order, &format!("dimension retained: {error}"));
                    self.mark_failed(source_order);
                }
            }
        }
    }

    fn decode_hatch(&mut self, source_order: usize, object: &ObjectDescriptor) {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

        let Some(scale) = self.unit_scale() else {
            self.scan_warning(
                source_order,
                "hatch retained because document units are unavailable",
            );
            return;
        };
        let Some(identity) = object.identity.as_ref() else {
            self.scan_warning(
                source_order,
                "hatch retained because identity is unavailable",
            );
            return;
        };
        let mut hatch = match crate::hatch::decode(
            self.expand,
            object.class_data_range.clone(),
            scale,
            self.archive(),
        ) {
            Ok(hatch) => hatch,
            Err(error) => {
                let future = matches!(
                    error,
                    crate::curves::GeometryError::UnsupportedVersion { .. }
                );
                self.scan_warning(
                    source_order,
                    &format!(
                        "hatch {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
                return;
            }
        };
        let duplicate_count =
            duplicate_userdata_count(&object.userdata, crate::hatch::V5_HATCH_EXTRA);
        if duplicate_count > 1 {
            self.typed_losses.push(
                RhinoLossCode::DuplicateRecordResolved.note(format!(
                    "hatch object at offset {} has {duplicate_count} matching userdata records; last valid serialized record wins",
                    object.range.start
                )),
            );
        }
        if let Err(error) = crate::hatch::apply_userdata(
            self.scan.data,
            &object.userdata,
            scale,
            self.archive(),
            &mut hatch,
        ) {
            self.scan_warning(
                source_order,
                &format!("hatch userdata extension failed: {error}"),
            );
        }
        let key = self.object_key(identity, source_order);
        let association = self.source_association(identity);
        let feature_id = FeatureId(format!("rhino:hatch:feature#{key}"));
        let transform = hatch_plane_transform(&hatch.plane, scale);
        for hatch_loop in &mut hatch.loops {
            if let Err(error) = transform_decoded_curve(&mut hatch_loop.curve, transform) {
                self.scan_warning(
                    source_order,
                    &format!("hatch loop placement failed: {error}"),
                );
                self.mark_failed(source_order);
                return;
            }
        }
        let loop_ids = hatch
            .loops
            .iter()
            .enumerate()
            .map(|(index, hatch_loop)| {
                (
                    hatch_loop.kind,
                    format!("rhino:object:curve#{key}.hatch-loop-{index}"),
                )
            })
            .collect::<Vec<_>>();
        let mut parameters = BTreeMap::from([
            ("pattern_index".to_string(), hatch.pattern_index.to_string()),
            ("pattern_scale".to_string(), hatch.pattern_scale.to_string()),
            (
                "pattern_rotation".to_string(),
                hatch.pattern_rotation.to_string(),
            ),
            (
                "basepoint".to_string(),
                format!("{},{}", hatch.basepoint[0], hatch.basepoint[1]),
            ),
        ]);
        if let Some(gradient) = hatch
            .gradient
            .as_ref()
            .and_then(crate::hatch::gradient_json)
        {
            parameters.insert("gradient".to_string(), gradient);
        }
        for (index, (kind, id)) in loop_ids.iter().enumerate() {
            parameters.insert(
                format!("loop_{index}"),
                format!(
                    "{}:{id}",
                    match kind {
                        crate::hatch::LoopKind::Outer => "outer",
                        crate::hatch::LoopKind::Inner => "inner",
                    }
                ),
            );
        }
        let feature = Feature {
            id: feature_id.clone(),
            ordinal: u64::try_from(hatch.source_range.start).expect("source offset fits u64"),
            name: (!identity.name.is_empty()).then(|| identity.name.clone()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("RhinoHatch".to_string()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: "hatch".to_string(),
                parameters,
                properties: BTreeMap::new(),
            },
            native_ref: Some(self.unknowns[source_order].id.to_string()),
        };
        let hatch_loops = hatch.loops;
        let result = self.validate_candidate(|candidate, candidate_annotations| {
            for (index, hatch_loop) in hatch_loops.into_iter().enumerate() {
                commit_curve_tree(
                    candidate,
                    candidate_annotations,
                    hatch_loop.curve,
                    &key,
                    &association,
                    None,
                    &format!("hatch-loop-{index}"),
                );
            }
            candidate.model.features.push(feature);
        });
        match result {
            Ok(()) => {
                for warning in hatch.warnings {
                    self.scan_warning(source_order, &warning);
                }
                let mut links = loop_ids.into_iter().map(|(_, id)| id).collect::<Vec<_>>();
                links.push(feature_id.to_string());
                self.append_links(source_order, &links);
                self.geometry_transferred = true;
                self.mark_native_retained(source_order, RhinoLossCode::HatchFillNotTransferred);
            }
            Err(error) => {
                self.scan_warning(source_order, &format!("hatch candidate rejected: {error}"));
                self.mark_failed(source_order);
            }
        }
    }

    fn decode_polyedge(&mut self, source_order: usize, object: &ObjectDescriptor) {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

        let Some(identity) = object.identity.as_ref() else {
            self.scan_warning(
                source_order,
                "polyedge retained because identity is unavailable",
            );
            return;
        };
        let polyedge = match crate::polyedge::decode(
            self.expand,
            object.class_data_range.clone(),
            self.archive(),
        ) {
            Ok(value) => value,
            Err(error) => {
                self.scan_warning(source_order, &format!("polyedge retained: {error}"));
                self.mark_failed(source_order);
                return;
            }
        };
        let Some(construction) = crate::polyedge::semantic_json(&polyedge) else {
            self.scan_warning(source_order, "polyedge semantic serialization failed");
            return;
        };
        let key = self.object_key(identity, source_order);
        let id = FeatureId(format!("rhino:polyedge:feature#{key}"));
        let segment_objects = polyedge
            .segments
            .iter()
            .map(|segment| segment.object_id)
            .collect::<Vec<_>>();
        let parameters = self
            .resolve_object_records(source_order, "polyedge segment", &segment_objects)
            .into_iter()
            .enumerate()
            .filter_map(|(index, resolved)| {
                resolved.map(|record| (format!("segment_{index}_object"), record))
            })
            .collect::<BTreeMap<_, _>>();
        let name = (!identity.name.is_empty()).then(|| identity.name.clone());
        let feature = Feature {
            id: id.clone(),
            ordinal: u64::try_from(source_order).expect("source order fits u64"),
            name,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("RhinoPolyEdgeReference".to_string()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: "polyedge_reference".to_string(),
                parameters,
                properties: BTreeMap::from([("construction".to_string(), construction)]),
            },
            native_ref: Some(Self::mint_unknown_id(source_order).to_string()),
        };
        match self
            .validate_candidate(|candidate, _annotations| candidate.model.features.push(feature))
        {
            Ok(()) => {
                self.append_link(source_order, id.to_string());
                self.mark_native_retained(
                    source_order,
                    RhinoLossCode::PolyedgeReferencesNotResolved,
                );
            }
            Err(error) => self.scan_warning(
                source_order,
                &format!("polyedge candidate rejected: {error}"),
            ),
        }
    }

    fn decode_detail(&mut self, source_order: usize, object: &ObjectDescriptor) {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

        let Some(identity) = object.identity.as_ref() else {
            self.scan_warning(
                source_order,
                "detail retained because identity is unavailable",
            );
            return;
        };
        let detail = match crate::detail::decode(
            self.scan.data,
            object.class_data_range.clone(),
            self.archive(),
        ) {
            Ok(detail) => detail,
            Err(error) => {
                let future = matches!(
                    error,
                    crate::curves::GeometryError::UnsupportedVersion { .. }
                );
                self.scan_warning(
                    source_order,
                    &format!(
                        "detail {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
                return;
            }
        };
        let key = self.object_key(identity, source_order);
        let association = self.source_association(identity);
        let curve_id = format!("rhino:object:curve#{key}.detail-boundary");
        let feature_id = FeatureId(format!("rhino:detail:feature#{key}"));
        let view = &self.scan.data[detail.view_range.clone()];
        let feature = Feature {
            id: feature_id.clone(),
            ordinal: u64::try_from(detail.source_range.start).expect("source offset fits u64"),
            name: (!identity.name.is_empty()).then(|| identity.name.clone()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("RhinoDetailView".to_string()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: "detail_view".to_string(),
                parameters: BTreeMap::from([
                    ("boundary".to_string(), curve_id.clone()),
                    (
                        "page_per_model_ratio".to_string(),
                        detail.page_per_model_ratio.to_string(),
                    ),
                ]),
                properties: BTreeMap::from([
                    ("view_bytes".to_string(), view.len().to_string()),
                    ("view_sha256".to_string(), sha256_hex(view)),
                ]),
            },
            native_ref: Some(self.unknowns[source_order].id.to_string()),
        };
        let result = self.validate_candidate(|candidate, candidate_annotations| {
            commit_curve_tree(
                candidate,
                candidate_annotations,
                detail.boundary,
                &key,
                &association,
                None,
                "detail-boundary",
            );
            candidate.model.features.push(feature);
        });
        match result {
            Ok(()) => {
                self.append_links(source_order, &[curve_id, feature_id.to_string()]);
                self.geometry_transferred = true;
                self.mark_native_retained(source_order, RhinoLossCode::DetailViewNotTransferred);
            }
            Err(error) => {
                self.scan_warning(source_order, &format!("detail candidate rejected: {error}"));
                self.mark_failed(source_order);
            }
        }
    }

    fn decode_cage(&mut self, source_order: usize, object: &ObjectDescriptor) {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

        let Some(scale) = self.unit_scale() else {
            self.scan_warning(
                source_order,
                "NURBS cage retained because document units are unavailable",
            );
            return;
        };
        let Some(identity) = object.identity.as_ref() else {
            self.scan_warning(
                source_order,
                "NURBS cage retained because identity is unavailable",
            );
            return;
        };
        let cage = match crate::cage::decode(
            self.expand,
            object.class_data_range.clone(),
            scale,
            self.archive(),
        ) {
            Ok(cage) => cage,
            Err(error) => {
                let future = matches!(
                    error,
                    crate::curves::GeometryError::UnsupportedVersion { .. }
                );
                self.scan_warning(
                    source_order,
                    &format!(
                        "NURBS cage {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
                return;
            }
        };
        let key = self.object_key(identity, source_order);
        let feature_id = FeatureId(format!("rhino:cage:feature#{key}"));
        let knots = cage
            .knots
            .iter()
            .map(|axis| {
                axis.iter()
                    .map(f64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect::<Vec<_>>();
        let control_points = cage
            .control_points
            .iter()
            .map(|point| {
                point
                    .iter()
                    .map(f64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect::<Vec<_>>()
            .join(";");
        let mut properties = BTreeMap::from([
            ("u_knots".to_string(), knots[0].clone()),
            ("v_knots".to_string(), knots[1].clone()),
            ("w_knots".to_string(), knots[2].clone()),
            ("control_points".to_string(), control_points),
        ]);
        if let Some(weights) = &cage.weights {
            properties.insert(
                "weights".to_string(),
                weights
                    .iter()
                    .map(f64::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        let feature = Feature {
            id: feature_id.clone(),
            ordinal: u64::try_from(cage.source_range.start).expect("source offset fits u64"),
            name: (!identity.name.is_empty()).then(|| identity.name.clone()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("RhinoNurbsCage".to_string()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: "nurbs_cage".to_string(),
                parameters: BTreeMap::from([
                    ("dimension".to_string(), cage.dimension.to_string()),
                    ("rational".to_string(), cage.rational.to_string()),
                    (
                        "orders".to_string(),
                        format!("{},{},{}", cage.orders[0], cage.orders[1], cage.orders[2]),
                    ),
                    (
                        "counts".to_string(),
                        format!("{},{},{}", cage.counts[0], cage.counts[1], cage.counts[2]),
                    ),
                ]),
                properties,
            },
            native_ref: Some(self.unknowns[source_order].id.to_string()),
        };
        match self
            .validate_candidate(|candidate, _annotations| candidate.model.features.push(feature))
        {
            Ok(()) => {
                self.append_link(source_order, feature_id.to_string());
                self.geometry_transferred = true;
                self.mark_native_retained(source_order, RhinoLossCode::CageLatticeNotTransferred);
            }
            Err(error) => {
                self.scan_warning(
                    source_order,
                    &format!("NURBS cage candidate rejected: {error}"),
                );
                self.mark_failed(source_order);
            }
        }
    }

    fn decode_morph(&mut self, source_order: usize, object: &ObjectDescriptor) {
        let Some(scale) = self.unit_scale() else {
            self.scan_warning(
                source_order,
                "morph control retained because document units are unavailable",
            );
            return;
        };
        let Some(identity) = object.identity.as_ref() else {
            self.scan_warning(
                source_order,
                "morph control retained because identity is unavailable",
            );
            return;
        };
        let morph = match crate::morph::decode(
            self.expand,
            object.class_data_range.clone(),
            scale,
            self.archive(),
        ) {
            Ok(morph) => morph,
            Err(error) => {
                let future = matches!(
                    error,
                    crate::curves::GeometryError::UnsupportedVersion { .. }
                );
                self.scan_warning(
                    source_order,
                    &format!(
                        "morph control {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
                return;
            }
        };
        let key = self.object_key(identity, source_order);
        let captives =
            self.resolve_object_records(source_order, "morph captive", &morph.captive_ids);
        let feature = crate::morph::project(
            &morph,
            &key,
            (!identity.name.is_empty()).then(|| identity.name.clone()),
            self.unknowns[source_order].id.to_string(),
            &captives,
        );
        let feature_id = feature.id.to_string();
        match self
            .validate_candidate(|candidate, _annotations| candidate.model.features.push(feature))
        {
            Ok(()) => {
                self.append_link(source_order, feature_id);
                self.geometry_transferred = true;
                self.mark_native_retained(source_order, RhinoLossCode::MorphDeformationNotApplied);
            }
            Err(error) => {
                self.scan_warning(source_order, &format!("morph candidate rejected: {error}"));
                self.mark_failed(source_order);
            }
        }
    }

    fn decode_curve_on_surface(&mut self, source_order: usize, object: &ObjectDescriptor) {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

        let Some(scale) = self.unit_scale() else {
            self.scan_warning(
                source_order,
                "curve-on-surface retained because document units are unavailable",
            );
            return;
        };
        let Some(identity) = object.identity.as_ref() else {
            self.scan_warning(
                source_order,
                "curve-on-surface retained because identity is unavailable",
            );
            return;
        };
        let construction = match crate::curve_on_surface::decode(
            self.scan.data,
            object.class_data_range.clone(),
            scale,
            self.archive(),
            0,
        ) {
            Ok(value) => value,
            Err(error) => {
                let future = matches!(
                    error,
                    crate::curves::GeometryError::UnsupportedVersion { .. }
                );
                self.scan_warning(
                    source_order,
                    &format!(
                        "curve-on-surface {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
                return;
            }
        };
        let key = self.object_key(identity, source_order);
        let association = self.source_association(identity);
        let parameter_id = format!("rhino:object:curve#{key}.curve-on-surface-c2");
        let model_id = construction
            .model_curve
            .as_ref()
            .map(|_| format!("rhino:object:curve#{key}.curve-on-surface-c3"));
        let surface_id: cadmpeg_ir::ids::SurfaceId =
            format!("rhino:object:surface#{key}.curve-on-surface-support").into();
        let feature_id = FeatureId(format!("rhino:curve-on-surface:feature#{key}"));
        let feature = Feature {
            id: feature_id.clone(),
            ordinal: u64::try_from(construction.source_range.start)
                .expect("source offset fits u64"),
            name: (!identity.name.is_empty()).then(|| identity.name.clone()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("RhinoCurveOnSurface".to_string()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: "curve_on_surface".to_string(),
                parameters: BTreeMap::from([
                    ("parameter_curve".to_string(), parameter_id.clone()),
                    ("support_surface".to_string(), surface_id.to_string()),
                ]),
                properties: model_id
                    .as_ref()
                    .map(|id| BTreeMap::from([("model_curve".to_string(), id.clone())]))
                    .unwrap_or_default(),
            },
            native_ref: Some(self.unknowns[source_order].id.to_string()),
        };
        let parameter_curve = construction.parameter_curve;
        let model_curve = construction.model_curve;
        let (surface_geometry, surface_derived) = match construction.surface {
            crate::surfaces::DecodedSurface::Typed {
                geometry, derived, ..
            } => (geometry, derived),
            crate::surfaces::DecodedSurface::Procedural { geometry, .. } => {
                (SurfaceGeometry::Nurbs(geometry), true)
            }
        };
        let result = self.validate_candidate(|candidate, candidate_annotations| {
            commit_curve_tree(
                candidate,
                candidate_annotations,
                parameter_curve,
                &key,
                &association,
                None,
                "curve-on-surface-c2",
            );
            if let Some(model_curve) = model_curve {
                commit_curve_tree(
                    candidate,
                    candidate_annotations,
                    model_curve,
                    &key,
                    &association,
                    None,
                    "curve-on-surface-c3",
                );
            }
            candidate.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: surface_geometry,
                source_object: Some(association),
            });
            candidate_annotations.exactness.insert(
                surface_id.to_string(),
                ExactnessNote {
                    entity: if surface_derived {
                        Exactness::Derived
                    } else {
                        Exactness::ByteExact
                    },
                    fields: BTreeMap::new(),
                },
            );
            candidate.model.features.push(feature);
        });
        match result {
            Ok(()) => {
                for warning in construction.warnings {
                    self.scan_warning(source_order, &warning);
                }
                let mut links = vec![parameter_id, surface_id.to_string(), feature_id.to_string()];
                if let Some(model_id) = model_id {
                    links.push(model_id);
                }
                self.append_links(source_order, &links);
                self.geometry_transferred = true;
                self.mark_native_retained(
                    source_order,
                    RhinoLossCode::CurveOnSurfaceBindingNotTransferred,
                );
            }
            Err(error) => {
                self.scan_warning(
                    source_order,
                    &format!("curve-on-surface candidate rejected: {error}"),
                );
                self.mark_failed(source_order);
            }
        }
    }

    fn is_definition_member(&self, object: &ObjectDescriptor) -> bool {
        let Some(identity) = object.identity.as_ref() else {
            return false;
        };
        self.scan
            .definitions
            .member_object_ids
            .contains(&identity.object_id)
    }

    fn object_key(&self, identity: &crate::objects::SourceIdentity, source_order: usize) -> String {
        self.instance_key.clone().unwrap_or_else(|| {
            identity
                .source_id
                .rsplit_once('#')
                .map_or_else(|| source_order.to_string(), |(_, key)| key.to_string())
        })
    }

    fn reference_segment(
        &self,
        source_order: usize,
        identity: &crate::objects::SourceIdentity,
    ) -> String {
        if !identity.object_id.is_nil()
            && self.resolve_object(identity.object_id) == ObjectReference::Resolved(source_order)
        {
            identity.object_id.to_string()
        } else {
            format!(
                "record-{source_order:06}-offset-{}",
                self.scan.objects[source_order].range.start
            )
        }
    }

    fn source_association(
        &self,
        identity: &crate::objects::SourceIdentity,
    ) -> SourceObjectAssociation {
        source_association(
            identity,
            &self.instance_path,
            self.instance_color,
            self.instance_visible,
        )
    }

    fn expand_reference(&mut self, source_order: usize) -> bool {
        let original_ids = ArenaLengths::EMPTY
            .appended_ids(&self.ir)
            .expect("capturing all Rhino IR identifiers cannot shrink arenas");
        let annotation_checkpoint = AnnotationCheckpoint::capture(&self.annotations);
        let original_links = self
            .unknowns
            .iter()
            .map(|record| record.links.clone())
            .collect::<Vec<_>>();
        let original_statuses = self.statuses.clone();
        let original_outcomes = self.outcomes.clone();
        let original_geometry_transferred = self.geometry_transferred;
        let original_warning_count = self.phase_warnings.len();
        let original_loss_count = self.typed_losses.len();
        let original_selection = self.selected_object;
        let original_key = self.instance_key.clone();
        let original_path = self.instance_path.clone();
        let original_color = self.instance_color;
        let original_visible = self.instance_visible;
        let original_expansion_budget = self.expansion_budget;
        let mut stack = Vec::new();
        let mut path = self.instance_path.clone();
        let parent = Transform::identity();
        let outcome = self.expand_reference_inner(source_order, parent, &mut path, &mut stack);
        // Mesh buffers stay charged in the session arena even on rollback.
        let mut rejection_warning = None;
        let accepted = match outcome {
            Ok(links) => {
                let validation =
                    cadmpeg_ir::admit(&self.ir, cadmpeg_ir::RHINO_INSTANCE_CHECKS, Vec::new());
                if validation.is_ok() {
                    self.append_links(source_order, &links);
                    self.mark_decoded(source_order);
                    self.geometry_transferred = true;
                    true
                } else {
                    rejection_warning = Some(format!(
                        "instance expansion rejected atomically by IR admission: {}",
                        validation_findings(&validation)
                    ));
                    false
                }
            }
            Err(message) => {
                rejection_warning = Some(format!("instance retained: {message}"));
                false
            }
        };
        if accepted {
            return true;
        }

        let current_ids = ArenaLengths::EMPTY
            .appended_ids(&self.ir)
            .expect("capturing all Rhino IR identifiers cannot shrink arenas");
        let added_ids = current_ids
            .difference(&original_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        ArenaLengths::remove_ids(&mut self.ir, &added_ids);
        annotation_checkpoint.rollback(&mut self.annotations);
        for (record, links) in self.unknowns.iter_mut().zip(original_links) {
            record.links = links;
        }
        self.statuses = original_statuses;
        self.outcomes = original_outcomes;
        self.geometry_transferred = original_geometry_transferred;
        self.phase_warnings.truncate(original_warning_count);
        self.typed_losses.truncate(original_loss_count);
        self.selected_object = original_selection;
        self.instance_key = original_key;
        self.instance_path = original_path;
        self.instance_color = original_color;
        self.instance_visible = original_visible;
        self.expansion_budget = original_expansion_budget;
        self.ir
            .set_native_unknowns_from("rhino", self.unknowns.iter().map(NativeUnknownRecord::from))
            .expect("Rhino unknown records serialize");
        if let Some(warning) = rejection_warning {
            self.scan_warning(source_order, &warning);
        }
        false
    }

    fn expand_reference_inner(
        &mut self,
        source_order: usize,
        parent: Transform,
        path: &mut Vec<String>,
        stack: &mut Vec<crate::wire::Uuid>,
    ) -> Result<Vec<String>, String> {
        const MAX_INSTANCE_DEPTH: usize = 64;
        let _nested = self
            .expand
            .ctx()
            .enter_nested("rhino_instance_nesting", None)
            .map_err(|error| error.to_string())?;
        self.expansion_budget.reference()?;
        self.charge_session_collections(1, "rhino_instance_reference")?;
        let depth_limit = usize::try_from(self.expand.ctx().policy().limits.max_recursion_depth)
            .unwrap_or(usize::MAX)
            .min(MAX_INSTANCE_DEPTH);
        if stack.len() >= depth_limit {
            return Err("instance nesting exceeds 64 levels".to_string());
        }
        let object = self
            .scan
            .objects
            .get(source_order)
            .ok_or_else(|| "reference object is missing".to_string())?;
        let identity = object
            .identity
            .as_ref()
            .ok_or_else(|| "reference identity is unavailable".to_string())?;
        let reference =
            crate::instances::parse_reference(self.scan.data, object.class_data_range.clone())
                .map_err(|error| error.to_string())?;
        if self
            .scan
            .definitions
            .ambiguous_ids
            .contains(&reference.definition_id)
        {
            return Err(format!(
                "definition {} is duplicated",
                reference.definition_id
            ));
        }
        let definition = self
            .definition_candidates
            .get(&reference.definition_id)
            .and_then(|index| self.scan.definitions.definitions.get(*index))
            .ok_or_else(|| format!("definition {} is missing", reference.definition_id))?;
        if matches!(definition.kind, crate::instances::DefinitionKind::Linked)
            && definition.members.is_empty()
        {
            return Err(format!(
                "linked external definition {} has no local members",
                definition.id
            ));
        }
        if matches!(definition.kind, crate::instances::DefinitionKind::Unset) {
            return Err(format!("definition {} has unset type", definition.id));
        }
        let unique_members = definition.members.iter().copied().collect::<BTreeSet<_>>();
        if unique_members.len() != definition.members.len() {
            return Err(format!(
                "definition {} contains duplicate member UUIDs",
                definition.id
            ));
        }
        if stack.contains(&definition.id) {
            return Err(format!("definition cycle reaches {}", definition.id));
        }
        let scale = self
            .unit_scale()
            .ok_or_else(|| "document units are unavailable".to_string())?;
        let local = crate::instances::scale_translation(reference.transform, scale)
            .ok_or_else(|| "scaled instance transform is invalid".to_string())?;
        let transform = parent.compose(local);
        let definition_id = definition.id;
        let definition_members = definition.members.clone();
        stack.push(definition_id);
        path.push(self.reference_segment(source_order, identity));
        let previous_color = self.instance_color;
        self.instance_color = identity.effective_color.map(color).or(previous_color);
        let previous_visible = self.instance_visible;
        self.instance_visible =
            Some(previous_visible.unwrap_or(true) && identity.effective_visible);
        let mut links = Vec::new();
        for member_id in definition_members {
            self.expansion_budget.member()?;
            self.charge_session_collections(1, "rhino_instance_member")?;
            let member_order = match self.resolve_object(member_id) {
                ObjectReference::Resolved(order) => order,
                ObjectReference::Missing => {
                    return Err(format!("definition member {member_id} is missing"))
                }
                ObjectReference::Ambiguous => {
                    return Err(format!("definition member {member_id} is ambiguous"))
                }
            };
            let member = &self.scan.objects[member_order];
            if crate::instances::is_reference_class(member.class_uuid) {
                let nested = self.expand_reference_inner(member_order, transform, path, stack)?;
                self.append_links(member_order, &nested);
                self.mark_decoded(member_order);
                links.extend(nested);
                continue;
            }
            let before = ModelCheckpoint::capture(&self.ir.model);
            let previous_selection = self.selected_object.replace(member_order);
            let previous_key =
                self.instance_key
                    .replace(format!("{}.{}", path.join("."), member_id));
            let previous_path = std::mem::replace(&mut self.instance_path, path.clone());
            self.decode_geometry();
            self.selected_object = previous_selection;
            self.instance_key = previous_key;
            self.instance_path = previous_path;
            let after = ModelCheckpoint::capture(&self.ir.model);
            if before == after {
                return Err(format!("definition member {member_id} did not decode"));
            }
            links.extend(self.transform_new_entities(&before, transform)?);
        }
        self.instance_color = previous_color;
        self.instance_visible = previous_visible;
        path.pop();
        stack.pop();
        Ok(links)
    }

    fn transform_new_entities(
        &mut self,
        before: &ModelCheckpoint,
        transform: Transform,
    ) -> Result<Vec<String>, String> {
        let mut links = Vec::new();
        let mut derived_ids = Vec::new();
        for body in before
            .added_mut::<Body>(&mut self.ir.model)
            .ok_or_else(|| "instance decode removed existing bodies".to_string())?
        {
            compose_body_transform(body, transform);
            links.push(body.id.to_string());
            derived_ids.push(body.id.to_string());
        }
        for point in before
            .added_mut::<Point>(&mut self.ir.model)
            .ok_or_else(|| "instance decode removed existing points".to_string())?
        {
            point.position = transform.apply_point(point.position);
            derived_ids.push(point.id.to_string());
        }
        for curve in before
            .added_mut::<Curve>(&mut self.ir.model)
            .ok_or_else(|| "instance decode removed existing curves".to_string())?
        {
            transform_curve(curve, transform)?;
            links.push(curve.id.to_string());
            derived_ids.push(curve.id.to_string());
        }
        for surface in before
            .added_mut::<Surface>(&mut self.ir.model)
            .ok_or_else(|| "instance decode removed existing surfaces".to_string())?
        {
            transform_surface(surface, transform)?;
            links.push(surface.id.to_string());
            derived_ids.push(surface.id.to_string());
        }
        for mesh in before
            .added_mut::<Tessellation>(&mut self.ir.model)
            .ok_or_else(|| "instance decode removed existing tessellations".to_string())?
        {
            for vertex in &mut mesh.vertices {
                *vertex = transform.apply_point(*vertex);
            }
            for value in &mut mesh.normals {
                *value = transform
                    .apply_normal(*value)
                    .ok_or_else(|| "mesh normal transform is singular".to_string())?;
            }
            links.push(mesh.id.clone());
            derived_ids.push(mesh.id.clone());
        }
        for subd in before
            .added_mut::<cadmpeg_ir::SubdSurface>(&mut self.ir.model)
            .ok_or_else(|| "instance decode removed existing subdivision surfaces".to_string())?
        {
            for vertex in &mut subd.vertices {
                vertex.point = transform.apply_point(vertex.point);
            }
            links.push(subd.id.to_string());
            derived_ids.push(subd.id.to_string());
        }
        let procedural_curve_start = before.arena_len::<ProceduralCurve>();
        let procedural_surface_start = before.arena_len::<ProceduralSurface>();
        if self.ir.model.procedural_curves.len() > procedural_curve_start
            || self.ir.model.procedural_surfaces.len() > procedural_surface_start
        {
            let omitted_ids = self.ir.model.procedural_curves[procedural_curve_start..]
                .iter()
                .map(|procedure| procedure.id.to_string())
                .chain(
                    self.ir.model.procedural_surfaces[procedural_surface_start..]
                        .iter()
                        .map(|procedure| procedure.id.to_string()),
                )
                .collect::<Vec<_>>();
            self.ir
                .model
                .procedural_curves
                .truncate(procedural_curve_start);
            self.ir
                .model
                .procedural_surfaces
                .truncate(procedural_surface_start);
            for id in omitted_ids {
                self.annotations.exactness.remove(&id);
                self.annotations.provenance.remove(&id);
            }
            self.phase_warnings.push(
                "instance: transformed procedural definition omitted; exact solved carrier retained"
                    .to_string(),
            );
        }
        for id in derived_ids {
            annotate_derived(&mut self.annotations, &id);
        }
        Ok(links)
    }

    fn decode_subd(&mut self, source_order: usize, object: &ObjectDescriptor) {
        let Some(scale) = self.unit_scale() else {
            self.scan_warning(
                source_order,
                "SubD retained because document units are unavailable",
            );
            return;
        };
        let Some(identity) = object.identity.as_ref() else {
            self.scan_warning(
                source_order,
                "SubD retained because identity is unavailable",
            );
            return;
        };
        let key = self.object_key(identity, source_order);
        let id: cadmpeg_ir::ids::SubdId = format!("rhino:object:subd#{key}").into();
        match crate::subd::decode(
            self.scan.data,
            object.class_data_range.clone(),
            self.archive(),
            scale,
            id,
        ) {
            Ok(crate::subd::DecodedSubd::Empty) => {
                self.mark_decoded(source_order);
            }
            Ok(crate::subd::DecodedSubd::Surface {
                surface,
                neutral_metadata,
                enum_diagnostics,
                warnings,
            }) => {
                if self.commit_subd_surface(
                    source_order,
                    *surface,
                    neutral_metadata,
                    enum_diagnostics,
                    warnings,
                    scale != 1.0,
                ) {
                    self.mark_decoded(source_order);
                } else {
                    self.scan_warning(
                        source_order,
                        "SubD candidate rejected atomically by IR validation",
                    );
                    self.mark_failed(source_order);
                }
            }
            Err(error) => {
                let future = matches!(error, crate::subd::SubdError::UnsupportedVersion { .. });
                self.scan_warning(
                    source_order,
                    &format!(
                        "SubD {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
            }
        }
    }

    fn commit_subd_surface(
        &mut self,
        source_order: usize,
        surface: cadmpeg_ir::subd::SubdSurface,
        neutral_metadata: bool,
        enum_diagnostics: Vec<crate::subd::SubdEnumDiagnostic>,
        warnings: Vec<String>,
        scaled: bool,
    ) -> bool {
        for warning in warnings {
            self.scan_warning(source_order, &warning);
        }
        for diagnostic in enum_diagnostics {
            self.typed_losses
                .push(RhinoLossCode::EnumerationValueDegraded.note(diagnostic.message()));
        }
        if neutral_metadata {
            self.scan_warning(
                source_order,
                "SubD cache, texture, symmetry, or packing metadata is retained without a neutral-IR mapping",
            );
        }
        self.commit_subd(source_order, surface, scaled)
    }

    fn commit_subd(
        &mut self,
        source_order: usize,
        mut surface: cadmpeg_ir::subd::SubdSurface,
        scaled: bool,
    ) -> bool {
        let Some(object) = self.scan.objects.get(source_order) else {
            return false;
        };
        let Some(identity) = object.identity.as_ref() else {
            return false;
        };
        surface.source_object = Some(self.source_association(identity));
        let id = surface.id.to_string();
        let result = self.validate_candidate(|candidate, candidate_annotations| {
            candidate.model.subds.push(surface);
            candidate_annotations.exactness.insert(
                id.clone(),
                ExactnessNote {
                    entity: if scaled {
                        Exactness::Derived
                    } else {
                        Exactness::ByteExact
                    },
                    fields: BTreeMap::new(),
                },
            );
            id.clone()
        });
        let link = match result {
            Ok(link) => link,
            Err(findings) => {
                self.scan_warning(
                    source_order,
                    &format!("SubD validation rejected candidate: {findings}"),
                );
                return false;
            }
        };
        self.append_link(source_order, link);
        self.geometry_transferred = true;
        true
    }

    fn decode_extrusion(&mut self, source_order: usize, object: &ObjectDescriptor) {
        let Some(scale) = self.unit_scale() else {
            self.scan_warning(
                source_order,
                "extrusion retained because document units are unavailable",
            );
            self.commit_unknown_surface(source_order);
            return;
        };
        let decoded = crate::extrusion::decode(
            self.expand,
            self.scan.data,
            object.class_data_range.clone(),
            self.archive(),
            self.scan.metadata.properties.writer_version,
            scale,
            &object.userdata,
            &mut self.mesh_budget,
        );
        match decoded {
            Ok(extrusion) => {
                for warning in &extrusion.warnings {
                    self.scan_warning(source_order, warning);
                }
                if self.commit_extrusion(source_order, extrusion) {
                    self.mark_decoded(source_order);
                } else {
                    self.scan_warning(
                        source_order,
                        "extrusion candidate rejected atomically by IR validation",
                    );
                    self.commit_unknown_surface(source_order);
                }
            }
            Err(error) => {
                self.scan_warning(
                    source_order,
                    &format!("extrusion degraded and retained: {error}"),
                );
                self.commit_unknown_surface(source_order);
            }
        }
    }

    /// Mints the stable unknown-record ID for source order.
    pub fn mint_unknown_id(source_order: usize) -> UnknownId {
        UnknownId(format!("rhino:object:record#{source_order:06}"))
    }

    /// Commits the transaction and produces canonical IR and report state.
    pub(crate) fn commit(mut self) -> DecodeResult {
        self.typed_losses
            .extend(crate::annotations::install(self.scan, &mut self.ir));
        for source in crate::document_data::install(self.scan, &mut self.ir) {
            self.retain_opaque_record(&source);
        }
        let presentation = crate::presentation::install(self.scan, &mut self.ir);
        self.typed_losses.extend(presentation.losses);
        for source in presentation.opaque_records {
            self.retain_opaque_record(&source);
        }
        crate::product::install(self.scan, &mut self.ir);
        let views = crate::views::install(self.scan, &mut self.ir);
        self.typed_losses.extend(views.losses);
        for source in views.opaque_records {
            self.retain_opaque_record(&source);
        }
        self.ir.finalize();
        let mut losses: Vec<LossNote> = Vec::new();
        let decoded = self
            .outcomes
            .values()
            .map(|outcome| outcome.decoded)
            .sum::<usize>();
        let total = self.scan.objects.len();
        losses.push(
            RhinoLossCode::ObjectRecordCensus
                .note(format!("decoded {decoded}/{total} Rhino object records")),
        );
        let mut omissions: Vec<LossNote> = Vec::new();
        for (class, outcome) in &self.outcomes {
            if outcome.retained > 0 {
                omissions.push(
                    RhinoLossCode::ObjectFamilyNotTransferred
                        .note(format!(
                            "retained {} object record(s) for class {class}; geometry is not decoded",
                            outcome.retained
                        ))
                        .with_provenance(loss_provenance(class, outcome)),
                );
            }
            // `native_code` is set only by a transition that also incremented
            // `native_retained`, so the count is nonzero whenever it is `Some`.
            if let Some(code) = outcome.native_code {
                omissions.push(
                    code.note(format!(
                        "framed and read {} object record(s) for class {class}; construction \
                         state is retained as native passthrough",
                        outcome.native_retained
                    ))
                    .with_provenance(loss_provenance(class, outcome)),
                );
            }
            if outcome.attribute_degraded > 0 {
                losses.push(
                    RhinoLossCode::ObjectAttributesDegraded
                        .note(format!(
                            "{} object record(s) for class {class} have degraded attributes",
                            outcome.attribute_degraded
                        ))
                        .with_provenance(loss_provenance(class, outcome)),
                );
            }
            if outcome.failed_framed > 0 {
                losses.push(
                    RhinoLossCode::ObjectFramingUndecodable
                        .note(format!(
                            "{} framed object record(s) for class {class} could not be decoded",
                            outcome.failed_framed
                        ))
                        .with_provenance(loss_provenance(class, outcome)),
                );
            }
        }
        self.typed_losses.extend(omissions);
        if let Some(first) = self.scan.definitions.diagnostics.first() {
            losses.push(
                RhinoLossCode::ContainerInstanceDefinitionDegraded
                    .note(format!(
                        "retained {} malformed, ambiguous, or checksum-degraded instance-definition record(s); first: {}",
                        self.scan.definitions.diagnostics.len(),
                        first.message
                    ))
                    .with_provenance(SourceProvenance {
                        format: "rhino".to_string(),
                        stream: String::new(),
                        offset: first.source_range.start as u64,
                        tag: Some("INSTANCE_DEFINITION_TABLE".to_string()),
                    }),
            );
        }
        losses.append(&mut self.typed_losses);
        losses.extend(self.scan.warnings.iter().map(|warning| {
            if integrity_diagnostic(warning) {
                RhinoLossCode::IntegrityFailure.note(warning.clone())
            } else if warning.contains(" has invalid color source ") {
                RhinoLossCode::EnumerationValueDegraded.note(warning.clone())
            } else if brep_mesh_cache_diagnostic(warning) {
                RhinoLossCode::BrepMeshCacheDegraded.note(warning.clone())
            } else if redundant_field_diagnostic(warning) {
                RhinoLossCode::RedundantFieldRepaired.note(warning.clone())
            } else if duplicate_resolution_diagnostic(warning) {
                RhinoLossCode::DuplicateRecordResolved.note(warning.clone())
            } else {
                RhinoLossCode::ContainerScanDiagnostic.note(warning.clone())
            }
        }));
        let mut phase_families = BTreeMap::<String, (usize, String)>::new();
        for warning in &self.phase_warnings {
            if integrity_diagnostic(warning) {
                losses.push(RhinoLossCode::IntegrityFailure.note(warning.clone()));
                continue;
            }
            if brep_mesh_cache_diagnostic(warning) {
                losses.push(RhinoLossCode::BrepMeshCacheDegraded.note(warning.clone()));
                continue;
            }
            if redundant_field_diagnostic(warning) {
                losses.push(RhinoLossCode::RedundantFieldRepaired.note(warning.clone()));
                continue;
            }
            let (family, detail) = warning
                .split_once(':')
                .map_or(("rhino", warning.as_str()), |(family, detail)| {
                    (family, detail.trim())
                });
            let entry = phase_families
                .entry(family.to_string())
                .or_insert_with(|| (0, detail.to_string()));
            entry.0 += 1;
        }
        losses.extend(phase_families.into_iter().map(|(family, (count, first))| {
            RhinoLossCode::ObjectDecodeDiagnostic.note(if count == 1 {
                format!("{family}: {first}")
            } else {
                format!("{family}: {count} decode warnings; first: {first}")
            })
        }));
        let byte_records = self
            .unknowns
            .iter()
            .filter(|record| record.data.is_some())
            .count()
            + self
                .opaque_records
                .iter()
                .filter(|record| record.data.is_some())
                .count();
        let note = if self.opaque_records.is_empty() {
            format!(
                "decoded {decoded}/{total} Rhino object records; retained metadata/digests for {} \
                 records and complete bytes for {byte_records}; document cap {} bytes, per-record cap {} bytes",
                self.unknowns.len(),
                RETAINED_DOCUMENT_CAP,
                RETAINED_RECORD_CAP
            )
        } else {
            format!(
                "decoded {decoded}/{total} Rhino object records; retained metadata/digests for {} \
                 object records and {} opaque records, with complete bytes for {byte_records}; \
                 document cap {} bytes, per-record cap {} bytes",
                self.unknowns.len(),
                self.opaque_records.len(),
                RETAINED_DOCUMENT_CAP,
                RETAINED_RECORD_CAP
            )
        };
        let notes = vec![note];
        let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(self.annotations);
        source_fidelity
            .attach_native_unknown_records(&mut self.ir, "rhino", self.unknowns)
            .expect("Rhino source records separate from product identities");
        source_fidelity.retain_unknown_records("rhino", self.opaque_records);
        DecodeResult::new(
            self.ir,
            DecodeReport {
                dialects: Vec::new(),
                format: "rhino".to_string(),
                container_only: false,
                geometry_transferred: self.geometry_transferred,
                coverage: std::collections::BTreeMap::new(),
                transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
                losses,
                notes,
            },
            source_fidelity,
        )
    }

    fn retain_object_records(&mut self) {
        for source_order in 0..self.scan.objects.len() {
            let object = &self.scan.objects[source_order];
            let id = Self::mint_unknown_id(source_order);
            let class = object.class_uuid.to_string();
            let object_type = object.object_type;
            let framing_degraded = object.framing_degraded;
            let attributes_degraded = object.attributes_degraded;
            let record = self.source_record(id, object.range.clone());
            let outcome = self.outcomes.entry(class.clone()).or_default();
            if outcome.retained == 0 {
                outcome.first_offset =
                    u64::try_from(object.range.start).expect("Rhino record offset fits u64");
                outcome.first_object_type = object_type;
            }
            outcome.retained += 1;
            if framing_degraded {
                outcome.retained -= 1;
                outcome.failed_framed += 1;
            }
            if attributes_degraded {
                outcome.attribute_degraded += 1;
            }
            self.unknowns.push(record);
            self.statuses.push(if framing_degraded {
                GeometryStatus::Failed
            } else {
                GeometryStatus::Retained
            });
        }
    }

    fn retain_opaque_records(&mut self) {
        for index in 0..self.scan.opaque_records.len() {
            let source = &self.scan.opaque_records[index];
            self.retain_opaque_record(source);
        }
    }

    fn retain_opaque_record(&mut self, source: &OpaqueRecord) {
        let id = UnknownId(format!(
            "rhino:opaque:record#{:08x}-{:08x}-{:016x}",
            source.table_typecode, source.record.typecode, source.record.range.start
        ));
        let record = self.source_record(id, source.record.range.clone());
        self.opaque_records.push(record);
    }

    fn source_record(&mut self, id: UnknownId, range: std::ops::Range<usize>) -> UnknownRecord {
        let bytes = &self.scan.data[range.clone()];
        let byte_len = u64::try_from(bytes.len()).expect("Rhino record length fits u64");
        let retain = bytes.len() <= self.retention_limits[0]
            && self
                .retained_bytes
                .checked_add(bytes.len())
                .is_some_and(|end| end <= self.retention_limits[1]);
        let data = retain.then(|| bytes.to_vec());
        if retain {
            self.retained_bytes = self
                .retained_bytes
                .checked_add(bytes.len())
                .expect("retention cap checked");
        }
        UnknownRecord {
            id,
            offset: u64::try_from(range.start).expect("Rhino record offset fits u64"),
            byte_len,
            sha256: sha256_hex(bytes),
            data,
            links: Vec::new(),
        }
    }

    fn scan_warning(&mut self, source_order: usize, message: &str) {
        let class = self.scan.objects[source_order].class_uuid.to_string();
        self.scan_warnings_for_class(&class, message);
    }

    fn scan_warnings_for_class(&mut self, class: &str, message: &str) {
        let outcome = self.outcomes.entry(class.to_string()).or_default();
        if outcome.first_offset == 0 {
            outcome.first_offset = self
                .scan
                .objects
                .iter()
                .find(|object| object.class_uuid.to_string() == class)
                .map_or(0, |object| object.range.start as u64);
        }
        self.phase_warnings.push(format!("{class}: {message}"));
    }

    fn charge_entities(&mut self, source_order: usize, amount: usize) -> bool {
        if let Err(message) = self.expansion_budget.entities(amount) {
            self.scan_warning(source_order, &message);
            false
        } else if let Err(message) = self.charge_session_entities(amount) {
            self.scan_warning(source_order, &message);
            false
        } else {
            true
        }
    }

    fn charge_session_entities(&self, amount: usize) -> Result<(), String> {
        self.expand
            .ctx()
            .charge_entities(
                u64::try_from(amount).unwrap_or(u64::MAX),
                "rhino_instance_entities",
            )
            .map_err(|error| error.to_string())
    }

    fn charge_session_collections(
        &self,
        amount: usize,
        operation: &'static str,
    ) -> Result<(), String> {
        self.expand
            .ctx()
            .charge_collection_items(u64::try_from(amount).unwrap_or(u64::MAX), operation)
            .map_err(|error| error.to_string())
    }

    fn commit_geometry(
        &mut self,
        source_order: usize,
        decoded: crate::curves::DecodedGeometry,
    ) -> bool {
        let Some(object) = self.scan.objects.get(source_order) else {
            return false;
        };
        let Some(identity) = object.identity.as_ref() else {
            return false;
        };
        let key = self.object_key(identity, source_order);
        let association = self.source_association(identity);
        let Some(unknown) = self
            .unknowns
            .get(source_order)
            .map(|record| record.id.clone())
        else {
            return false;
        };
        match decoded {
            crate::curves::DecodedGeometry::Point { position, scaled } => {
                if !self.charge_entities(source_order, 5) {
                    return false;
                }
                let body_id: cadmpeg_ir::ids::BodyId = format!("rhino:object:body#{key}").into();
                let region_id: cadmpeg_ir::ids::RegionId =
                    format!("rhino:object:region#{key}").into();
                let shell_id: cadmpeg_ir::ids::ShellId = format!("rhino:object:shell#{key}").into();
                let point_id: cadmpeg_ir::ids::PointId = format!("rhino:object:point#{key}").into();
                let vertex_id: cadmpeg_ir::ids::VertexId =
                    format!("rhino:object:vertex#{key}").into();
                self.ir.model.points.push(Point {
                    id: point_id.clone(),
                    position,
                    source_object: Some(association.clone()),
                });
                self.ir.model.vertices.push(Vertex {
                    id: vertex_id.clone(),
                    point: point_id.clone(),
                    tolerance: None,
                });
                self.ir.model.shells.push(Shell {
                    id: shell_id.clone(),
                    region: region_id.clone(),
                    faces: Vec::new(),
                    wire_edges: Vec::new(),
                    free_vertices: vec![vertex_id.clone()],
                });
                self.ir.model.regions.push(Region {
                    id: region_id.clone(),
                    body: body_id.clone(),
                    shells: vec![shell_id.clone()],
                });
                self.ir.model.bodies.push(body(
                    identity,
                    body_id.clone(),
                    vec![region_id.clone()],
                    &association,
                ));
                self.annotate_point_topology(
                    &point_id, &vertex_id, &shell_id, &region_id, &body_id, scaled,
                );
                self.append_link(source_order, body_id.to_string());
            }
            crate::curves::DecodedGeometry::PointCloud(cloud) => {
                let crate::curves::PointCloud {
                    points,
                    scaled,
                    warnings,
                } = cloud;
                self.phase_warnings.extend(
                    warnings
                        .into_iter()
                        .map(|warning| format!("{}: {warning}", identity.source_id)),
                );
                let Some(entity_count) = points
                    .len()
                    .checked_mul(2)
                    .and_then(|count| count.checked_add(3))
                else {
                    self.scan_warning(source_order, "point-cloud entity count overflow");
                    return false;
                };
                if !self.charge_entities(source_order, entity_count) {
                    return false;
                }
                let body_id: cadmpeg_ir::ids::BodyId = format!("rhino:object:body#{key}").into();
                let region_id: cadmpeg_ir::ids::RegionId =
                    format!("rhino:object:region#{key}").into();
                let shell_id: cadmpeg_ir::ids::ShellId = format!("rhino:object:shell#{key}").into();
                let mut vertices = Vec::with_capacity(points.len());
                for (index, position) in points.into_iter().enumerate() {
                    let point_id: cadmpeg_ir::ids::PointId =
                        format!("rhino:object:point#{key}.{index}").into();
                    let vertex_id: cadmpeg_ir::ids::VertexId =
                        format!("rhino:object:vertex#{key}.{index}").into();
                    self.ir.model.points.push(Point {
                        id: point_id.clone(),
                        position,
                        source_object: Some(association.clone()),
                    });
                    self.ir.model.vertices.push(Vertex {
                        id: vertex_id.clone(),
                        point: point_id,
                        tolerance: None,
                    });
                    vertices.push(vertex_id);
                }
                self.ir.model.shells.push(Shell {
                    id: shell_id.clone(),
                    region: region_id.clone(),
                    faces: Vec::new(),
                    wire_edges: Vec::new(),
                    free_vertices: vertices,
                });
                self.ir.model.regions.push(Region {
                    id: region_id.clone(),
                    body: body_id.clone(),
                    shells: vec![shell_id],
                });
                self.ir.model.bodies.push(body(
                    identity,
                    body_id.clone(),
                    vec![region_id],
                    &association,
                ));
                let point_ids: Vec<String> = self
                    .ir
                    .model
                    .points
                    .iter()
                    .filter(|point| {
                        point
                            .id
                            .as_str()
                            .starts_with(&format!("rhino:object:point#{key}."))
                    })
                    .map(|point| point.id.to_string())
                    .collect();
                for point_id in point_ids {
                    self.annotations.exactness.insert(
                        point_id,
                        ExactnessNote {
                            entity: if scaled {
                                Exactness::Derived
                            } else {
                                Exactness::ByteExact
                            },
                            fields: BTreeMap::new(),
                        },
                    );
                }
                self.append_link(source_order, body_id.to_string());
            }
            crate::curves::DecodedGeometry::Curve { curve } => {
                if !self.charge_entities(source_order, decoded_curve_entity_count(&curve)) {
                    return false;
                }
                let warnings = curve_warnings(&curve);
                self.phase_warnings.extend(
                    warnings
                        .into_iter()
                        .map(|warning| format!("{}: {warning}", identity.source_id)),
                );
                let parent_id = commit_curve_tree(
                    &mut self.ir,
                    &mut self.annotations,
                    curve,
                    &key,
                    &association,
                    Some(unknown),
                    "root",
                );
                self.append_link(source_order, parent_id.to_string());
            }
            crate::curves::DecodedGeometry::Surface { surface } => match surface {
                crate::surfaces::DecodedSurface::Typed {
                    geometry, derived, ..
                } => {
                    if !self.charge_entities(source_order, 1) {
                        return false;
                    }
                    let surface_id: cadmpeg_ir::ids::SurfaceId =
                        format!("rhino:object:surface#{key}").into();
                    self.ir.model.surfaces.push(Surface {
                        id: surface_id.clone(),
                        geometry,
                        source_object: Some(association.clone()),
                    });
                    self.annotations.exactness.insert(
                        surface_id.to_string(),
                        ExactnessNote {
                            entity: if derived {
                                Exactness::Derived
                            } else {
                                Exactness::ByteExact
                            },
                            fields: BTreeMap::new(),
                        },
                    );
                    self.append_link(source_order, surface_id.to_string());
                }
                crate::surfaces::DecodedSurface::Procedural {
                    geometry,
                    definition,
                    children,
                } => {
                    return self.commit_procedural_surface(
                        source_order,
                        &key,
                        association,
                        geometry,
                        definition,
                        children,
                    );
                }
            },
        }
        self.geometry_transferred = true;
        true
    }

    fn commit_procedural_surface(
        &mut self,
        source_order: usize,
        key: &str,
        association: SourceObjectAssociation,
        geometry: cadmpeg_ir::geometry::NurbsSurface,
        definition: crate::surfaces::DecodedProceduralSurface,
        children: Vec<crate::curves::DecodedCurve>,
    ) -> bool {
        let expected_children = match &definition {
            crate::surfaces::DecodedProceduralSurface::Revolution { .. } => 1,
            crate::surfaces::DecodedProceduralSurface::Sum { .. } => 2,
        };
        if children.len() != expected_children {
            return false;
        }
        let Some(unknown) = self
            .unknowns
            .get(source_order)
            .map(|record| record.id.clone())
        else {
            return false;
        };
        let result = self.validate_candidate(|candidate, candidate_annotations| {
            let mut child_ids = Vec::with_capacity(children.len());
            for (index, child) in children.into_iter().enumerate() {
                let path = match (expected_children, index) {
                    (1, 0) => "directrix",
                    (2, 0) => "first",
                    (2, 1) => "second",
                    _ => unreachable!("child cardinality checked"),
                };
                child_ids.push(commit_curve_tree(
                    candidate,
                    candidate_annotations,
                    child,
                    key,
                    &association,
                    Some(unknown.clone()),
                    path,
                ));
            }
            let surface_id: cadmpeg_ir::ids::SurfaceId =
                format!("rhino:object:surface#{key}").into();
            candidate.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Nurbs(geometry),
                source_object: Some(association),
            });
            let procedural_id: cadmpeg_ir::ids::ProceduralSurfaceId =
                format!("rhino:object:procedural-surface#{key}").into();
            let ir_definition = match definition {
                crate::surfaces::DecodedProceduralSurface::Revolution {
                    axis_origin,
                    axis_direction,
                    angular_interval,
                    parameter_interval,
                    transposed,
                } => ProceduralSurfaceDefinition::Revolution {
                    directrix: child_ids.remove(0),
                    axis_origin,
                    axis_direction,
                    angular_interval,
                    angular_parameter_interval: None,
                    parameter_interval: Some(parameter_interval),
                    transposed,
                    revision_form: None,
                },
                crate::surfaces::DecodedProceduralSurface::Sum { basepoint } => {
                    ProceduralSurfaceDefinition::Sum {
                        first: child_ids.remove(0),
                        second: child_ids.remove(0),
                        basepoint,
                        revision_form: None,
                    }
                }
            };
            candidate.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id.clone(),
                surface: surface_id.clone(),
                definition: ir_definition,
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            for id in [surface_id.to_string(), procedural_id.to_string()] {
                candidate_annotations.exactness.insert(
                    id,
                    ExactnessNote {
                        entity: Exactness::Derived,
                        fields: BTreeMap::new(),
                    },
                );
            }
            vec![surface_id.to_string()]
        });
        let links = match result {
            Ok(links) => links,
            Err(findings) => {
                self.phase_warnings.push(format!(
                    "procedural-surface: candidate rejected by IR validation: {findings}"
                ));
                return false;
            }
        };
        self.append_links(source_order, &links);
        self.geometry_transferred = true;
        true
    }

    fn commit_extrusion(
        &mut self,
        source_order: usize,
        extrusion: crate::extrusion::DecodedExtrusion,
    ) -> bool {
        let Some(object) = self.scan.objects.get(source_order) else {
            return false;
        };
        let Some(identity) = object.identity.as_ref() else {
            return false;
        };
        let Some(unknown) = self
            .unknowns
            .get(source_order)
            .map(|record| record.id.clone())
        else {
            return false;
        };
        let key = self.object_key(identity, source_order);
        if extrusion.boundaries.len() != extrusion.laterals.len() || extrusion.boundaries.is_empty()
        {
            return false;
        }
        let association = self.source_association(identity);
        let result = self.validate_candidate_fallible(|candidate, candidate_annotations| {
            let mut links = Vec::new();
            let mut directrices = Vec::with_capacity(extrusion.boundaries.len());
            for (index, boundary) in extrusion.boundaries.iter().enumerate() {
                let id = commit_curve_tree(
                    candidate,
                    candidate_annotations,
                    boundary.start_curve.clone(),
                    &key,
                    &association,
                    Some(unknown.clone()),
                    &format!("profile-{index}.start"),
                );
                directrices.push(id);
            }
            for (index, geometry) in extrusion.laterals.iter().cloned().enumerate() {
                let surface_id: cadmpeg_ir::ids::SurfaceId =
                    format!("rhino:object:surface#{key}.lateral-{index}").into();
                let procedure_id: cadmpeg_ir::ids::ProceduralSurfaceId =
                    format!("rhino:object:procedural-surface#{key}.lateral-{index}").into();
                candidate.model.surfaces.push(Surface {
                    id: surface_id.clone(),
                    geometry: SurfaceGeometry::Nurbs(geometry),
                    source_object: Some(association.clone()),
                });
                candidate.model.procedural_surfaces.push(ProceduralSurface {
                    id: procedure_id.clone(),
                    surface: surface_id.clone(),
                    definition: ProceduralSurfaceDefinition::Extrusion {
                        directrix: directrices[index].clone(),
                        parameter_interval: None,
                        direction: extrusion.direction,
                        native_position: None,
                        revision_form: None,
                    },
                    cache_fit_tolerance: None,
                    record_bounds: None,
                });
                annotate_derived(candidate_annotations, &surface_id.to_string());
                annotate_derived(candidate_annotations, &procedure_id.to_string());
                links.push(surface_id.to_string());
            }
            if (extrusion.caps[0] || extrusion.caps[1])
                && !stage_extrusion_caps(
                    candidate,
                    candidate_annotations,
                    &key,
                    &association,
                    &extrusion,
                    &directrices,
                    &mut links,
                )
            {
                return Err("extrusion cap staging failed".to_string());
            }
            for (index, mut mesh) in extrusion.meshes.into_iter().enumerate() {
                mesh.tessellation.id = format!("rhino:object:tessellation#{key}.cache-{index}");
                mesh.tessellation.source_object = Some(association.clone());
                annotate_derived(candidate_annotations, &mesh.tessellation.id);
                links.push(mesh.tessellation.id.clone());
                candidate.model.tessellations.push(mesh.tessellation);
            }
            Ok(links)
        });
        let links = match result {
            Ok(links) => links,
            Err(findings) => {
                self.scan_warning(
                    source_order,
                    &format!("extrusion candidate rejected by IR validation: {findings}"),
                );
                return false;
            }
        };
        self.append_links(source_order, &links);
        self.geometry_transferred = true;
        true
    }

    fn commit_unknown_surface(&mut self, source_order: usize) {
        let Some(object) = self.scan.objects.get(source_order) else {
            return;
        };
        let Some(identity) = object.identity.as_ref() else {
            return;
        };
        let Some(unknown) = self
            .unknowns
            .get(source_order)
            .map(|record| record.id.clone())
        else {
            return;
        };
        let key = self.object_key(identity, source_order);
        let id: cadmpeg_ir::ids::SurfaceId = format!("rhino:object:surface#{key}").into();
        let association = self.source_association(identity);
        let validation = self.validate_candidate(|candidate, candidate_annotations| {
            candidate.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: SurfaceGeometry::Unknown {
                    record: Some(unknown.clone()),
                },
                source_object: Some(association),
            });
            candidate_annotations.exactness.insert(
                id.to_string(),
                ExactnessNote {
                    entity: Exactness::Unknown,
                    fields: BTreeMap::new(),
                },
            );
            id.to_string()
        });
        match validation {
            Ok(link) => {
                self.append_link(source_order, link);
            }
            Err(findings) => self.scan_warning(
                source_order,
                &format!("unknown surface validation rejected candidate: {findings}"),
            ),
        }
    }

    fn annotate_point_topology(
        &mut self,
        point: &cadmpeg_ir::ids::PointId,
        vertex: &cadmpeg_ir::ids::VertexId,
        shell: &cadmpeg_ir::ids::ShellId,
        region: &cadmpeg_ir::ids::RegionId,
        body: &cadmpeg_ir::ids::BodyId,
        scaled: bool,
    ) {
        let point_exactness = if scaled {
            Exactness::Derived
        } else {
            Exactness::ByteExact
        };
        self.annotations.exactness.insert(
            point.to_string(),
            ExactnessNote {
                entity: point_exactness,
                fields: BTreeMap::new(),
            },
        );
        for id in [
            vertex.to_string(),
            shell.to_string(),
            region.to_string(),
            body.to_string(),
        ] {
            self.annotations.exactness.insert(
                id,
                ExactnessNote {
                    entity: Exactness::Derived,
                    fields: BTreeMap::new(),
                },
            );
        }
    }

    fn commit_mesh(&mut self, source_order: usize, mesh: crate::mesh::DecodedMesh) -> bool {
        let Some(object) = self.scan.objects.get(source_order) else {
            return false;
        };
        let Some(identity) = object.identity.as_ref() else {
            return false;
        };
        if !self.charge_entities(source_order, 1) {
            return false;
        }
        self.phase_warnings.extend(
            mesh.warnings
                .into_iter()
                .map(|warning| format!("{}: {warning}", identity.source_id)),
        );
        let id = mesh.tessellation.id.clone();
        self.ir.model.tessellations.push(Tessellation {
            id: id.clone(),
            body: None,
            faces: mesh.tessellation.faces,
            chordal_deflection: mesh.tessellation.chordal_deflection,
            source_object: Some(self.source_association(identity)),
            vertices: mesh.tessellation.vertices,
            triangles: mesh.tessellation.triangles,
            feature_edges: mesh.tessellation.feature_edges,
            strip_lengths: mesh.tessellation.strip_lengths,
            normals: mesh.tessellation.normals,
            corner_normals: mesh.tessellation.corner_normals,
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels: mesh.tessellation.channels,
        });
        self.annotations.exactness.insert(
            id.clone(),
            ExactnessNote {
                entity: if mesh.scaled || mesh.quad_count != 0 {
                    Exactness::Derived
                } else {
                    Exactness::ByteExact
                },
                fields: BTreeMap::new(),
            },
        );
        if mesh.ngon_count != 0 {
            self.typed_losses
                .push(RhinoLossCode::MeshNgonGroupingDropped.note(format!(
                    "{} n-gon grouping record(s) were not transferred for mesh {id}",
                    mesh.ngon_count
                )));
        }
        if mesh.quad_count != 0 {
            self.typed_losses
                .push(RhinoLossCode::MeshQuadTopologyTriangulated.note(format!(
                    "{} quadrilateral face(s) were triangulated for mesh {id}",
                    mesh.quad_count
                )));
        }
        self.append_link(source_order, id);
        true
    }

    fn decode_brep(&mut self, source_order: usize, object: &ObjectDescriptor) {
        let parsed = crate::brep::parse(
            self.scan.data,
            object.class_data_range.clone(),
            self.archive(),
            self.scan.metadata.properties.writer_version,
            &object.userdata,
        );
        let parsed = match parsed {
            Ok(value) => value,
            Err(error) => {
                let future = matches!(
                    error,
                    crate::curves::GeometryError::UnsupportedVersion { .. }
                );
                self.scan_warning(
                    source_order,
                    &format!(
                        "Brep {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
                return;
            }
        };
        let warnings = match &parsed {
            crate::brep::BrepParse::Valid(value) => value.warnings(),
            crate::brep::BrepParse::SemanticInvalid { warnings, .. } => warnings,
        };
        for warning in warnings {
            if warning.starts_with("invalid Brep is_solid value ") {
                self.typed_losses
                    .push(RhinoLossCode::EnumerationValueDegraded.note(warning));
            } else {
                self.scan_warning(source_order, warning);
            }
        }
        let Some(identity) = object.identity.as_ref() else {
            self.scan_warning(
                source_order,
                "Brep retained because identity is unavailable",
            );
            return;
        };
        let Some(scale) = self.unit_scale() else {
            self.scan_warning(
                source_order,
                "Brep retained because document units are unavailable",
            );
            return;
        };
        let association = self.source_association(identity);
        let key = self.object_key(identity, source_order);
        let unknown = self.unknowns[source_order].id.clone();
        self.ir
            .set_native_unknowns_from("rhino", self.unknowns.iter().map(NativeUnknownRecord::from))
            .expect("Rhino unknown records serialize");
        let staged = match &parsed {
            crate::brep::BrepParse::Valid(brep) => stage_brep(BrepTransferInput {
                expand: self.expand,
                data: self.scan.data,
                archive: self.archive(),
                writer_version: self.scan.metadata.properties.writer_version,
                brep,
                key: &key,
                association: &association,
                unknown: &unknown,
                scale,
                mesh_budget: &mut self.mesh_budget,
            }),
            crate::brep::BrepParse::SemanticInvalid { raw, error, .. } => Ok(stage_invalid_brep(
                BrepCarrierInput {
                    expand: self.expand,
                    data: self.scan.data,
                    archive: self.archive(),
                    writer_version: self.scan.metadata.properties.writer_version,
                    raw,
                    key: &key,
                    association: &association,
                    unknown: &unknown,
                    scale,
                    mesh_budget: &mut self.mesh_budget,
                },
                error,
            )),
        };
        match staged {
            Ok(staged) => {
                let links = staged.links.clone();
                let warnings = staged.warnings.clone();
                let full_topology = matches!(staged.kind, BrepTransferKind::FullTopology);
                let emitted_geometry = !staged.draft.model().curves.is_empty()
                    || !staged.draft.model().surfaces.is_empty();
                let cache_only = !full_topology
                    && !emitted_geometry
                    && !staged.draft.model().tessellations.is_empty();
                let entity_count = staged.draft.entity_count();
                let committed = self
                    .expansion_budget
                    .entities(entity_count)
                    .and_then(|()| staged.apply(&mut self.ir, &mut self.annotations));
                if let Err(error) = committed {
                    self.scan_warning(
                        source_order,
                        &format!("Brep draft rejected before commit: {error}"),
                    );
                } else {
                    self.append_links(source_order, &links);
                    for warning in warnings {
                        if let Some(cause) = warning.strip_prefix("Brep topology fallback: ") {
                            self.typed_losses.push(
                                RhinoLossCode::TopologyBrepFallback
                                    .note(format!("Brep topology fallback: {cause}")),
                            );
                        } else if warning.contains("polycurve join moved endpoints") {
                            self.typed_losses
                                .push(RhinoLossCode::PolycurveJoinGap.note(&warning));
                        } else if warning.contains(" C2 omitted: ") {
                            self.typed_losses
                                .push(RhinoLossCode::TrimPcurveDropped.note(&warning));
                        } else {
                            self.scan_warning(source_order, &warning);
                        }
                    }
                    if cache_only {
                        self.scan_warning(
                            source_order,
                            "Brep emitted cache tessellations without decoded geometry",
                        );
                    }
                    self.geometry_transferred |= full_topology || emitted_geometry;
                    if full_topology {
                        self.mark_decoded(source_order);
                    } else {
                        self.scan_warning(
                            source_order,
                            "Brep topology invalid; decoded child carriers retained",
                        );
                    }
                }
            }
            Err(error) => {
                self.scan_warning(
                    source_order,
                    &format!("Brep geometry/topology degraded: {error}"),
                );
            }
        }
    }

    fn transition(&mut self, source_order: usize, next: GeometryStatus) -> bool {
        let Some(current) = self.statuses.get(source_order).copied() else {
            return false;
        };
        if current == next
            || matches!(
                current,
                GeometryStatus::Decoded | GeometryStatus::Failed | GeometryStatus::NativeRetained
            )
        {
            return false;
        }
        let object = &self.scan.objects[source_order];
        let class = object.class_uuid.to_string();
        let outcome = self.outcomes.get_mut(&class).expect("status class exists");
        match current {
            GeometryStatus::Retained => outcome.retained -= 1,
            GeometryStatus::Decoded | GeometryStatus::Failed | GeometryStatus::NativeRetained => {
                unreachable!()
            }
        }
        match next {
            GeometryStatus::Retained => outcome.retained += 1,
            GeometryStatus::NativeRetained => outcome.native_retained += 1,
            GeometryStatus::Decoded => outcome.decoded += 1,
            GeometryStatus::Failed => outcome.failed_framed += 1,
        }
        self.statuses[source_order] = next;
        true
    }
}

fn object_geometry_archive(archive: ArchiveVersion) -> bool {
    matches!(
        archive,
        ArchiveVersion::V2
            | ArchiveVersion::V3
            | ArchiveVersion::V4
            | ArchiveVersion::V5
            | ArchiveVersion::V6
            | ArchiveVersion::V7
            | ArchiveVersion::V8
            | ArchiveVersion::V9
    )
}

fn integrity_diagnostic(message: &str) -> bool {
    message.contains("CRC mismatch") || message.contains("checksum mismatch")
}

fn duplicate_resolution_diagnostic(message: &str) -> bool {
    message.starts_with("duplicate layer index ")
        || message.starts_with("duplicate layer UUID ")
        || message.starts_with("duplicate singleton metadata record ")
}

fn redundant_field_diagnostic(message: &str) -> bool {
    message.starts_with("redundant ")
        || message.contains(": redundant ")
        || message.contains("invalid optional Brep region topology discarded")
}

fn brep_mesh_cache_diagnostic(message: &str) -> bool {
    message.contains("Brep mesh cache") || message.contains(" mesh cache slot ")
}

fn duplicate_userdata_count(
    userdata: &[crate::objects::UserdataDescriptor],
    class: crate::wire::Uuid,
) -> usize {
    userdata
        .iter()
        .filter(|value| value.class_uuid == class)
        .count()
}

#[cfg(test)]
fn append_record_links(ir: &mut CadIr, unknown: &UnknownId, links: &[String]) {
    let Ok(mut unknowns) = ir.native_unknowns("rhino") else {
        return;
    };
    let Some(record) = unknowns.iter_mut().find(|record| record.id == *unknown) else {
        return;
    };
    append_links_to_native_record(record, links);
    let _ = ir.set_native_unknowns("rhino", &unknowns);
}

#[cfg(test)]
fn append_links_to_native_record(record: &mut NativeUnknownRecord, links: &[String]) {
    append_links(&record.id, &mut record.links, links);
}

fn append_links_to_record(record: &mut UnknownRecord, links: &[String]) {
    append_links(&record.id, &mut record.links, links);
}

fn append_links(unknown_id: &UnknownId, record_links: &mut Vec<String>, links: &[String]) {
    let unknown = unknown_id.to_string();
    let mut additions = links
        .iter()
        .filter(|link| *link != &unknown)
        .cloned()
        .collect::<Vec<_>>();
    additions.sort();
    additions.dedup();
    if additions.is_empty() {
        return;
    }
    let existing = std::mem::take(record_links);
    let mut merged = Vec::with_capacity(existing.len().saturating_add(additions.len()));
    let (mut left, mut right) = (
        existing.into_iter().peekable(),
        additions.into_iter().peekable(),
    );
    while let (Some(existing), Some(addition)) = (left.peek(), right.peek()) {
        match existing.cmp(addition) {
            std::cmp::Ordering::Less => merged.push(left.next().expect("peeked")),
            std::cmp::Ordering::Equal => {
                merged.push(left.next().expect("peeked"));
                right.next();
            }
            std::cmp::Ordering::Greater => merged.push(right.next().expect("peeked")),
        }
    }
    merged.extend(left);
    merged.extend(right);
    *record_links = merged;
}

fn validation_findings(report: &cadmpeg_ir::report::ValidationReport) -> String {
    report
        .findings
        .iter()
        .filter(|finding| finding.severity >= Severity::Error)
        .take(3)
        .map(|finding| {
            finding.entity.as_ref().map_or_else(
                || format!("{}: {}", finding.check, finding.message),
                |entity| format!("{} ({entity}): {}", finding.check, finding.message),
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn annotate_derived(annotations: &mut cadmpeg_ir::Annotations, id: &str) {
    annotations.exactness.insert(
        id.to_string(),
        ExactnessNote {
            entity: Exactness::Derived,
            fields: BTreeMap::new(),
        },
    );
}

fn stage_extrusion_caps(
    ir: &mut CadIr,
    annotations: &mut cadmpeg_ir::Annotations,
    key: &str,
    association: &SourceObjectAssociation,
    extrusion: &crate::extrusion::DecodedExtrusion,
    directrices: &[cadmpeg_ir::ids::CurveId],
    links: &mut Vec<String>,
) -> bool {
    if directrices.len() != extrusion.boundaries.len() {
        return false;
    }
    let body_id: cadmpeg_ir::ids::BodyId = format!("rhino:object:body#{key}.caps").into();
    let mut region_ids = Vec::new();
    for cap in 0..2 {
        if !extrusion.caps[cap] {
            continue;
        }
        let region_id: cadmpeg_ir::ids::RegionId =
            format!("rhino:object:region#{key}.cap-{cap}").into();
        let shell_id: cadmpeg_ir::ids::ShellId =
            format!("rhino:object:shell#{key}.cap-{cap}").into();
        let surface_id: cadmpeg_ir::ids::SurfaceId =
            format!("rhino:object:surface#{key}.cap-{cap}").into();
        let face_id: cadmpeg_ir::ids::FaceId = format!("rhino:object:face#{key}.cap-{cap}").into();
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: extrusion.cap_origins[cap],
                normal: extrusion.cap_normals[cap],
                u_axis: extrusion.cap_u_axes[cap],
            },
            source_object: Some(association.clone()),
        });
        let mut loop_ids = Vec::with_capacity(extrusion.boundaries.len());
        for (profile, boundary) in extrusion.boundaries.iter().enumerate() {
            let suffix = format!("cap-{cap}.profile-{profile}");
            let curve_id = if cap == 0 {
                directrices[profile].clone()
            } else {
                let id: cadmpeg_ir::ids::CurveId =
                    format!("rhino:object:curve#{key}.{suffix}").into();
                ir.model.curves.push(Curve {
                    id: id.clone(),
                    geometry: CurveGeometry::Nurbs(boundary.end_nurbs.clone()),
                    source_object: Some(association.clone()),
                });
                annotate_derived(annotations, &id.to_string());
                id
            };
            let endpoint = if cap == 0 {
                boundary.start_nurbs.control_points.first().copied()
            } else {
                boundary.end_nurbs.control_points.first().copied()
            };
            let Some(endpoint) = endpoint else {
                return false;
            };
            let point_id: cadmpeg_ir::ids::PointId =
                format!("rhino:object:point#{key}.{suffix}").into();
            let vertex_id: cadmpeg_ir::ids::VertexId =
                format!("rhino:object:vertex#{key}.{suffix}").into();
            let edge_id: cadmpeg_ir::ids::EdgeId =
                format!("rhino:object:edge#{key}.{suffix}").into();
            let loop_id: cadmpeg_ir::ids::LoopId =
                format!("rhino:object:loop#{key}.{suffix}").into();
            let coedge_id: cadmpeg_ir::ids::CoedgeId =
                format!("rhino:object:coedge#{key}.{suffix}").into();
            let pcurve_id: cadmpeg_ir::ids::PcurveId =
                format!("rhino:object:pcurve#{key}.{suffix}").into();
            let pcurve = if cap == 0 {
                &boundary.start_pcurve
            } else {
                &boundary.end_pcurve
            };
            let Ok(degree) = usize::try_from(pcurve.degree) else {
                return false;
            };
            let Some(end_index) = pcurve.knots.len().checked_sub(degree + 1) else {
                return false;
            };
            let Some(parameter_range) = pcurve
                .knots
                .get(degree)
                .copied()
                .zip(pcurve.knots.get(end_index).copied())
                .map(|(start, end)| [start, end])
            else {
                return false;
            };
            ir.model.points.push(Point {
                id: point_id.clone(),
                position: endpoint,
                source_object: Some(association.clone()),
            });
            ir.model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id.clone(),
                tolerance: None,
            });
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                curve: Some(curve_id),
                start: vertex_id.clone(),
                end: vertex_id.clone(),
                param_range: Some(parameter_range),
                tolerance: None,
            });
            ir.model.pcurves.push(Pcurve {
                id: pcurve_id.clone(),
                geometry: PcurveGeometry::Nurbs {
                    degree: pcurve.degree,
                    knots: pcurve.knots.clone(),
                    control_points: pcurve.control_points.clone(),
                    weights: pcurve.weights.clone(),
                    periodic: pcurve.periodic,
                },
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range: Some(parameter_range),
                fit_tolerance: None,
            });
            ir.model.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id.clone(),
                next: coedge_id.clone(),
                previous: coedge_id.clone(),
                radial_next: coedge_id.clone(),
                sense: Sense::Forward,
                pcurves: vec![cadmpeg_ir::topology::PcurveUse {
                    pcurve: pcurve_id.clone(),
                    isoparametric: None,
                    parameter_range: None,
                }],
                use_curve: None,
                use_curve_parameter_range: None,
            });
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                coedges: vec![coedge_id.clone()],
                vertex_uses: Vec::new(),
            });
            loop_ids.push(loop_id.clone());
            for id in [
                point_id.to_string(),
                vertex_id.to_string(),
                edge_id.to_string(),
                pcurve_id.to_string(),
                coedge_id.to_string(),
                loop_id.to_string(),
            ] {
                annotate_derived(annotations, &id);
            }
        }
        ir.model.faces.push(Face {
            id: face_id.clone(),
            shell: shell_id.clone(),
            surface: surface_id.clone(),
            sense: if cap == 0 {
                Sense::Reversed
            } else {
                Sense::Forward
            },
            loops: loop_ids,
            name: None,
            color: association.color,
            tolerance: None,
        });
        annotate_derived(annotations, &surface_id.to_string());
        annotate_derived(annotations, &face_id.to_string());
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: vec![face_id],
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id.clone()],
        });
        annotate_derived(annotations, &shell_id.to_string());
        annotate_derived(annotations, &region_id.to_string());
        region_ids.push(region_id);
    }
    if region_ids.is_empty() {
        return false;
    }
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: BodyKind::Sheet,
        regions: region_ids,
        transform: None,
        name: association.name.clone(),
        color: association.color,
        visible: association.visible,
    });
    annotate_derived(annotations, &body_id.to_string());
    links.push(body_id.to_string());
    true
}

#[derive(Debug, Default)]
struct BrepDraft {
    kind: BrepTransferKind,
    draft: ModelDraft,
    links: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum BrepTransferKind {
    #[default]
    FullTopology,
    FreeCarrierFallback,
}

struct BrepTransferInput<'a> {
    expand: crate::mesh::MeshExpand<'a>,
    data: &'a [u8],
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    brep: &'a crate::brep::ValidatedRawBrep,
    key: &'a str,
    association: &'a SourceObjectAssociation,
    unknown: &'a UnknownId,
    scale: f64,
    mesh_budget: &'a mut crate::mesh::MeshBudget,
}

struct BrepCarrierInput<'a> {
    expand: crate::mesh::MeshExpand<'a>,
    data: &'a [u8],
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    raw: &'a crate::brep::RawBrep,
    key: &'a str,
    association: &'a SourceObjectAssociation,
    unknown: &'a UnknownId,
    scale: f64,
    mesh_budget: &'a mut crate::mesh::MeshBudget,
}

struct BrepCarrierDraft {
    staged: BrepDraft,
    c3: BTreeMap<i32, cadmpeg_ir::ids::CurveId>,
    surfaces: BTreeMap<i32, cadmpeg_ir::ids::SurfaceId>,
    plane_parameterizations: BTreeMap<i32, crate::surfaces::PlaneParameterization>,
    child_failed: bool,
    child_cause: Option<String>,
}

struct BrepStageContext<'a> {
    key: &'a str,
    association: &'a SourceObjectAssociation,
    unknown: &'a UnknownId,
}

impl BrepDraft {
    fn apply(
        self,
        ir: &mut CadIr,
        annotations: &mut cadmpeg_ir::Annotations,
    ) -> Result<(), String> {
        self.draft
            .commit(
                ir,
                annotations,
                &mut Vec::new(),
                &mut cadmpeg_ir::report::TransferLedger::default(),
            )
            .map_err(|error| error.to_string())
    }

    fn free_carrier_fallback(mut self, cause: impl Into<String>) -> Self {
        self.kind = BrepTransferKind::FreeCarrierFallback;
        let emitted: BTreeSet<String> = self
            .draft
            .model()
            .curves
            .iter()
            .map(|value| value.id.to_string())
            .chain(
                self.draft
                    .model()
                    .surfaces
                    .iter()
                    .map(|value| value.id.to_string()),
            )
            .chain(
                self.draft
                    .model()
                    .tessellations
                    .iter()
                    .map(|value| value.id.clone()),
            )
            .chain(
                self.draft
                    .model()
                    .procedural_curves
                    .iter()
                    .map(|value| value.id.to_string()),
            )
            .collect();
        self.links.retain(|id| emitted.contains(id));
        self.draft.retain_exactness(|id| emitted.contains(id));
        let model = self.draft.model_mut();
        model.bodies.clear();
        model.regions.clear();
        model.shells.clear();
        model.faces.clear();
        model.loops.clear();
        model.coedges.clear();
        model.edges.clear();
        model.vertices.clear();
        model.points.clear();
        model.pcurves.clear();
        self.warnings
            .push(format!("Brep topology fallback: {}", cause.into()));
        self
    }
}

fn stage_brep_carriers(input: BrepCarrierInput<'_>) -> BrepCarrierDraft {
    let BrepCarrierInput {
        expand,
        data,
        archive,
        writer_version,
        raw,
        key,
        association,
        unknown,
        scale,
        mesh_budget,
    } = input;
    let mut staged = BrepDraft {
        kind: BrepTransferKind::FullTopology,
        ..BrepDraft::default()
    };
    let mut c3 = BTreeMap::new();
    let mut surfaces = BTreeMap::new();
    let mut plane_parameterizations = BTreeMap::new();
    let mut child_failed = false;
    let mut child_cause = None;
    for (kind, slots) in [
        ("render", &raw.render_meshes),
        ("analysis", &raw.analysis_meshes),
    ] {
        for (index, slot) in slots.iter().enumerate() {
            let Some(child) = slot.mesh.as_ref() else {
                continue;
            };
            let id = format!("rhino:object:tessellation#{key}.{kind}-{index}");
            match crate::mesh::decode(
                expand,
                data,
                child.class_data_range.clone(),
                archive,
                crate::mesh::MeshDecodeOptions {
                    writer_version,
                    association: Some(association.clone()),
                    id,
                    scale,
                    userdata: &slot.userdata,
                },
                mesh_budget,
            ) {
                Ok(mesh) => {
                    staged.warnings.extend(mesh.warnings.clone());
                    staged.draft.exactness(
                        mesh.tessellation.id.clone(),
                        if mesh.scaled {
                            Exactness::Derived
                        } else {
                            Exactness::ByteExact
                        },
                    );
                    staged.links.push(mesh.tessellation.id.clone());
                    staged
                        .draft
                        .model_mut()
                        .tessellations
                        .push(mesh.tessellation);
                }
                Err(error) => {
                    staged
                        .warnings
                        .push(format!("invalid {kind} mesh cache slot {index}: {error}"));
                }
            }
        }
    }
    for (index, child) in raw
        .c3
        .slots
        .iter()
        .enumerate()
        .filter_map(|(index, child)| child.as_ref().map(|child| (index, child)))
    {
        let decoded = crate::curves::decode(
            data,
            child.class_uuid,
            child.class_data_range.clone(),
            scale,
            archive,
        );
        match decoded {
            Ok(crate::curves::DecodedGeometry::Curve { curve }) => {
                staged.warnings.extend(
                    curve_warnings(&curve)
                        .into_iter()
                        .map(|warning| format!("C3 slot {index}: {warning}")),
                );
                let id = stage_curve_tree(
                    &mut staged,
                    curve,
                    key,
                    &format!("c3-{index}"),
                    association,
                    unknown,
                );
                c3.insert(index as i32, id);
            }
            Ok(_) => {
                child_failed = true;
                child_cause = Some(format!("C3 slot {index} is not a curve"));
            }
            Err(error) => {
                child_failed = true;
                child_cause = Some(format!("C3 slot {index}: {error}"));
            }
        }
    }
    for (index, child) in raw
        .surfaces
        .slots
        .iter()
        .enumerate()
        .filter_map(|(index, child)| child.as_ref().map(|child| (index, child)))
    {
        let decoded = crate::curves::decode(
            data,
            child.class_uuid,
            child.class_data_range.clone(),
            scale,
            archive,
        );
        match decoded {
            Ok(crate::curves::DecodedGeometry::Surface {
                surface:
                    crate::surfaces::DecodedSurface::Typed {
                        geometry,
                        derived,
                        plane_parameterization,
                    },
            }) => {
                let id: cadmpeg_ir::ids::SurfaceId =
                    format!("rhino:object:surface#{key}.slot-{index}").into();
                staged.draft.model_mut().surfaces.push(Surface {
                    id: id.clone(),
                    geometry,
                    source_object: Some(association.clone()),
                });
                staged.draft.exactness(
                    id.to_string(),
                    if derived {
                        Exactness::Derived
                    } else {
                        Exactness::ByteExact
                    },
                );
                if let Some(parameterization) = plane_parameterization {
                    plane_parameterizations.insert(index as i32, parameterization);
                }
                surfaces.insert(index as i32, id);
            }
            Ok(crate::curves::DecodedGeometry::Surface {
                surface:
                    crate::surfaces::DecodedSurface::Procedural {
                        geometry,
                        definition,
                        children,
                    },
            }) => match stage_brep_procedural_surface(
                &mut staged,
                index,
                geometry,
                definition,
                children,
                &BrepStageContext {
                    key,
                    association,
                    unknown,
                },
            ) {
                Ok(id) => {
                    surfaces.insert(index as i32, id);
                }
                Err(error) => {
                    child_failed = true;
                    child_cause = Some(format!("surface slot {index}: {error}"));
                }
            },
            Ok(_) => {
                child_failed = true;
                child_cause = Some(format!("surface slot {index} is not a surface"));
            }
            Err(error) => {
                child_failed = true;
                child_cause = Some(format!("surface slot {index}: {error}"));
            }
        }
    }
    BrepCarrierDraft {
        staged,
        c3,
        surfaces,
        plane_parameterizations,
        child_failed,
        child_cause,
    }
}

fn stage_invalid_brep(
    input: BrepCarrierInput<'_>,
    semantic_error: &crate::curves::GeometryError,
) -> BrepDraft {
    let carriers = stage_brep_carriers(input);
    finish_brep_fallback(carriers.staged, semantic_error.to_string())
}

fn stage_brep(input: BrepTransferInput<'_>) -> Result<BrepDraft, crate::curves::GeometryError> {
    let BrepTransferInput {
        expand,
        data,
        archive,
        writer_version,
        brep,
        key,
        association,
        unknown,
        scale,
        mesh_budget,
    } = input;
    let raw = brep.raw();
    let BrepCarrierDraft {
        mut staged,
        c3,
        surfaces,
        plane_parameterizations,
        child_failed,
        child_cause,
    } = stage_brep_carriers(BrepCarrierInput {
        expand,
        data,
        archive,
        writer_version,
        raw,
        key,
        association,
        unknown,
        scale,
        mesh_budget,
    });
    if child_failed {
        return Ok(finish_brep_fallback(
            staged,
            child_cause.unwrap_or_else(|| "child geometry decode failed".to_string()),
        ));
    }
    let (c2, pcurves, pcurve_warnings) =
        decode_pcurves(data, archive, raw, key, &plane_parameterizations);
    staged.warnings.extend(pcurve_warnings);
    staged.draft.model_mut().pcurves = pcurves;
    let body_id: cadmpeg_ir::ids::BodyId = format!("rhino:object:body#{key}").into();
    let mut vertex_ids = Vec::with_capacity(raw.vertices.len());
    for (index, vertex) in raw.vertices.iter().enumerate() {
        let point_id: cadmpeg_ir::ids::PointId =
            format!("rhino:object:point#{key}.vertex-{index}").into();
        let vertex_id: cadmpeg_ir::ids::VertexId =
            format!("rhino:object:vertex#{key}.slot-{index}").into();
        staged.draft.model_mut().points.push(Point {
            id: point_id.clone(),
            position: Point3::new(
                crate::wire::scaled_coordinate(vertex.point.0[0], scale).ok_or_else(|| {
                    crate::curves::error(0, "scaled Brep vertex coordinate is invalid")
                })?,
                crate::wire::scaled_coordinate(vertex.point.0[1], scale).ok_or_else(|| {
                    crate::curves::error(0, "scaled Brep vertex coordinate is invalid")
                })?,
                crate::wire::scaled_coordinate(vertex.point.0[2], scale).ok_or_else(|| {
                    crate::curves::error(0, "scaled Brep vertex coordinate is invalid")
                })?,
            ),
            source_object: Some(association.clone()),
        });
        staged.draft.model_mut().vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point_id,
            tolerance: scaled_tolerance(vertex.tolerance, scale)?,
        });
        vertex_ids.push(vertex_id);
    }
    let mut edge_ids = Vec::with_capacity(raw.edges.len());
    for (index, edge) in raw.edges.iter().enumerate() {
        let id: cadmpeg_ir::ids::EdgeId = format!("rhino:object:edge#{key}.slot-{index}").into();
        let curve = c3.get(&edge.curve).cloned();
        let vertices = edge_vertices(edge);
        staged.draft.model_mut().edges.push(Edge {
            id: id.clone(),
            curve,
            start: vertex_ids[vertices[0]].clone(),
            end: vertex_ids[vertices[1]].clone(),
            param_range: Some(edge_param_range(edge)),
            tolerance: scaled_tolerance(edge.tolerance, scale)?,
        });
        edge_ids.push(id);
    }
    let components = face_components(raw);
    let grouping = region_shell_groups(raw, &components)?;
    let free_vertex_indices = brep_free_vertex_indices(raw)?;
    if !free_vertex_indices.is_empty() && grouping.shell_faces.len() != 1 {
        return Ok(finish_brep_fallback(
            staged,
            "Brep free vertices have no unique shell membership",
        ));
    }
    let free_vertex_ids = free_vertex_indices
        .iter()
        .map(|index| format!("rhino:object:vertex#{key}.slot-{index}").into())
        .collect::<Vec<cadmpeg_ir::ids::VertexId>>();
    if grouping.fallback {
        staged.warnings.push(
            "Brep 3.3 region topology was not representable; incidence-derived shells used"
                .to_string(),
        );
    }
    let mut face_ids = Vec::with_capacity(raw.faces.len());
    for (index, face) in raw.faces.iter().enumerate() {
        let surface = surfaces.get(&face.surface).cloned().ok_or_else(|| {
            crate::curves::error(face.source_range.start, "surface child missing")
        })?;
        let component = grouping.face_groups[index];
        let id: cadmpeg_ir::ids::FaceId = format!("rhino:object:face#{key}.slot-{index}").into();
        staged.draft.model_mut().faces.push(Face {
            id: id.clone(),
            shell: format!("rhino:object:shell#{key}.component-{component}").into(),
            surface,
            sense: face_sense(face.reversed_surface != 0),
            loops: Vec::new(),
            name: None,
            color: face.color.map(color),
            tolerance: None,
        });
        face_ids.push(id);
    }
    let mut synthetic_edges = BTreeMap::new();
    for (index, loop_record) in raw.loops.iter().enumerate() {
        let id: cadmpeg_ir::ids::LoopId = format!("rhino:object:loop#{key}.slot-{index}").into();
        let face_id = face_ids[loop_record.face as usize].clone();
        let mut coedges = Vec::with_capacity(loop_record.trims.len());
        let coedge_start = staged.draft.model_mut().coedges.len();
        for trim_index in &loop_record.trims {
            let trim = &raw.trims[*trim_index as usize];
            let coedge_id: cadmpeg_ir::ids::CoedgeId =
                format!("rhino:object:coedge#{key}.slot-{trim_index}").into();
            let edge_id = if trim.edge >= 0 {
                edge_ids.get(trim.edge as usize).cloned().ok_or_else(|| {
                    crate::curves::error(trim.source_range.start, "trim edge missing")
                })?
            } else {
                let synthetic_id: cadmpeg_ir::ids::EdgeId =
                    format!("rhino:object:edge#{key}.singular-{trim_index}").into();
                if !synthetic_edges.contains_key(trim_index) {
                    staged.draft.model_mut().edges.push(Edge {
                        id: synthetic_id.clone(),
                        curve: None,
                        start: vertex_ids[trim.vertices[0] as usize].clone(),
                        end: vertex_ids[trim.vertices[0] as usize].clone(),
                        param_range: None,
                        tolerance: scaled_tolerance(trim.tolerances[1], scale)?,
                    });
                    synthetic_edges.insert(*trim_index, synthetic_id.clone());
                }
                synthetic_id
            };
            let pcurve = if trim.trim_type == 6 {
                None
            } else {
                c2.get(trim_index).cloned()
            };
            staged.draft.model_mut().coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: id.clone(),
                edge: edge_id,
                next: coedge_id.clone(),
                previous: coedge_id.clone(),
                radial_next: coedge_id.clone(),
                sense: coedge_sense(
                    trim.reversed_3d != 0,
                    trim.edge >= 0 && raw.edges[trim.edge as usize].proxy_reversed != 0,
                ),
                pcurves: pcurve
                    .into_iter()
                    .map(|pcurve| cadmpeg_ir::topology::PcurveUse {
                        pcurve,
                        isoparametric: None,
                        parameter_range: None,
                    })
                    .collect(),
                use_curve: None,
                use_curve_parameter_range: None,
            });
            coedges.push(coedge_id);
        }
        for offset in 0..coedges.len() {
            let next = coedges[(offset + 1) % coedges.len()].clone();
            let previous = coedges[(offset + coedges.len() - 1) % coedges.len()].clone();
            staged.draft.model_mut().coedges[coedge_start + offset].next = next;
            staged.draft.model_mut().coedges[coedge_start + offset].previous = previous;
        }
        staged.draft.model_mut().loops.push(Loop {
            id: id.clone(),
            face: face_id.clone(),
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            coedges,
            vertex_uses: Vec::new(),
        });
        staged.draft.model_mut().faces[loop_record.face as usize]
            .loops
            .push(id);
    }
    let coedge_positions: BTreeMap<cadmpeg_ir::ids::CoedgeId, usize> = staged
        .draft
        .model()
        .coedges
        .iter()
        .enumerate()
        .map(|(index, coedge)| (coedge.id.clone(), index))
        .collect();
    for edge_index in 0..raw.edges.len() {
        let uses: Vec<_> = raw.edges[edge_index]
            .trims
            .iter()
            .map(|trim| format!("rhino:object:coedge#{key}.slot-{trim}").into())
            .collect::<Vec<cadmpeg_ir::ids::CoedgeId>>();
        if uses.is_empty() {
            continue;
        }
        for (offset, id) in uses.iter().enumerate() {
            let next = uses[(offset + 1) % uses.len()].clone();
            staged.draft.model_mut().coedges[*coedge_positions.get(id).expect("coedge staged")]
                .radial_next = next;
        }
    }
    let mut regions = Vec::new();
    let mut region_shell_ids: BTreeMap<i32, Vec<cadmpeg_ir::ids::ShellId>> = BTreeMap::new();
    for (component, faces) in grouping.shell_faces.iter().enumerate() {
        let region_label = grouping.region_labels[component];
        let region_id: cadmpeg_ir::ids::RegionId =
            format!("rhino:object:region#{key}.slot-{region_label}").into();
        let shell_id: cadmpeg_ir::ids::ShellId =
            format!("rhino:object:shell#{key}.component-{component}").into();
        region_shell_ids
            .entry(region_label)
            .or_default()
            .push(shell_id.clone());
        staged.draft.model_mut().shells.push(Shell {
            id: shell_id,
            region: region_id.clone(),
            faces: faces.iter().map(|index| face_ids[*index].clone()).collect(),
            wire_edges: Vec::new(),
            free_vertices: if component == 0 {
                free_vertex_ids.clone()
            } else {
                Vec::new()
            },
        });
        if !regions.iter().any(|region: &Region| region.id == region_id) {
            regions.push(Region {
                id: region_id,
                body: body_id.clone(),
                shells: Vec::new(),
            });
        }
    }
    for (label, shell_ids) in region_shell_ids {
        if let Some(region) = regions
            .iter_mut()
            .find(|region| region.id == format!("rhino:object:region#{key}.slot-{label}").into())
        {
            region.shells = shell_ids;
        }
    }
    staged.draft.model_mut().regions = regions;
    let body_regions = staged
        .draft
        .model()
        .regions
        .iter()
        .map(|region| region.id.clone())
        .collect();
    staged.draft.model_mut().bodies.push(Body {
        id: body_id.clone(),
        kind: brep_body_kind(raw, writer_version),
        regions: body_regions,
        transform: None,
        name: association.name.clone(),
        color: association.color,
        visible: association.visible,
    });
    staged.links.extend(
        staged
            .draft
            .model()
            .curves
            .iter()
            .map(|curve| curve.id.to_string())
            .chain(
                staged
                    .draft
                    .model()
                    .surfaces
                    .iter()
                    .map(|surface| surface.id.to_string()),
            ),
    );
    staged.links.push(body_id.to_string());
    let derived_ids = {
        let model = staged.draft.model();
        model
            .bodies
            .iter()
            .map(|value| value.id.to_string())
            .chain(model.regions.iter().map(|value| value.id.to_string()))
            .chain(model.shells.iter().map(|value| value.id.to_string()))
            .chain(model.faces.iter().map(|value| value.id.to_string()))
            .chain(model.loops.iter().map(|value| value.id.to_string()))
            .chain(model.coedges.iter().map(|value| value.id.to_string()))
            .chain(model.edges.iter().map(|value| value.id.to_string()))
            .chain(model.vertices.iter().map(|value| value.id.to_string()))
            .chain(model.points.iter().map(|value| value.id.to_string()))
            .chain(model.pcurves.iter().map(|value| value.id.to_string()))
            .collect::<Vec<_>>()
    };
    for id in derived_ids {
        staged.draft.exactness(id, Exactness::Derived);
    }
    scale_plane_pcurves(&mut staged, scale);
    Ok(staged)
}

fn finish_brep_fallback(mut staged: BrepDraft, cause: impl Into<String>) -> BrepDraft {
    staged.links.extend(
        staged
            .draft
            .model()
            .curves
            .iter()
            .map(|curve| curve.id.to_string())
            .chain(
                staged
                    .draft
                    .model()
                    .surfaces
                    .iter()
                    .map(|surface| surface.id.to_string()),
            ),
    );
    staged.free_carrier_fallback(cause)
}

/// Projects one embedded Brep into a self-contained semantic topology value.
pub(crate) fn embedded_brep_json(
    expand: crate::mesh::MeshExpand<'_>,
    data: &[u8],
    range: std::ops::Range<usize>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    scale: f64,
) -> Option<String> {
    let parsed = crate::brep::parse(data, range, archive, writer_version, &[]).ok()?;
    let brep = match parsed {
        crate::brep::BrepParse::Valid(value) => value,
        crate::brep::BrepParse::SemanticInvalid { .. } => return None,
    };
    let association = SourceObjectAssociation {
        format: "rhino".to_string(),
        object_id: "embedded-history-brep".to_string(),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let unknown = UnknownId("rhino:history:embedded-brep".to_string());
    let mut mesh_budget = crate::mesh::MeshBudget::from_session(expand.ctx());
    let staged = stage_brep(BrepTransferInput {
        expand,
        data,
        archive,
        writer_version,
        brep: &brep,
        key: "history:embedded-brep",
        association: &association,
        unknown: &unknown,
        scale,
        mesh_budget: &mut mesh_budget,
    })
    .ok()?;
    if staged.kind != BrepTransferKind::FullTopology {
        return None;
    }
    let model = staged.draft.model();
    serde_json::to_string(&serde_json::json!({
        "kind": "brep",
        "bodies": model.bodies,
        "regions": model.regions,
        "shells": model.shells,
        "faces": model.faces,
        "loops": model.loops,
        "coedges": model.coedges,
        "edges": model.edges,
        "vertices": model.vertices,
        "points": model.points,
        "surfaces": model.surfaces,
        "curves": model.curves,
        "procedural_curves": model.procedural_curves,
        "procedural_surfaces": model.procedural_surfaces,
        "pcurves": model.pcurves,
        "tessellations": model.tessellations,
    }))
    .ok()
}

/// Rhino trim curves live in the surface's native parameter space. A plane's
/// parameters are lengths, so a unit-scaled document moves the plane's
/// parameterization to millimeters while the trims stay in native units;
/// the UV poles of pcurves on plane faces scale to match. NURBS surface
/// parameters are knot-domain values and do not scale.
fn scale_plane_pcurves(staged: &mut BrepDraft, scale: f64) {
    if scale == 1.0 {
        return;
    }
    let plane_surfaces = staged
        .draft
        .model()
        .surfaces
        .iter()
        .filter(|surface| matches!(surface.geometry, SurfaceGeometry::Plane { .. }))
        .map(|surface| surface.id.0.clone())
        .collect::<BTreeSet<_>>();
    let plane_faces = staged
        .draft
        .model()
        .faces
        .iter()
        .filter(|face| plane_surfaces.contains(&face.surface.0))
        .map(|face| face.id.0.clone())
        .collect::<BTreeSet<_>>();
    let plane_loops = staged
        .draft
        .model()
        .loops
        .iter()
        .filter(|value| plane_faces.contains(&value.face.0))
        .map(|value| value.id.0.clone())
        .collect::<BTreeSet<_>>();
    let plane_pcurves = staged
        .draft
        .model()
        .coedges
        .iter()
        .filter(|coedge| plane_loops.contains(&coedge.owner_loop.0))
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| use_.pcurve.0.clone()))
        .collect::<BTreeSet<_>>();
    for pcurve in &mut staged.draft.model_mut().pcurves {
        if !plane_pcurves.contains(&pcurve.id.0) {
            continue;
        }
        if let PcurveGeometry::Nurbs { control_points, .. } = &mut pcurve.geometry {
            for pole in control_points {
                pole.u *= scale;
                pole.v *= scale;
            }
        }
    }
}

fn edge_param_range(edge: &crate::brep::RawBrepEdge) -> [f64; 2] {
    edge.proxy_domain.0
}

fn edge_vertices(edge: &crate::brep::RawBrepEdge) -> [usize; 2] {
    if edge.proxy_reversed != 0 {
        [edge.vertices[1] as usize, edge.vertices[0] as usize]
    } else {
        [edge.vertices[0] as usize, edge.vertices[1] as usize]
    }
}

fn face_sense(face_reversed: bool) -> Sense {
    if face_reversed {
        Sense::Reversed
    } else {
        Sense::Forward
    }
}

fn coedge_sense(reversed_3d: bool, edge_proxy_reversed: bool) -> Sense {
    if reversed_3d ^ edge_proxy_reversed {
        Sense::Reversed
    } else {
        Sense::Forward
    }
}

fn brep_body_kind(raw: &crate::brep::RawBrep, writer_version: Option<i64>) -> BodyKind {
    let closed = !raw.faces.is_empty()
        && raw.edges.iter().enumerate().all(|(edge, _)| {
            raw.trims
                .iter()
                .filter(|trim| trim.edge == edge as i32)
                .count()
                == 2
        });
    serialized_brep_body_kind(raw.minor, raw.is_solid, writer_version, closed)
}

fn serialized_brep_body_kind(
    minor: u8,
    is_solid: Option<i32>,
    writer_version: Option<i64>,
    closed: bool,
) -> BodyKind {
    let stored = (minor >= 2 && writer_version.is_none_or(|version| version >= 200_210_020))
        .then_some(is_solid)
        .flatten();
    match stored {
        Some(1 | 2) => BodyKind::Solid,
        Some(0) => {
            if closed {
                BodyKind::Solid
            } else {
                BodyKind::Sheet
            }
        }
        _ if closed => BodyKind::Solid,
        _ => BodyKind::Sheet,
    }
}

fn stage_brep_procedural_surface(
    staged: &mut BrepDraft,
    index: usize,
    geometry: cadmpeg_ir::geometry::NurbsSurface,
    definition: crate::surfaces::DecodedProceduralSurface,
    children: Vec<crate::curves::DecodedCurve>,
    context: &BrepStageContext<'_>,
) -> Result<cadmpeg_ir::ids::SurfaceId, crate::curves::GeometryError> {
    let expected_children = match definition {
        crate::surfaces::DecodedProceduralSurface::Revolution { .. } => 1,
        crate::surfaces::DecodedProceduralSurface::Sum { .. } => 2,
    };
    if children.len() != expected_children {
        return Err(crate::curves::error(
            0,
            "procedural surface child count mismatch",
        ));
    }
    let child_ids = children
        .into_iter()
        .enumerate()
        .map(|(child_index, child)| {
            stage_curve_tree(
                staged,
                child,
                context.key,
                &format!("surface-{index}.child-{child_index}"),
                context.association,
                context.unknown,
            )
        })
        .collect::<Vec<_>>();
    let surface_id: cadmpeg_ir::ids::SurfaceId =
        format!("rhino:object:surface#{}.slot-{index}", context.key).into();
    staged.draft.model_mut().surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Nurbs(geometry),
        source_object: Some(context.association.clone()),
    });
    let definition = match definition {
        crate::surfaces::DecodedProceduralSurface::Revolution {
            axis_origin,
            axis_direction,
            angular_interval,
            parameter_interval,
            transposed,
        } => ProceduralSurfaceDefinition::Revolution {
            directrix: child_ids[0].clone(),
            axis_origin,
            axis_direction,
            angular_interval,
            angular_parameter_interval: None,
            parameter_interval: Some(parameter_interval),
            transposed,
            revision_form: None,
        },
        crate::surfaces::DecodedProceduralSurface::Sum { basepoint } => {
            ProceduralSurfaceDefinition::Sum {
                first: child_ids[0].clone(),
                second: child_ids[1].clone(),
                basepoint,
                revision_form: None,
            }
        }
    };
    let procedural_id: cadmpeg_ir::ids::ProceduralSurfaceId = format!(
        "rhino:object:procedural-surface#{}.slot-{index}",
        context.key
    )
    .into();
    staged
        .draft
        .model_mut()
        .procedural_surfaces
        .push(ProceduralSurface {
            id: procedural_id.clone(),
            surface: surface_id.clone(),
            definition,
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    staged
        .draft
        .exactness(surface_id.to_string(), Exactness::Derived);
    staged
        .draft
        .exactness(procedural_id.to_string(), Exactness::Derived);
    staged.links.push(surface_id.to_string());
    staged.links.push(procedural_id.to_string());
    Ok(surface_id)
}

fn stage_curve_tree(
    staged: &mut BrepDraft,
    curve: crate::curves::DecodedCurve,
    key: &str,
    path: &str,
    association: &SourceObjectAssociation,
    unknown: &UnknownId,
) -> cadmpeg_ir::ids::CurveId {
    let mut component_ids = Vec::new();
    if let Some(compound) = &curve.compound {
        for (index, child) in compound.children.iter().cloned().enumerate() {
            component_ids.push(stage_curve_tree(
                staged,
                child,
                key,
                &format!("{path}.component-{index}"),
                association,
                unknown,
            ));
        }
    }
    let id: cadmpeg_ir::ids::CurveId = format!("rhino:object:curve#{key}.{path}").into();
    let geometry = if curve.compound.is_some() {
        CurveGeometry::Unknown {
            record: Some(unknown.clone()),
        }
    } else {
        curve.geometry
    };
    staged.draft.model_mut().curves.push(Curve {
        id: id.clone(),
        geometry,
        source_object: Some(association.clone()),
    });
    staged.draft.exactness(id.to_string(), Exactness::Derived);
    staged.links.push(id.to_string());
    if let Some(compound) = curve.compound {
        let procedure_id: cadmpeg_ir::ids::ProceduralCurveId =
            format!("rhino:object:procedural-curve#{key}.{path}").into();
        staged
            .draft
            .exactness(procedure_id.to_string(), Exactness::Derived);
        staged_links_procedure(
            staged,
            ProceduralCurve {
                id: procedure_id,
                curve: id.clone(),
                definition: ProceduralCurveDefinition::Compound {
                    parameters: compound.parameters.clone(),
                    component_parameters: compound.parameters[..compound.parameters.len() - 1]
                        .to_vec(),
                    components: component_ids,
                },
                cache_fit_tolerance: None,
            },
        );
    }
    id
}

fn staged_links_procedure(staged: &mut BrepDraft, procedure: ProceduralCurve) {
    staged.links.push(procedure.id.to_string());
    staged.draft.model_mut().procedural_curves.push(procedure);
}

fn decode_pcurves(
    data: &[u8],
    archive: ArchiveVersion,
    raw: &crate::brep::RawBrep,
    key: &str,
    plane_parameterizations: &BTreeMap<i32, crate::surfaces::PlaneParameterization>,
) -> (
    BTreeMap<i32, cadmpeg_ir::ids::PcurveId>,
    Vec<Pcurve>,
    Vec<String>,
) {
    let mut ids = BTreeMap::new();
    let mut values = Vec::new();
    let mut decoded_slots = BTreeMap::<i32, Option<NurbsCurve>>::new();
    let mut warnings = Vec::new();
    for (index, trim) in raw.trims.iter().enumerate() {
        if trim.trim_type == 6 {
            continue;
        }
        let nurbs = if let Some(nurbs) = decoded_slots.get(&trim.curve) {
            let Some(nurbs) = nurbs else { continue };
            nurbs.clone()
        } else {
            let decoded = (|| -> Result<crate::curves::NurbsJoin, crate::curves::GeometryError> {
                let child = raw
                    .c2
                    .slots
                    .get(trim.curve as usize)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| {
                        crate::curves::error(trim.source_range.start, "trim C2 slot missing")
                    })?;
                let decoded = crate::curves::decode_2d(
                    data,
                    child.class_uuid,
                    child.class_data_range.clone(),
                    archive,
                )?;
                let crate::curves::DecodedGeometry::Curve { curve } = decoded else {
                    return Err(crate::curves::error(
                        trim.source_range.start,
                        "C2 child is not a curve",
                    ));
                };
                c2_curve_to_nurbs_join(curve, trim.source_range.start)
            })();
            match decoded {
                Ok(joined) => {
                    warnings.extend(
                        joined
                            .warnings
                            .into_iter()
                            .map(|warning| format!("trim {index}: {warning}")),
                    );
                    decoded_slots.insert(trim.curve, Some(joined.curve.clone()));
                    joined.curve
                }
                Err(error) => {
                    warnings.push(format!("trim {index} C2 omitted: {error}"));
                    decoded_slots.insert(trim.curve, None);
                    continue;
                }
            }
        };
        let plane_parameterization = raw
            .loops
            .get(trim.loop_index as usize)
            .and_then(|loop_record| raw.faces.get(loop_record.face as usize))
            .and_then(|face| plane_parameterizations.get(&face.surface).copied());
        let control_points = nurbs
            .control_points
            .into_iter()
            .map(|point| {
                let point = Point2::new(point.x, point.y);
                plane_parameterization.map_or(point, |map| map.map_point(point))
            })
            .collect();
        let id: cadmpeg_ir::ids::PcurveId =
            format!("rhino:object:pcurve#{key}.trim-{index}").into();
        values.push(Pcurve {
            id: id.clone(),
            geometry: PcurveGeometry::Nurbs {
                degree: nurbs.degree,
                knots: nurbs.knots,
                control_points,
                weights: nurbs.weights,
                periodic: nurbs.periodic,
            },
            wrapper_reversed: Some(trim.proxy_reversed != 0),
            native_tail_flags: None,
            parameter_range: Some(trim.domain.0),
            fit_tolerance: finite_tolerance(trim.tolerances[0]),
        });
        ids.insert(index as i32, id);
    }
    (ids, values, warnings)
}

fn c2_curve_to_nurbs_join(
    curve: crate::curves::DecodedCurve,
    offset: usize,
) -> Result<crate::curves::NurbsJoin, crate::curves::GeometryError> {
    let Some(compound) = curve.compound else {
        return match curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Ok(crate::curves::NurbsJoin {
                curve: nurbs,
                warnings: Vec::new(),
            }),
            _ => Err(crate::curves::error(
                offset,
                "C2 child has no parameter-space representation",
            )),
        };
    };
    if compound.children.len().checked_add(1) != Some(compound.parameters.len()) {
        return Err(crate::curves::error(
            offset,
            "C2 polycurve parameter count mismatch",
        ));
    }
    let mut segments = Vec::with_capacity(compound.children.len());
    let mut warnings = Vec::new();
    for (index, child) in compound.children.into_iter().enumerate() {
        let target = [compound.parameters[index], compound.parameters[index + 1]];
        if !target[0].is_finite() || !target[1].is_finite() || target[0] >= target[1] {
            return Err(crate::curves::error(
                offset,
                "C2 polycurve segment domain is invalid",
            ));
        }
        let joined = c2_curve_to_nurbs_join(child, offset)?;
        warnings.extend(joined.warnings);
        segments.push(crate::curves::remap_nurbs_domain(
            joined.curve,
            target,
            offset,
        )?);
    }
    let mut joined = crate::curves::join_nurbs_segments(segments, offset)?;
    warnings.append(&mut joined.warnings);
    joined.warnings = warnings;
    Ok(joined)
}

fn finite_tolerance(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

fn scaled_tolerance(value: f64, scale: f64) -> Result<Option<f64>, crate::curves::GeometryError> {
    if !value.is_finite() || value <= 0.0 {
        return Ok(None);
    }
    let scaled = crate::wire::scaled_coordinate(value, scale)
        .ok_or_else(|| crate::curves::error(0, "scaled tolerance is invalid"))?;
    Ok(Some(scaled))
}

fn face_components(raw: &crate::brep::RawBrep) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..raw.faces.len()).collect();
    for edge in &raw.edges {
        let faces: Vec<usize> = edge
            .trims
            .iter()
            .map(|trim| raw.loops[raw.trims[*trim as usize].loop_index as usize].face as usize)
            .collect();
        for pair in faces.windows(2) {
            let left = disjoint_root(&mut parent, pair[0]);
            let right = disjoint_root(&mut parent, pair[1]);
            parent[left] = right;
        }
    }
    let roots: Vec<usize> = (0..parent.len())
        .map(|index| disjoint_root(&mut parent, index))
        .collect();
    let mut labels = BTreeMap::new();
    roots
        .into_iter()
        .map(|value| {
            let next = labels.len();
            *labels.entry(value).or_insert(next)
        })
        .collect()
}

fn brep_free_vertex_indices(
    raw: &crate::brep::RawBrep,
) -> Result<Vec<usize>, crate::curves::GeometryError> {
    let mut attached = alloc_filled(
        raw.vertices.len(),
        false,
        "Rhino Brep free-vertex attachment flags",
    )
    .map_err(|error| {
        crate::curves::GeometryError::malformed(
            0,
            format!("Brep free-vertex allocation refused: {error}"),
        )
    })?;
    for (index, vertex) in raw.vertices.iter().enumerate() {
        if !vertex.edges.is_empty() {
            attached[index] = true;
        }
    }
    for trim in &raw.trims {
        if trim.edge < 0 {
            attached[trim.vertices[0] as usize] = true;
        }
    }
    Ok(attached
        .into_iter()
        .enumerate()
        .filter_map(|(index, attached)| (!attached).then_some(index))
        .collect())
}

struct ShellGrouping {
    face_groups: Vec<usize>,
    region_labels: Vec<i32>,
    shell_faces: Vec<Vec<usize>>,
    fallback: bool,
}

fn region_shell_groups(
    raw: &crate::brep::RawBrep,
    components: &[usize],
) -> Result<ShellGrouping, crate::curves::GeometryError> {
    if raw.minor < 3 || raw.regions.is_empty() {
        let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (face, component) in components.iter().copied().enumerate() {
            groups.entry(component).or_default().push(face);
        }
        let mut shell_faces = Vec::new();
        let mut face_groups =
            alloc_filled(components.len(), 0usize, "Rhino Brep fallback face groups").map_err(
                |error| {
                    crate::curves::GeometryError::malformed(
                        0,
                        format!("Brep face-group allocation refused: {error}"),
                    )
                },
            )?;
        let mut region_labels = Vec::new();
        for (group, (component, faces)) in groups.into_iter().enumerate() {
            for face in &faces {
                face_groups[*face] = group;
            }
            let _ = component;
            shell_faces.push(faces);
            region_labels.push(group as i32);
        }
        return Ok(ShellGrouping {
            face_groups,
            region_labels,
            shell_faces,
            fallback: false,
        });
    }
    let mut grouped: BTreeMap<(i32, usize), Vec<usize>> = BTreeMap::new();
    let solid_regions: BTreeSet<i32> = raw
        .regions
        .iter()
        .filter(|region| region.region_type == 1)
        .map(|region| region.index)
        .collect();
    for face in 0..raw.faces.len() {
        let bounded_sides: Vec<_> = raw
            .face_sides
            .iter()
            .filter(|side| side.face == face as i32)
            .filter(|side| solid_regions.contains(&side.region))
            .collect();
        if bounded_sides.len() != 1 {
            return region_shell_groups_without_records(components);
        }
        let side = bounded_sides[0];
        let region = side.region;
        grouped
            .entry((region, components[face]))
            .or_default()
            .push(face);
    }
    let mut face_groups = alloc_filled(components.len(), 0usize, "Rhino Brep region face groups")
        .map_err(|error| {
        crate::curves::GeometryError::malformed(
            0,
            format!("Brep face-group allocation refused: {error}"),
        )
    })?;
    let mut region_labels = Vec::new();
    let mut shell_faces = Vec::new();
    for (group, ((region, _component), faces)) in grouped.into_iter().enumerate() {
        for face in &faces {
            face_groups[*face] = group;
        }
        region_labels.push(region);
        shell_faces.push(faces);
    }
    Ok(ShellGrouping {
        face_groups,
        region_labels,
        shell_faces,
        fallback: false,
    })
}

fn region_shell_groups_without_records(
    components: &[usize],
) -> Result<ShellGrouping, crate::curves::GeometryError> {
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (face, component) in components.iter().copied().enumerate() {
        groups.entry(component).or_default().push(face);
    }
    let mut face_groups =
        alloc_filled(components.len(), 0usize, "Rhino Brep incidence face groups").map_err(
            |error| {
                crate::curves::GeometryError::malformed(
                    0,
                    format!("Brep face-group allocation refused: {error}"),
                )
            },
        )?;
    let mut region_labels = Vec::new();
    let mut shell_faces = Vec::new();
    for (group, (_component, faces)) in groups.into_iter().enumerate() {
        for face in &faces {
            face_groups[*face] = group;
        }
        region_labels.push(group as i32);
        shell_faces.push(faces);
    }
    Ok(ShellGrouping {
        face_groups,
        region_labels,
        shell_faces,
        fallback: true,
    })
}

fn disjoint_root(parent: &mut [usize], mut value: usize) -> usize {
    while parent[value] != value {
        parent[value] = parent[parent[value]];
        value = parent[value];
    }
    value
}

fn curve_warnings(curve: &crate::curves::DecodedCurve) -> Vec<String> {
    let mut warnings = curve.warnings.clone();
    if let Some(compound) = &curve.compound {
        for child in &compound.children {
            warnings.extend(curve_warnings(child));
        }
    }
    warnings
}

fn commit_curve_tree(
    ir: &mut CadIr,
    annotations: &mut cadmpeg_ir::Annotations,
    curve: crate::curves::DecodedCurve,
    key: &str,
    association: &SourceObjectAssociation,
    record: Option<UnknownId>,
    path: &str,
) -> cadmpeg_ir::ids::CurveId {
    let mut component_ids = Vec::new();
    if let Some(compound) = &curve.compound {
        for (index, child) in compound.children.iter().cloned().enumerate() {
            let child_path = format!("{path}.component-{index}");
            component_ids.push(commit_curve_tree(
                ir,
                annotations,
                child,
                key,
                association,
                None,
                &child_path,
            ));
        }
    }
    let id: cadmpeg_ir::ids::CurveId = if path == "root" {
        format!("rhino:object:curve#{key}").into()
    } else {
        format!("rhino:object:curve#{key}.{path}").into()
    };
    let geometry = if curve.compound.is_some() {
        CurveGeometry::Unknown { record }
    } else {
        curve.geometry
    };
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry,
        source_object: Some(association.clone()),
    });
    annotations.exactness.insert(
        id.to_string(),
        ExactnessNote {
            entity: Exactness::Derived,
            fields: BTreeMap::new(),
        },
    );
    if let Some(compound) = curve.compound {
        let procedure_id: cadmpeg_ir::ids::ProceduralCurveId = if path == "root" {
            format!("rhino:object:procedural-curve#{key}").into()
        } else {
            format!("rhino:object:procedural-curve#{key}.{path}").into()
        };
        ir.model.procedural_curves.push(ProceduralCurve {
            id: procedure_id,
            curve: id.clone(),
            definition: ProceduralCurveDefinition::Compound {
                parameters: compound.parameters.clone(),
                component_parameters: compound.parameters[..compound.parameters.len() - 1].to_vec(),
                components: component_ids,
            },
            cache_fit_tolerance: None,
        });
    }
    id
}

fn decoded_curve_entity_count(curve: &crate::curves::DecodedCurve) -> usize {
    let child_count = curve.compound.as_ref().map_or(0, |compound| {
        compound
            .children
            .iter()
            .map(decoded_curve_entity_count)
            .fold(0_usize, usize::saturating_add)
    });
    child_count
        .saturating_add(1)
        .saturating_add(usize::from(curve.compound.is_some()))
}

fn compose_body_transform(body: &mut Body, transform: Transform) {
    body.transform = Some(match body.transform {
        Some(existing) => transform.compose(existing),
        None => transform,
    });
}

fn hatch_plane_transform(plane: &crate::settings::Plane, scale: f64) -> Transform {
    let origin = plane.origin.0;
    let x = plane.xaxis.0;
    let y = plane.yaxis.0;
    let z = plane.zaxis.0;
    Transform {
        rows: [
            [x[0] * scale, y[0] * scale, z[0] * scale, origin[0] * scale],
            [x[1] * scale, y[1] * scale, z[1] * scale, origin[1] * scale],
            [x[2] * scale, y[2] * scale, z[2] * scale, origin[2] * scale],
            [0.0, 0.0, 0.0, 1.0],
        ],
    }
}

fn transform_decoded_curve(
    curve: &mut crate::curves::DecodedCurve,
    transform: Transform,
) -> Result<(), String> {
    if let Some(compound) = &mut curve.compound {
        for child in &mut compound.children {
            transform_decoded_curve(child, transform)?;
        }
        return Ok(());
    }
    let geometry = std::mem::replace(&mut curve.geometry, CurveGeometry::Unknown { record: None });
    let mut carrier = Curve {
        id: "rhino:hatch:placement".into(),
        geometry,
        source_object: None,
    };
    transform_curve(&mut carrier, transform)?;
    curve.geometry = carrier.geometry;
    Ok(())
}

fn transform_curve(curve: &mut Curve, transform: Transform) -> Result<(), String> {
    let geometry = std::mem::replace(&mut curve.geometry, CurveGeometry::Unknown { record: None });
    curve.geometry = match geometry {
        CurveGeometry::Nurbs(mut nurbs) => {
            for pole in &mut nurbs.control_points {
                *pole = transform.apply_point(*pole);
            }
            CurveGeometry::Nurbs(nurbs)
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let decoded = crate::curves::DecodedCurve {
                geometry: CurveGeometry::Circle {
                    center,
                    axis,
                    ref_direction,
                    radius,
                },
                compound: None,
                warnings: Vec::new(),
            };
            let mut nurbs = crate::curves::exact_nurbs(&decoded, 0)
                .map_err(|error| format!("analytic instance curve conversion failed: {error}"))?;
            for pole in &mut nurbs.control_points {
                *pole = transform.apply_point(*pole);
            }
            CurveGeometry::Nurbs(nurbs)
        }
        CurveGeometry::Line { origin, direction } => {
            let transformed_origin = transform.apply_point(origin);
            let endpoint = transform.apply_point(Point3::new(
                origin.x + direction.x,
                origin.y + direction.y,
                origin.z + direction.z,
            ));
            let value = cadmpeg_ir::math::Vector3::new(
                endpoint.x - transformed_origin.x,
                endpoint.y - transformed_origin.y,
                endpoint.z - transformed_origin.z,
            );
            let norm = value.norm();
            if !norm.is_finite() || norm == 0.0 {
                return Err("instance line transform collapsed its direction".to_string());
            }
            CurveGeometry::Line {
                origin: transformed_origin,
                direction: cadmpeg_ir::math::Vector3::new(
                    value.x / norm,
                    value.y / norm,
                    value.z / norm,
                ),
            }
        }
        CurveGeometry::Degenerate { point } => CurveGeometry::Degenerate {
            point: transform.apply_point(point),
        },
        CurveGeometry::Unknown { record } => {
            curve.geometry = CurveGeometry::Unknown { record };
            return Err("unknown free curve cannot be transformed exactly".to_string());
        }
        other => {
            curve.geometry = other;
            return Err(
                "analytic curve family has no exact general-affine instance conversion".to_string(),
            );
        }
    };
    Ok(())
}

fn transform_surface(surface: &mut Surface, transform: Transform) -> Result<(), String> {
    let geometry = std::mem::replace(
        &mut surface.geometry,
        SurfaceGeometry::Unknown { record: None },
    );
    surface.geometry = match geometry {
        SurfaceGeometry::Nurbs(mut nurbs) => {
            for pole in &mut nurbs.control_points {
                *pole = transform.apply_point(*pole);
            }
            SurfaceGeometry::Nurbs(nurbs)
        }
        SurfaceGeometry::Plane {
            origin: source_origin,
            normal,
            u_axis,
        } => {
            let origin = transform.apply_point(source_origin);
            let normal = transform
                .apply_normal(normal)
                .ok_or_else(|| "instance plane normal transform is singular".to_string())?;
            let endpoint = transform.apply_point(Point3::new(
                source_origin.x + u_axis.x,
                source_origin.y + u_axis.y,
                source_origin.z + u_axis.z,
            ));
            let projected = cadmpeg_ir::math::Vector3::new(
                endpoint.x - origin.x,
                endpoint.y - origin.y,
                endpoint.z - origin.z,
            );
            let dot = projected.x * normal.x + projected.y * normal.y + projected.z * normal.z;
            let value = cadmpeg_ir::math::Vector3::new(
                projected.x - dot * normal.x,
                projected.y - dot * normal.y,
                projected.z - dot * normal.z,
            );
            let length = value.norm();
            if !length.is_finite() || length == 0.0 {
                return Err("instance plane transform collapsed its frame".to_string());
            }
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis: cadmpeg_ir::math::Vector3::new(
                    value.x / length,
                    value.y / length,
                    value.z / length,
                ),
            }
        }
        SurfaceGeometry::Unknown { record } => {
            surface.geometry = SurfaceGeometry::Unknown { record };
            return Err("unknown free surface cannot be transformed exactly".to_string());
        }
        other => {
            surface.geometry = other;
            return Err(
                "analytic surface family has no exact general-affine instance conversion"
                    .to_string(),
            );
        }
    };
    Ok(())
}

fn source_association(
    identity: &crate::objects::SourceIdentity,
    instance_path: &[String],
    parent_color: Option<Color>,
    parent_visible: Option<bool>,
) -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: "rhino".to_string(),
        object_id: identity.object_id.to_string(),
        name: (!identity.name.is_empty()).then(|| identity.name.clone()),
        color: identity.effective_color.map(color).or(parent_color),
        visible: Some(parent_visible.unwrap_or(true) && identity.effective_visible),
        layer: identity
            .layer_id
            .map(|id| id.to_string())
            .or_else(|| identity.layer_name.clone()),
        instance_path: instance_path.to_vec(),
    }
}

fn color(value: [u8; 4]) -> Color {
    Color {
        r: f32::from(value[0]) / 255.0,
        g: f32::from(value[1]) / 255.0,
        b: f32::from(value[2]) / 255.0,
        a: 1.0 - f32::from(value[3]) / 255.0,
    }
}

fn body(
    identity: &crate::objects::SourceIdentity,
    id: cadmpeg_ir::ids::BodyId,
    regions: Vec<cadmpeg_ir::ids::RegionId>,
    association: &SourceObjectAssociation,
) -> Body {
    Body {
        id,
        kind: BodyKind::General,
        regions,
        transform: None,
        name: (!identity.name.is_empty()).then(|| identity.name.clone()),
        color: association.color,
        visible: association.visible,
    }
}

fn loss_provenance(class: &str, outcome: &ClassOutcome) -> SourceProvenance {
    SourceProvenance {
        format: "rhino".to_string(),
        stream: String::new(),
        offset: outcome.first_offset,
        tag: Some(format!(
            "OBJECT_RECORD/class={class}/type=0x{:08x}",
            outcome.first_object_type
        )),
    }
}

/// Builds the metadata-only Rhino decode transaction.
pub(crate) fn decode(scan: &Scan<'_>, expand: crate::mesh::MeshExpand<'_>) -> DecodeResult {
    let mut context = DecodeContext::new(scan, expand);
    context.decode_geometry();
    context.decode_dimensions();
    let geometry_context = context.unit_scale().map(|scale| {
        (
            expand,
            scan.archive,
            scan.metadata.properties.writer_version,
            scale,
        )
    });
    let untyped = context.validate_candidate(|candidate, _annotations| {
        crate::history::project(&scan.history, geometry_context, candidate)
    });
    match untyped {
        Ok((0, 0, 0, 0)) => {}
        Ok((untyped, failed, dropped_dependencies, redundant_repairs)) => {
            if untyped != 0 {
                context
                    .typed_losses
                    .push(RhinoLossCode::HistoryGeometryNotTransferred.note(format!(
                        "{untyped} history value(s) decoded without a neutral carrier"
                    )));
            }
            if failed != 0 {
                context
                    .typed_losses
                    .push(RhinoLossCode::HistoryEmbeddedGeometryDropped.note(format!(
                        "{failed} embedded history geometry value(s) could not be decoded"
                    )));
            }
            if dropped_dependencies != 0 {
                context
                    .typed_losses
                    .push(RhinoLossCode::HistoryDependencyDropped.note(format!(
                        "{dropped_dependencies} history dependency edge(s) point to later or ambiguous producers"
                    )));
            }
            if redundant_repairs != 0 {
                context
                    .typed_losses
                    .push(RhinoLossCode::RedundantFieldRepaired.note(format!(
                        "{redundant_repairs} history geometry optional channel repair(s)"
                    )));
            }
        }
        Err(error) => context.scan_warnings_for_class(
            "history",
            &format!("history projection rejected atomically by IR validation: {error}"),
        ),
    }
    context.commit()
}

#[cfg(test)]
pub(crate) fn with_expand_bytes<R>(
    data: &[u8],
    f: impl FnOnce(crate::mesh::MeshExpand<'_>) -> R,
) -> R {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
        .expect("root view");
    f(crate::mesh::MeshExpand::new(&ctx, root))
}

#[cfg(test)]
pub(crate) fn with_expand<R>(
    scan: &Scan<'_>,
    f: impl FnOnce(crate::mesh::MeshExpand<'_>) -> R,
) -> R {
    with_expand_bytes(scan.data, f)
}

#[cfg(test)]
pub(crate) fn decode_for_test(scan: &Scan<'_>) -> DecodeResult {
    with_expand(scan, |expand| decode(scan, expand))
}

fn build_ir(scan: &Scan<'_>) -> CadIr {
    let units = Units::default();
    let mut ir = CadIr::empty(units);
    ir.source = Some(source_meta(scan));
    if let Some(source_units) = &scan.metadata.settings.units {
        if let Some(linear) = source_units.absolute_tolerance_millimeters {
            ir.tolerances.linear = linear;
        }
        ir.tolerances.angular = source_units.angular_tolerance;
    }
    ir
}

fn source_meta(scan: &Scan<'_>) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "archive_version".to_string(),
        scan.archive.value().to_string(),
    );
    attributes.insert("container_kind".to_string(), "3dm-chunks".to_string());
    let settings = &scan.metadata.settings;
    if let Some(units) = &settings.units {
        attributes.insert("unit_value".to_string(), units.unit_value.to_string());
        attributes.insert(
            "unit_system".to_string(),
            match &units.unit {
                crate::settings::UnitSystem::None => "none".to_string(),
                crate::settings::UnitSystem::Unset => "unset".to_string(),
                crate::settings::UnitSystem::Standard(value) => format!("standard:{value}"),
                crate::settings::UnitSystem::Custom { name, .. } => format!("custom:{name}"),
            },
        );
        if let crate::settings::UnitSystem::Custom {
            meters_per_unit,
            name,
        } = &units.unit
        {
            attributes.insert("custom_unit_name".to_string(), name.clone());
            attributes.insert(
                "custom_meters_per_unit".to_string(),
                meters_per_unit.to_string(),
            );
        }
        if let Some(scale) = units.millimeters_per_unit {
            attributes.insert("millimeters_per_unit".to_string(), scale.to_string());
        }
        attributes.insert(
            "absolute_tolerance_native".to_string(),
            units.absolute_tolerance.to_string(),
        );
        attributes.insert(
            "absolute_tolerance_millimeters".to_string(),
            units
                .absolute_tolerance_millimeters
                .map_or_else(|| "unresolved".to_string(), |value| value.to_string()),
        );
        attributes.insert(
            "angular_tolerance".to_string(),
            units.angular_tolerance.to_string(),
        );
        attributes.insert(
            "relative_tolerance".to_string(),
            units.relative_tolerance.to_string(),
        );
        if let Some(mode) = units.distance_display_mode {
            attributes.insert("distance_display_mode".to_string(), mode.to_string());
        }
        if let Some(precision) = units.distance_display_precision {
            attributes.insert(
                "distance_display_precision".to_string(),
                precision.to_string(),
            );
        }
    }
    if let Some(application) = &scan.metadata.properties.application {
        attributes.insert("application_name".to_string(), application.name.clone());
        attributes.insert("application_url".to_string(), application.url.clone());
        attributes.insert(
            "application_details".to_string(),
            application.details.clone(),
        );
    }
    if let Some(current) = settings.current_layer {
        attributes.insert("current_layer".to_string(), current.to_string());
    }
    if let Some(current) = settings.current_material {
        attributes.insert("current_material".to_string(), current.to_string());
    }
    if let Some(current) = settings.current_material_source {
        attributes.insert("current_material_source".to_string(), current.to_string());
    }
    if let Some(current) = settings.current_color {
        attributes.insert(
            "current_color".to_string(),
            current
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    if let Some(current) = settings.current_color_source {
        attributes.insert("current_color_source".to_string(), current.to_string());
    }
    if let Some(current) = settings.current_wire_density {
        attributes.insert("current_wire_density".to_string(), current.to_string());
    }
    if let Some(current) = settings.current_font {
        attributes.insert("current_font".to_string(), current.to_string());
    }
    if let Some(current) = settings.current_dimstyle {
        attributes.insert("current_dimstyle".to_string(), current.to_string());
    }
    if let Some(url) = &settings.model_url {
        attributes.insert("model_url".to_string(), url.clone());
    }
    for layer in &scan.metadata.layers {
        let prefix = format!("layer.{}", layer.index);
        attributes.insert(format!("{prefix}.name"), layer.name.clone());
        attributes.insert(format!("{prefix}.visible"), layer.visible.to_string());
        attributes.insert(format!("{prefix}.locked"), layer.locked.to_string());
        if let Some(id) = layer.id {
            attributes.insert(format!("{prefix}.uuid"), id.to_string());
        }
    }
    SourceMeta {
        declared: BTreeMap::new(),
        dialect: None,
        format: "rhino".to_string(),
        attributes,
    }
}

#[cfg(test)]
mod tests;
