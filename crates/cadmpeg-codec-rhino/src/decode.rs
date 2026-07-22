// SPDX-License-Identifier: Apache-2.0
//! Decode Rhino metadata and retain object records for later geometry phases.

use cadmpeg_ir::annotations::ExactnessNote;
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossCode, LossNote, Severity};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Color, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::{NativeUnknownRecord, UnknownRecord};
use cadmpeg_ir::LossProvenance;
use cadmpeg_ir::{Exactness, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

use crate::chunks::ArchiveVersion;
use crate::container::Scan;
use crate::objects::ObjectDescriptor;

/// Maximum bytes retained for one Rhino object record.
pub(crate) const RETAINED_RECORD_CAP: usize = 16 * 1024 * 1024;
/// Maximum bytes retained across all Rhino object records in one document.
pub(crate) const RETAINED_DOCUMENT_CAP: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
struct ClassOutcome {
    decoded: usize,
    retained: usize,
    attribute_degraded: usize,
    failed_framed: usize,
    first_offset: u64,
    first_object_type: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeometryStatus {
    Retained,
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
}

impl ArenaLengths {
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
        }
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
        ]
        .into_iter()
        .try_fold(0_usize, |total, (after, before)| {
            total.checked_add(after.checked_sub(before)?)
        })
    }
}

// Instance expansion re-enters `decode_geometry` per member, which reaches the mesh decoder's
// `begin_expand` path; these caps bound reference, member, and entity
// amplification from a hostile definition graph independently of the platform
// budget, and are kept as defense in depth.
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
    statuses: Vec<GeometryStatus>,
    outcomes: BTreeMap<String, ClassOutcome>,
    retained_bytes: usize,
    retention_limits: [usize; 2],
    mesh_budget: crate::mesh::MeshBudget,
    geometry_transferred: bool,
    phase_warnings: Vec<String>,
    /// Typed loss notes raised at semantic conversion boundaries. Distinct
    /// from `phase_warnings`, which aggregate into
    /// untyped `DecodeDiagnostic` notes; these carry the boundary's own loss
    /// code and drain into the report's `losses` at finalization.
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
            statuses: Vec::with_capacity(scan.objects.len()),
            outcomes: BTreeMap::new(),
            retained_bytes: 0,
            retention_limits: [RETAINED_RECORD_CAP, RETAINED_DOCUMENT_CAP],
            mesh_budget: crate::mesh::MeshBudget::new(),
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
            expansion_budget: ExpansionBudget::new(),
        };
        context.retain_object_records();
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
        self.statuses.clear();
        self.outcomes.clear();
        self.retained_bytes = 0;
        self.retain_object_records();
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
        self.scan
            .metadata
            .settings
            .units
            .as_ref()?
            .millimeters_per_unit
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

    fn validate_candidate(
        &mut self,
        apply: impl FnOnce(&mut CadIr, &mut cadmpeg_ir::Annotations),
    ) -> Result<(), String> {
        let mut candidate = self.lightweight_candidate();
        let mut annotations = self.annotations.clone();
        apply(&mut candidate, &mut annotations);
        self.commit_valid_candidate(candidate, annotations)
    }

    fn lightweight_candidate(&mut self) -> CadIr {
        let mut candidate = self.ir.clone();
        let unknowns = self
            .unknowns
            .iter()
            .map(NativeUnknownRecord::from)
            .collect::<Vec<_>>();
        candidate
            .set_native_unknowns("rhino", &unknowns)
            .expect("Rhino unknown records serialize");
        candidate
    }

    fn commit_valid_candidate(
        &mut self,
        mut candidate: CadIr,
        annotations: cadmpeg_ir::Annotations,
    ) -> Result<(), String> {
        candidate.model.finalize();
        let source_fidelity = cadmpeg_ir::SourceFidelity {
            annotations: annotations.clone(),
            ..cadmpeg_ir::SourceFidelity::default()
        };
        let validation =
            cadmpeg_ir::validate_with_source_fidelity(&candidate, &source_fidelity, Vec::new());
        if validation.is_ok() {
            let added = ArenaLengths::capture(&candidate)
                .added_since(ArenaLengths::capture(&self.ir))
                .ok_or_else(|| "candidate removed existing IR entities".to_string())?;
            self.expansion_budget.entities(added)?;
            let unknowns = candidate
                .native_unknowns("rhino")
                .map_err(|error| error.to_string())?;
            for reference in unknowns {
                let record = self
                    .unknowns
                    .iter_mut()
                    .find(|record| record.id == reference.id)
                    .ok_or_else(|| format!("candidate introduced unknown {}", reference.id))?;
                record.links = reference.links;
            }
            self.ir = candidate;
            self.annotations = annotations;
            Ok(())
        } else {
            Err(validation_findings(&validation))
        }
    }

    fn lightweight_context_candidate(&mut self) -> Self {
        let payloads = detach_unknown_payloads(&mut self.unknowns);
        let candidate = self.clone();
        restore_unknown_payloads(&mut self.unknowns, payloads);
        candidate
    }

    fn transfer_unknown_payloads_to(&mut self, candidate: &mut Self) {
        let payloads = detach_unknown_payloads(&mut self.unknowns);
        restore_unknown_payloads(&mut candidate.unknowns, payloads);
    }

    /// Returns mutable IR for the current decode transaction.
    #[cfg(test)]
    pub(crate) fn ir_mut(&mut self) -> &mut CadIr {
        &mut self.ir
    }

    #[cfg(test)]
    pub(crate) fn reject_duplicate_unknown_candidate(&mut self) -> (bool, String) {
        let mut payloads_detached = false;
        let result = self.validate_candidate(|candidate, _annotations| {
            let mut unknowns = candidate
                .native_unknowns("rhino")
                .expect("required invariant");
            payloads_detached = unknowns.iter().all(|record| {
                let value = serde_json::to_value(record).expect("required invariant");
                value.get("data").is_none()
            });
            if let Some(record) = unknowns.first().cloned() {
                unknowns.push(record);
            }
            candidate
                .set_native_unknowns("rhino", &unknowns)
                .expect("required invariant");
        });
        (
            payloads_detached,
            result.expect_err("duplicate unknown ID must fail validation"),
        )
    }

    /// Marks one retained object as successfully decoded.
    pub(crate) fn mark_decoded(&mut self, source_order: usize) -> bool {
        self.transition(source_order, GeometryStatus::Decoded)
    }

    /// Marks one framed object as failed after a skippable payload error.
    pub(crate) fn mark_failed(&mut self, source_order: usize) -> bool {
        self.transition(source_order, GeometryStatus::Failed)
    }

    /// Decode and atomically commit supported simple geometry.
    pub(crate) fn decode_geometry(&mut self) {
        if !matches!(
            self.archive(),
            ArchiveVersion::V5 | ArchiveVersion::V6 | ArchiveVersion::V7 | ArchiveVersion::V8
        ) {
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
                    },
                    &mut self.mesh_budget,
                );
                match decoded {
                    Ok(mesh) => {
                        if self.commit_mesh(source_order, mesh) {
                            self.mark_decoded(source_order);
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
            ArchiveVersion::V5 | ArchiveVersion::V6 | ArchiveVersion::V7 | ArchiveVersion::V8
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
                    if let Err(error) = crate::dimensions::apply_userdata(
                        self.scan.data,
                        &object.userdata,
                        self.archive(),
                        &mut dimension,
                    ) {
                        self.scan_warning(
                            source_order,
                            &format!("dimension extension retained: {error}"),
                        );
                        continue;
                    }
                    let native_ref = Self::mint_unknown_id(source_order).to_string();
                    let (feature, parameter) = crate::dimensions::project(
                        &dimension,
                        &key,
                        (!identity.name.is_empty()).then(|| identity.name.clone()),
                        native_ref,
                    );
                    let links = [feature.id.to_string(), parameter.id.0.clone()];
                    let result = self.validate_candidate(|candidate, _annotations| {
                        candidate.model.features.push(feature);
                        candidate.model.parameters.push(parameter);
                    });
                    match result {
                        Ok(()) => {
                            self.append_links(source_order, &links);
                            self.mark_decoded(source_order);
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
                self.mark_decoded(source_order);
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
        let feature = Feature {
            id: id.clone(),
            ordinal: u64::try_from(source_order).expect("source order fits u64"),
            name: (!identity.name.is_empty()).then(|| identity.name.clone()),
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
                parameters: BTreeMap::new(),
                properties: BTreeMap::from([("construction".to_string(), construction)]),
            },
            native_ref: Some(Self::mint_unknown_id(source_order).to_string()),
        };
        match self
            .validate_candidate(|candidate, _annotations| candidate.model.features.push(feature))
        {
            Ok(()) => {
                self.append_link(source_order, id.to_string());
                self.mark_decoded(source_order);
            }
            Err(error) => self.scan_warning(
                source_order,
                &format!("polyedge candidate rejected: {error}"),
            ),
        }
    }

    fn decode_detail(&mut self, source_order: usize, object: &ObjectDescriptor) {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

        let Some(scale) = self.unit_scale() else {
            self.scan_warning(
                source_order,
                "detail retained because document units are unavailable",
            );
            return;
        };
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
            scale,
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
                self.mark_decoded(source_order);
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
                self.mark_decoded(source_order);
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
        let feature = crate::morph::project(
            &morph,
            &key,
            (!identity.name.is_empty()).then(|| identity.name.clone()),
            self.unknowns[source_order].id.to_string(),
        );
        let feature_id = feature.id.to_string();
        match self
            .validate_candidate(|candidate, _annotations| candidate.model.features.push(feature))
        {
            Ok(()) => {
                self.append_link(source_order, feature_id);
                self.geometry_transferred = true;
                self.mark_decoded(source_order);
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
            crate::surfaces::DecodedSurface::Typed { geometry, derived } => (geometry, derived),
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
                self.mark_decoded(source_order);
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
            && self
                .object_candidates
                .get(&identity.object_id)
                .is_some_and(|candidates| candidates.as_slice() == [source_order])
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
        let mut candidate = self.lightweight_context_candidate();
        let mut stack = Vec::new();
        let mut path = self.instance_path.clone();
        let parent = Transform::identity();
        let outcome = candidate.expand_reference_inner(source_order, parent, &mut path, &mut stack);
        // Every member the candidate decoded inflated its mesh buffers into the
        // shared session arena, which cannot reclaim them. Adopt the
        // candidate's retained-byte count before any discard below, so a rejected
        // expansion still charges the parent for the bytes it left in the arena. A
        // refund here — dropping the charge with the candidate while the arena keeps
        // the bytes — would let a hostile document ratchet arena memory past the cap
        // by failing one reference after another while `used` returns to zero. On
        // the commit path `*self = candidate` re-adopts the same count.
        self.mesh_budget = candidate.mesh_budget.clone();
        match outcome {
            Ok(links) => {
                let validation = cadmpeg_ir::validate::validate(&candidate.ir, Vec::new());
                if !validation.is_ok() {
                    self.scan_warning(
                        source_order,
                        &format!(
                            "instance expansion rejected atomically by IR validation: {}",
                            validation_findings(&validation)
                        ),
                    );
                    return false;
                }
                candidate.append_links(source_order, &links);
                candidate.mark_decoded(source_order);
                candidate.geometry_transferred = true;
                self.transfer_unknown_payloads_to(&mut candidate);
                *self = candidate;
                true
            }
            Err(message) => {
                self.scan_warning(source_order, &format!("instance retained: {message}"));
                false
            }
        }
    }

    fn expand_reference_inner(
        &mut self,
        source_order: usize,
        parent: Transform,
        path: &mut Vec<String>,
        stack: &mut Vec<crate::wire::Uuid>,
    ) -> Result<Vec<String>, String> {
        // Bounds instance recursion depth on the geometry-expansion path that
        // reaches the mesh decoder, independently of the platform depth gauge.
        const MAX_INSTANCE_DEPTH: usize = 64;
        self.expansion_budget.reference()?;
        if stack.len() >= MAX_INSTANCE_DEPTH {
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
        let transform = crate::instances::compose(parent, local);
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
            let matches = self
                .object_candidates
                .get(&member_id)
                .map_or(&[][..], Vec::as_slice);
            let [member_order] = matches else {
                return Err(if matches.is_empty() {
                    format!("definition member {member_id} is missing")
                } else {
                    format!("definition member {member_id} is ambiguous")
                });
            };
            let member_order = *member_order;
            let member = &self.scan.objects[member_order];
            if crate::instances::is_reference_class(member.class_uuid) {
                let nested = self.expand_reference_inner(member_order, transform, path, stack)?;
                self.append_links(member_order, &nested);
                self.mark_decoded(member_order);
                links.extend(nested);
                continue;
            }
            let before = ArenaLengths::capture(&self.ir);
            let previous_selection = self.selected_object.replace(member_order);
            let previous_key =
                self.instance_key
                    .replace(format!("{}.{}", path.join("."), member_id));
            let previous_path = std::mem::replace(&mut self.instance_path, path.clone());
            self.decode_geometry();
            self.selected_object = previous_selection;
            self.instance_key = previous_key;
            self.instance_path = previous_path;
            let after = ArenaLengths::capture(&self.ir);
            if before == after {
                return Err(format!("definition member {member_id} did not decode"));
            }
            links.extend(self.transform_new_entities(before, transform)?);
        }
        self.instance_color = previous_color;
        self.instance_visible = previous_visible;
        path.pop();
        stack.pop();
        Ok(links)
    }

    fn transform_new_entities(
        &mut self,
        before: ArenaLengths,
        transform: Transform,
    ) -> Result<Vec<String>, String> {
        let mut owned_curves = BTreeSet::new();
        let mut owned_surfaces = BTreeSet::new();
        for edge in &self.ir.model.edges[before.edges..] {
            if let Some(curve) = &edge.curve {
                owned_curves.insert(curve.clone());
            }
        }
        for face in &self.ir.model.faces[before.faces..] {
            owned_surfaces.insert(face.surface.clone());
        }
        let mut links = Vec::new();
        let mut derived_ids = Vec::new();
        for body in &mut self.ir.model.bodies[before.bodies..] {
            compose_body_transform(body, transform);
            links.push(body.id.to_string());
            derived_ids.push(body.id.to_string());
        }
        for point in &mut self.ir.model.points[before.points..] {
            if before.bodies == self.ir.model.bodies.len() {
                point.position = crate::instances::point(transform, point.position);
                derived_ids.push(point.id.to_string());
            }
        }
        for curve in &mut self.ir.model.curves[before.curves..] {
            if !owned_curves.contains(&curve.id) {
                transform_curve(curve, transform)?;
                links.push(curve.id.to_string());
                derived_ids.push(curve.id.to_string());
            }
        }
        for surface in &mut self.ir.model.surfaces[before.surfaces..] {
            if !owned_surfaces.contains(&surface.id) {
                transform_surface(surface, transform)?;
                links.push(surface.id.to_string());
                derived_ids.push(surface.id.to_string());
            }
        }
        for mesh in &mut self.ir.model.tessellations[before.tessellations..] {
            for vertex in &mut mesh.vertices {
                *vertex = crate::instances::point(transform, *vertex);
            }
            for value in &mut mesh.normals {
                *value = crate::instances::normal(transform, *value)
                    .ok_or_else(|| "mesh normal transform is singular".to_string())?;
            }
            links.push(mesh.id.clone());
            derived_ids.push(mesh.id.clone());
        }
        for subd in &mut self.ir.model.subds[before.subds..] {
            for vertex in &mut subd.vertices {
                vertex.point = crate::instances::point(transform, vertex.point);
            }
            links.push(subd.id.to_string());
            derived_ids.push(subd.id.to_string());
        }
        if self.ir.model.procedural_curves.len() > before.procedural_curves
            || self.ir.model.procedural_surfaces.len() > before.procedural_surfaces
        {
            let omitted_ids = self.ir.model.procedural_curves[before.procedural_curves..]
                .iter()
                .map(|procedure| procedure.id.to_string())
                .chain(
                    self.ir.model.procedural_surfaces[before.procedural_surfaces..]
                        .iter()
                        .map(|procedure| procedure.id.to_string()),
                )
                .collect::<Vec<_>>();
            self.ir
                .model
                .procedural_curves
                .truncate(before.procedural_curves);
            self.ir
                .model
                .procedural_surfaces
                .truncate(before.procedural_surfaces);
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
                warnings,
            }) => {
                for warning in warnings {
                    self.scan_warning(source_order, &warning);
                }
                if neutral_metadata {
                    self.scan_warning(
                        source_order,
                        "SubD cache, texture, symmetry, or packing metadata is retained without a neutral-IR mapping",
                    );
                }
                if self.commit_subd(source_order, *surface, scale != 1.0) {
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
            append_record_links_at(candidate, source_order, std::slice::from_ref(&id));
        });
        if let Err(findings) = result {
            self.scan_warning(
                source_order,
                &format!("SubD validation rejected candidate: {findings}"),
            );
            return false;
        }
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
        crate::annotations::install(self.scan, &mut self.ir);
        crate::document_data::install(self.scan, &mut self.ir);
        crate::presentation::install(self.scan, &mut self.ir);
        crate::product::install(self.scan, &mut self.ir);
        crate::views::install(self.scan, &mut self.ir);
        let unknown_refs = self
            .unknowns
            .iter()
            .map(NativeUnknownRecord::from)
            .collect::<Vec<_>>();
        self.ir
            .set_native_unknowns("rhino", &unknown_refs)
            .expect("Rhino unknown records serialize");
        self.ir.finalize();
        let mut losses: Vec<LossNote> = Vec::new();
        let decoded = self
            .outcomes
            .values()
            .map(|outcome| outcome.decoded)
            .sum::<usize>();
        let total = self.scan.objects.len();
        losses.push(LossNote {
            code: LossCode::ObjectRecordsUntransferred,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: format!("decoded {decoded}/{total} Rhino object records"),
            provenance: None,
        });
        let mut omissions: Vec<LossNote> = Vec::new();
        for (class, outcome) in &self.outcomes {
            if outcome.retained > 0 {
                omissions.push(LossNote {
                    code: LossCode::UnsupportedObjectFamily,
                    category: LossCategory::Geometry,
                    severity: Severity::Warning,
                    message: format!(
                        "retained {} object record(s) for class {class}; geometry is not decoded",
                        outcome.retained
                    ),
                    provenance: Some(loss_provenance(class, outcome)),
                });
            }
            if outcome.attribute_degraded > 0 {
                losses.push(LossNote {
                    code: LossCode::AttributesNotTransferred,
                    category: LossCategory::Attribute,
                    severity: Severity::Warning,
                    message: format!(
                        "{} object record(s) for class {class} have degraded attributes",
                        outcome.attribute_degraded
                    ),
                    provenance: Some(loss_provenance(class, outcome)),
                });
            }
            if outcome.failed_framed > 0 {
                losses.push(LossNote {
                    code: LossCode::DecodeDiagnostic,
                    category: LossCategory::Other,
                    severity: Severity::Error,
                    message: format!(
                        "{} framed object record(s) for class {class} could not be decoded",
                        outcome.failed_framed
                    ),
                    provenance: Some(loss_provenance(class, outcome)),
                });
            }
        }
        self.typed_losses.extend(omissions);
        if let Some(first) = self.scan.definitions.diagnostics.first() {
            losses.push(LossNote {
                code: LossCode::DecodeDiagnostic,
                category: LossCategory::Other,
                severity: Severity::Warning,
                message: format!(
                    "retained {} malformed, ambiguous, or checksum-degraded instance-definition record(s); first: {}",
                    self.scan.definitions.diagnostics.len(),
                    first.message
                ),
                provenance: Some(LossProvenance {
                    format: "rhino".to_string(),
                    stream: String::new(),
                    offset: first.source_range.start as u64,
                    tag: Some("INSTANCE_DEFINITION_TABLE".to_string()),
                }),
            });
        }
        losses.append(&mut self.typed_losses);
        losses.extend(self.scan.warnings.iter().map(|warning| LossNote {
            code: LossCode::DecodeDiagnostic,
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: warning.clone(),
            provenance: None,
        }));
        let mut phase_families = BTreeMap::<String, (usize, String)>::new();
        for warning in &self.phase_warnings {
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
        losses.extend(
            phase_families
                .into_iter()
                .map(|(family, (count, first))| LossNote {
                    code: LossCode::DecodeDiagnostic,
                    category: LossCategory::Other,
                    severity: Severity::Warning,
                    message: if count == 1 {
                        format!("{family}: {first}")
                    } else {
                        format!("{family}: {count} decode warnings; first: {first}")
                    },
                    provenance: None,
                }),
        );
        let byte_records = self
            .unknowns
            .iter()
            .filter(|record| record.data.is_some())
            .count();
        let notes = vec![format!(
            "decoded {decoded}/{total} Rhino object records; retained metadata/digests for {} \
             records and complete bytes for {byte_records}; document cap {} bytes, per-record cap {} bytes",
            self.unknowns.len(),
            RETAINED_DOCUMENT_CAP,
            RETAINED_RECORD_CAP
        )];
        let mut source_fidelity = cadmpeg_ir::SourceFidelity {
            annotations: self.annotations,
            ..Default::default()
        };
        source_fidelity
            .attach_native_unknown_records(&mut self.ir, "rhino", &self.unknowns)
            .expect("Rhino source records separate from product identities");
        DecodeResult::with_source_fidelity(
            self.ir,
            DecodeReport {
                format: "rhino".to_string(),
                container_only: false,
                geometry_transferred: self.geometry_transferred,
                coverage: std::collections::BTreeMap::new(),
                losses,
                notes,
            },
            source_fidelity,
        )
    }

    fn retain_object_records(&mut self) {
        for (source_order, object) in self.scan.objects.iter().enumerate() {
            let id = Self::mint_unknown_id(source_order);
            let bytes = &self.scan.data[object.range.clone()];
            let byte_len = u64::try_from(bytes.len()).expect("Rhino record length fits u64");
            let class = object.class_uuid.to_string();
            let outcome = self.outcomes.entry(class.clone()).or_default();
            if outcome.retained == 0 {
                outcome.first_offset =
                    u64::try_from(object.range.start).expect("Rhino record offset fits u64");
                outcome.first_object_type = object.object_type;
            }
            outcome.retained += 1;
            if object.framing_degraded {
                outcome.retained -= 1;
                outcome.failed_framed += 1;
            }
            if object.attributes_degraded {
                outcome.attribute_degraded += 1;
            }
            let data = if bytes.len() <= self.retention_limits[0]
                && self
                    .retained_bytes
                    .checked_add(bytes.len())
                    .is_some_and(|end| end <= self.retention_limits[1])
            {
                self.retained_bytes = self
                    .retained_bytes
                    .checked_add(bytes.len())
                    .expect("retention cap checked");
                Some(bytes.to_vec())
            } else {
                None
            };
            self.unknowns.push(UnknownRecord {
                id,
                offset: u64::try_from(object.range.start).expect("Rhino record offset fits u64"),
                byte_len,
                sha256: sha256_hex(bytes),
                data,
                links: Vec::new(),
            });
            self.statuses.push(if object.framing_degraded {
                GeometryStatus::Failed
            } else {
                GeometryStatus::Retained
            });
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
        } else {
            true
        }
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
                let Some(entity_count) = cloud
                    .points
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
                let mut vertices = Vec::with_capacity(cloud.points.len());
                for (index, position) in cloud.points.into_iter().enumerate() {
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
                            entity: if cloud.scaled {
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
                crate::surfaces::DecodedSurface::Typed { geometry, derived } => {
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
        let mut candidate = self.lightweight_candidate();
        let mut candidate_annotations = self.annotations.clone();
        let Some(unknown) = candidate
            .native_unknowns("rhino")
            .ok()
            .as_ref()
            .and_then(|records| records.get(source_order).map(|record| record.id.clone()))
        else {
            return false;
        };
        let mut child_ids = Vec::with_capacity(children.len());
        for (index, child) in children.into_iter().enumerate() {
            let path = match (expected_children, index) {
                (1, 0) => "directrix",
                (2, 0) => "first",
                (2, 1) => "second",
                _ => return false,
            };
            child_ids.push(commit_curve_tree(
                &mut candidate,
                &mut candidate_annotations,
                child,
                key,
                &association,
                Some(unknown.clone()),
                path,
            ));
        }
        let surface_id: cadmpeg_ir::ids::SurfaceId = format!("rhino:object:surface#{key}").into();
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
        append_record_links_at(&mut candidate, source_order, &[surface_id.to_string()]);
        if let Err(findings) = self.commit_valid_candidate(candidate, candidate_annotations) {
            self.phase_warnings.push(format!(
                "procedural-surface: candidate rejected by IR validation: {findings}"
            ));
            return false;
        }
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
        let mut candidate = self.lightweight_candidate();
        let mut candidate_annotations = self.annotations.clone();
        let mut links = Vec::new();
        let mut directrices = Vec::with_capacity(extrusion.boundaries.len());
        for (index, boundary) in extrusion.boundaries.iter().enumerate() {
            let id = commit_curve_tree(
                &mut candidate,
                &mut candidate_annotations,
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
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            annotate_derived(&mut candidate_annotations, &surface_id.to_string());
            annotate_derived(&mut candidate_annotations, &procedure_id.to_string());
            links.push(surface_id.to_string());
        }
        if (extrusion.caps[0] || extrusion.caps[1])
            && !stage_extrusion_caps(
                &mut candidate,
                &mut candidate_annotations,
                &key,
                &association,
                &extrusion,
                &directrices,
                &mut links,
            )
        {
            return false;
        }
        for (index, mut mesh) in extrusion.meshes.into_iter().enumerate() {
            mesh.tessellation.id = format!("rhino:object:tessellation#{key}.cache-{index}");
            mesh.tessellation.source_object = Some(association.clone());
            annotate_derived(&mut candidate_annotations, &mesh.tessellation.id);
            links.push(mesh.tessellation.id.clone());
            candidate.model.tessellations.push(mesh.tessellation);
        }
        append_record_links_at(&mut candidate, source_order, &links);
        if let Err(findings) = self.commit_valid_candidate(candidate, candidate_annotations) {
            self.scan_warning(
                source_order,
                &format!("extrusion candidate rejected by IR validation: {findings}"),
            );
            return false;
        }
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
        if let Err(findings) = self.validate_candidate(|candidate, candidate_annotations| {
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
            append_record_links_at(candidate, source_order, &[id.to_string()]);
        }) {
            self.scan_warning(
                source_order,
                &format!("unknown surface validation rejected candidate: {findings}"),
            );
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
            strip_lengths: mesh.tessellation.strip_lengths,
            normals: mesh.tessellation.normals,
            channels: mesh.tessellation.channels,
        });
        self.annotations.exactness.insert(
            id.clone(),
            ExactnessNote {
                entity: if mesh.scaled {
                    Exactness::Derived
                } else {
                    Exactness::ByteExact
                },
                fields: BTreeMap::new(),
            },
        );
        self.append_link(source_order, id);
        true
    }

    fn decode_brep(&mut self, source_order: usize, object: &ObjectDescriptor) {
        let parsed = crate::brep::parse(
            self.scan.data,
            object.class_data_range.clone(),
            self.archive(),
            self.scan.metadata.properties.writer_version,
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
        let (raw, warnings, semantic_error) = match parsed {
            crate::brep::BrepParse::Valid(value) => (value.raw, value.warnings, None),
            crate::brep::BrepParse::SemanticInvalid {
                raw,
                error,
                warnings,
            } => (raw, warnings, Some(error)),
        };
        for warning in warnings {
            self.scan_warning(source_order, &warning);
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
        let transfer = BrepTransferInput {
            expand: self.expand,
            data: self.scan.data,
            archive: self.archive(),
            writer_version: self.scan.metadata.properties.writer_version,
            raw: &raw,
            key: &key,
            association: &association,
            unknown: &unknown,
            scale,
            semantic_error: semantic_error.as_ref(),
            mesh_budget: &mut self.mesh_budget,
        };
        match stage_brep(transfer) {
            Ok(staged) => {
                let links = staged.links.clone();
                let warnings = staged.warnings.clone();
                let full_topology = matches!(staged.kind, BrepTransferKind::FullTopology);
                let emitted_geometry = !staged.curves.is_empty() || !staged.surfaces.is_empty();
                let cache_only =
                    !full_topology && !emitted_geometry && !staged.tessellations.is_empty();
                let fallback = staged.clone().free_carrier_fallback("IR validation");
                let validation = self.validate_candidate(|candidate, candidate_annotations| {
                    staged.apply(candidate, candidate_annotations);
                    append_record_links_at(candidate, source_order, &links);
                });
                if validation.is_ok() {
                    for warning in warnings {
                        if let Some(cause) = warning.strip_prefix("Brep topology fallback: ") {
                            self.typed_losses.push(LossNote {
                                code: LossCode::TopologyNotTransferred,
                                category: LossCategory::Topology,
                                severity: Severity::Warning,
                                message: format!("Brep topology fallback: {cause}"),
                                provenance: None,
                            });
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
                } else {
                    self.scan_warning(
                        source_order,
                        &format!(
                            "Brep transfer rejected by IR validation: {}",
                            validation.expect_err("checked error")
                        ),
                    );
                    let fallback_links = fallback.links.clone();
                    let fallback_validation =
                        self.validate_candidate(|candidate, candidate_annotations| {
                            fallback.apply(candidate, candidate_annotations);
                            append_record_links_at(candidate, source_order, &fallback_links);
                        });
                    if fallback_validation.is_ok() {
                        self.geometry_transferred |= emitted_geometry;
                    } else {
                        self.scan_warning(
                            source_order,
                            &format!(
                                "Brep fallback rejected by IR validation: {}",
                                fallback_validation.expect_err("checked error")
                            ),
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
        if current == next || matches!(current, GeometryStatus::Decoded | GeometryStatus::Failed) {
            return false;
        }
        let object = &self.scan.objects[source_order];
        let class = object.class_uuid.to_string();
        let outcome = self.outcomes.get_mut(&class).expect("status class exists");
        match current {
            GeometryStatus::Retained => outcome.retained -= 1,
            GeometryStatus::Decoded | GeometryStatus::Failed => unreachable!(),
        }
        match next {
            GeometryStatus::Retained => outcome.retained += 1,
            GeometryStatus::Decoded => outcome.decoded += 1,
            GeometryStatus::Failed => outcome.failed_framed += 1,
        }
        self.statuses[source_order] = next;
        true
    }
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

fn append_record_links_at(ir: &mut CadIr, source_order: usize, links: &[String]) {
    let Ok(mut unknowns) = ir.native_unknowns("rhino") else {
        return;
    };
    if let Some(record) = unknowns.get_mut(source_order) {
        append_links_to_native_record(record, links);
    }
    let _ = ir.set_native_unknowns("rhino", &unknowns);
}

fn append_links_to_record(record: &mut UnknownRecord, links: &[String]) {
    append_links(&record.id, &mut record.links, links);
}

fn append_links_to_native_record(record: &mut NativeUnknownRecord, links: &[String]) {
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

fn detach_unknown_payloads(records: &mut [UnknownRecord]) -> Vec<Option<Vec<u8>>> {
    records
        .iter_mut()
        .map(|record| record.data.take())
        .collect()
}

fn restore_unknown_payloads(records: &mut [UnknownRecord], payloads: Vec<Option<Vec<u8>>>) {
    for (record, payload) in records.iter_mut().zip(payloads) {
        record.data = payload;
    }
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

#[derive(Debug, Clone, Default)]
struct StagedBrep {
    kind: BrepTransferKind,
    bodies: Vec<Body>,
    regions: Vec<Region>,
    shells: Vec<Shell>,
    faces: Vec<Face>,
    loops: Vec<Loop>,
    coedges: Vec<Coedge>,
    edges: Vec<Edge>,
    vertices: Vec<Vertex>,
    points: Vec<Point>,
    surfaces: Vec<Surface>,
    curves: Vec<Curve>,
    procedural_curves: Vec<ProceduralCurve>,
    procedural_surfaces: Vec<ProceduralSurface>,
    pcurves: Vec<Pcurve>,
    tessellations: Vec<Tessellation>,
    exactness: Vec<(String, Exactness)>,
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
    raw: &'a crate::brep::RawBrep,
    key: &'a str,
    association: &'a SourceObjectAssociation,
    unknown: &'a UnknownId,
    scale: f64,
    semantic_error: Option<&'a crate::curves::GeometryError>,
    mesh_budget: &'a mut crate::mesh::MeshBudget,
}

struct BrepStageContext<'a> {
    key: &'a str,
    association: &'a SourceObjectAssociation,
    unknown: &'a UnknownId,
}

impl StagedBrep {
    fn apply(self, ir: &mut CadIr, annotations: &mut cadmpeg_ir::Annotations) {
        ir.model.bodies.extend(self.bodies);
        ir.model.regions.extend(self.regions);
        ir.model.shells.extend(self.shells);
        ir.model.faces.extend(self.faces);
        ir.model.loops.extend(self.loops);
        ir.model.coedges.extend(self.coedges);
        ir.model.edges.extend(self.edges);
        ir.model.vertices.extend(self.vertices);
        ir.model.points.extend(self.points);
        ir.model.surfaces.extend(self.surfaces);
        ir.model.curves.extend(self.curves);
        ir.model.procedural_curves.extend(self.procedural_curves);
        ir.model
            .procedural_surfaces
            .extend(self.procedural_surfaces);
        ir.model.pcurves.extend(self.pcurves);
        ir.model.tessellations.extend(self.tessellations);
        for (id, exactness) in self.exactness {
            annotations.exactness.insert(
                id,
                ExactnessNote {
                    entity: exactness,
                    fields: BTreeMap::new(),
                },
            );
        }
    }

    fn free_carrier_fallback(mut self, cause: impl Into<String>) -> Self {
        self.kind = BrepTransferKind::FreeCarrierFallback;
        let emitted: BTreeSet<String> = self
            .curves
            .iter()
            .map(|value| value.id.to_string())
            .chain(self.surfaces.iter().map(|value| value.id.to_string()))
            .chain(self.tessellations.iter().map(|value| value.id.clone()))
            .chain(
                self.procedural_curves
                    .iter()
                    .map(|value| value.id.to_string()),
            )
            .collect();
        self.links.retain(|id| emitted.contains(id));
        self.exactness.retain(|(id, _)| emitted.contains(id));
        self.bodies.clear();
        self.regions.clear();
        self.shells.clear();
        self.faces.clear();
        self.loops.clear();
        self.coedges.clear();
        self.edges.clear();
        self.vertices.clear();
        self.points.clear();
        self.pcurves.clear();
        self.warnings
            .push(format!("Brep topology fallback: {}", cause.into()));
        self
    }
}

fn stage_brep(input: BrepTransferInput<'_>) -> Result<StagedBrep, crate::curves::GeometryError> {
    let BrepTransferInput {
        expand,
        data,
        archive,
        writer_version,
        raw,
        key,
        association,
        unknown,
        scale,
        semantic_error,
        mesh_budget,
    } = input;
    let mut staged = StagedBrep {
        kind: BrepTransferKind::FullTopology,
        ..StagedBrep::default()
    };
    let mut c3 = BTreeMap::new();
    let mut surfaces = BTreeMap::new();
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
                },
                mesh_budget,
            ) {
                Ok(mesh) => {
                    staged.warnings.extend(mesh.warnings.clone());
                    staged.exactness.push((
                        mesh.tessellation.id.clone(),
                        if mesh.scaled {
                            Exactness::Derived
                        } else {
                            Exactness::ByteExact
                        },
                    ));
                    staged.links.push(mesh.tessellation.id.clone());
                    staged.tessellations.push(mesh.tessellation);
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
                surface: crate::surfaces::DecodedSurface::Typed { geometry, derived },
            }) => {
                let id: cadmpeg_ir::ids::SurfaceId =
                    format!("rhino:object:surface#{key}.slot-{index}").into();
                staged.surfaces.push(Surface {
                    id: id.clone(),
                    geometry,
                    source_object: Some(association.clone()),
                });
                staged.exactness.push((
                    id.to_string(),
                    if derived {
                        Exactness::Derived
                    } else {
                        Exactness::ByteExact
                    },
                ));
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
    if semantic_error.is_some() || child_failed {
        staged.links.extend(
            staged
                .curves
                .iter()
                .map(|curve| curve.id.to_string())
                .chain(staged.surfaces.iter().map(|surface| surface.id.to_string())),
        );
        return Ok(staged.free_carrier_fallback(
            semantic_error
                .map(ToString::to_string)
                .or(child_cause)
                .unwrap_or_else(|| "child geometry decode failed".to_string()),
        ));
    }
    let (c2, pcurves) = match decode_pcurves(data, archive, raw, key) {
        Ok(value) => value,
        Err(error) => {
            return Ok(staged.free_carrier_fallback(format!("C2 curve decode failed: {error}")));
        }
    };
    staged.pcurves = pcurves;
    let body_id: cadmpeg_ir::ids::BodyId = format!("rhino:object:body#{key}").into();
    let mut vertex_ids = Vec::with_capacity(raw.vertices.len());
    for (index, vertex) in raw.vertices.iter().enumerate() {
        let point_id: cadmpeg_ir::ids::PointId =
            format!("rhino:object:point#{key}.vertex-{index}").into();
        let vertex_id: cadmpeg_ir::ids::VertexId =
            format!("rhino:object:vertex#{key}.slot-{index}").into();
        staged.points.push(Point {
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
        staged.vertices.push(Vertex {
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
        staged.edges.push(Edge {
            id: id.clone(),
            curve,
            start: vertex_ids[edge.vertices[0] as usize].clone(),
            end: vertex_ids[edge.vertices[1] as usize].clone(),
            param_range: Some(edge_param_range(edge)),
            tolerance: scaled_tolerance(edge.tolerance, scale)?,
        });
        edge_ids.push(id);
    }
    let components = face_components(raw);
    let grouping = region_shell_groups(raw, &components);
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
        let serialized_sense = grouping.directions.as_ref().map(|values| values[index]);
        let id: cadmpeg_ir::ids::FaceId = format!("rhino:object:face#{key}.slot-{index}").into();
        staged.faces.push(Face {
            id: id.clone(),
            shell: format!("rhino:object:shell#{key}.component-{component}").into(),
            surface,
            sense: face_sense(face.reversed_surface != 0, serialized_sense),
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
        let coedge_start = staged.coedges.len();
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
                    staged.edges.push(Edge {
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
                Some(c2.get(trim_index).cloned().ok_or_else(|| {
                    crate::curves::error(trim.source_range.start, "trim C2 missing")
                })?)
            };
            staged.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: id.clone(),
                edge: edge_id,
                next: coedge_id.clone(),
                previous: coedge_id.clone(),
                radial_next: coedge_id.clone(),
                sense: coedge_sense(trim.reversed_3d != 0),
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
            staged.coedges[coedge_start + offset].next = next;
            staged.coedges[coedge_start + offset].previous = previous;
        }
        staged.loops.push(Loop {
            id: id.clone(),
            face: face_id.clone(),
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            coedges,
            vertex_uses: Vec::new(),
        });
        staged.faces[loop_record.face as usize].loops.push(id);
    }
    let coedge_positions: BTreeMap<cadmpeg_ir::ids::CoedgeId, usize> = staged
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
            staged.coedges[*coedge_positions.get(id).expect("coedge staged")].radial_next = next;
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
        staged.shells.push(Shell {
            id: shell_id,
            region: region_id.clone(),
            faces: faces.iter().map(|index| face_ids[*index].clone()).collect(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
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
    staged.regions = regions;
    staged.bodies.push(Body {
        id: body_id.clone(),
        kind: brep_body_kind(raw.minor, raw.is_solid),
        regions: staged
            .regions
            .iter()
            .map(|region| region.id.clone())
            .collect(),
        transform: None,
        name: association.name.clone(),
        color: association.color,
        visible: association.visible,
    });
    staged.links.extend(
        staged
            .curves
            .iter()
            .map(|curve| curve.id.to_string())
            .chain(staged.surfaces.iter().map(|surface| surface.id.to_string())),
    );
    staged.links.push(body_id.to_string());
    for id in staged
        .bodies
        .iter()
        .map(|value| value.id.to_string())
        .chain(staged.regions.iter().map(|value| value.id.to_string()))
        .chain(staged.shells.iter().map(|value| value.id.to_string()))
        .chain(staged.faces.iter().map(|value| value.id.to_string()))
        .chain(staged.loops.iter().map(|value| value.id.to_string()))
        .chain(staged.coedges.iter().map(|value| value.id.to_string()))
        .chain(staged.edges.iter().map(|value| value.id.to_string()))
        .chain(staged.vertices.iter().map(|value| value.id.to_string()))
        .chain(staged.points.iter().map(|value| value.id.to_string()))
        .chain(staged.pcurves.iter().map(|value| value.id.to_string()))
    {
        staged.exactness.push((id, Exactness::Derived));
    }
    let _ = writer_version;
    scale_plane_pcurves(&mut staged, scale);
    Ok(staged)
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
    let parsed = crate::brep::parse(data, range, archive, writer_version).ok()?;
    let raw = match parsed {
        crate::brep::BrepParse::Valid(value) => value.raw,
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
    let mut mesh_budget = crate::mesh::MeshBudget::new();
    let staged = stage_brep(BrepTransferInput {
        expand,
        data,
        archive,
        writer_version,
        raw: &raw,
        key: "history:embedded-brep",
        association: &association,
        unknown: &unknown,
        scale,
        semantic_error: None,
        mesh_budget: &mut mesh_budget,
    })
    .ok()?;
    if staged.kind != BrepTransferKind::FullTopology {
        return None;
    }
    serde_json::to_string(&serde_json::json!({
        "kind": "brep",
        "bodies": staged.bodies,
        "regions": staged.regions,
        "shells": staged.shells,
        "faces": staged.faces,
        "loops": staged.loops,
        "coedges": staged.coedges,
        "edges": staged.edges,
        "vertices": staged.vertices,
        "points": staged.points,
        "surfaces": staged.surfaces,
        "curves": staged.curves,
        "procedural_curves": staged.procedural_curves,
        "procedural_surfaces": staged.procedural_surfaces,
        "pcurves": staged.pcurves,
        "tessellations": staged.tessellations,
    }))
    .ok()
}

/// Rhino trim curves live in the surface's native parameter space. A plane's
/// parameters are lengths, so a unit-scaled document moves the plane's
/// parameterization to millimeters while the trims stay in native units;
/// the UV poles of pcurves on plane faces scale to match. NURBS surface
/// parameters are knot-domain values and do not scale.
fn scale_plane_pcurves(staged: &mut StagedBrep, scale: f64) {
    if scale == 1.0 {
        return;
    }
    let plane_surfaces = staged
        .surfaces
        .iter()
        .filter(|surface| matches!(surface.geometry, SurfaceGeometry::Plane { .. }))
        .map(|surface| surface.id.0.as_str())
        .collect::<BTreeSet<_>>();
    let plane_faces = staged
        .faces
        .iter()
        .filter(|face| plane_surfaces.contains(face.surface.0.as_str()))
        .map(|face| face.id.0.as_str())
        .collect::<BTreeSet<_>>();
    let plane_loops = staged
        .loops
        .iter()
        .filter(|value| plane_faces.contains(value.face.0.as_str()))
        .map(|value| value.id.0.as_str())
        .collect::<BTreeSet<_>>();
    let plane_pcurves = staged
        .coedges
        .iter()
        .filter(|coedge| plane_loops.contains(coedge.owner_loop.0.as_str()))
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| use_.pcurve.0.clone()))
        .collect::<BTreeSet<_>>();
    for pcurve in &mut staged.pcurves {
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
    if edge.proxy_reversed != 0 {
        [edge.proxy_domain.0[1], edge.proxy_domain.0[0]]
    } else {
        edge.proxy_domain.0
    }
}

fn face_sense(face_reversed: bool, region_direction: Option<i32>) -> Sense {
    if region_direction.map_or(face_reversed, |direction| direction < 0) {
        Sense::Reversed
    } else {
        Sense::Forward
    }
}

fn coedge_sense(reversed_3d: bool) -> Sense {
    if reversed_3d {
        Sense::Reversed
    } else {
        Sense::Forward
    }
}

fn brep_body_kind(minor: u8, is_solid: Option<i32>) -> BodyKind {
    match is_solid {
        Some(1 | 2) if minor >= 2 => BodyKind::Solid,
        Some(3) => BodyKind::General,
        _ => BodyKind::Sheet,
    }
}

fn stage_brep_procedural_surface(
    staged: &mut StagedBrep,
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
    staged.surfaces.push(Surface {
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
    staged.procedural_surfaces.push(ProceduralSurface {
        id: procedural_id.clone(),
        surface: surface_id.clone(),
        definition,
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    staged
        .exactness
        .push((surface_id.to_string(), Exactness::Derived));
    staged
        .exactness
        .push((procedural_id.to_string(), Exactness::Derived));
    staged.links.push(surface_id.to_string());
    staged.links.push(procedural_id.to_string());
    Ok(surface_id)
}

fn stage_curve_tree(
    staged: &mut StagedBrep,
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
    staged.curves.push(Curve {
        id: id.clone(),
        geometry,
        source_object: Some(association.clone()),
    });
    staged.exactness.push((id.to_string(), Exactness::Derived));
    staged.links.push(id.to_string());
    if let Some(compound) = curve.compound {
        let procedure_id: cadmpeg_ir::ids::ProceduralCurveId =
            format!("rhino:object:procedural-curve#{key}.{path}").into();
        staged
            .exactness
            .push((procedure_id.to_string(), Exactness::Derived));
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

fn staged_links_procedure(staged: &mut StagedBrep, procedure: ProceduralCurve) {
    staged.links.push(procedure.id.to_string());
    staged.procedural_curves.push(procedure);
}

fn decode_pcurves(
    data: &[u8],
    archive: ArchiveVersion,
    raw: &crate::brep::RawBrep,
    key: &str,
) -> Result<(BTreeMap<i32, cadmpeg_ir::ids::PcurveId>, Vec<Pcurve>), crate::curves::GeometryError> {
    let mut ids = BTreeMap::new();
    let mut values = Vec::new();
    let mut decoded_slots = BTreeMap::<i32, NurbsCurve>::new();
    for (index, trim) in raw.trims.iter().enumerate() {
        if trim.trim_type == 6 {
            continue;
        }
        let nurbs = if let Some(nurbs) = decoded_slots.get(&trim.curve) {
            nurbs.clone()
        } else {
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
            let nurbs = c2_curve_to_nurbs(curve, trim.source_range.start)?;
            decoded_slots.insert(trim.curve, nurbs.clone());
            nurbs
        };
        let control_points = nurbs
            .control_points
            .into_iter()
            .map(|point| Point2::new(point.x, point.y))
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
    Ok((ids, values))
}

fn c2_curve_to_nurbs(
    curve: crate::curves::DecodedCurve,
    offset: usize,
) -> Result<NurbsCurve, crate::curves::GeometryError> {
    let Some(compound) = curve.compound else {
        return match curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Ok(nurbs),
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
    for (index, child) in compound.children.into_iter().enumerate() {
        let target = [compound.parameters[index], compound.parameters[index + 1]];
        if !target[0].is_finite() || !target[1].is_finite() || target[0] >= target[1] {
            return Err(crate::curves::error(
                offset,
                "C2 polycurve segment domain is invalid",
            ));
        }
        segments.push(remap_nurbs_domain(
            c2_curve_to_nurbs(child, offset)?,
            target,
            offset,
        )?);
    }
    merge_nurbs_segments(segments, offset)
}

fn remap_nurbs_domain(
    mut curve: NurbsCurve,
    target: [f64; 2],
    offset: usize,
) -> Result<NurbsCurve, crate::curves::GeometryError> {
    let degree = usize::try_from(curve.degree)
        .map_err(|_| crate::curves::error(offset, "C2 degree does not fit memory"))?;
    let end_index = curve
        .knots
        .len()
        .checked_sub(degree + 1)
        .ok_or_else(|| crate::curves::error(offset, "C2 knot vector is invalid"))?;
    let source = [
        *curve
            .knots
            .get(degree)
            .ok_or_else(|| crate::curves::error(offset, "C2 knot vector is invalid"))?,
        curve.knots[end_index],
    ];
    let denominator = source[1] - source[0];
    if !denominator.is_finite() || denominator <= 0.0 {
        return Err(crate::curves::error(offset, "C2 curve domain is invalid"));
    }
    let scale = (target[1] - target[0]) / denominator;
    for knot in &mut curve.knots {
        *knot = target[0] + (*knot - source[0]) * scale;
        if !knot.is_finite() {
            return Err(crate::curves::error(offset, "C2 knot remap overflowed"));
        }
    }
    Ok(curve)
}

fn merge_nurbs_segments(
    mut segments: Vec<NurbsCurve>,
    offset: usize,
) -> Result<NurbsCurve, crate::curves::GeometryError> {
    let Some(first) = segments.first() else {
        return Err(crate::curves::error(offset, "C2 polycurve has no segments"));
    };
    if segments.len() == 1 {
        return Ok(segments.remove(0));
    }
    let degree = first.degree;
    if segments.iter().any(|segment| segment.degree != degree) {
        return Err(crate::curves::error(
            offset,
            "C2 polycurve segments have unequal degrees",
        ));
    }
    let multiplicity = usize::try_from(degree)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| crate::curves::error(offset, "C2 degree overflow"))?;
    for segment in &segments {
        if segment.knots.len() < multiplicity {
            return Err(crate::curves::error(
                offset,
                "C2 polycurve segment knot vector is invalid",
            ));
        }
        let start = segment.knots.get(multiplicity - 1).copied();
        let end = segment
            .knots
            .len()
            .checked_sub(multiplicity)
            .and_then(|index| segment.knots.get(index))
            .copied();
        if start.is_none()
            || end.is_none()
            || segment.knots[..multiplicity]
                .iter()
                .any(|value| Some(*value) != start)
            || segment.knots[segment.knots.len() - multiplicity..]
                .iter()
                .any(|value| Some(*value) != end)
        {
            return Err(crate::curves::error(
                offset,
                "C2 polycurve segment is not endpoint-clamped",
            ));
        }
    }
    let rational = segments.iter().any(|segment| segment.weights.is_some());
    let control_count = segments.iter().try_fold(0_usize, |total, segment| {
        total.checked_add(segment.control_points.len())
    });
    let knot_count = segments
        .iter()
        .try_fold(0_usize, |total, segment| {
            total.checked_add(segment.knots.len())
        })
        .and_then(|total| {
            (segments.len() - 1)
                .checked_mul(multiplicity)
                .and_then(|duplicate_count| total.checked_sub(duplicate_count))
        })
        .ok_or_else(|| crate::curves::error(offset, "C2 polycurve size overflow"))?;
    let mut control_points = Vec::with_capacity(
        control_count.ok_or_else(|| crate::curves::error(offset, "C2 polycurve size overflow"))?,
    );
    let mut knots = Vec::with_capacity(knot_count);
    let mut weights = rational.then(|| Vec::with_capacity(control_points.capacity()));
    for (index, segment) in segments.into_iter().enumerate() {
        if let Some(target) = &mut weights {
            match segment.weights {
                Some(values) => target.extend(values),
                None => target.extend(std::iter::repeat_n(1.0, segment.control_points.len())),
            }
        }
        control_points.extend(segment.control_points);
        knots.extend(
            segment
                .knots
                .into_iter()
                .skip(if index == 0 { 0 } else { multiplicity }),
        );
    }
    Ok(NurbsCurve {
        degree,
        knots,
        control_points,
        weights,
        periodic: false,
    })
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

struct ShellGrouping {
    face_groups: Vec<usize>,
    region_labels: Vec<i32>,
    shell_faces: Vec<Vec<usize>>,
    directions: Option<Vec<i32>>,
    fallback: bool,
}

fn region_shell_groups(raw: &crate::brep::RawBrep, components: &[usize]) -> ShellGrouping {
    if raw.minor < 3 || raw.regions.is_empty() {
        let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (face, component) in components.iter().copied().enumerate() {
            groups.entry(component).or_default().push(face);
        }
        let mut shell_faces = Vec::new();
        let mut face_groups = vec![0; components.len()];
        let mut region_labels = Vec::new();
        for (group, (component, faces)) in groups.into_iter().enumerate() {
            for face in &faces {
                face_groups[*face] = group;
            }
            let _ = component;
            shell_faces.push(faces);
            region_labels.push(group as i32);
        }
        return ShellGrouping {
            face_groups,
            region_labels,
            shell_faces,
            directions: None,
            fallback: false,
        };
    }
    let mut grouped: BTreeMap<(i32, usize), Vec<usize>> = BTreeMap::new();
    let mut directions = vec![0; raw.faces.len()];
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
        directions[face] = side.direction;
        grouped
            .entry((region, components[face]))
            .or_default()
            .push(face);
    }
    let mut face_groups = vec![0; components.len()];
    let mut region_labels = Vec::new();
    let mut shell_faces = Vec::new();
    for (group, ((region, _component), faces)) in grouped.into_iter().enumerate() {
        for face in &faces {
            face_groups[*face] = group;
        }
        region_labels.push(region);
        shell_faces.push(faces);
    }
    ShellGrouping {
        face_groups,
        region_labels,
        shell_faces,
        directions: Some(directions),
        fallback: false,
    }
}

fn region_shell_groups_without_records(components: &[usize]) -> ShellGrouping {
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (face, component) in components.iter().copied().enumerate() {
        groups.entry(component).or_default().push(face);
    }
    let mut face_groups = vec![0; components.len()];
    let mut region_labels = Vec::new();
    let mut shell_faces = Vec::new();
    for (group, (_component, faces)) in groups.into_iter().enumerate() {
        for face in &faces {
            face_groups[*face] = group;
        }
        region_labels.push(group as i32);
        shell_faces.push(faces);
    }
    ShellGrouping {
        face_groups,
        region_labels,
        shell_faces,
        directions: None,
        fallback: true,
    }
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
        Some(existing) => crate::instances::compose(transform, existing),
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
                *pole = crate::instances::point(transform, *pole);
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
                *pole = crate::instances::point(transform, *pole);
            }
            CurveGeometry::Nurbs(nurbs)
        }
        CurveGeometry::Line { origin, direction } => {
            let transformed_origin = crate::instances::point(transform, origin);
            let endpoint = crate::instances::point(
                transform,
                Point3::new(
                    origin.x + direction.x,
                    origin.y + direction.y,
                    origin.z + direction.z,
                ),
            );
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
            point: crate::instances::point(transform, point),
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
                *pole = crate::instances::point(transform, *pole);
            }
            SurfaceGeometry::Nurbs(nurbs)
        }
        SurfaceGeometry::Plane {
            origin: source_origin,
            normal,
            u_axis,
        } => {
            let origin = crate::instances::point(transform, source_origin);
            let normal = crate::instances::normal(transform, normal)
                .ok_or_else(|| "instance plane normal transform is singular".to_string())?;
            let endpoint = crate::instances::point(
                transform,
                Point3::new(
                    source_origin.x + u_axis.x,
                    source_origin.y + u_axis.y,
                    source_origin.z + u_axis.z,
                ),
            );
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

fn loss_provenance(class: &str, outcome: &ClassOutcome) -> LossProvenance {
    LossProvenance {
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
    crate::history::project(&scan.history, geometry_context, &mut context.ir);
    context.commit()
}

#[cfg(test)]
pub(crate) fn with_expand_bytes<R>(
    data: &[u8],
    f: impl FnOnce(crate::mesh::MeshExpand<'_>) -> R,
) -> R {
    let arena = cadmpeg_ir::decode::DecodeArena::new();
    let policy = cadmpeg_ir::decode::DecodePolicy::default();
    let (ctx, root) = cadmpeg_ir::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
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
        format: "rhino".to_string(),
        attributes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve};
    use cadmpeg_ir::math::{Point3, Vector3};

    fn line_nurbs(start: f64, end: f64, rational: bool) -> NurbsCurve {
        NurbsCurve {
            degree: 1,
            knots: vec![start, start, end, end],
            control_points: vec![Point3::new(start, 0.0, 0.0), Point3::new(end, 0.0, 0.0)],
            weights: rational.then(|| vec![2.0, 1.0]),
            periodic: false,
        }
    }

    fn decoded_nurbs(curve: NurbsCurve) -> crate::curves::DecodedCurve {
        crate::curves::DecodedCurve {
            geometry: CurveGeometry::Nurbs(curve),
            compound: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn hatch_plane_places_and_scales_plane_space_loops_once() {
        let plane = crate::settings::Plane {
            origin: crate::settings::Point3([10.0, 20.0, 30.0]),
            xaxis: crate::settings::Vector3([0.0, 1.0, 0.0]),
            yaxis: crate::settings::Vector3([-1.0, 0.0, 0.0]),
            zaxis: crate::settings::Vector3([0.0, 0.0, 1.0]),
            equation: [0.0, 0.0, 1.0, -30.0],
        };
        let mut curve = decoded_nurbs(line_nurbs(0.0, 2.0, false));
        transform_decoded_curve(&mut curve, hatch_plane_transform(&plane, 10.0))
            .expect("required invariant");
        let CurveGeometry::Nurbs(curve) = curve.geometry else {
            panic!("hatch loop must remain NURBS");
        };
        assert_eq!(curve.control_points[0], Point3::new(100.0, 200.0, 300.0));
        assert_eq!(curve.control_points[1], Point3::new(100.0, 220.0, 300.0));
    }

    #[test]
    fn body_instance_transform_composes_before_existing_body_transform() {
        let mut body = Body {
            id: "body".into(),
            kind: BodyKind::General,
            regions: Vec::new(),
            transform: Some(Transform {
                rows: [
                    [2.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            }),
            name: None,
            color: None,
            visible: None,
        };
        let instance = Transform {
            rows: [
                [1.0, 0.0, 0.0, 10.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        compose_body_transform(&mut body, instance);
        assert_eq!(
            crate::instances::point(
                body.transform.expect("required invariant"),
                Point3::new(1.0, 0.0, 0.0)
            ),
            Point3::new(12.0, 0.0, 0.0)
        );
    }

    fn region_raw(
        face_sides: Vec<crate::brep::RawBrepFaceSide>,
        regions: Vec<crate::brep::RawBrepRegion>,
    ) -> crate::brep::RawBrep {
        let empty_curves = || crate::brep::RawBrepChildren {
            slots: Vec::new(),
            source_range: 0..0,
            expected_type: crate::brep::RawBrepBaseType::Curve,
        };
        crate::brep::RawBrep {
            minor: 3,
            c2: empty_curves(),
            c3: empty_curves(),
            surfaces: crate::brep::RawBrepChildren {
                slots: Vec::new(),
                source_range: 0..0,
                expected_type: crate::brep::RawBrepBaseType::Surface,
            },
            vertices: Vec::new(),
            edges: Vec::new(),
            trims: Vec::new(),
            loops: Vec::new(),
            faces: vec![crate::brep::RawBrepFace {
                index: 0,
                loops: Vec::new(),
                surface: 0,
                reversed_surface: 0,
                material_channel: 0,
                uuid: None,
                color: None,
                source_range: 0..0,
            }],
            bounds: crate::settings::BoundingBox {
                minimum: crate::settings::Point3([0.0, 0.0, 0.0]),
                maximum: crate::settings::Point3([1.0, 1.0, 1.0]),
            },
            render_meshes: Vec::new(),
            analysis_meshes: Vec::new(),
            render_mesh_array_range: 0..0,
            analysis_mesh_array_range: 0..0,
            is_solid: None,
            face_sides,
            regions,
            region_wrapper_range: Some(0..0),
            source_range: 0..0,
            vertex_array_range: 0..0,
            edge_array_range: 0..0,
            trim_array_range: 0..0,
            loop_array_range: 0..0,
            face_array_range: 0..0,
        }
    }

    fn region(index: i32, region_type: i32) -> crate::brep::RawBrepRegion {
        crate::brep::RawBrepRegion {
            index,
            region_type,
            sides: Vec::new(),
            bounds: crate::settings::BoundingBox {
                minimum: crate::settings::Point3([0.0, 0.0, 0.0]),
                maximum: crate::settings::Point3([1.0, 1.0, 1.0]),
            },
            source_range: 0..0,
        }
    }

    fn append_line_payload(
        data: &mut Vec<u8>,
        from: [f64; 3],
        to: [f64; 3],
        dimension: i32,
    ) -> std::ops::Range<usize> {
        let start = data.len();
        data.push(0x10);
        for value in from.into_iter().chain(to) {
            data.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0_f64, 1.0] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        data.extend_from_slice(&dimension.to_le_bytes());
        start..data.len()
    }

    fn append_plane_payload(data: &mut Vec<u8>) -> std::ops::Range<usize> {
        let start = data.len();
        data.push(0x11);
        for value in [
            0.0_f64, 0.0, 0.0, // origin
            1.0, 0.0, 0.0, // x
            0.0, 1.0, 0.0, // y
            0.0, 0.0, 1.0, // z
            0.0, 0.0, 1.0, 0.0, // equation
        ] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        for _ in 0..4 {
            for value in [0.0_f64, 1.0] {
                data.extend_from_slice(&value.to_le_bytes());
            }
        }
        start..data.len()
    }

    fn class_uuid(wire: [u8; 16]) -> crate::wire::Uuid {
        crate::wire::Uuid::from_wire(wire)
    }

    fn child(
        class_uuid: crate::wire::Uuid,
        class_data_range: std::ops::Range<usize>,
        base_type: crate::brep::RawBrepBaseType,
    ) -> crate::brep::RawBrepChild {
        crate::brep::RawBrepChild {
            class_uuid,
            source_range: class_data_range.clone(),
            class_data_range,
            base_type,
        }
    }

    fn source_shaped_plane_brep() -> (Vec<u8>, crate::brep::RawBrep) {
        let line_uuid = class_uuid([
            0xdb, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01,
            0x22, 0xf0,
        ]);
        let plane_uuid = class_uuid([
            0xdf, 0xd4, 0xd7, 0x4e, 0x47, 0xe9, 0xd3, 0x11, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01,
            0x22, 0xf0,
        ]);
        let mut data = Vec::new();
        let c3_ranges = [
            append_line_payload(&mut data, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 3),
            append_line_payload(&mut data, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 3),
            append_line_payload(&mut data, [0.0, 1.0, 0.0], [0.0, 0.0, 0.0], 3),
        ];
        let c2_ranges = [
            append_line_payload(&mut data, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 2),
            append_line_payload(&mut data, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 2),
            append_line_payload(&mut data, [0.0, 1.0, 0.0], [0.0, 0.0, 0.0], 2),
        ];
        let surface_range = append_plane_payload(&mut data);
        let interval = crate::settings::Interval([0.0, 1.0]);
        let endpoints = [[0, 1], [1, 2], [2, 0]];
        let vertices = [[0, 2], [0, 1], [1, 2]]
            .into_iter()
            .enumerate()
            .map(|(index, edges)| crate::brep::RawBrepVertex {
                index: i32::try_from(index).expect("index"),
                point: crate::settings::Point3([
                    f64::from((index == 1) as u8),
                    f64::from((index == 2) as u8),
                    0.0,
                ]),
                edges: edges.into_iter().collect(),
                tolerance: 0.01,
                source_range: 0..0,
            })
            .collect();
        let edges = endpoints
            .into_iter()
            .enumerate()
            .map(|(index, vertices)| crate::brep::RawBrepEdge {
                index: i32::try_from(index).expect("index"),
                curve: i32::try_from(index).expect("index"),
                proxy_reversed: 0,
                proxy_domain: interval,
                vertices,
                trims: vec![i32::try_from(index).expect("index")],
                tolerance: 0.01,
                domain: interval,
                source_range: 0..0,
            })
            .collect();
        let trims = endpoints
            .into_iter()
            .enumerate()
            .map(|(index, vertices)| crate::brep::RawBrepTrim {
                index: i32::try_from(index).expect("index"),
                curve: i32::try_from(index).expect("index"),
                proxy_domain: interval,
                edge: i32::try_from(index).expect("index"),
                vertices,
                reversed_3d: 0,
                trim_type: 1,
                iso: 0,
                loop_index: 0,
                tolerances: [0.02, 0.03],
                domain: interval,
                proxy_reversed: 0,
                reserved: Vec::new(),
                legacy_tolerances: [0.02, 0.03],
                source_range: 0..0,
            })
            .collect();
        (
            data,
            crate::brep::RawBrep {
                minor: 2,
                c2: crate::brep::RawBrepChildren {
                    slots: c2_ranges
                        .into_iter()
                        .map(|range| {
                            Some(child(line_uuid, range, crate::brep::RawBrepBaseType::Curve))
                        })
                        .collect(),
                    source_range: 0..0,
                    expected_type: crate::brep::RawBrepBaseType::Curve,
                },
                c3: crate::brep::RawBrepChildren {
                    slots: c3_ranges
                        .into_iter()
                        .map(|range| {
                            Some(child(line_uuid, range, crate::brep::RawBrepBaseType::Curve))
                        })
                        .collect(),
                    source_range: 0..0,
                    expected_type: crate::brep::RawBrepBaseType::Curve,
                },
                surfaces: crate::brep::RawBrepChildren {
                    slots: vec![Some(child(
                        plane_uuid,
                        surface_range,
                        crate::brep::RawBrepBaseType::Surface,
                    ))],
                    source_range: 0..0,
                    expected_type: crate::brep::RawBrepBaseType::Surface,
                },
                vertices,
                edges,
                trims,
                loops: vec![crate::brep::RawBrepLoop {
                    index: 0,
                    trims: vec![0, 1, 2],
                    loop_type: 1,
                    face: 0,
                    source_range: 0..0,
                }],
                faces: vec![crate::brep::RawBrepFace {
                    index: 0,
                    loops: vec![0],
                    surface: 0,
                    reversed_surface: 0,
                    material_channel: 0,
                    uuid: None,
                    color: None,
                    source_range: 0..0,
                }],
                bounds: crate::settings::BoundingBox {
                    minimum: crate::settings::Point3([0.0, 0.0, 0.0]),
                    maximum: crate::settings::Point3([1.0, 1.0, 0.0]),
                },
                render_meshes: Vec::new(),
                analysis_meshes: Vec::new(),
                render_mesh_array_range: 0..0,
                analysis_mesh_array_range: 0..0,
                is_solid: Some(3),
                face_sides: Vec::new(),
                regions: Vec::new(),
                region_wrapper_range: None,
                source_range: 0..0,
                vertex_array_range: 0..0,
                edge_array_range: 0..0,
                trim_array_range: 0..0,
                loop_array_range: 0..0,
                face_array_range: 0..0,
            },
        )
    }

    #[test]
    fn fallback_discards_topology_and_unknown_record_self_link() {
        let curve_id: cadmpeg_ir::ids::CurveId = "rhino:object:curve#x.c3-0".into();
        let surface_id: cadmpeg_ir::ids::SurfaceId = "rhino:object:surface#x.slot-0".into();
        let mut staged = StagedBrep {
            curves: vec![Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Unknown { record: None },
                source_object: None,
            }],
            surfaces: vec![Surface {
                id: surface_id.clone(),
                geometry: cadmpeg_ir::geometry::SurfaceGeometry::Unknown { record: None },
                source_object: None,
            }],
            links: vec![
                curve_id.to_string(),
                surface_id.to_string(),
                "rhino:object:body#x".to_string(),
                "rhino:object:record#x".to_string(),
            ],
            bodies: vec![Body {
                id: "rhino:object:body#x".into(),
                kind: BodyKind::Sheet,
                regions: Vec::new(),
                transform: None,
                name: None,
                color: None,
                visible: None,
            }],
            ..StagedBrep::default()
        };
        staged = staged.free_carrier_fallback("C2 failure");
        assert_eq!(staged.kind, BrepTransferKind::FreeCarrierFallback);
        assert!(staged.bodies.is_empty());
        assert_eq!(
            staged.links,
            vec![curve_id.to_string(), surface_id.to_string()]
        );
        assert!(staged.warnings.iter().any(|warning| warning.contains("C2")));
    }

    #[test]
    fn fallback_candidate_links_free_carrier_before_full_ir_validation() {
        let unknown: UnknownId = "rhino:object:record#x".into();
        let curve_id: cadmpeg_ir::ids::CurveId = "rhino:object:curve#x.c3-0".into();
        let mut candidate = CadIr::empty(Units::default());
        candidate
            .set_native_unknowns(
                "rhino",
                &[NativeUnknownRecord {
                    id: unknown.clone(),
                    links: Vec::new(),
                }],
            )
            .expect("required invariant");
        let staged = StagedBrep {
            kind: BrepTransferKind::FreeCarrierFallback,
            curves: vec![Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Nurbs(line_nurbs(0.0, 1.0, false)),
                source_object: None,
            }],
            links: vec![unknown.to_string(), curve_id.to_string()],
            ..StagedBrep::default()
        };
        let links = staged.links.clone();
        staged.apply(&mut candidate, &mut cadmpeg_ir::Annotations::default());
        append_record_links(&mut candidate, &unknown, &links);
        assert_eq!(
            candidate
                .native_unknowns("rhino")
                .expect("required invariant")[0]
                .links,
            vec![curve_id.to_string()]
        );
        let report = cadmpeg_ir::validate::validate(&candidate, Vec::new());
        assert!(report.is_ok(), "{report:?}");
    }

    #[test]
    fn colliding_staged_ids_fail_only_the_cloned_candidate() {
        let curve_id: cadmpeg_ir::ids::CurveId = "rhino:object:curve#x.c3-0".into();
        let curve = Curve {
            id: curve_id,
            geometry: CurveGeometry::Nurbs(line_nurbs(0.0, 1.0, false)),
            source_object: None,
        };
        let mut live = CadIr::empty(Units::default());
        live.model.curves.push(curve.clone());
        let mut candidate = live.clone();
        StagedBrep {
            curves: vec![curve],
            ..StagedBrep::default()
        }
        .apply(&mut candidate, &mut cadmpeg_ir::Annotations::default());
        assert!(!cadmpeg_ir::validate::validate(&candidate, Vec::new()).is_ok());
        assert_eq!(live.model.curves.len(), 1);
    }

    #[test]
    fn source_shaped_plane_brep_stages_complete_scaled_valid_ir() {
        let (data, raw) = source_shaped_plane_brep();
        let association = SourceObjectAssociation {
            format: "rhino".to_string(),
            object_id: "plane-brep".to_string(),
            name: Some("plane".to_string()),
            color: None,
            visible: Some(true),
            layer: None,
            instance_path: Vec::new(),
        };
        let unknown: UnknownId = "rhino:object:record#plane".into();
        let staged = with_expand_bytes(&data, |expand| {
            stage_brep(BrepTransferInput {
                expand,
                data: &data,
                archive: ArchiveVersion::V5,
                writer_version: Some(200_206_180),
                raw: &raw,
                key: "plane",
                association: &association,
                unknown: &unknown,
                scale: 25.4,
                semantic_error: None,
                mesh_budget: &mut crate::mesh::MeshBudget::new(),
            })
        })
        .expect("stage plane Brep");
        assert_eq!(staged.kind, BrepTransferKind::FullTopology);
        assert_eq!(
            (
                staged.bodies.len(),
                staged.regions.len(),
                staged.shells.len(),
                staged.faces.len(),
                staged.loops.len(),
                staged.coedges.len(),
                staged.edges.len(),
                staged.vertices.len(),
                staged.pcurves.len(),
                staged.curves.len(),
                staged.surfaces.len(),
            ),
            (1, 1, 1, 1, 1, 3, 3, 3, 3, 3, 1)
        );
        assert_eq!(staged.points[1].position.x, 25.4);
        assert_eq!(staged.vertices[0].tolerance, Some(0.254));
        assert_eq!(staged.edges[0].tolerance, Some(0.254));
        assert_eq!(staged.pcurves[0].fit_tolerance, Some(0.02));
        let PcurveGeometry::Nurbs { control_points, .. } = &staged.pcurves[0].geometry else {
            panic!("line C2 must be a NURBS pcurve");
        };
        // Plane parameters are lengths: the native `u = 1.0` trim endpoint
        // scales with the document (inches -> millimeters).
        assert_eq!(control_points[1].u, 25.4);
        assert_eq!(staged.coedges[0].radial_next, staged.coedges[0].id);
        let links = staged.links.clone();
        let mut candidate = CadIr::empty(Units::default());
        candidate
            .set_native_unknowns(
                "rhino",
                &[NativeUnknownRecord {
                    id: unknown.clone(),
                    links: Vec::new(),
                }],
            )
            .expect("required invariant");
        staged.apply(&mut candidate, &mut cadmpeg_ir::Annotations::default());
        append_record_links(&mut candidate, &unknown, &links);
        let report = cadmpeg_ir::validate::validate(&candidate, Vec::new());
        assert!(report.is_ok(), "{report:?}");
    }

    #[test]
    fn disconnected_incidence_produces_deterministic_shell_groups() {
        let grouping = region_shell_groups_without_records(&[1, 0, 1, 0]);
        assert!(grouping.fallback);
        assert!(grouping.directions.is_none());
        assert_eq!(grouping.face_groups, vec![1, 0, 1, 0]);
        assert_eq!(grouping.region_labels, vec![0, 1]);
        assert_eq!(grouping.shell_faces, vec![vec![1, 3], vec![0, 2]]);
    }

    #[test]
    fn tolerance_scaling_maps_unset_and_zero_to_none() {
        assert_eq!(
            scaled_tolerance(0.0, 25.4).expect("required invariant"),
            None
        );
        assert_eq!(
            scaled_tolerance(0.5, 25.4).expect("required invariant"),
            Some(12.7)
        );
        assert_eq!(finite_tolerance(0.5), Some(0.5));
        assert_eq!(finite_tolerance(-1.0), None);
    }

    #[test]
    fn edge_domain_maps_proxy_subdomain_and_reversal_to_c3() {
        let edge = crate::brep::RawBrepEdge {
            index: 0,
            curve: 0,
            proxy_reversed: 0,
            proxy_domain: crate::settings::Interval([3.0, 7.0]),
            vertices: [0, 1],
            trims: Vec::new(),
            tolerance: 0.0,
            domain: crate::settings::Interval([100.0, 200.0]),
            source_range: 0..0,
        };
        assert_eq!(edge_param_range(&edge), [3.0, 7.0]);
        let reversed = crate::brep::RawBrepEdge {
            proxy_reversed: 1,
            ..edge
        };
        assert_eq!(edge_param_range(&reversed), [7.0, 3.0]);
    }

    #[test]
    fn coedge_and_edge_proxy_reversals_are_independent() {
        for trim_reversed in [false, true] {
            for _edge_proxy_reversed in [false, true] {
                assert_eq!(
                    coedge_sense(trim_reversed),
                    if trim_reversed {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    }
                );
            }
        }
    }

    #[test]
    fn face_reversal_and_region_direction_have_exact_precedence() {
        assert_eq!(face_sense(false, None), Sense::Forward);
        assert_eq!(face_sense(true, None), Sense::Reversed);
        assert_eq!(face_sense(true, Some(1)), Sense::Forward);
        assert_eq!(face_sense(false, Some(-1)), Sense::Reversed);
    }

    #[test]
    fn inward_and_outward_serialized_solids_both_remain_solids() {
        assert_eq!(brep_body_kind(2, Some(1)), BodyKind::Solid);
        assert_eq!(brep_body_kind(2, Some(2)), BodyKind::Solid);
        assert_eq!(brep_body_kind(2, Some(3)), BodyKind::General);
        assert_eq!(brep_body_kind(1, Some(1)), BodyKind::Sheet);
    }

    #[test]
    fn representable_region_uses_bounded_membership_and_serialized_direction() {
        let raw = region_raw(
            vec![
                crate::brep::RawBrepFaceSide {
                    index: 0,
                    region: 1,
                    face: 0,
                    direction: 1,
                    source_range: 0..0,
                },
                crate::brep::RawBrepFaceSide {
                    index: 1,
                    region: 0,
                    face: 0,
                    direction: -1,
                    source_range: 0..0,
                },
            ],
            vec![region(0, 0), region(1, 1)],
        );
        let grouping = region_shell_groups(&raw, &[0]);
        assert!(!grouping.fallback);
        assert_eq!(grouping.face_groups, vec![0]);
        assert_eq!(grouping.region_labels, vec![1]);
        assert_eq!(grouping.shell_faces, vec![vec![0]]);
        assert_eq!(grouping.directions, Some(vec![1]));
    }

    #[test]
    fn two_bounded_regions_sharing_one_face_use_deterministic_incidence_fallback() {
        let raw = region_raw(
            vec![
                crate::brep::RawBrepFaceSide {
                    index: 0,
                    region: 1,
                    face: 0,
                    direction: 1,
                    source_range: 0..0,
                },
                crate::brep::RawBrepFaceSide {
                    index: 1,
                    region: 2,
                    face: 0,
                    direction: -1,
                    source_range: 0..0,
                },
            ],
            vec![region(0, 0), region(1, 1), region(2, 1)],
        );
        let grouping = region_shell_groups(&raw, &[0]);
        assert!(grouping.fallback);
        assert_eq!(grouping.region_labels, vec![0]);
        assert_eq!(grouping.shell_faces, vec![vec![0]]);
        assert!(grouping.directions.is_none());
    }

    #[test]
    fn c2_polycurve_merges_clamped_rational_segments_in_parent_domain() {
        let compound = crate::curves::DecodedCurve {
            geometry: CurveGeometry::Unknown { record: None },
            compound: Some(crate::curves::Compound {
                children: vec![
                    decoded_nurbs(line_nurbs(0.0, 1.0, true)),
                    decoded_nurbs(line_nurbs(-2.0, 2.0, false)),
                ],
                parameters: vec![10.0, 20.0, 40.0],
            }),
            warnings: Vec::new(),
        };
        let merged = c2_curve_to_nurbs(compound, 0).expect("merge");
        assert_eq!(merged.knots, vec![10.0, 10.0, 20.0, 20.0, 40.0, 40.0]);
        assert_eq!(merged.control_points.len(), 4);
        assert_eq!(merged.weights, Some(vec![2.0, 1.0, 1.0, 1.0]));
        assert!(!merged.periodic);
    }

    #[test]
    fn recursive_c2_polycurve_preserves_nested_parent_parameterization() {
        let nested = crate::curves::DecodedCurve {
            geometry: CurveGeometry::Unknown { record: None },
            compound: Some(crate::curves::Compound {
                children: vec![
                    decoded_nurbs(line_nurbs(0.0, 1.0, false)),
                    decoded_nurbs(line_nurbs(0.0, 1.0, false)),
                ],
                parameters: vec![0.0, 1.0, 2.0],
            }),
            warnings: Vec::new(),
        };
        let outer = crate::curves::DecodedCurve {
            geometry: CurveGeometry::Unknown { record: None },
            compound: Some(crate::curves::Compound {
                children: vec![nested],
                parameters: vec![5.0, 9.0],
            }),
            warnings: Vec::new(),
        };
        let merged = c2_curve_to_nurbs(outer, 0).expect("nested merge");
        assert_eq!(merged.knots, vec![5.0, 5.0, 7.0, 7.0, 9.0, 9.0]);
    }

    #[test]
    fn incompatible_c2_polycurve_degrades_before_pcurve_emission() {
        let quadratic = NurbsCurve {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.5, 1.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
            ],
            weights: Some(vec![1.0, 0.5, 1.0]),
            periodic: false,
        };
        let compound = crate::curves::DecodedCurve {
            geometry: CurveGeometry::Unknown { record: None },
            compound: Some(crate::curves::Compound {
                children: vec![
                    decoded_nurbs(line_nurbs(0.0, 1.0, false)),
                    decoded_nurbs(quadratic),
                ],
                parameters: vec![0.0, 1.0, 2.0],
            }),
            warnings: Vec::new(),
        };
        assert!(c2_curve_to_nurbs(compound, 0).is_err());
    }

    fn cap_boundary(points: &[Point3]) -> crate::extrusion::ExtrusionBoundary {
        let knots = vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0];
        let start = NurbsCurve {
            degree: 1,
            knots: knots.clone(),
            control_points: points.to_vec(),
            weights: None,
            periodic: false,
        };
        let end_points = points
            .iter()
            .map(|point| Point3::new(point.x, point.y, point.z + 5.0))
            .collect::<Vec<_>>();
        let end = NurbsCurve {
            degree: 1,
            knots: knots.clone(),
            control_points: end_points,
            weights: None,
            periodic: false,
        };
        let pcurve_points = points
            .iter()
            .map(|point| Point2::new(point.x, point.y))
            .collect::<Vec<_>>();
        let pcurve = crate::extrusion::CapPcurve {
            degree: 1,
            knots,
            control_points: pcurve_points,
            weights: None,
            periodic: false,
        };
        crate::extrusion::ExtrusionBoundary {
            start_curve: decoded_nurbs(start.clone()),
            start_nurbs: start,
            end_nurbs: end,
            start_pcurve: pcurve.clone(),
            end_pcurve: pcurve,
        }
    }

    fn cap_extrusion(caps: [bool; 2]) -> crate::extrusion::DecodedExtrusion {
        let outer = cap_boundary(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
            Point3::new(4.0, 4.0, 0.0),
            Point3::new(0.0, 4.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ]);
        let inner = cap_boundary(&[
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(2.0, 2.0, 0.0),
            Point3::new(2.0, 1.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ]);
        crate::extrusion::DecodedExtrusion {
            boundaries: vec![outer, inner],
            laterals: Vec::new(),
            direction: Vector3::new(0.0, 0.0, 5.0),
            cap_origins: [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 5.0)],
            cap_normals: [Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 0.0, 1.0)],
            cap_u_axes: [Vector3::new(1.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)],
            caps,
            meshes: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn test_association() -> SourceObjectAssociation {
        SourceObjectAssociation {
            format: "rhino".to_string(),
            object_id: "extrusion".to_string(),
            name: Some("Extrusion".to_string()),
            color: None,
            visible: Some(true),
            layer: None,
            instance_path: Vec::new(),
        }
    }

    #[test]
    fn extrusion_caps_build_outer_and_hole_loops_with_opposite_face_senses() {
        for (caps, expected_faces) in [([true, false], 1), ([false, true], 1), ([true, true], 2)] {
            let mut ir = CadIr::empty(Units::default());
            let association = test_association();
            let extrusion = cap_extrusion(caps);
            let directrices = extrusion
                .boundaries
                .iter()
                .enumerate()
                .map(|(index, boundary)| {
                    let id: cadmpeg_ir::ids::CurveId =
                        format!("rhino:object:curve#cap-{index}").into();
                    ir.model.curves.push(Curve {
                        id: id.clone(),
                        geometry: CurveGeometry::Nurbs(boundary.start_nurbs.clone()),
                        source_object: Some(association.clone()),
                    });
                    id
                })
                .collect::<Vec<_>>();
            let mut links = Vec::new();
            assert!(stage_extrusion_caps(
                &mut ir,
                &mut cadmpeg_ir::Annotations::default(),
                "caps",
                &association,
                &extrusion,
                &directrices,
                &mut links,
            ));
            assert_eq!(ir.model.faces.len(), expected_faces);
            assert_eq!(ir.model.regions.len(), expected_faces);
            assert_eq!(ir.model.shells.len(), expected_faces);
            assert_eq!(ir.model.loops.len(), expected_faces * 2);
            assert_eq!(ir.model.pcurves.len(), expected_faces * 2);
            if expected_faces == 2 {
                assert_eq!(ir.model.faces[0].sense, Sense::Reversed);
                assert_eq!(ir.model.faces[1].sense, Sense::Forward);
            }
            assert_eq!(cadmpeg_ir::validate(&ir, Vec::new()).error_count(), 0);
        }
    }

    #[test]
    fn cap_staging_failure_leaves_original_transaction_unmodified() {
        let original = CadIr::empty(Units::default());
        let mut candidate = original.clone();
        let mut links = Vec::new();
        assert!(!stage_extrusion_caps(
            &mut candidate,
            &mut cadmpeg_ir::Annotations::default(),
            "failure",
            &test_association(),
            &cap_extrusion([true, true]),
            &[],
            &mut links,
        ));
        assert_eq!(candidate, original);
        assert!(links.is_empty());
    }
}
