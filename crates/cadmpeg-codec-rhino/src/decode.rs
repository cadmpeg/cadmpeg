// SPDX-License-Identifier: Apache-2.0
//! Decode Rhino metadata and retain object records for later geometry phases.

use crate::loss::Diagnostics;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::draft::{DraftAccounting, ModelCheckpoint, ModelDraft};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::NurbsCurve,
    pcurve::{Pcurve, PcurveGeometry, PcurveNurbs, PcurveNurbsPoles, WeightedPole2},
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{IdentityKey, UnknownId};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::{loss::LossNote, Severity};
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Color, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{FinitePoint2, OrthonormalFrame3, UnitVector3};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::SourceProvenance;
use cadmpeg_ir::{Exactness, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::num::NonZeroUsize;

fn instance_members_are_unique(members: &[crate::wire::Uuid]) -> bool {
    let unique_members = members.iter().collect::<BTreeSet<_>>();
    unique_members.len() == members.len()
}

fn insert_feature_property(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: std::fmt::Arguments<'_>,
    value: std::fmt::Arguments<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    insert_feature_property_owned(properties, key, value.to_string())
}

fn insert_feature_property_owned(
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: std::fmt::Arguments<'_>,
    value: String,
) -> Result<(), cadmpeg_core::CodecError> {
    let key = cadmpeg_core::text::NonBlankString::new(key.to_string())
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("blank generated Rhino property key"))?;
    properties.insert(key, value);
    Ok(())
}

struct CageFiniteList<'a>(&'a [cadmpeg_ir::scalar::FiniteReal]);

impl std::fmt::Display for CageFiniteList<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, value) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            write!(f, "{}", value.get())?;
        }
        Ok(())
    }
}

struct CageWeightList<'a>(&'a [cadmpeg_ir::scalar::NonZeroReal]);

impl std::fmt::Display for CageWeightList<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, value) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            write!(f, "{}", value.get())?;
        }
        Ok(())
    }
}

struct CagePointList<'a>(&'a [Vec<cadmpeg_ir::scalar::FiniteReal>]);

impl std::fmt::Display for CagePointList<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, point) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(";")?;
            }
            write!(f, "{}", CageFiniteList(point))?;
        }
        Ok(())
    }
}

use crate::chunks::ArchiveVersion;
use crate::container::{OpaqueRecord, Scan};
use crate::loss::RhinoLossCode;
use crate::objects::{ObjectDescriptor, ObjectRecord, UserdataDescriptor};
use crate::settings::MillimeterScale;

/// Maximum bytes retained for one Rhino object record.
pub(crate) const RETAINED_RECORD_CAP: usize = 16 * 1024 * 1024;
/// Maximum bytes retained across all Rhino object records in one document.
pub(crate) const RETAINED_DOCUMENT_CAP: usize = 256 * 1024 * 1024;

#[derive(Debug)]
enum CandidateError {
    Admission(String),
    Validation(String),
    Codec(cadmpeg_core::CodecError),
}

impl From<String> for CandidateError {
    fn from(message: String) -> Self {
        Self::Admission(message)
    }
}

impl From<cadmpeg_core::CodecError> for CandidateError {
    fn from(error: cadmpeg_core::CodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<crate::history::ProjectionError> for CandidateError {
    fn from(error: crate::history::ProjectionError) -> Self {
        match error {
            crate::history::ProjectionError::Admission(message) => Self::Admission(message),
            crate::history::ProjectionError::Codec(error) => Self::Codec(error),
        }
    }
}

#[derive(Debug)]
enum ReferenceFailure {
    Semantic(String),
    Codec(cadmpeg_core::CodecError),
}

impl From<String> for ReferenceFailure {
    fn from(message: String) -> Self {
        Self::Semantic(message)
    }
}

impl From<cadmpeg_core::CodecError> for ReferenceFailure {
    fn from(error: cadmpeg_core::CodecError) -> Self {
        Self::Codec(error)
    }
}

impl std::fmt::Display for CandidateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Admission(message) | Self::Validation(message) => formatter.write_str(message),
            Self::Codec(error) => error.fmt(formatter),
        }
    }
}

#[derive(Debug)]
struct ClassOutcome<'a> {
    decoded: usize,
    retained: usize,
    native: Option<(RhinoLossCode, NonZeroUsize)>,
    attribute_degraded: usize,
    failed_framed: usize,
    first_object: &'a ObjectRecord,
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
enum GeometryOutcome {
    NativeRetained(RhinoLossCode),
    Decoded,
    Failed,
}

#[derive(Clone, Debug, Default)]
struct ReportBuckets {
    phase_warnings: Diagnostics,
    phase_losses: Vec<LossNote>,
    typed_losses: Vec<LossNote>,
}

#[derive(Debug, Clone, Copy)]
struct ReportCheckpoint {
    phase_warnings: usize,
    phase_losses: usize,
    typed_losses: usize,
}

impl ReportBuckets {
    fn checkpoint(&self) -> ReportCheckpoint {
        ReportCheckpoint {
            phase_warnings: self.phase_warnings.len(),
            phase_losses: self.phase_losses.len(),
            typed_losses: self.typed_losses.len(),
        }
    }

    fn rollback(&mut self, checkpoint: ReportCheckpoint) {
        self.phase_warnings.truncate(checkpoint.phase_warnings);
        self.phase_losses.truncate(checkpoint.phase_losses);
        self.typed_losses.truncate(checkpoint.typed_losses);
    }
}

const MAX_INSTANCE_REFERENCES: usize = 1 << 20;
const MAX_INSTANCE_MEMBERS: usize = 1 << 20;
const MAX_INSTANCE_ENTITIES: usize = 1 << 20;
/// Member visits, copied entities, and the points, control points, mesh
/// vertices and triangles, and cage vertices that instance expansion copies in
/// one document. Rejected expansions stay charged, so the copy work of the
/// document is bounded even when every reference is rolled back.
const MAX_INSTANCE_COPY_ELEMENTS: usize = 1 << 26;
const ENTITY_BUDGET_EXCEEDED: &str = "document instance entity budget exceeded";
const COPY_ELEMENT_BUDGET_EXCEEDED: &str =
    "document instance copy element budget exceeded (MAX_INSTANCE_COPY_ELEMENTS)";

#[derive(Debug, Clone, Copy)]
struct ExpansionBudget {
    references: usize,
    members: usize,
    entities: usize,
    copied_elements: usize,
    limits: [usize; 3],
    copy_element_limit: usize,
}

impl ExpansionBudget {
    fn new() -> Self {
        Self {
            references: 0,
            members: 0,
            entities: 0,
            copied_elements: 0,
            limits: [
                MAX_INSTANCE_REFERENCES,
                MAX_INSTANCE_MEMBERS,
                MAX_INSTANCE_ENTITIES,
            ],
            copy_element_limit: MAX_INSTANCE_COPY_ELEMENTS,
        }
    }

    /// Charges one local expansion counter. `false` means the local cap
    /// rejects this item; the caller skips it and the decode continues.
    fn charge(value: &mut usize, amount: usize, limit: usize) -> bool {
        match value.checked_add(amount) {
            Some(requested) if requested <= limit => {
                *value = requested;
                true
            }
            _ => false,
        }
    }

    fn reference(&mut self) -> Result<(), ReferenceFailure> {
        if Self::charge(&mut self.references, 1, self.limits[0]) {
            Ok(())
        } else {
            Err(ReferenceFailure::Semantic(
                "document instance reference budget exceeded".to_owned(),
            ))
        }
    }

    fn member(&mut self) -> Result<(), ReferenceFailure> {
        if Self::charge(&mut self.members, 1, self.limits[1]) {
            Ok(())
        } else {
            Err(ReferenceFailure::Semantic(
                "document instance member budget exceeded".to_owned(),
            ))
        }
    }

    /// `false` means the local entity cap rejects this item.
    fn entities(&mut self, amount: usize) -> bool {
        Self::charge(&mut self.entities, amount, self.limits[2])
    }

    /// Exhausts the copy budget at the limit, so later references fail before
    /// they decode another member.
    fn copy_elements(&mut self, amount: usize) -> Result<(), ReferenceFailure> {
        if Self::charge(&mut self.copied_elements, amount, self.copy_element_limit) {
            Ok(())
        } else {
            self.copied_elements = self.copy_element_limit;
            Err(ReferenceFailure::Semantic(
                COPY_ELEMENT_BUDGET_EXCEEDED.to_owned(),
            ))
        }
    }

    fn copy_entity(&mut self, payload: usize) -> Result<(), ReferenceFailure> {
        self.copy_elements(
            payload
                .checked_add(1)
                .ok_or_else(|| COPY_ELEMENT_BUDGET_EXCEEDED.to_owned())?,
        )
    }
}

/// Source-record links and object outcomes that one top-level instance
/// expansion added. A rejected expansion removes exactly these, so rollback
/// work follows the expansion, not the document.
#[derive(Debug, Default)]
struct ExpansionUndo {
    links: Vec<(usize, String)>,
    outcomes: Vec<usize>,
}

impl ExpansionUndo {
    fn absorb(&mut self, mut inner: Self) {
        self.links.append(&mut inner.links);
        self.outcomes.append(&mut inner.outcomes);
    }
}

#[derive(Debug)]
struct InstanceSelection {
    source_order: usize,
    key: IdentityKey,
    path: Vec<String>,
}

struct InstanceKey<'a> {
    path: &'a [String],
    member: crate::wire::Uuid,
}

impl std::fmt::Display for InstanceKey<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, segment) in self.path.iter().enumerate() {
            if index != 0 {
                formatter.write_str(".")?;
            }
            formatter.write_str(segment)?;
        }
        write!(formatter, ".{}", self.member)
    }
}

impl InstanceSelection {
    fn new(
        source_order: usize,
        path: &[String],
        member: crate::wire::Uuid,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let key = format!("{}", InstanceKey { path, member });
        let key = IdentityKey::try_new(key).map_err(cadmpeg_core::CodecError::malformed)?;
        Ok(Self {
            source_order,
            key,
            path: path.to_vec(),
        })
    }
}

#[derive(Clone, Copy)]
struct InstanceDisplay {
    color: Option<Color>,
    visible: bool,
}

/// Mutable decode state shared by metadata and geometry phases.
pub(crate) struct DecodeContext<'a> {
    scan: &'a Scan<'a>,
    expand: crate::mesh::MeshExpand<'a>,
    session: cadmpeg_ir::draft::CommitSession<'a, CadIr>,
    annotations: cadmpeg_ir::Annotations,
    opaque_records: Vec<UnknownRecord>,
    statuses: Vec<Option<GeometryOutcome>>,
    retained_bytes: usize,
    retention_limits: [usize; 2],
    mesh_budget: crate::mesh::MeshBudget,
    geometry_transferred: bool,
    /// Transactional report buckets produced by semantic decode phases.
    report: ReportBuckets,
    instance_selection: Option<InstanceSelection>,
    instance_display: Option<InstanceDisplay>,
    object_candidates: HashMap<crate::wire::Uuid, Vec<usize>>,
    definition_candidates: HashMap<crate::wire::Uuid, usize>,
    expansion_budget: ExpansionBudget,
    expansion_undo: Option<ExpansionUndo>,
}

impl<'a> DecodeContext<'a> {
    /// Starts a transaction from a completed Rhino scan.
    pub(crate) fn new(
        scan: &'a Scan<'a>,
        expand: crate::mesh::MeshExpand<'a>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let session = expand.ctx();

        let mut object_candidates = HashMap::new();
        for (source_order, object) in scan.objects.iter().enumerate() {
            if let Some(identity) = object.identity() {
                object_candidates
                    .entry(identity.object_id)
                    .or_insert_with(Vec::new)
                    .push(source_order);
            }
        }
        let mut definition_candidates = HashMap::new();
        for (index, definition) in scan.definitions.definitions().iter().enumerate() {
            let id = definition.id();
            definition_candidates.insert(id, index);
        }
        let report = ReportBuckets::default();
        let ir = build_ir(scan);
        let mut context = Self {
            scan,
            expand,
            session: cadmpeg_ir::draft::CommitSession::new(ir, session, Some("rhino"))?,
            annotations: cadmpeg_ir::Annotations::default(),
            opaque_records: Vec::new(),
            statuses: Vec::new(),
            retained_bytes: 0,
            retention_limits: [RETAINED_RECORD_CAP, RETAINED_DOCUMENT_CAP],
            mesh_budget: crate::mesh::MeshBudget::new(),
            geometry_transferred: false,
            report,
            instance_selection: None,
            instance_display: None,
            object_candidates,
            definition_candidates,
            expansion_budget: ExpansionBudget::new(),
            expansion_undo: None,
        };
        context.retain_object_records()?;
        context.retain_opaque_records()?;
        Ok(context)
    }

    #[cfg(test)]
    pub(crate) fn set_expansion_limits(&mut self, limits: [usize; 3]) {
        self.expansion_budget.limits = limits;
    }

    #[cfg(test)]
    pub(crate) fn set_copy_element_limit(&mut self, limit: usize) {
        self.expansion_budget.copy_element_limit = limit;
    }

    #[cfg(test)]
    pub(crate) fn set_retention_limits(
        &mut self,
        record: usize,
        document: usize,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.retention_limits = [record, document];
        self.session.replace_unknowns(Vec::new())?;
        self.opaque_records.clear();
        self.statuses.clear();
        self.retained_bytes = 0;
        self.retain_object_records()?;
        self.retain_opaque_records()
    }

    /// Returns the document mesh budget's retained-byte count.
    #[cfg(test)]
    pub(crate) fn mesh_budget_used(&self) -> usize {
        self.mesh_budget.used()
    }

    /// Returns the source archive version.
    fn archive(&self) -> ArchiveVersion {
        self.scan.archive
    }

    /// Returns the source coordinate binding.
    fn unit_binding(&self) -> crate::settings::UnitBinding {
        crate::settings::UnitBinding::from_units(self.scan.metadata.settings.units.as_ref())
    }

    /// Returns the scale that is safe for canonical millimetre geometry.
    fn neutral_scale(&self) -> Option<MillimeterScale> {
        self.unit_binding().neutral_scale()
    }

    /// Looks up a scanned object by deterministic source order.
    #[cfg(test)]
    fn object(&self, source_order: usize) -> Option<&ObjectDescriptor> {
        self.scan
            .objects
            .get(source_order)
            .and_then(|object| object.framed())
    }

    /// Looks up the retained unknown record for a source-order object.
    #[cfg(test)]
    fn unknown(&self, source_order: usize) -> Option<&UnknownRecord> {
        self.session.unknowns().get(source_order)
    }

    #[cfg(test)]
    fn unknown_links_mut(&mut self, source_order: usize) -> Option<&mut Vec<String>> {
        self.session
            .unknown_links_mut(source_order)
            .expect("test link mutation")
            .map(|(_, links)| links)
    }

    #[cfg(test)]
    fn unknown_count(&self) -> usize {
        self.session.unknowns().len()
    }

    /// Appends a later geometry-phase link to an object record.
    fn append_link(
        &mut self,
        source_order: usize,
        link: &str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.append_links(source_order, &[link])
    }

    /// Merges links into an object record's sorted, duplicate-free link list.
    /// Each new copy is retained output; an active expansion logs it for
    /// rollback.
    fn append_links<S: AsRef<str>>(
        &mut self,
        source_order: usize,
        incoming: &[S],
    ) -> Result<(), cadmpeg_core::CodecError> {
        let ctx = self.expand.ctx();
        let Some((id, links)) = self.session.unknown_links_mut(source_order)? else {
            return Ok(());
        };
        let mut added = Vec::new();
        for link in incoming {
            let link = link.as_ref();
            if link != id
                && links
                    .binary_search_by(|existing| existing.as_str().cmp(link))
                    .is_err()
            {
                added.push(link);
            }
        }
        added.sort_unstable();
        added.dedup();
        for link in &added {
            ctx.retain_bytes(u64_from_index(link.len()), "Rhino source record link")?;
        }
        links.extend(added.iter().map(|link| (*link).to_owned()));
        // Two sorted runs: the stable sort merges them in linear time.
        links.sort();
        if let Some(undo) = &mut self.expansion_undo {
            undo.links.extend(
                added
                    .into_iter()
                    .map(|link| (source_order, link.to_owned())),
            );
        }
        Ok(())
    }

    fn validate_candidate<T>(
        &mut self,
        apply: impl FnOnce(
            &mut CadIr,
            &mut cadmpeg_ir::annotations::SparseAnnotationTransaction<'_>,
        ) -> T,
    ) -> Result<T, CandidateError> {
        self.validate_candidate_fallible(|ir, annotations| Ok::<_, String>(apply(ir, annotations)))
    }

    fn validate_candidate_fallible<T, E: Into<CandidateError>>(
        &mut self,
        apply: impl FnOnce(
            &mut CadIr,
            &mut cadmpeg_ir::annotations::SparseAnnotationTransaction<'_>,
        ) -> Result<T, E>,
    ) -> Result<T, CandidateError> {
        let session = self.expand.ctx();
        let mut annotations = self.annotations.sparse_transaction();
        let mut candidate = CadIr::empty();
        candidate.tolerances = self.session.document().tolerances;

        let value = apply(&mut candidate, &mut annotations).map_err(Into::into)?;
        let entity_count = candidate.model.entity_count();
        let mut budget = self.expansion_budget;
        let expanding = self.instance_selection.is_some();
        let appended = self.session.try_admit_append(
            candidate,
            annotations,
            cadmpeg_ir::RHINO_DRAFT_CHECKS,
            |validation, annotations| {
                let validation = match validation {
                    Ok(report) => report,
                    Err(error) => return Ok(Err(CandidateError::Admission(format!("{error}")))),
                };
                if !validation.is_ok() {
                    return Ok(Err(CandidateError::Validation(validation_findings(
                        validation,
                    ))));
                }
                if expanding && !budget.entities(entity_count) {
                    return Ok(Err(CandidateError::Admission(
                        ENTITY_BUDGET_EXCEEDED.to_string(),
                    )));
                }
                session.charge_entities(u64_from_index(entity_count), "rhino_instance_entities")?;
                let annotations = annotations.prepare();
                Ok(Ok((value, annotations)))
            },
        );
        let (value, annotations) = appended??;
        annotations.apply(&mut self.annotations);
        self.expansion_budget = budget;
        Ok(value)
    }

    /// Returns mutable IR for the current decode transaction.
    #[cfg(test)]
    fn ir_mut(&mut self) -> &mut CadIr {
        self.session.document_mut().expect("test document mutation")
    }

    #[cfg(test)]
    fn reject_duplicate_entity_candidate(&mut self) -> String {
        self.session
            .document_mut()
            .expect("test document mutation")
            .model
            .points
            .push(Point::new(
                "rhino:test:point#duplicate"
                    .try_into()
                    .expect("valid identity"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
                    .expect("a finite position is a point"),
                None,
            ));
        let result = self.validate_candidate(|candidate, _annotations| {
            let point = Point::new(
                "rhino:test:point#duplicate"
                    .try_into()
                    .expect("valid identity"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                    .expect("a finite position is a point"),
                None,
            );
            candidate.model.points.push(point);
        });
        result
            .expect_err("duplicate entity ID must fail validation")
            .to_string()
    }

    /// Marks one retained object as successfully decoded.
    pub(crate) fn mark_decoded(&mut self, source_order: usize) -> bool {
        self.transition(source_order, GeometryOutcome::Decoded)
    }

    /// Marks one framed object as failed after a skippable payload error.
    fn mark_failed(&mut self, source_order: usize) -> bool {
        self.transition(source_order, GeometryOutcome::Failed)
    }

    /// Marks one object as read but retained as native passthrough.
    ///
    /// Class totals are derived from the object outcomes when building the report.
    fn mark_native_retained(&mut self, source_order: usize, code: RhinoLossCode) -> bool {
        self.transition(source_order, GeometryOutcome::NativeRetained(code))
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

    /// Resolves a foreign object UUID to its native record identity.
    /// Non-nil UUIDs that do not resolve are charged against `role`.
    fn resolve_object_record(
        &mut self,
        source_order: usize,
        role: &str,
        id: crate::wire::Uuid,
    ) -> Option<String> {
        if id.is_nil() {
            return None;
        }
        let code = match self.resolve_object(id) {
            ObjectReference::Resolved(order) => {
                return Some(format!("{}", Self::mint_unknown_id(order)));
            }
            ObjectReference::Missing => RhinoLossCode::ReferenceMemberUnresolved,
            ObjectReference::Ambiguous => RhinoLossCode::ReferenceMemberAmbiguous,
        };

        self.report.typed_losses.push(code.note(format_args!(
            "{role} in object record {source_order} references object {id}"
        )));
        None
    }

    /// Decode and atomically commit supported simple geometry.
    pub(crate) fn decode_geometry(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        if !self.archive().is_chunked() {
            return Ok(());
        }
        let objects = self.scan.objects.len();
        // An instance member decode visits only the selected object.
        let orders = match &self.instance_selection {
            Some(selected) if selected.source_order < objects => {
                selected.source_order..selected.source_order + 1
            }
            Some(_) => 0..0,
            None => 0..objects,
        };
        for source_order in orders {
            let Some(object) = self.scan.objects[source_order].framed() else {
                continue;
            };
            if self.instance_selection.is_none() && self.is_definition_member(object) {
                continue;
            }
            if crate::instances::is_reference_class(object.class_uuid) {
                self.expand_reference(source_order)?;
                continue;
            }
            if crate::subd::supported_class(object.class_uuid) {
                self.decode_subd(source_order, object)?;
                continue;
            }
            if crate::brep::supported_class(object.class_uuid) {
                self.decode_brep(source_order, object)?;
                continue;
            }
            if crate::extrusion::supported_class(object.class_uuid) {
                self.decode_extrusion(source_order, object)?;
                continue;
            }
            if object.class_uuid == crate::hatch::CLASS {
                self.decode_hatch(source_order, object)?;
                continue;
            }
            if object.class_uuid == crate::detail::CLASS {
                self.decode_detail(source_order, object)?;
                continue;
            }
            if object.class_uuid == crate::cage::CLASS {
                self.decode_cage(source_order, object)?;
                continue;
            }
            if object.class_uuid == crate::morph::CLASS {
                self.decode_morph(source_order, object)?;
                continue;
            }
            if object.class_uuid == crate::curve_on_surface::CLASS {
                self.decode_curve_on_surface(source_order, object)?;
                continue;
            }
            if object.class_uuid == crate::polyedge::CURVE_CLASS {
                self.decode_polyedge(source_order, object)?;
                continue;
            }
            if !crate::curves::supported_class(object.class_uuid)
                && !crate::mesh::supported_class(object.class_uuid)
            {
                continue;
            }
            let Some(scale) = self.neutral_scale() else {
                self.scan_unbound_unit_warning(source_order, "simple geometry");
                continue;
            };
            if crate::mesh::supported_class(object.class_uuid) {
                let identity = &object.identity;
                let Some(key) = self.checked_object_key(identity, source_order) else {
                    continue;
                };
                let decoded = crate::mesh::decode(
                    self.expand,
                    self.scan.data,
                    object.class_data_range.clone(),
                    self.archive(),
                    crate::mesh::MeshDecodeOptions {
                        writer_version: self.scan.metadata.properties.writer_version,
                        association: Some(self.source_association(identity)?),
                        id: crate::mesh::MeshId::Ready(
                            cadmpeg_ir::tessellation::TessellationId::mint(format!(
                                "rhino:object:tessellation#{key}"
                            ))
                            .map_err(|error| {
                                cadmpeg_core::CodecError::Malformed(error.to_string())
                            })?,
                        ),
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
                            .filter_map(UserdataDescriptor::known)
                            .find(|extra| {
                                extra.class_uuid == crate::subd::SUBD_MESH_PROXY_USERDATA
                                    && extra.item_uuid == crate::subd::SUBD_MESH_PROXY_USERDATA
                            })
                            .cloned();
                        let mut proxy_transferred = false;
                        if let (Some(extra), Some(fingerprint)) = (proxy, mesh.proxy_fingerprint) {
                            let subd_id = {
                                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::SubdId::compose(
                                    &cadmpeg_ir::identity_namespace!("rhino", "object", "subd"),
                                    key.clone(),
                                ))
                            }?;
                            match crate::subd::decode_mesh_proxy(
                                self.scan.data,
                                &extra,
                                self.archive(),
                                scale,
                                subd_id,
                                fingerprint,
                            ) {
                                Ok(Some(decoded)) => {
                                    proxy_transferred = self.commit_subd_surface(
                                        source_order,
                                        decoded,
                                        scale != MillimeterScale::IDENTITY,
                                    )?;
                                    if proxy_transferred {
                                        self.mark_decoded(source_order);
                                    } else {
                                        self.scan_warning(source_order, format_args!("valid SubD mesh proxy rejected by IR validation; parent mesh retained"));
                                    }
                                }
                                Ok(None) => self.scan_warning(source_order, format_args!("SubD mesh proxy failed its validity or parent-mesh identity checks; parent mesh retained")),
                                Err(crate::subd::SubdError::Resource(limit)) => {
                                    return Err(cadmpeg_core::CodecError::ResourceLimit(limit));
                                }
                                Err(error) => self.scan_warning(source_order, format_args!("SubD mesh proxy dropped: {error}; parent mesh retained")),
                            }
                        }
                        if !proxy_transferred && self.commit_mesh(source_order, mesh)? {
                            self.mark_decoded(source_order);
                        } else if !proxy_transferred {
                            self.mark_failed(source_order);
                        }
                    }
                    Err(crate::curves::GeometryError::Codec(error)) => return Err(error),
                    Err(error) => {
                        let future = matches!(
                            error,
                            crate::curves::GeometryError::UnsupportedVersion { .. }
                        );
                        self.scan_warning(
                            source_order,
                            format_args!(
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
                self.expand.ctx(),
                self.scan.data,
                object.class_uuid,
                object.class_data_range.clone(),
                scale,
                self.archive(),
            );
            let procedural_surface = crate::surfaces::is_procedural_class(object.class_uuid);
            match decoded {
                Ok(value) => {
                    if self.commit_geometry(source_order, value)? {
                        self.mark_decoded(source_order);
                    } else if procedural_surface {
                        self.scan_warning(
                            source_order,
                            format_args!("procedural surface candidate rejected by IR validation"),
                        );
                        self.commit_unknown_surface(source_order)?;
                    } else {
                        self.mark_failed(source_order);
                    }
                }
                Err(crate::curves::GeometryError::Codec(error)) => return Err(error),
                Err(error) => {
                    let future = matches!(
                        error,
                        crate::curves::GeometryError::UnsupportedVersion { .. }
                    );
                    self.scan_warning(
                        source_order,
                        format_args!(
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
                        self.commit_unknown_surface(source_order)?;
                    } else if !future {
                        self.mark_failed(source_order);
                    }
                }
            }
        }
        Ok(())
    }

    /// Decode semantic dimensions independently of shape carriers.
    fn decode_dimensions(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        if !self.archive().is_chunked() {
            return Ok(());
        }
        for source_order in 0..self.scan.objects.len() {
            let Some(object) = self.scan.objects[source_order].framed() else {
                continue;
            };
            if !crate::dimensions::supported_class(object.class_uuid) {
                continue;
            }
            if self.is_definition_member(object) {
                self.scan_warning(source_order, format_args!("definition-member dimension retained because annotation instance expansion is unsupported"));
                continue;
            }
            let Some(scale) = self.neutral_scale() else {
                self.scan_unbound_unit_warning(source_order, "dimension");
                continue;
            };
            let identity = &object.identity;
            let Some(key) = self.checked_object_key(identity, source_order) else {
                continue;
            };
            match crate::dimensions::decode(
                self.expand.ctx(),
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
                                self.report.typed_losses.push(RhinoLossCode::DuplicateRecordResolved.note(format_args!(
                                    "{label} object at offset {} has {count} matching userdata records; first serialized record wins",
                                    object.range.start
                                )));
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
                                format_args!("dimension extension retained: {error}"),
                            );
                            continue;
                        }
                    }
                    // `SemanticAnnotation::order` must be globally unique and
                    // is a `u32`. The arena length is the dense next index and
                    // rolls back with the arena, unlike a standalone counter.
                    let Ok(order) =
                        u32::try_from(self.session.document().model.semantic_annotations.len())
                    else {
                        self.scan_warning(source_order, format_args!("dimension retained because the annotation arena exceeds u32 ordinals"));
                        continue;
                    };
                    let object = self.session.unknowns()[source_order].id().as_str();
                    let (annotation, unresolved) = match crate::dimensions::project(
                        &dimension,
                        key.as_str(),
                        (!identity.name.is_empty()).then_some(identity.name.as_str()),
                        object,
                        order,
                    ) {
                        Ok(value) => value,
                        Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => {
                            return Err(error)
                        }
                        Err(error) => {
                            self.scan_warning(source_order, format_args!("{error}"));
                            continue;
                        }
                    };
                    if dimension.override_present {
                        self.report.typed_losses.push(
                            RhinoLossCode::DimensionOverrideDropped.note(format_args!(
                                "dimension object at offset {} has an unapplied style override",
                                dimension.source_range.start
                            )),
                        );
                    }
                    let links = [annotation.id.as_str().to_owned()];
                    let result = self.validate_candidate(|candidate, _annotations| {
                        candidate.model.semantic_annotations.push(annotation);
                    });
                    match result {
                        Ok(()) => {
                            self.append_links(source_order, &links)?;
                            self.mark_decoded(source_order);
                            for code in unresolved {
                                self.report.typed_losses.push(code.note(format_args!(
                                    "dimension record {source_order} reference is not resolved to a \
                                     decoded record"
                                )));
                            }
                        }
                        Err(CandidateError::Codec(error)) => return Err(error),
                        Err(error) => self.scan_warning(
                            source_order,
                            format_args!("dimension candidate rejected: {error}"),
                        ),
                    }
                }
                Err(crate::chunks::FramingError::Resource(limit)) => {
                    return Err(cadmpeg_core::CodecError::ResourceLimit(limit));
                }
                Err(error) => {
                    self.scan_warning(source_order, format_args!("dimension retained: {error}"));
                    self.mark_failed(source_order);
                }
            }
        }
        Ok(())
    }

    fn decode_hatch(
        &mut self,
        source_order: usize,
        object: &ObjectDescriptor,
    ) -> Result<(), cadmpeg_core::CodecError> {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

        let Some(scale) = self.neutral_scale() else {
            self.scan_unbound_unit_warning(source_order, "hatch");
            return Ok(());
        };
        let identity = &object.identity;
        let mut hatch = match crate::hatch::decode(
            self.expand,
            object.class_data_range.clone(),
            scale,
            self.archive(),
        ) {
            Ok(hatch) => hatch,
            Err(crate::curves::GeometryError::Codec(error)) => return Err(error),
            Err(error) => {
                let future = matches!(
                    error,
                    crate::curves::GeometryError::UnsupportedVersion { .. }
                );
                self.scan_warning(
                    source_order,
                    format_args!(
                        "hatch {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
                return Ok(());
            }
        };
        let duplicate_count =
            duplicate_userdata_count(&object.userdata, crate::hatch::V5_HATCH_EXTRA);
        if duplicate_count > 1 {
            self.report.typed_losses.push(RhinoLossCode::DuplicateRecordResolved.note(format_args!(
                "hatch object at offset {} has {duplicate_count} matching userdata records; last valid serialized record wins",
                object.range.start
            )));
        }
        if let Err(errors) = crate::hatch::apply_userdata(
            self.scan.data,
            &object.userdata,
            scale,
            self.archive(),
            &mut hatch,
        ) {
            let class = self.scan.objects[source_order]
                .class_uuid()
                .unwrap_or_else(crate::wire::Uuid::nil);
            for error in errors {
                self.report.phase_warnings.push_coded(
                    RhinoLossCode::ObjectDecodeDiagnostic,
                    format_args!("{class}: hatch userdata extension failed: {error}"),
                );
            }
        }
        let Some(key) = self.checked_object_key(identity, source_order) else {
            return Ok(());
        };
        let association = self.source_association(identity)?;
        let feature_id = {
            Ok::<_, cadmpeg_core::CodecError>(FeatureId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "hatch", "feature"),
                key.clone(),
            ))
        }?;
        let transform =
            match hatch_plane_transform(&hatch.plane, scale, &format!("rhino hatch record #{key}"))
            {
                Ok(transform) => transform,
                Err(error) => {
                    self.scan_warning(
                        source_order,
                        format_args!("hatch placement failed: {error}"),
                    );
                    self.mark_failed(source_order);
                    return Ok(());
                }
            };
        for hatch_loop in &mut hatch.loops {
            match transform_decoded_curve(self.expand.ctx(), &mut hatch_loop.curve, transform) {
                Ok(()) => {}
                Err(ReferenceFailure::Codec(error)) => return Err(error),
                Err(ReferenceFailure::Semantic(error)) => {
                    self.scan_warning(
                        source_order,
                        format_args!("hatch loop placement failed: {error}"),
                    );
                    self.mark_failed(source_order);
                    return Ok(());
                }
            }
        }
        let loop_ids = hatch_loop_ids(
            key.as_str(),
            hatch.loops.iter().map(|hatch_loop| hatch_loop.kind),
        );
        let mut parameters = BTreeMap::new();
        insert_feature_property(
            &mut parameters,
            format_args!("pattern_index"),
            format_args!("{}", hatch.pattern_index),
        )?;
        insert_feature_property(
            &mut parameters,
            format_args!("pattern_scale"),
            format_args!("{}", hatch.pattern_scale.get()),
        )?;
        insert_feature_property(
            &mut parameters,
            format_args!("pattern_rotation"),
            format_args!("{}", hatch.pattern_rotation.get()),
        )?;
        insert_feature_property(
            &mut parameters,
            format_args!("basepoint"),
            format_args!("{},{}", hatch.basepoint[0].get(), hatch.basepoint[1].get()),
        )?;
        if let Some(gradient) = hatch.gradient.as_ref() {
            let gradient = crate::hatch::gradient_json(gradient)?;
            insert_feature_property_owned(&mut parameters, format_args!("gradient"), gradient)?;
        }
        for (index, (kind, id)) in loop_ids.iter().enumerate() {
            insert_feature_property(
                &mut parameters,
                format_args!("loop_{index}"),
                format_args!(
                    "{}:{id}",
                    match kind {
                        crate::hatch::LoopKind::Outer => "outer",
                        crate::hatch::LoopKind::Inner => "inner",
                    }
                ),
            )?;
        }
        let feature = Feature {
            id: feature_id.clone(),
            ordinal: cadmpeg_core::decode::u64_from_index(hatch.source_range.start),
            name: (!identity.name.is_empty()).then(|| identity.name.clone()),
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: Some("RhinoHatch".to_string()),
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "hatch".into(),
                    parameters,
                }),
            ),
            native_ref: Some(
                self.session.unknowns()[source_order]
                    .id()
                    .as_str()
                    .to_owned(),
            ),
        };
        let hatch_loops = hatch.loops;
        let session = self.expand.ctx();
        let result = self.validate_candidate_fallible(|candidate, candidate_annotations| {
            for (index, hatch_loop) in hatch_loops.into_iter().enumerate() {
                commit_curve_tree(
                    session,
                    candidate,
                    candidate_annotations,
                    hatch_loop.curve,
                    CurveCommitSource {
                        key: key.as_str(),
                        association: &association,
                        record: None,
                        path: &format!("hatch-loop-{index}"),
                    },
                )?;
            }
            candidate.model.features.push(feature);
            Ok::<(), CandidateError>(())
        });
        match result {
            Ok(()) => {
                for warning in hatch.warnings {
                    self.scan_diagnostic(source_order, &warning);
                }
                let links = hatch_source_links(loop_ids, &feature_id);
                self.append_links(source_order, &links)?;
                self.geometry_transferred = true;
                self.mark_native_retained(source_order, RhinoLossCode::HatchFillNotTransferred);
            }
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("hatch candidate rejected: {error}"),
                );
                self.mark_failed(source_order);
            }
        }
        Ok(())
    }

    fn decode_polyedge(
        &mut self,
        source_order: usize,
        object: &ObjectDescriptor,
    ) -> Result<(), cadmpeg_core::CodecError> {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

        let identity = &object.identity;
        let polyedge = match crate::polyedge::decode(
            self.expand,
            object.class_data_range.clone(),
            self.archive(),
        ) {
            Ok(value) => value,
            Err(crate::chunks::FramingError::Resource(limit)) => {
                return Err(cadmpeg_core::CodecError::ResourceLimit(limit));
            }
            Err(error) => {
                self.scan_warning(source_order, format_args!("polyedge retained: {error}"));
                self.mark_failed(source_order);
                return Ok(());
            }
        };
        let Some(construction) = crate::polyedge::semantic_json(&polyedge) else {
            self.scan_warning(
                source_order,
                format_args!("polyedge semantic serialization failed"),
            );
            return Ok(());
        };
        let Some(key) = self.checked_object_key(identity, source_order) else {
            return Ok(());
        };
        let id = {
            Ok::<_, cadmpeg_core::CodecError>(FeatureId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "polyedge", "feature"),
                key.clone(),
            ))
        }?;
        let mut parameters = BTreeMap::new();
        for (index, segment) in polyedge.segments.iter().enumerate() {
            if let Some(record) = self.resolve_object_record(
                source_order,
                "polyedge segment",
                segment.reference.object_id,
            ) {
                insert_feature_property_owned(
                    &mut parameters,
                    format_args!("segment_{index}_object"),
                    record,
                )?;
            }
        }
        let mut source_properties = BTreeMap::new();
        insert_feature_property_owned(
            &mut source_properties,
            format_args!("construction"),
            construction,
        )?;
        let name = (!identity.name.is_empty()).then(|| identity.name.clone());
        let feature = Feature {
            id: id.clone(),
            ordinal: cadmpeg_core::decode::u64_from_index(source_order),
            name,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties,
            source_tag: Some("RhinoPolyEdgeReference".to_string()),
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "polyedge_reference".into(),
                    parameters,
                }),
            ),
            native_ref: Some(Self::mint_unknown_id(source_order).to_string()),
        };
        match self
            .validate_candidate(|candidate, _annotations| candidate.model.features.push(feature))
        {
            Ok(()) => {
                self.append_link(source_order, id.as_str())?;
                self.mark_native_retained(
                    source_order,
                    RhinoLossCode::PolyedgeReferencesNotResolved,
                );
            }
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(error) => self.scan_warning(
                source_order,
                format_args!("polyedge candidate rejected: {error}"),
            ),
        }
        Ok(())
    }

    fn decode_detail(
        &mut self,
        source_order: usize,
        object: &ObjectDescriptor,
    ) -> Result<(), cadmpeg_core::CodecError> {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

        let identity = &object.identity;
        let detail = match crate::detail::decode(
            self.scan.data,
            object.class_data_range.clone(),
            self.archive(),
        ) {
            Ok(detail) => detail,
            Err(crate::curves::GeometryError::Codec(error)) => return Err(error),
            Err(error) => {
                let future = matches!(
                    error,
                    crate::curves::GeometryError::UnsupportedVersion { .. }
                );
                self.scan_warning(
                    source_order,
                    format_args!(
                        "detail {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
                return Ok(());
            }
        };
        let Some(key) = self.checked_object_key(identity, source_order) else {
            return Ok(());
        };
        let association = self.source_association(identity)?;
        let curve_id = format!("rhino:object:curve#{key}.detail-boundary");
        let feature_id = {
            Ok::<_, cadmpeg_core::CodecError>(FeatureId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "detail", "feature"),
                key.clone(),
            ))
        }?;
        let view = &self.scan.data[detail.view_range.clone()];
        let mut source_properties = BTreeMap::new();
        insert_feature_property(
            &mut source_properties,
            format_args!("view_bytes"),
            format_args!("{}", view.len()),
        )?;
        insert_feature_property(
            &mut source_properties,
            format_args!("view_sha256"),
            format_args!("{}", sha256_hex(view)),
        )?;
        let mut parameters = BTreeMap::new();
        insert_feature_property(
            &mut parameters,
            format_args!("boundary"),
            format_args!("{curve_id}"),
        )?;
        insert_feature_property(
            &mut parameters,
            format_args!("page_per_model_ratio"),
            format_args!("{}", detail.page_per_model_ratio.get()),
        )?;
        let feature = Feature {
            id: feature_id.clone(),
            ordinal: cadmpeg_core::decode::u64_from_index(detail.source_range.start),
            name: (!identity.name.is_empty()).then(|| identity.name.clone()),
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties,
            source_tag: Some("RhinoDetailView".to_string()),
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "detail_view".into(),
                    parameters,
                }),
            ),
            native_ref: Some(
                self.session.unknowns()[source_order]
                    .id()
                    .as_str()
                    .to_owned(),
            ),
        };
        let session = self.expand.ctx();
        let result = self.validate_candidate_fallible(|candidate, candidate_annotations| {
            commit_curve_tree(
                session,
                candidate,
                candidate_annotations,
                detail.boundary,
                CurveCommitSource {
                    key: key.as_str(),
                    association: &association,
                    record: None,
                    path: "detail-boundary",
                },
            )?;
            candidate.model.features.push(feature);
            Ok::<(), CandidateError>(())
        });
        match result {
            Ok(()) => {
                self.append_links(source_order, &[curve_id, feature_id.to_string()])?;
                self.geometry_transferred = true;
                self.mark_native_retained(source_order, RhinoLossCode::DetailViewNotTransferred);
            }
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("detail candidate rejected: {error}"),
                );
                self.mark_failed(source_order);
            }
        }
        Ok(())
    }

    fn decode_cage(
        &mut self,
        source_order: usize,
        object: &ObjectDescriptor,
    ) -> Result<(), cadmpeg_core::CodecError> {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

        let Some(scale) = self.neutral_scale() else {
            self.scan_unbound_unit_warning(source_order, "NURBS cage");
            return Ok(());
        };
        let identity = &object.identity;
        let cage = match crate::cage::decode(
            self.expand,
            object.class_data_range.clone(),
            scale,
            self.archive(),
        ) {
            Ok(cage) => cage,
            Err(crate::curves::GeometryError::Codec(error)) => return Err(error),
            Err(error) => {
                let future = matches!(
                    error,
                    crate::curves::GeometryError::UnsupportedVersion { .. }
                );
                self.scan_warning(
                    source_order,
                    format_args!(
                        "NURBS cage {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
                return Ok(());
            }
        };
        let Some(key) = self.checked_object_key(identity, source_order) else {
            return Ok(());
        };
        let feature_id = {
            Ok::<_, cadmpeg_core::CodecError>(FeatureId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "cage", "feature"),
                key.clone(),
            ))
        }?;
        let mut properties = BTreeMap::new();
        for (axis, knots) in ["u", "v", "w"].into_iter().zip(&cage.knots) {
            insert_feature_property(
                &mut properties,
                format_args!("{axis}_knots"),
                format_args!("{}", CageFiniteList(knots)),
            )?;
        }
        insert_feature_property(
            &mut properties,
            format_args!("control_points"),
            format_args!("{}", CagePointList(&cage.control_points)),
        )?;
        if let Some(weights) = &cage.weights {
            insert_feature_property(
                &mut properties,
                format_args!("weights"),
                format_args!("{}", CageWeightList(weights)),
            )?;
        }
        let mut parameters = BTreeMap::new();
        insert_feature_property(
            &mut parameters,
            format_args!("dimension"),
            format_args!("{}", cage.dimension),
        )?;
        insert_feature_property(
            &mut parameters,
            format_args!("rational"),
            format_args!("{}", cage.rational()),
        )?;
        insert_feature_property(
            &mut parameters,
            format_args!("orders"),
            format_args!("{},{},{}", cage.orders[0], cage.orders[1], cage.orders[2]),
        )?;
        insert_feature_property(
            &mut parameters,
            format_args!("counts"),
            format_args!("{},{},{}", cage.counts[0], cage.counts[1], cage.counts[2]),
        )?;
        let feature = Feature {
            id: feature_id.clone(),
            ordinal: cadmpeg_core::decode::u64_from_index(cage.source_range.start),
            name: (!identity.name.is_empty()).then(|| identity.name.clone()),
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: properties,
            source_tag: Some("RhinoNurbsCage".to_string()),
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "nurbs_cage".into(),
                    parameters,
                }),
            ),
            native_ref: Some(
                self.session.unknowns()[source_order]
                    .id()
                    .as_str()
                    .to_owned(),
            ),
        };
        match self
            .validate_candidate(|candidate, _annotations| candidate.model.features.push(feature))
        {
            Ok(()) => {
                self.append_link(source_order, feature_id.as_str())?;
                self.geometry_transferred = true;
                self.mark_native_retained(source_order, RhinoLossCode::CageLatticeNotTransferred);
            }
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("NURBS cage candidate rejected: {error}"),
                );
                self.mark_failed(source_order);
            }
        }
        Ok(())
    }

    fn decode_morph(
        &mut self,
        source_order: usize,
        object: &ObjectDescriptor,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let Some(scale) = self.neutral_scale() else {
            self.scan_unbound_unit_warning(source_order, "morph control");
            return Ok(());
        };
        let identity = &object.identity;
        let morph = match crate::morph::decode(
            self.expand,
            object.class_data_range.clone(),
            scale,
            self.archive(),
        ) {
            Ok(morph) => morph,
            Err(crate::curves::GeometryError::Codec(error)) => return Err(error),
            Err(error) => {
                let future = matches!(
                    error,
                    crate::curves::GeometryError::UnsupportedVersion { .. }
                );
                self.scan_warning(
                    source_order,
                    format_args!(
                        "morph control {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
                return Ok(());
            }
        };
        let Some(key) = self.checked_object_key(identity, source_order) else {
            return Ok(());
        };
        let feature = match crate::morph::project(
            &morph,
            key.as_str(),
            (!identity.name.is_empty()).then(|| identity.name.clone()),
            self.session.unknowns()[source_order]
                .id()
                .as_str()
                .to_owned(),
            |id| self.resolve_object_record(source_order, "morph captive", id),
        ) {
            Ok(feature) => feature,
            Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                self.scan_warning(source_order, format_args!("morph control failed: {error}"));
                self.mark_failed(source_order);
                return Ok(());
            }
        };
        let feature_id = feature.id.to_string();
        match self
            .validate_candidate(|candidate, _annotations| candidate.model.features.push(feature))
        {
            Ok(()) => {
                self.append_link(source_order, &feature_id)?;
                self.geometry_transferred = true;
                self.mark_native_retained(source_order, RhinoLossCode::MorphDeformationNotApplied);
            }
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("morph candidate rejected: {error}"),
                );
                self.mark_failed(source_order);
            }
        }
        Ok(())
    }

    fn decode_curve_on_surface(
        &mut self,
        source_order: usize,
        object: &ObjectDescriptor,
    ) -> Result<(), cadmpeg_core::CodecError> {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

        let Some(scale) = self.neutral_scale() else {
            self.scan_unbound_unit_warning(source_order, "curve-on-surface");
            return Ok(());
        };
        let identity = &object.identity;
        let construction = match crate::curve_on_surface::decode(
            self.expand.ctx(),
            self.scan.data,
            object.class_data_range.clone(),
            scale,
            self.archive(),
            0,
        ) {
            Ok(value) => value,
            Err(crate::curves::GeometryError::Codec(error)) => return Err(error),
            Err(error) => {
                let future = matches!(
                    error,
                    crate::curves::GeometryError::UnsupportedVersion { .. }
                );
                self.scan_warning(
                    source_order,
                    format_args!(
                        "curve-on-surface {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
                return Ok(());
            }
        };
        let Some(key) = self.checked_object_key(identity, source_order) else {
            return Ok(());
        };
        let association = self.source_association(identity)?;
        let parameter_id = format!("rhino:object:curve#{key}.curve-on-surface-c2");
        let model_id = construction
            .model_curve
            .as_ref()
            .map(|_| format!("rhino:object:curve#{key}.curve-on-surface-c3"));
        let surface_id = {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::SurfaceId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
                key.clone()
                    .then(cadmpeg_ir::identity_key!(".curve-on-surface-support")),
            ))
        }?;
        let feature_id = {
            Ok::<_, cadmpeg_core::CodecError>(FeatureId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "curve-on-surface", "feature"),
                key.clone(),
            ))
        }?;
        let mut source_properties = BTreeMap::new();
        if let Some(id) = model_id.as_ref() {
            insert_feature_property(
                &mut source_properties,
                format_args!("model_curve"),
                format_args!("{id}"),
            )?;
        }
        let mut parameters = BTreeMap::new();
        insert_feature_property(
            &mut parameters,
            format_args!("parameter_curve"),
            format_args!("{parameter_id}"),
        )?;
        insert_feature_property(
            &mut parameters,
            format_args!("support_surface"),
            format_args!("{surface_id}"),
        )?;
        let feature = Feature {
            id: feature_id.clone(),
            ordinal: cadmpeg_core::decode::u64_from_index(construction.source_range.start),
            name: (!identity.name.is_empty()).then(|| identity.name.clone()),
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties,
            source_tag: Some("RhinoCurveOnSurface".to_string()),
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "curve_on_surface".into(),
                    parameters,
                }),
            ),
            native_ref: Some(
                self.session.unknowns()[source_order]
                    .id()
                    .as_str()
                    .to_owned(),
            ),
        };
        let parameter_curve = construction.parameter_curve;
        let model_curve = construction.model_curve;
        let (surface_geometry, surface_derived) = match construction.surface {
            crate::surfaces::DecodedSurface::Typed {
                geometry, derived, ..
            } => (geometry.into_geometry(), derived),
            crate::surfaces::DecodedSurface::Procedural { geometry, .. } => (
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(geometry)),
                true,
            ),
        };
        let session = self.expand.ctx();
        let result = self.validate_candidate_fallible(|candidate, candidate_annotations| {
            commit_curve_tree(
                session,
                candidate,
                candidate_annotations,
                parameter_curve,
                CurveCommitSource {
                    key: key.as_str(),
                    association: &association,
                    record: None,
                    path: "curve-on-surface-c2",
                },
            )?;
            if let Some(model_curve) = model_curve {
                commit_curve_tree(
                    session,
                    candidate,
                    candidate_annotations,
                    model_curve,
                    CurveCommitSource {
                        key: key.as_str(),
                        association: &association,
                        record: None,
                        path: "curve-on-surface-c3",
                    },
                )?;
            }
            candidate.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: surface_geometry,
                source_object: Some(association),
            });
            set_exactness(
                candidate_annotations,
                &surface_id,
                if surface_derived {
                    Exactness::Derived
                } else {
                    Exactness::ByteExact
                },
            );
            candidate.model.features.push(feature);
            Ok::<(), CandidateError>(())
        });
        match result {
            Ok(()) => {
                for warning in construction.warnings {
                    self.scan_diagnostic(source_order, &warning);
                }
                let mut links = vec![parameter_id, surface_id.to_string(), feature_id.to_string()];
                if let Some(model_id) = model_id {
                    links.push(model_id);
                }
                self.append_links(source_order, &links)?;
                self.geometry_transferred = true;
                self.mark_native_retained(
                    source_order,
                    RhinoLossCode::CurveOnSurfaceBindingNotTransferred,
                );
            }
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("curve-on-surface candidate rejected: {error}"),
                );
                self.mark_failed(source_order);
            }
        }
        Ok(())
    }

    fn is_definition_member(&self, object: &ObjectDescriptor) -> bool {
        let identity = &object.identity;
        self.scan.definitions.contains_member(identity.object_id)
    }

    /// Admit the source-derived object key before composing any typed identity.
    ///
    /// Object and instance keys are source data. A malformed key rejects the
    /// owning object through the normal decode outcome instead of aborting the
    /// whole document.
    fn checked_object_key(
        &mut self,
        identity: &crate::objects::SourceIdentity,
        source_order: usize,
    ) -> std::option::Option<cadmpeg_ir::ids::IdentityKey> {
        let value = if let Some(selected) = &self.instance_selection {
            selected.key.as_str().to_owned()
        } else if let Some((_, key)) = identity.source_id.rsplit_once('#') {
            key.to_owned()
        } else {
            format!("{source_order}")
        };
        match IdentityKey::try_new(value) {
            Ok(key) => Some(key),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("object identity key is invalid: {error}"),
                );
                self.mark_failed(source_order);
                None
            }
        }
    }

    fn reference_segment(
        &self,
        source_order: usize,
        identity: &crate::objects::SourceIdentity,
    ) -> Result<String, cadmpeg_core::CodecError> {
        if !identity.object_id.is_nil()
            && self.resolve_object(identity.object_id) == ObjectReference::Resolved(source_order)
        {
            Ok(format!("{}", identity.object_id))
        } else {
            let object = self.scan.objects.get(source_order).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino reference object is missing")
            })?;
            Ok(format!(
                "record-{source_order:06}-offset-{}",
                object.range().start
            ))
        }
    }

    fn source_association(
        &self,
        identity: &crate::objects::SourceIdentity,
    ) -> Result<SourceObjectAssociation, cadmpeg_core::CodecError> {
        source_association(
            identity,
            self.instance_selection
                .as_ref()
                .map_or(&[], |selected| selected.path.as_slice()),
            self.instance_display.and_then(|display| display.color),
            self.instance_display.map(|display| display.visible),
        )
    }

    fn expand_reference(&mut self, source_order: usize) -> Result<bool, cadmpeg_core::CodecError> {
        let annotations = std::mem::take(&mut self.annotations);
        let outcome = self.expand_reference_staged(source_order);
        match outcome {
            Ok(true) => {
                let changes = std::mem::take(&mut self.annotations);
                let prepared = annotations.prepare_overlay(changes);
                self.annotations = annotations;
                prepared.apply(&mut self.annotations);
                Ok(true)
            }
            other => {
                self.annotations = annotations;
                other
            }
        }
    }

    fn expand_reference_staged(
        &mut self,
        source_order: usize,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let original_model = ModelCheckpoint::capture(&self.session.document().model);
        let outer_undo = self.expansion_undo.replace(ExpansionUndo::default());
        let original_geometry_transferred = self.geometry_transferred;
        let report_checkpoint = self.report.checkpoint();
        let original_selection = self.instance_selection.take();
        let original_display = self.instance_display;
        let original_expansion_budget = self.expansion_budget;
        let initial_path = original_selection
            .as_ref()
            .map_or(&[][..], |selected| selected.path.as_slice());

        let mut traversal = initial_path.to_vec();
        let mut stack = Vec::new();
        let parent = Transform::identity();
        let outcome = self.expand_reference_inner(source_order, parent, &mut traversal, &mut stack);
        self.instance_selection = original_selection;
        // Mesh buffers stay charged in the session arena even on rollback.
        let rejection_warning = match outcome {
            Ok(links) => {
                let validation = self
                    .session
                    .admit_appended(&self.annotations, cadmpeg_ir::RHINO_INSTANCE_CHECKS)?;
                if validation
                    .as_ref()
                    .is_ok_and(cadmpeg_ir::report::check::ValidationReport::is_ok)
                {
                    self.append_links(source_order, &links)?;
                    self.mark_decoded(source_order);
                    self.geometry_transferred = true;
                    let undo = std::mem::replace(&mut self.expansion_undo, outer_undo);
                    if let (Some(outer), Some(undo)) = (&mut self.expansion_undo, undo) {
                        outer.absorb(undo);
                    }
                    return Ok(true);
                }
                let findings = match validation {
                    Ok(report) => validation_findings(&report),
                    Err(error) => format!("{error}"),
                };
                format!("instance expansion rejected atomically by IR admission: {findings}")
            }
            Err(ReferenceFailure::Codec(error)) => return Err(error),
            Err(ReferenceFailure::Semantic(message)) => format!("instance retained: {message}"),
        };

        original_model.discard_appended(self.session.edit_appended_model(&original_model)?)?;
        let undo = std::mem::replace(&mut self.expansion_undo, outer_undo);
        self.undo_expansion(undo.unwrap_or_default())?;
        self.geometry_transferred = original_geometry_transferred;
        self.report.rollback(report_checkpoint);
        self.instance_display = original_display;
        self.expansion_budget = ExpansionBudget {
            copied_elements: self.expansion_budget.copied_elements,
            ..original_expansion_budget
        };
        self.scan_warning(source_order, format_args!("{rejection_warning}"));
        Ok(false)
    }

    /// Removes the links and outcomes a rejected expansion added. Links were
    /// only inserted, each once, into sorted lists, so removing them restores
    /// every touched record exactly.
    fn undo_expansion(&mut self, undo: ExpansionUndo) -> Result<(), cadmpeg_core::CodecError> {
        let mut links = undo.links;
        links.sort_unstable();
        for group in links.chunk_by(|left, right| left.0 == right.0) {
            let Some((record, _)) = group.first() else {
                continue;
            };
            let (_, target) = self.session.unknown_links_mut(*record)?.ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(
                    "Rhino source record disappeared during instance expansion",
                )
            })?;
            target.retain(|link| {
                group
                    .binary_search_by(|(_, added)| added.as_str().cmp(link))
                    .is_err()
            });
        }
        for source_order in undo.outcomes {
            if let Some(status) = self.statuses.get_mut(source_order) {
                *status = None;
            }
        }
        Ok(())
    }

    fn expand_reference_inner(
        &mut self,
        source_order: usize,
        parent: Transform,
        path: &mut Vec<String>,
        stack: &mut Vec<crate::wire::Uuid>,
    ) -> Result<Vec<String>, ReferenceFailure> {
        const MAX_INSTANCE_DEPTH: usize = 64;
        let _nested = self.expand.ctx().enter_nested("rhino_instance_nesting")?;
        self.expansion_budget.reference()?;

        // Session recursion depth refuses through `enter_nested`; the local
        // nesting cap retains this one reference.
        if stack.len() >= MAX_INSTANCE_DEPTH {
            return Err(ReferenceFailure::Semantic(
                "instance nesting exceeds 64 levels".to_owned(),
            ));
        }
        let object = self
            .scan
            .objects
            .get(source_order)
            .ok_or_else(|| "reference object is missing".to_string())?;
        let object = object
            .framed()
            .ok_or_else(|| "reference identity is unavailable".to_string())?;
        let identity = &object.identity;
        let reference =
            crate::instances::parse_reference(self.scan.data, object.class_data_range.clone())
                .map_err(|error| error.to_string())?;
        if self
            .scan
            .definitions
            .is_ambiguous(reference.definition_id())
        {
            return Err(format!("definition {} is duplicated", reference.definition_id()).into());
        }
        let definitions = self.scan.definitions.definitions();
        let definition = self
            .definition_candidates
            .get(&reference.definition_id())
            .and_then(|index| definitions.get(*index))
            .ok_or_else(|| format!("definition {} is missing", reference.definition_id()))?;
        if matches!(definition.kind, crate::instances::DefinitionKind::Linked)
            && definition.members.is_empty()
        {
            return Err(format!(
                "linked external definition {} has no local members",
                definition.id()
            )
            .into());
        }
        if matches!(definition.kind, crate::instances::DefinitionKind::Unset) {
            return Err(format!("definition {} has unset type", definition.id()).into());
        }
        if !instance_members_are_unique(&definition.members) {
            return Err(format!(
                "definition {} contains duplicate member UUIDs",
                definition.id()
            )
            .into());
        }
        if stack.contains(&definition.id()) {
            return Err(format!("definition cycle reaches {}", definition.id()).into());
        }
        let binding = self.unit_binding();
        let crate::settings::UnitBinding::Millimeters(scale) = binding else {
            return Err(format!(
                "document has no physical millimetre binding ({})",
                binding.label()
            )
            .into());
        };
        let local = crate::instances::scale_translation(reference.transform(), scale)
            .ok_or_else(|| "scaled instance transform is invalid".to_string())?;
        let transform = parent.compose(local).map_err(|error| error.to_string())?;
        let definition_id = definition.id();
        let definition_members = &definition.members;

        stack.push(definition_id);

        path.push(self.reference_segment(source_order, identity)?);
        let previous_display = self.instance_display;
        self.instance_display = Some(InstanceDisplay {
            color: identity
                .effective_color
                .map(color)
                .or(previous_display.and_then(|display| display.color)),
            visible: previous_display.is_none_or(|display| display.visible)
                && identity.effective_visible,
        });
        let mut links = Vec::new();
        for &member_id in definition_members {
            self.expansion_budget.member()?;
            self.expansion_budget.copy_elements(1)?;

            let member_order = match self.resolve_object(member_id) {
                ObjectReference::Resolved(order) => order,
                ObjectReference::Missing => {
                    return Err(format!("definition member {member_id} is missing").into());
                }
                ObjectReference::Ambiguous => {
                    return Err(format!("definition member {member_id} is ambiguous").into());
                }
            };
            let member = &self.scan.objects[member_order];
            if member
                .class_uuid()
                .is_some_and(crate::instances::is_reference_class)
            {
                let nested = self.expand_reference_inner(member_order, transform, path, stack)?;
                self.append_links(member_order, &nested)?;
                self.mark_decoded(member_order);

                links.extend(nested);
                continue;
            }
            let before = ModelCheckpoint::capture(&self.session.document().model);
            let selection = InstanceSelection::new(member_order, path, member_id)?;
            let previous_selection = self.instance_selection.replace(selection);
            let decoded = self.decode_geometry();
            self.instance_selection = previous_selection;
            decoded?;
            let after = ModelCheckpoint::capture(&self.session.document().model);
            if before.same_state(&after) {
                return Err(format!("definition member {member_id} did not decode").into());
            }
            let transformed = self.transform_new_entities(&before, transform)?;

            links.extend(transformed);
        }
        self.instance_display = previous_display;
        path.pop();
        stack.pop();
        Ok(links)
    }

    fn transform_new_entities(
        &mut self,
        before: &ModelCheckpoint,
        transform: Transform,
    ) -> Result<Vec<String>, ReferenceFailure> {
        let model = self.session.edit_appended_model(before)?;
        let budget = &mut self.expansion_budget;
        let mut links = Vec::new();
        let mut derived_ids = Vec::new();
        let bodies = before
            .added_mut::<Body>(model)
            .ok_or_else(|| "instance decode removed existing bodies".to_string())?;
        budget.copy_elements(bodies.len())?;
        for body in bodies {
            let id = body.id.as_str().to_string();
            links.push(id);

            let id = body.id.as_str().to_string();
            derived_ids.push(id);
        }
        for pcurve in before
            .added_mut::<Pcurve>(model)
            .ok_or_else(|| "instance decode removed existing pcurves".to_string())?
        {
            budget.copy_entity(match &pcurve.geometry {
                PcurveGeometry::Nurbs { nurbs } => nurbs.pole_rows().count(),
                _ => 0,
            })?;
        }
        let points = before
            .added_mut::<Point>(model)
            .ok_or_else(|| "instance decode removed existing points".to_string())?;
        budget.copy_elements(points.len())?;
        for point in points {
            let placed = placed_finite_point(transform, point.position())?;
            point.set_position(placed);

            let id = point.id.as_str().to_string();
            derived_ids.push(id);
        }
        for curve in before
            .added_mut::<Curve>(model)
            .ok_or_else(|| "instance decode removed existing curves".to_string())?
        {
            if let Some(cache) = curve.geometry.solved_cache() {
                curve.geometry = CurveGeometry::Solved(cache.clone());
            }
            budget.copy_entity(match &curve.geometry {
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => nurbs.pole_count(),
                _ => 0,
            })?;
            transform_curve(self.expand.ctx(), curve, transform)?;

            let id = curve.id.as_str().to_string();
            links.push(id);

            let id = curve.id.as_str().to_string();
            derived_ids.push(id);
        }
        for surface in before
            .added_mut::<Surface>(model)
            .ok_or_else(|| "instance decode removed existing surfaces".to_string())?
        {
            if let Some(cache) = surface.geometry.solved_cache() {
                surface.geometry = SurfaceGeometry::Solved(cache.clone());
            }
            budget.copy_entity(match &surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => nurbs
                    .u_count()
                    .checked_mul(nurbs.v_count())
                    .ok_or_else(|| COPY_ELEMENT_BUDGET_EXCEEDED.to_owned())?,
                _ => 0,
            })?;
            transform_surface(surface, transform)?;

            let id = surface.id.as_str().to_string();
            links.push(id);

            let id = surface.id.as_str().to_string();
            derived_ids.push(id);
        }
        for mesh in before
            .added_mut::<Tessellation>(model)
            .ok_or_else(|| "instance decode removed existing tessellations".to_string())?
        {
            budget.copy_entity(
                mesh.vertex_count()
                    .checked_add(mesh.triangle_count())
                    .ok_or_else(|| COPY_ELEMENT_BUDGET_EXCEEDED.to_owned())?,
            )?;
            mesh.edit_vertices(|vertex| {
                *vertex = transform
                    .apply_point(*vertex)
                    .ok_or_else(|| {
                        cadmpeg_ir::tessellation::TessellationError::EditRefused(
                            "instance mesh vertex transform produced a non-finite coordinate"
                                .to_string(),
                        )
                    })?
                    .get();
                Ok(())
            })
            .map_err(|error| error.to_string())?;
            if !mesh.vertex_normals().is_empty() {
                mesh.edit_normals(|value| {
                    *value = transform
                        .apply_normal(*value)
                        .map(cadmpeg_ir::math::Vector3::from)
                        .ok_or_else(|| {
                            cadmpeg_ir::tessellation::TessellationError::EditRefused(
                                "mesh normal transform could not produce a finite unit normal"
                                    .to_string(),
                            )
                        })?;
                    Ok(())
                })
                .map_err(|error| error.to_string())?;
            }

            let id = mesh.id.as_str().to_string();
            links.push(id);

            let id = mesh.id.as_str().to_string();
            derived_ids.push(id);
        }
        for subd in before
            .added_mut::<cadmpeg_ir::SubdSurface>(model)
            .ok_or_else(|| "instance decode removed existing subdivision surfaces".to_string())?
        {
            subd.cage
                .edit_vertices(|vertices| {
                    budget.copy_entity(vertices.len()).map_err(|_| {
                        cadmpeg_ir::subd::SubdError::EditRefused(
                            COPY_ELEMENT_BUDGET_EXCEEDED.to_owned(),
                        )
                    })?;
                    for vertex in vertices {
                        let moved =
                            transform.apply_point(vertex.point().get()).ok_or_else(|| {
                                cadmpeg_ir::subd::SubdError::EditRefused(
                                "instance cage vertex transform produced a non-finite coordinate"
                                    .to_string(),
                            )
                            })?;
                        vertex.set_point(moved);
                    }
                    Ok(())
                })
                .map_err(|error| error.to_string())?;

            let id = subd.id.as_str().to_string();
            links.push(id);

            let id = subd.id.as_str().to_string();
            derived_ids.push(id);
        }
        let procedural_curve_start = before.arena_len::<ProceduralCurve>();
        let procedural_surface_start = before.arena_len::<ProceduralSurface>();
        if model.procedural_curves.len() > procedural_curve_start
            || model.procedural_surfaces.len() > procedural_surface_start
        {
            let mut annotations = AnnotationBuilder::resume(std::mem::take(&mut self.annotations));
            for procedure in &model.procedural_curves[procedural_curve_start..] {
                annotations.remove_entity(procedure.id.as_str());
            }
            for procedure in &model.procedural_surfaces[procedural_surface_start..] {
                annotations.remove_entity(procedure.id.as_str());
            }
            self.annotations = annotations.build();
            model.procedural_curves.truncate(procedural_curve_start);
            model.procedural_surfaces.truncate(procedural_surface_start);
            self.report.phase_warnings.push(format_args!("instance: transformed procedural definition omitted; exact solved carrier retained"));
        }
        for id in derived_ids {
            annotate_derived(&mut self.annotations, &id);
        }
        Ok(links)
    }

    fn decode_subd(
        &mut self,
        source_order: usize,
        object: &ObjectDescriptor,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let Some(scale) = self.neutral_scale() else {
            self.scan_unbound_unit_warning(source_order, "SubD");
            return Ok(());
        };
        let identity = &object.identity;
        let Some(key) = self.checked_object_key(identity, source_order) else {
            return Ok(());
        };
        let id = cadmpeg_ir::ids::SubdId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "subd"),
            key,
        );
        match crate::subd::decode(
            self.scan.data,
            object.class_data_range.clone(),
            self.archive(),
            scale,
            id,
        ) {
            Ok(None) => {
                self.mark_decoded(source_order);
            }
            Ok(Some(decoded)) => {
                if self.commit_subd_surface(
                    source_order,
                    decoded,
                    scale != MillimeterScale::IDENTITY,
                )? {
                    self.mark_decoded(source_order);
                } else {
                    self.scan_warning(
                        source_order,
                        format_args!("SubD candidate rejected atomically by IR validation"),
                    );
                    self.mark_failed(source_order);
                }
            }
            Err(crate::subd::SubdError::Resource(limit)) => {
                return Err(cadmpeg_core::CodecError::ResourceLimit(limit));
            }
            Err(error) => {
                let future = matches!(error, crate::subd::SubdError::UnsupportedVersion { .. });
                self.scan_warning(
                    source_order,
                    format_args!(
                        "SubD {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
            }
        }
        Ok(())
    }

    fn commit_subd_surface(
        &mut self,
        source_order: usize,
        decoded: crate::subd::DecodedSubd,
        scaled: bool,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let crate::subd::DecodedSubd {
            mut surface,
            neutral_metadata,
            enum_diagnostics,
            warnings,
        } = decoded;
        for warning in warnings {
            self.scan_diagnostic(source_order, &warning);
        }
        for diagnostic in enum_diagnostics {
            self.report
                .typed_losses
                .push(RhinoLossCode::EnumerationValueDegraded.note(format_args!("{diagnostic}")));
        }
        if neutral_metadata {
            self.scan_warning(source_order, format_args!("SubD cache, texture, symmetry, or packing metadata is retained without a neutral-IR mapping"));
        }
        let Some(object) = self.scan.objects.get(source_order) else {
            return Ok(false);
        };
        let Some(identity) = object.identity() else {
            return Ok(false);
        };
        surface.source_object = Some(self.source_association(identity)?);
        let id = surface.id.to_string();
        let result = self.validate_candidate_fallible(|candidate, candidate_annotations| {
            candidate.model.subds.push(surface);
            set_exactness(
                candidate_annotations,
                &id,
                if scaled {
                    Exactness::Derived
                } else {
                    Exactness::ByteExact
                },
            );
            Ok::<_, cadmpeg_core::CodecError>(id.clone())
        });
        let link = match result {
            Ok(link) => link,
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(findings) => {
                self.scan_warning(
                    source_order,
                    format_args!("SubD validation rejected candidate: {findings}"),
                );
                return Ok(false);
            }
        };
        self.append_link(source_order, &link)?;
        self.geometry_transferred = true;
        Ok(true)
    }

    fn decode_extrusion(
        &mut self,
        source_order: usize,
        object: &ObjectDescriptor,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let Some(scale) = self.neutral_scale() else {
            self.scan_unbound_unit_warning(source_order, "extrusion");
            self.commit_unknown_surface(source_order)?;
            return Ok(());
        };
        let decoded = crate::extrusion::decode(
            self.expand,
            self.scan.data,
            object.class_data_range.clone(),
            crate::extrusion::ExtrusionFormat {
                archive: self.archive(),
                writer_version: self.scan.metadata.properties.writer_version,
                scale,
            },
            &object.userdata,
            &mut self.mesh_budget,
        );
        match decoded {
            Ok(extrusion) => {
                for warning in &extrusion.warnings {
                    self.scan_diagnostic(source_order, warning);
                }
                if self.commit_extrusion(source_order, extrusion)? {
                    self.mark_decoded(source_order);
                } else {
                    self.scan_warning(
                        source_order,
                        format_args!("extrusion candidate rejected atomically"),
                    );
                    self.commit_unknown_surface(source_order)?;
                }
            }
            Err(crate::curves::GeometryError::Codec(error)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("extrusion degraded and retained: {error}"),
                );
                self.commit_unknown_surface(source_order)?;
            }
        }
        Ok(())
    }

    /// Mints the stable unknown-record ID for source order.
    fn mint_unknown_id(source_order: usize) -> UnknownId {
        UnknownId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "record"),
            cadmpeg_ir::ids::IdentityKey::zero_padded(
                cadmpeg_core::decode::u64_from_index(source_order),
                6,
            ),
        )
    }

    /// Commits the transaction and produces canonical IR and report state.
    pub(crate) fn commit(mut self) -> Result<Decoded, cadmpeg_core::CodecError> {
        let ctx = self.expand.ctx();
        for loss in &self.scan.metadata.losses {
            self.report.phase_losses.push(loss.clone());
        }
        self.report.typed_losses.extend(crate::annotations::install(
            ctx,
            self.scan,
            self.session.document_mut()?,
        )?);
        let document_data =
            crate::document_data::install(ctx, self.scan, self.session.document_mut()?)?;
        self.report.typed_losses.extend(document_data.losses);
        for source in document_data.opaque_records {
            self.retain_opaque_record(&source)?;
        }
        let presentation =
            crate::presentation::install(ctx, self.scan, self.session.document_mut()?)?;
        self.report.typed_losses.extend(presentation.losses);
        for source in presentation.opaque_records {
            self.retain_opaque_record(&source)?;
        }
        self.report.typed_losses.extend(crate::product::install(
            ctx,
            self.scan,
            self.session.document_mut()?,
        )?);
        let views = crate::views::install(ctx, self.scan, self.session.document_mut()?)?;
        self.report.typed_losses.extend(views.losses);
        for source in views.opaque_records {
            self.retain_opaque_record(&source)?;
        }
        self.session.document_mut()?.finalize();
        let mut losses: Vec<LossNote> = Vec::new();
        let outcomes = self.class_outcomes()?;
        let decoded = outcomes
            .iter()
            .map(|(_, outcome)| outcome.decoded)
            .sum::<usize>();
        let total = self.scan.objects.len();

        losses.push(RhinoLossCode::ObjectRecordCensus.note(format_args!(
            "decoded {decoded}/{total} Rhino object records"
        )));
        let mut omissions: Vec<LossNote> = Vec::new();
        for (class, outcome) in &outcomes {
            if outcome.retained > 0 {
                omissions.push(
                    RhinoLossCode::ObjectFamilyNotTransferred.note(format_args!(
                            "retained {} object record(s) for class {class}; geometry is not decoded",
                            outcome.retained
                        ))
                        .with_provenance(loss_provenance( class, outcome)),
                );
            }
            if let Some((code, count)) = outcome.native {
                omissions.push(
                    code.note(format_args!(
                        "framed and read {} object record(s) for class {class}; construction \
                     state is retained as native passthrough",
                        count.get()
                    ))
                    .with_provenance(loss_provenance(class, outcome)),
                );
            }
            if outcome.attribute_degraded > 0 {
                losses.push(
                    RhinoLossCode::ObjectAttributesDegraded
                        .note(format_args!(
                            "{} object record(s) for class {class} have degraded attributes",
                            outcome.attribute_degraded
                        ))
                        .with_provenance(loss_provenance(class, outcome)),
                );
            }
            if outcome.failed_framed > 0 {
                losses.push(
                    RhinoLossCode::ObjectFramingUndecodable
                        .note(format_args!(
                            "{} framed object record(s) for class {class} could not be decoded",
                            outcome.failed_framed
                        ))
                        .with_provenance(loss_provenance(class, outcome)),
                );
            }
        }

        self.report.typed_losses.extend(omissions);
        for diagnostic in self.scan.definitions.diagnostics() {
            losses.push(diagnostic.to_loss());
        }

        losses.append(&mut self.report.typed_losses);
        for diagnostic in &self.scan.warnings {
            losses.push(
                diagnostic
                    .code
                    .unwrap_or(RhinoLossCode::ContainerScanDiagnostic)
                    .note(format_args!("{}", diagnostic.message)),
            );
        }

        losses.append(&mut self.report.phase_losses);
        let mut phase_families = BTreeMap::<String, (usize, String)>::new();
        for diagnostic in &self.report.phase_warnings {
            if let Some(code) = diagnostic.code {
                losses.push(code.note(format_args!("{}", diagnostic.message)));
                continue;
            }
            let warning = &diagnostic.message;
            let (family, detail) = warning
                .split_once(':')
                .map_or(("rhino", warning.as_str()), |(family, detail)| {
                    (family, detail.trim())
                });
            if !phase_families.contains_key(family) {
                let family_key = family.to_owned();
                let first_detail = detail.to_owned();
                phase_families.insert(family_key, (0, first_detail));
            }
            let entry = phase_families.get_mut(family).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino warning family group missing")
            })?;
            entry.0 += 1;
        }
        for (family, (count, first)) in phase_families {
            let loss = if count == 1 {
                RhinoLossCode::ObjectDecodeDiagnostic.note(format_args!("{family}: {first}"))
            } else {
                RhinoLossCode::ObjectDecodeDiagnostic.note(format_args!(
                    "{family}: {count} decode warnings; first: {first}"
                ))
            };
            losses.push(loss);
        }
        let byte_records = self
            .session
            .unknowns()
            .iter()
            .filter(|record| record.data().is_some())
            .count()
            + self
                .opaque_records
                .iter()
                .filter(|record| record.data().is_some())
                .count();
        let note = if self.opaque_records.is_empty() {
            format!("decoded {decoded}/{total} Rhino object records; retained metadata/digests for {} \
             records and complete bytes for {byte_records}; document cap {} bytes, per-record cap {} bytes",
            self.session.unknowns().len(),
            RETAINED_DOCUMENT_CAP,
            RETAINED_RECORD_CAP)
        } else {
            format!(
                "decoded {decoded}/{total} Rhino object records; retained metadata/digests for {} \
             object records and {} opaque records, with complete bytes for {byte_records}; \
             document cap {} bytes, per-record cap {} bytes",
                self.session.unknowns().len(),
                self.opaque_records.len(),
                RETAINED_DOCUMENT_CAP,
                RETAINED_RECORD_CAP
            )
        };
        let notes = vec![note];
        let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(self.annotations);
        let (mut ir, unknowns) = self.session.into_parts();
        source_fidelity.attach_native_unknown_records(&mut ir, "rhino", unknowns, ctx)?;
        source_fidelity.retain_unknown_records("rhino", self.opaque_records)?;
        let primary = crate::container::dialect_match(self.scan);
        // Charged from the admission the source records, so the document-level
        // residual admission and its loss cannot be reported apart.
        if let Some(loss) = crate::dialect::admission_loss(&primary) {
            losses.push(loss);
        }
        let attributes = full_source_attributes(self.scan)?;
        ir.source = Some(crate::container::source_meta(
            primary,
            crate::container::SourceMetaDetail::Full {
                scan: self.scan,
                attributes,
            },
        )?);
        Ok(Decoded {
            ir,
            body: DecodeBody {
                transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(
                    self.geometry_transferred,
                ),
                coverage: cadmpeg_ir::report::decode::Coverage::default(),
                losses,
                notes,
                transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
            },
            source_fidelity,
        })
    }

    fn retain_object_records(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let mut records = Vec::new();
        for source_order in 0..self.scan.objects.len() {
            let object = &self.scan.objects[source_order];
            let range = object.range();
            let degraded = object.is_degraded();
            let id = Self::mint_unknown_id(source_order);
            let record = self.source_record(id, range)?;

            records.push(record);
            self.statuses
                .push(degraded.then_some(GeometryOutcome::Failed));
        }
        self.session.replace_unknowns(records)?;
        Ok(())
    }

    fn retain_opaque_records(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        for index in 0..self.scan.opaque_records.len() {
            let source = &self.scan.opaque_records[index];
            self.retain_opaque_record(source)?;
        }
        Ok(())
    }

    /// Retains complete history records whose embedded geometry cannot enter
    /// canonical millimetre IR.  The feature projection still keeps the
    /// scalar history values and points to this source boundary by ID.
    fn retain_unbound_history_geometry(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        if self.neutral_scale().is_some() {
            return Ok(());
        }
        let binding = self.unit_binding();
        for index in 0..self.scan.history.len() {
            let record = &self.scan.history[index];
            if !record.values.iter().any(|value| {
                matches!(&value.value, crate::history::Value::Geometries(values) if !values.is_empty())
            }) {
                continue;
            }
            let range = record.source_range.clone();
            let id = UnknownId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "history", "source"),
                IdentityKey::zero_padded(cadmpeg_core::decode::u64_from_index(range.start), 12),
            );

            let retained = self.source_record(id, range.clone())?;
            self.opaque_records.push(retained);
            self.report.phase_warnings.push_coded(RhinoLossCode::HistoryGeometryNotTransferred, format_args!(
                "history record at source range {}..{} retained as complete source for {} unit binding",
                range.start,
                range.end,
                binding.label()
            ));
        }
        Ok(())
    }

    fn retain_opaque_record(
        &mut self,
        source: &OpaqueRecord,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let table_key = source.table_typecode.to_be_bytes();
        let record_key = source.record.typecode.to_be_bytes();
        let offset_key =
            (cadmpeg_core::decode::u64_from_index(source.record.range.start)).to_be_bytes();
        let key = IdentityKey::hex_byte(table_key[0])
            .with_hex_bytes(&table_key[1..])
            .dash(IdentityKey::hex_byte(record_key[0]).with_hex_bytes(&record_key[1..]))
            .dash(IdentityKey::hex_byte(offset_key[0]).with_hex_bytes(&offset_key[1..]));
        let id = UnknownId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "opaque", "record"),
            key,
        );

        let record = self.source_record(id, source.record.range.clone())?;
        self.opaque_records.push(record);
        Ok(())
    }

    fn source_record(
        &mut self,
        id: UnknownId,
        range: std::ops::Range<usize>,
    ) -> Result<UnknownRecord, cadmpeg_core::CodecError> {
        let bytes = &self.scan.data[range.clone()];
        let byte_len = cadmpeg_core::decode::u64_from_index(bytes.len());
        let retained_end = self.retained_bytes.checked_add(bytes.len()).filter(|end| {
            bytes.len() <= self.retention_limits[0] && *end <= self.retention_limits[1]
        });
        let offset = cadmpeg_core::decode::u64_from_index(range.start);
        match retained_end {
            Some(end) => {
                self.expand
                    .ctx()
                    .retain_bytes(byte_len, "Rhino source record bytes")?;
                let data = bytes.to_vec();
                self.retained_bytes = end;
                Ok(UnknownRecord::retained(id, offset, data, Vec::new()))
            }
            None => Ok(UnknownRecord::unavailable(
                id,
                offset,
                byte_len,
                sha256_hex(bytes),
                Vec::new(),
            )),
        }
    }

    fn scan_warning(&mut self, source_order: usize, message: std::fmt::Arguments<'_>) {
        let class = self.scan.objects[source_order]
            .class_uuid()
            .unwrap_or_else(crate::wire::Uuid::nil);
        self.report
            .phase_warnings
            .push(format_args!("{class}: {message}"));
    }

    fn scan_unbound_unit_warning(&mut self, source_order: usize, kind: &str) {
        let binding = self.unit_binding();
        self.scan_warning(
            source_order,
            format_args!(
                "{kind} retained because the document has no physical millimetre binding ({})",
                binding.label()
            ),
        );
    }

    fn scan_diagnostic(&mut self, source_order: usize, diagnostic: &crate::loss::RhinoDiagnostic) {
        let class = self.scan.objects[source_order]
            .class_uuid()
            .unwrap_or_else(crate::wire::Uuid::nil);
        self.report.phase_warnings.push_coded(
            diagnostic.code,
            format_args!("{class}: {}", diagnostic.message),
        );
    }

    fn scan_warnings_for_class(&mut self, class: &str, message: std::fmt::Arguments<'_>) {
        self.report
            .phase_warnings
            .push(format_args!("{class}: {message}"));
    }

    /// Charges entities for one object. Inside an instance expansion they also
    /// count toward `MAX_INSTANCE_ENTITIES`; `false` means that cap rejects the
    /// object, which is reported and skipped.
    fn charge_entities(
        &mut self,
        source_order: usize,
        amount: usize,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let mut budget = self.expansion_budget;
        if self.instance_selection.is_some() && !budget.entities(amount) {
            self.scan_warning(source_order, format_args!("{ENTITY_BUDGET_EXCEEDED}"));
            return Ok(false);
        }
        self.expand
            .ctx()
            .charge_entities(u64_from_index(amount), "rhino_instance_entities")?;
        self.expansion_budget = budget;
        Ok(true)
    }

    fn commit_geometry(
        &mut self,
        source_order: usize,
        decoded: crate::curves::DecodedGeometry,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let Some(object) = self.scan.objects.get(source_order) else {
            return Ok(false);
        };
        let Some(identity) = object.identity() else {
            return Ok(false);
        };
        let Some(key) = self.checked_object_key(identity, source_order) else {
            return Ok(false);
        };
        let association = self.source_association(identity)?;
        let Some(unknown) = self
            .session
            .unknowns()
            .get(source_order)
            .map(|record| record.id().clone())
        else {
            return Ok(false);
        };
        match decoded {
            crate::curves::DecodedGeometry::Point { position, scaled } => {
                if !self.charge_entities(source_order, 5)? {
                    return Ok(false);
                }
                let body_id = {
                    Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::BodyId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "body"),
                        key.clone(),
                    ))
                }?;
                let region_id = {
                    Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::RegionId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "region"),
                        key.clone(),
                    ))
                }?;
                let shell_id = {
                    Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::ShellId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "shell"),
                        key.clone(),
                    ))
                }?;
                let point_id = {
                    Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::PointId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "point"),
                        key.clone(),
                    ))
                }?;
                let vertex_id = {
                    Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::VertexId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "vertex"),
                        key.clone(),
                    ))
                }?;
                self.session.append_model_mut()?.points.push(Point::new(
                    point_id.clone(),
                    position,
                    Some(association.clone()),
                ));
                self.session.append_model_mut()?.vertices.push(Vertex {
                    id: vertex_id.clone(),
                    point: point_id.clone(),
                    tolerance: None,
                });
                self.session
                    .append_model_mut()?
                    .shells
                    .push(Shell::with_free_vertex(
                        shell_id.clone(),
                        region_id.clone(),
                        vertex_id.clone(),
                    ));
                self.session.append_model_mut()?.regions.push(Region {
                    id: region_id.clone(),
                    body: body_id.clone(),
                    shells: vec![shell_id.clone()],
                });
                self.session.append_model_mut()?.bodies.push(body(
                    identity,
                    body_id.clone(),
                    vec![region_id.clone()],
                    &association,
                ));
                self.annotate_point_topology(
                    &point_id, &vertex_id, &shell_id, &region_id, &body_id, scaled,
                );
                self.append_link(source_order, body_id.as_str())?;
            }
            crate::curves::DecodedGeometry::PointCloud(cloud) => {
                let crate::curves::PointCloud {
                    points,
                    scaled,
                    warnings,
                } = cloud;
                self.report
                    .phase_warnings
                    .append_prefixed(warnings, format_args!("{}", identity.source_id));
                let Some(entity_count) = points
                    .len()
                    .checked_mul(2)
                    .and_then(|count| count.checked_add(3))
                else {
                    self.scan_warning(
                        source_order,
                        format_args!("point-cloud entity count overflow"),
                    );
                    return Ok(false);
                };
                if !self.charge_entities(source_order, entity_count)? {
                    return Ok(false);
                }
                let body_id = {
                    Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::BodyId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "body"),
                        key.clone(),
                    ))
                }?;
                let region_id = {
                    Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::RegionId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "region"),
                        key.clone(),
                    ))
                }?;
                let shell_id = {
                    Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::ShellId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "shell"),
                        key.clone(),
                    ))
                }?;

                let mut vertices = Vec::new();

                for (index, position) in points.into_iter().enumerate() {
                    let point_key = key.clone().then(cadmpeg_ir::identity_key!(".")).then(index);
                    let point_id = cadmpeg_ir::ids::PointId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "point"),
                        point_key,
                    );

                    let vertex_key = key.clone().then(cadmpeg_ir::identity_key!(".")).then(index);
                    let vertex_id = cadmpeg_ir::ids::VertexId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "vertex"),
                        vertex_key,
                    );
                    self.session.append_model_mut()?.points.push(Point::new(
                        point_id.clone(),
                        position,
                        Some(association.clone()),
                    ));
                    self.session.append_model_mut()?.vertices.push(Vertex {
                        id: vertex_id.clone(),
                        point: point_id,
                        tolerance: None,
                    });
                    vertices.push(vertex_id);
                }
                let shell = match Shell::new(
                    shell_id.clone(),
                    region_id.clone(),
                    Vec::new(),
                    Vec::new(),
                    vertices,
                ) {
                    Ok(shell) => shell,
                    Err(error) => {
                        self.scan_warning(source_order, format_args!("{error}"));
                        return Ok(false);
                    }
                };
                self.session.append_model_mut()?.shells.push(shell);
                self.session.append_model_mut()?.regions.push(Region {
                    id: region_id.clone(),
                    body: body_id.clone(),
                    shells: vec![shell_id],
                });
                self.session.append_model_mut()?.bodies.push(body(
                    identity,
                    body_id.clone(),
                    vec![region_id],
                    &association,
                ));
                let point_prefix = format!("rhino:object:point#{key}.");
                for point in self
                    .session
                    .document()
                    .model
                    .points
                    .iter()
                    .filter(|point| point.id.as_str().starts_with(&point_prefix))
                {
                    set_exactness(
                        &mut self.annotations,
                        &point.id,
                        if scaled {
                            Exactness::Derived
                        } else {
                            Exactness::ByteExact
                        },
                    );
                }
                self.append_link(source_order, body_id.as_str())?;
            }
            crate::curves::DecodedGeometry::Curve { curve } => {
                append_curve_warnings(
                    self.expand.ctx(),
                    &mut self.report.phase_warnings,
                    &curve,
                    &identity.source_id,
                )?;
                let session = self.expand.ctx();
                let parent_id = match self.validate_candidate_fallible(|candidate, annotations| {
                    commit_curve_tree(
                        session,
                        candidate,
                        annotations,
                        curve,
                        CurveCommitSource {
                            key: key.as_str(),
                            association: &association,
                            record: Some(unknown),
                            path: "root",
                        },
                    )
                }) {
                    Ok(id) => id,
                    Err(CandidateError::Codec(error)) => return Err(error),
                    Err(error) => {
                        self.report
                            .phase_warnings
                            .push(format!("curve candidate rejected: {error}"));
                        return Ok(false);
                    }
                };
                self.append_link(source_order, parent_id.as_str())?;
            }
            crate::curves::DecodedGeometry::Surface { surface } => match surface {
                crate::surfaces::DecodedSurface::Typed {
                    geometry, derived, ..
                } => {
                    if !self.charge_entities(source_order, 1)? {
                        return Ok(false);
                    }
                    let surface_id = {
                        Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::SurfaceId::compose(
                            &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
                            key.clone(),
                        ))
                    }?;
                    self.session.append_model_mut()?.surfaces.push(Surface {
                        id: surface_id.clone(),
                        geometry: geometry.into_geometry(),
                        source_object: Some(association.clone()),
                    });
                    set_exactness(
                        &mut self.annotations,
                        &surface_id,
                        if derived {
                            Exactness::Derived
                        } else {
                            Exactness::ByteExact
                        },
                    );
                    self.append_link(source_order, surface_id.as_str())?;
                }
                crate::surfaces::DecodedSurface::Procedural {
                    geometry,
                    definition,
                } => {
                    return self.commit_procedural_surface(
                        source_order,
                        key.as_str(),
                        association,
                        geometry,
                        definition,
                    );
                }
            },
        }
        self.geometry_transferred = true;
        Ok(true)
    }

    fn commit_procedural_surface(
        &mut self,
        source_order: usize,
        key: &str,
        association: SourceObjectAssociation,
        geometry: cadmpeg_ir::geometry::nurbs::NurbsSurface,
        definition: crate::surfaces::DecodedProceduralSurface,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let Some(unknown) = self
            .session
            .unknowns()
            .get(source_order)
            .map(|record| record.id().clone())
        else {
            return Ok(false);
        };
        let session = self.expand.ctx();
        let result = self.validate_candidate_fallible(|candidate, candidate_annotations| {
            let ir_definition = definition.into_definition(
                |_, path, child| {
                    commit_curve_tree(
                        session,
                        candidate,
                        candidate_annotations,
                        child,
                        CurveCommitSource {
                            key,
                            association: &association,
                            record: Some(unknown.clone()),
                            path,
                        },
                    )
                },
                |error| CandidateError::Admission(error.to_string()),
            )?;
            let key = IdentityKey::try_new(key.to_owned()).map_err(|error| error.to_string())?;
            let surface_id = {
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::SurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
                    key.clone(),
                ))
            }?;
            candidate.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(geometry)),
                source_object: Some(association),
            });
            let procedural_id = {
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::ProceduralSurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "procedural-surface"),
                    key.clone(),
                ))
            }?;
            candidate
                .model
                .add_procedural_surface(
                    &surface_id,
                    ProceduralSurface::new(procedural_id.clone(), ir_definition, None),
                )
                .map_err(|error| error.to_string())?;
            for id in [surface_id.to_string(), procedural_id.to_string()] {
                set_exactness(candidate_annotations, id, Exactness::Derived);
            }
            Ok::<_, CandidateError>(vec![surface_id.to_string()])
        });
        let links = match result {
            Ok(links) => links,
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(findings) => {
                self.report.phase_warnings.push(format_args!(
                    "procedural-surface: candidate rejected by IR validation: {findings}"
                ));
                return Ok(false);
            }
        };
        self.append_links(source_order, &links)?;
        self.geometry_transferred = true;
        Ok(true)
    }

    fn commit_extrusion(
        &mut self,
        source_order: usize,
        mut extrusion: crate::extrusion::DecodedExtrusion,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let Some(object) = self.scan.objects.get(source_order) else {
            return Ok(false);
        };
        let Some(identity) = object.identity() else {
            return Ok(false);
        };
        let Some(unknown) = self
            .session
            .unknowns()
            .get(source_order)
            .map(|record| record.id().clone())
        else {
            return Ok(false);
        };
        let Some(key) = self.checked_object_key(identity, source_order) else {
            return Ok(false);
        };
        if extrusion.boundaries.is_empty() {
            return Ok(false);
        }
        let association = self.source_association(identity)?;
        let session = self.expand.ctx();
        let mut source_boundaries = std::mem::take(&mut extrusion.boundaries);
        let result = self.validate_candidate_fallible(|candidate, candidate_annotations| {
            let mut links = Vec::new();
            let mut boundaries = Vec::new();
            for (index, boundary) in source_boundaries.iter_mut().enumerate() {
                let id = commit_curve_tree(
                    session,
                    candidate,
                    candidate_annotations,
                    std::mem::replace(&mut boundary.start_curve, crate::curves::DecodedCurve::leaf(
                        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                        Diagnostics::new(),
                    )),
                    CurveCommitSource {
                        key: key.as_str(),
                        association: &association,
                        record: Some(unknown.clone()),
                        path: &format!("profile-{index}.start"),
                    },
                )?;
                boundaries.push(CommittedExtrusionBoundary {
                    boundary,
                    directrix: id,
                });
            }
            for (index, boundary) in boundaries.iter().enumerate() {
                let surface_id = {  Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::SurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".lateral-"))
                        .then(index),
                )) }?;
                let procedure_id = {  Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::ProceduralSurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "procedural-surface"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".lateral-"))
                        .then(index),
                )) }?;
                candidate.model.surfaces.push(Surface {
                    id: surface_id.clone(),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                        boundary.boundary.lateral.clone(),
                    )),
                    source_object: Some(association.clone()),
                });
                candidate
                    .model
                    .add_procedural_surface(&surface_id, cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                            boundary.directrix.clone(),
                            None,
                            extrusion.direction,
                            None,
                            cadmpeg_ir::geometry::CacheContract::from_form(None),
                        )
                        .map_err(|error| CandidateError::Admission(error.to_string())).map(|admitted_payload| ProceduralSurface::new(
                                procedure_id.clone(),
                                ProceduralSurfaceDefinition::Extrusion(admitted_payload),
                                None,
                            ))
                        ?)
                    .map_err(|error| error.to_string())?;
                annotate_derived(candidate_annotations, &surface_id.to_string());
                annotate_derived(candidate_annotations, &procedure_id.to_string());
                links.push(surface_id.to_string());
            }
            if extrusion.caps[0] || extrusion.caps[1] {
                links.push(stage_extrusion_caps(
                    candidate,
                    candidate_annotations,
                    key.as_str(),
                    &association,
                    &extrusion,
                    &boundaries,
                )?);
            }
            for (index, mut mesh) in extrusion.meshes.into_iter().enumerate() {
                mesh.tessellation.id = cadmpeg_ir::tessellation::TessellationId::mint(format!(
                    "rhino:object:tessellation#{key}.cache-{index}"
                ))
                .map_err(|error| error.to_string())?;
                mesh.tessellation.source_object = Some(association.clone());
                annotate_derived(candidate_annotations, mesh.tessellation.id.as_str());
                links.push(mesh.tessellation.id.to_string());
                candidate.model.tessellations.push(mesh.tessellation);
            }
            Ok::<_, CandidateError>(links)
        });
        let links = match result {
            Ok(links) => links,
            Err(CandidateError::Admission(error)) => {
                self.scan_warning(source_order, format_args!("{error}"));
                return Ok(false);
            }
            Err(CandidateError::Validation(findings)) => {
                self.scan_warning(
                    source_order,
                    format_args!("extrusion candidate rejected by IR validation: {findings}"),
                );
                return Ok(false);
            }
            Err(CandidateError::Codec(error)) => return Err(error),
        };
        self.append_links(source_order, &links)?;
        self.geometry_transferred = true;
        Ok(true)
    }

    fn commit_unknown_surface(
        &mut self,
        source_order: usize,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let Some(object) = self.scan.objects.get(source_order) else {
            return Ok(());
        };
        let Some(identity) = object.identity() else {
            return Ok(());
        };
        let Some(unknown) = self
            .session
            .unknowns()
            .get(source_order)
            .map(|record| record.id().clone())
        else {
            return Ok(());
        };
        let Some(key) = self.checked_object_key(identity, source_order) else {
            return Ok(());
        };
        let id = cadmpeg_ir::ids::SurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
            key,
        );
        let association = self.source_association(identity)?;
        let validation = self.validate_candidate_fallible(|candidate, candidate_annotations| {
            candidate.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                    record: Some(unknown.clone()),
                }),
                source_object: Some(association),
            });
            set_exactness(candidate_annotations, &id, Exactness::Unknown);
            Ok::<_, CandidateError>(id.to_string())
        });
        match validation {
            Ok(link) => {
                self.append_link(source_order, &link)?;
            }
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(findings) => self.scan_warning(
                source_order,
                format_args!("unknown surface validation rejected candidate: {findings}"),
            ),
        }
        Ok(())
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
        set_exactness(&mut self.annotations, point, point_exactness);
        for id in [
            vertex.to_string(),
            shell.to_string(),
            region.to_string(),
            body.to_string(),
        ] {
            set_exactness(&mut self.annotations, id, Exactness::Derived);
        }
    }

    fn commit_mesh(
        &mut self,
        source_order: usize,
        mesh: crate::mesh::DecodedMesh,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let Some(object) = self.scan.objects.get(source_order) else {
            return Ok(false);
        };
        let Some(identity) = object.identity() else {
            return Ok(false);
        };
        if !self.charge_entities(source_order, 1)? {
            return Ok(false);
        }
        for mut loss in mesh.losses {
            loss.message = format!("{}: {}", identity.source_id, loss.message);
            self.report.phase_losses.push(loss);
        }
        self.report
            .phase_warnings
            .append_prefixed(mesh.warnings, format_args!("{}", identity.source_id));
        let id = mesh.tessellation.id.to_string();
        let mut tessellation = mesh.tessellation;
        tessellation.source_object = Some(self.source_association(identity)?);
        self.session
            .append_model_mut()?
            .tessellations
            .push(tessellation);
        set_exactness(
            &mut self.annotations,
            &id,
            if mesh.scaled || mesh.quad_count != 0 {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        );
        if mesh.ngon_count != 0 {
            self.report
                .typed_losses
                .push(RhinoLossCode::MeshNgonGroupingDropped.note(format_args!(
                    "{} n-gon grouping record(s) were not transferred for mesh {id}",
                    mesh.ngon_count
                )));
        }
        if mesh.quad_count != 0 {
            self.report
                .typed_losses
                .push(
                    RhinoLossCode::MeshQuadTopologyTriangulated.note(format_args!(
                        "{} quadrilateral face(s) were triangulated for mesh {id}",
                        mesh.quad_count
                    )),
                );
        }
        self.append_link(source_order, &id)?;
        Ok(true)
    }

    fn decode_brep(
        &mut self,
        source_order: usize,
        object: &ObjectDescriptor,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let parsed = crate::brep::parse(
            self.expand.ctx(),
            self.scan.data,
            object.class_data_range.clone(),
            self.archive(),
            self.scan.metadata.properties.writer_version,
            &object.userdata,
        );
        let parsed = match parsed {
            Ok(value) => value,
            Err(crate::curves::GeometryError::Codec(error)) => return Err(error),
            Err(error) => {
                let future = matches!(
                    error,
                    crate::curves::GeometryError::UnsupportedVersion { .. }
                );
                self.scan_warning(
                    source_order,
                    format_args!(
                        "Brep {}: {error}",
                        if future { "retained" } else { "failed" }
                    ),
                );
                if !future {
                    self.mark_failed(source_order);
                }
                return Ok(());
            }
        };
        let raw = match &parsed {
            crate::brep::BrepParse::Valid(value) => value.raw(),
            crate::brep::BrepParse::SemanticInvalid { raw, .. } => raw,
        };
        let warnings = match &parsed {
            crate::brep::BrepParse::Valid(value) => value.warnings(),
            crate::brep::BrepParse::SemanticInvalid { warnings, .. } => warnings,
        };
        for warning in warnings {
            match warning.code {
                Some(code @ RhinoLossCode::EnumerationValueDegraded) => self
                    .report
                    .typed_losses
                    .push(code.note(format_args!("{}", warning.message))),
                _ => self.scan_diagnostic(source_order, warning),
            }
        }
        let identity = &object.identity;
        for loss in &raw.losses {
            let mut copied = loss.clone();
            copied.message = format!("{}: {}", object.class_uuid, loss.message);
            self.report.phase_losses.push(copied);
        }
        let Some(scale) = self.neutral_scale() else {
            self.scan_unbound_unit_warning(source_order, "Brep");
            return Ok(());
        };
        let association = self.source_association(identity)?;
        let Some(key) = self.checked_object_key(identity, source_order) else {
            return Ok(());
        };
        let unknown = self.session.unknowns()[source_order].id().clone();
        let staged = match &parsed {
            crate::brep::BrepParse::Valid(brep) => stage_brep(BrepTransferInput {
                expand: self.expand,
                data: self.scan.data,
                archive: self.archive(),
                writer_version: self.scan.metadata.properties.writer_version,
                brep,
                key: key.as_str(),
                association: &association,
                unknown: &unknown,
                scale,
                mesh_budget: &mut self.mesh_budget,
            }),
            crate::brep::BrepParse::SemanticInvalid { raw, error, .. } => stage_invalid_brep(
                BrepCarrierInput {
                    expand: self.expand,
                    data: self.scan.data,
                    archive: self.archive(),
                    writer_version: self.scan.metadata.properties.writer_version,
                    raw,
                    key: key.as_str(),
                    association: &association,
                    unknown: &unknown,
                    scale,
                    mesh_budget: &mut self.mesh_budget,
                },
                error,
            ),
        };
        match staged {
            Ok(staged) => {
                let BrepDraft {
                    kind,
                    draft,
                    links,
                    warnings,
                    typed_losses,
                } = staged;
                let full_topology = matches!(kind, BrepTransferKind::FullTopology);
                let emitted_geometry =
                    !draft.model().curves.is_empty() || !draft.model().surfaces.is_empty();
                let cache_only =
                    !full_topology && !emitted_geometry && !draft.model().tessellations.is_empty();
                let entity_count = draft.entity_count();
                let mut budget = self.expansion_budget;
                let committed =
                    if self.instance_selection.is_none() || budget.entities(entity_count) {
                        match self.session.validate_source_records()? {
                            Ok(()) => match self.session.commit(draft, &mut self.annotations)? {
                                Ok(()) => Ok(()),
                                Err(error) => Err(format!("{error}")),
                            },
                            Err(error) => Err(format!("{error}")),
                        }
                    } else {
                        Err(ENTITY_BUDGET_EXCEEDED.to_string())
                    };
                if let Err(error) = committed {
                    self.scan_warning(
                        source_order,
                        format_args!("Brep draft rejected before commit: {error}"),
                    );
                } else {
                    self.expansion_budget = budget;
                    self.append_links(source_order, &links)?;

                    self.report.typed_losses.extend(typed_losses);
                    for warning in warnings {
                        match warning.code {
                            Some(
                                code @ (RhinoLossCode::TopologyBrepFallback
                                | RhinoLossCode::PolycurveJoinGap
                                | RhinoLossCode::TrimPcurveDropped),
                            ) => {
                                self.report
                                    .typed_losses
                                    .push(code.note(format_args!("{}", warning.message)));
                            }
                            _ => self.scan_diagnostic(source_order, &warning),
                        }
                    }
                    if cache_only {
                        self.scan_warning(
                            source_order,
                            format_args!(
                                "Brep emitted cache tessellations without decoded geometry"
                            ),
                        );
                    }
                    self.geometry_transferred |= full_topology || emitted_geometry;
                    if full_topology {
                        self.mark_decoded(source_order);
                    } else {
                        self.scan_warning(
                            source_order,
                            format_args!("Brep topology invalid; decoded child carriers retained"),
                        );
                    }
                }
            }
            Err(crate::curves::GeometryError::Codec(error)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("Brep geometry/topology degraded: {error}"),
                );
            }
        }
        Ok(())
    }

    fn transition(&mut self, source_order: usize, next: GeometryOutcome) -> bool {
        let Some(status @ None) = self.statuses.get_mut(source_order) else {
            return false;
        };
        *status = Some(next);
        if let Some(undo) = &mut self.expansion_undo {
            undo.outcomes.push(source_order);
        }
        true
    }

    fn class_outcomes(&self) -> Result<Vec<(String, ClassOutcome<'a>)>, cadmpeg_core::CodecError> {
        let mut outcomes = HashMap::new();
        for (object, status) in self.scan.objects.iter().zip(&self.statuses) {
            let class = object.class_uuid().unwrap_or_else(crate::wire::Uuid::nil);
            let outcome = outcomes.entry(class).or_insert_with(|| ClassOutcome {
                decoded: 0,
                retained: 0,
                native: None,
                attribute_degraded: 0,
                failed_framed: 0,
                first_object: object,
            });
            // Keep the first framed source, or the last degraded source if none was framed.
            if outcome.first_object.is_degraded() {
                outcome.first_object = object;
            }
            if object.framed().is_some_and(|object| {
                matches!(object.attributes, crate::objects::AttributeState::Degraded)
            }) {
                outcome.attribute_degraded += 1;
            }
            match status {
                None => outcome.retained += 1,
                Some(GeometryOutcome::Decoded) => outcome.decoded += 1,
                Some(GeometryOutcome::Failed) => outcome.failed_framed += 1,
                Some(GeometryOutcome::NativeRetained(code)) => {
                    let count = outcome
                        .native
                        .as_ref()
                        .map_or(Some(NonZeroUsize::MIN), |(_, count)| {
                            count.get().checked_add(1).and_then(NonZeroUsize::new)
                        })
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Rhino class outcome count overflow",
                            )
                        })?;
                    outcome.native = Some((*code, count));
                }
            }
        }
        let mut sorted: Vec<_> = outcomes
            .into_iter()
            .map(|(class, outcome)| (format!("{class}"), outcome))
            .collect();
        sorted.sort_unstable_by(|(first, _), (second, _)| first.cmp(second));
        Ok(sorted)
    }
}

fn duplicate_userdata_count(userdata: &[UserdataDescriptor], class: crate::wire::Uuid) -> usize {
    userdata
        .iter()
        .filter_map(UserdataDescriptor::known)
        .filter(|value| value.class_uuid == class)
        .count()
}

#[cfg(test)]
fn append_record_links(ir: &mut CadIr, unknown: &UnknownId, links: &[String]) {
    let mut unknowns = ir
        .native_unknowns("rhino")
        .expect("fixture unknown records");
    let record = unknowns
        .iter_mut()
        .find(|record| record.id == *unknown)
        .expect("fixture unknown record exists");
    record.links.extend(
        links
            .iter()
            .filter(|link| link.as_str() != record.id.as_str())
            .map(|link| {
                cadmpeg_ir::ids::Identity::new(link.clone()).expect("fixture link identity")
            }),
    );
    record.links.sort();
    record.links.dedup();
    ir.set_native_unknowns(
        &cadmpeg_test_support::service_decode_context(),
        "rhino",
        &unknowns,
    )
    .expect("fixture unknown records");
}

struct ValidationFindings<'a>([Option<&'a cadmpeg_ir::report::check::Finding>; 3]);

impl std::fmt::Display for ValidationFindings<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (position, finding) in self.0.iter().flatten().enumerate() {
            if position != 0 {
                formatter.write_str("; ")?;
            }
            match &finding.entity {
                Some(entity) => write!(
                    formatter,
                    "{} ({entity}): {}",
                    finding.check, finding.message
                )?,
                None => write!(formatter, "{}: {}", finding.check, finding.message)?,
            }
        }
        Ok(())
    }
}

fn validation_findings(report: &cadmpeg_ir::report::check::ValidationReport) -> String {
    let mut selected = [None; 3];
    let mut count = 0;
    for finding in &report.findings {
        if finding.severity < Severity::Error {
            continue;
        }
        selected[count] = Some(finding);
        count += 1;
        if count == selected.len() {
            break;
        }
    }
    format!("{}", ValidationFindings(selected))
}

trait ExactnessTarget {
    fn set(&mut self, id: &str, exactness: Exactness);
}

impl ExactnessTarget for cadmpeg_ir::Annotations {
    fn set(&mut self, id: &str, exactness: Exactness) {
        let mut builder = AnnotationBuilder::resume(std::mem::take(self));
        builder.exactness(id, exactness);
        *self = builder.build();
    }
}

impl ExactnessTarget for cadmpeg_ir::annotations::SparseAnnotationTransaction<'_> {
    fn set(&mut self, id: &str, exactness: Exactness) {
        self.exactness(id, exactness);
    }
}

fn annotate_derived(annotations: &mut impl ExactnessTarget, id: &str) {
    set_exactness(annotations, id, Exactness::Derived);
}

fn set_exactness(
    annotations: &mut impl ExactnessTarget,
    id: impl std::fmt::Display,
    exactness: Exactness,
) {
    let id = format!("{id}");
    annotations.set(&id, exactness);
}

struct CommittedExtrusionBoundary<'a> {
    boundary: &'a crate::extrusion::ExtrusionBoundary,
    directrix: cadmpeg_ir::ids::CurveId,
}

fn stage_extrusion_caps(
    ir: &mut CadIr,
    annotations: &mut impl ExactnessTarget,
    key: &str,
    association: &SourceObjectAssociation,
    extrusion: &crate::extrusion::DecodedExtrusion,
    boundaries: &[CommittedExtrusionBoundary<'_>],
) -> Result<String, CandidateError> {
    let key = IdentityKey::try_new(key.to_owned()).map_err(|error| error.to_string())?;
    let body_id = {
        Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::BodyId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "body"),
            key.clone().then(cadmpeg_ir::identity_key!(".caps")),
        ))
    }?;
    let mut region_ids = Vec::new();
    for cap in 0..2 {
        if !extrusion.caps[cap] {
            continue;
        }

        let cap_key = key
            .clone()
            .then(cadmpeg_ir::identity_key!(".cap-"))
            .then(cap);
        let region_id = {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::RegionId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "region"),
                cap_key.clone(),
            ))
        }?;
        let shell_id = {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::ShellId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "shell"),
                cap_key.clone(),
            ))
        }?;
        let surface_id = {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::SurfaceId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
                cap_key.clone(),
            ))
        }?;
        let face_id = {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::FaceId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "face"),
                cap_key.clone(),
            ))
        }?;
        let frame = cadmpeg_ir::units::OrthonormalFrame3::from_units(
            extrusion.cap_normals[cap],
            extrusion.cap_u_axes[cap],
        )
        .ok_or_else(|| {
            "extrusion cap staging: PlaneSurface.normal/u_axis must form an orthonormal frame"
                .to_string()
        })?;
        let origin = cadmpeg_ir::features::FinitePoint3::new(extrusion.cap_origins[cap])
            .ok_or_else(|| {
                "extrusion cap staging: PlaneSurface.origin must be finite".to_string()
            })?;
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::new(origin, frame),
            )),
            source_object: Some(association.clone()),
        });
        let mut loop_ids = Vec::new();
        for (profile, committed) in boundaries.iter().enumerate() {
            let boundary = committed.boundary;

            let suffix = key
                .clone()
                .then(cadmpeg_ir::identity_key!(".cap-"))
                .then(cap)
                .then(cadmpeg_ir::identity_key!(".profile-"))
                .then(profile);
            let curve_id = if cap == 0 {
                committed.directrix.clone()
            } else {
                let id = {
                    Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::CurveId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "curve"),
                        suffix.clone(),
                    ))
                }?;
                ir.model.curves.push(Curve {
                    parameter_range: None,
                    id: id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                        boundary.end_nurbs.clone(),
                    )),
                    source_object: Some(association.clone()),
                });
                annotate_derived(annotations, &id.to_string());
                id
            };
            let endpoint = if cap == 0 {
                boundary.start_nurbs.control_points().first().copied()
            } else {
                boundary.end_nurbs.control_points().first().copied()
            };
            let Some(endpoint) = endpoint else {
                return Err(format!(
                    "extrusion cap staging: cap {cap} profile {profile} has no endpoint"
                )
                .into());
            };
            let point_id = {
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::PointId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "point"),
                    suffix.clone(),
                ))
            }?;
            let vertex_id = {
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::VertexId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "vertex"),
                    suffix.clone(),
                ))
            }?;
            let edge_id = {
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::EdgeId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "edge"),
                    suffix.clone(),
                ))
            }?;
            let loop_id = {
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::LoopId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "loop"),
                    suffix.clone(),
                ))
            }?;
            let coedge_id = {
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::CoedgeId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "coedge"),
                    suffix.clone(),
                ))
            }?;
            let pcurve_id = {
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::PcurveId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "pcurve"),
                    suffix.clone(),
                ))
            }?;
            let pcurve = if cap == 0 {
                &boundary.start_pcurve
            } else {
                &boundary.end_pcurve
            };
            let degree = usize::try_from(pcurve.degree).map_err(|error| {
                format!(
                    "extrusion cap staging: pcurve degree {}: {error}",
                    pcurve.degree
                )
            })?;
            let end_index = degree.checked_add(1)
                .and_then(|order| pcurve.knots.len().checked_sub(order))
                .ok_or_else(|| format!(
                    "extrusion cap staging: pcurve knot count {} cannot supply degree {degree} support",
                    pcurve.knots.len()
                ))?;
            let parameter_range = pcurve
                .knots
                .get(degree)
                .copied()
                .zip(pcurve.knots.get(end_index).copied())
                .map(|(start, end)| [start, end])
                .ok_or_else(|| format!(
                    "extrusion cap staging: pcurve parameter range indexes {degree} and {end_index} exceed knot count {}",
                    pcurve.knots.len()
                ))?;
            let nurbs = PcurveNurbs::from_lanes(
                pcurve.degree,
                pcurve.knots.clone(),
                pcurve.control_points.clone(),
                pcurve.weights.clone(),
                pcurve.periodic,
            )
            .map_err(|error| format!("extrusion cap staging: {error}"))?;
            let carrier =
                cadmpeg_ir::topology::EdgeCarrier::new(Some(curve_id), Some(parameter_range))
                    .map_err(|error| format!("extrusion cap staging: {error}"))?;
            ir.model.points.push(Point::new(
                point_id.clone(),
                endpoint,
                Some(association.clone()),
            ));
            ir.model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id.clone(),
                tolerance: None,
            });
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                carrier,
                start: vertex_id.clone(),
                end: vertex_id.clone(),
                tolerance: None,
            });
            ir.model.pcurves.push(Pcurve {
                id: pcurve_id.clone(),
                geometry: PcurveGeometry::Nurbs { nurbs },
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                    None,
                    Some(
                        cadmpeg_ir::units::FiniteVector::new(parameter_range).ok_or_else(|| {
                            format!(
                                "extrusion cap staging: {}",
                                cadmpeg_ir::geometry::pcurve::PcurveMetadata::NON_FINITE_PARAMETER_RANGE
                            )
                        })?,
                    ),
                    None,
                ),
            });
            ir.model.coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: loop_id.clone(),
                edge: edge_id.clone(),
                radial_next: coedge_id.clone(),
                sense: Sense::Forward,
                pcurves: vec![cadmpeg_ir::topology::PcurveUse {
                    pcurve: pcurve_id.clone(),
                    isoparametric: None,
                    parameter_range: None,
                }],
                use_curve: None,
            });
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                    cadmpeg_ir::topology::LoopRing::new(vec![coedge_id.clone()], Vec::new())
                        .map_err(|error| format!("extrusion cap staging: {error}"))?,
                ),
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
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(loop_ids),
            name: None,
            color: association.color,
            tolerance: None,
        });
        annotate_derived(annotations, &surface_id.to_string());
        annotate_derived(annotations, &face_id.to_string());
        ir.model.shells.push(Shell::with_face(
            shell_id.clone(),
            region_id.clone(),
            face_id,
        ));
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
        return Err("extrusion cap staging: no enabled caps".to_string().into());
    }
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: BodyKind::Sheet,
        regions: region_ids,
        transform: None,
        name: association
            .name
            .as_deref()
            .map(std::borrow::ToOwned::to_owned),
        color: association.color,
        visible: association.visible,
    });
    annotate_derived(annotations, &body_id.to_string());
    Ok(body_id.to_string())
}

#[derive(Debug)]
struct BrepDraft {
    kind: BrepTransferKind,
    draft: ModelDraft<DraftAccounting>,
    links: Vec<String>,
    warnings: Diagnostics,
    typed_losses: Vec<LossNote>,
}

impl Default for BrepDraft {
    fn default() -> Self {
        Self {
            kind: BrepTransferKind::FullTopology,
            draft: ModelDraft::new().with_accounting(),
            links: Vec::new(),
            warnings: Diagnostics::default(),
            typed_losses: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrepTransferKind {
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
    scale: MillimeterScale,
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
    scale: MillimeterScale,
    mesh_budget: &'a mut crate::mesh::MeshBudget,
}

struct BrepCarrierDraft {
    staged: BrepDraft,
    c3: HashMap<usize, cadmpeg_ir::ids::CurveId>,
    surfaces: HashMap<usize, StagedBrepSurface>,
    child_cause: Option<String>,
}

struct StagedBrepSurface {
    id: cadmpeg_ir::ids::SurfaceId,
    plane_parameterization: Option<crate::surfaces::PlaneParameterization>,
}

struct BrepStageContext<'a> {
    ctx: &'a cadmpeg_core::decode::DecodeContext<'a>,
    key: &'a str,
    association: &'a SourceObjectAssociation,
    unknown: &'a UnknownId,
}

impl BrepDraft {
    /// Records the loss for one unreadable Brep display-mesh cache slot.
    fn mesh_cache_slot_dropped(
        &mut self,
        kind: &str,
        index: usize,
        error: &impl std::fmt::Display,
    ) {
        self.warnings.push_coded(
            RhinoLossCode::BrepMeshCacheDegraded,
            format_args!("invalid {kind} mesh cache slot {index}: {error}"),
        );
    }

    fn free_carrier_fallback(mut self, cause: impl std::fmt::Display) -> Self {
        self.kind = BrepTransferKind::FreeCarrierFallback;
        let mut emitted = BTreeSet::new();
        for id in self
            .draft
            .model()
            .curves
            .iter()
            .map(|value| value.id.as_str())
            .chain(
                self.draft
                    .model()
                    .surfaces
                    .iter()
                    .map(|value| value.id.as_str()),
            )
            .chain(
                self.draft
                    .model()
                    .tessellations
                    .iter()
                    .map(|value| value.id.as_str()),
            )
            .chain(
                self.draft
                    .model()
                    .procedural_curves
                    .iter()
                    .map(|value| value.id.as_str()),
            )
        {
            if !emitted.contains(id) {
                emitted.insert(id.to_owned());
            }
        }
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
        self.warnings.push_coded(
            RhinoLossCode::TopologyBrepFallback,
            format_args!("Brep topology fallback: {cause}"),
        );
        self
    }
}

fn stage_brep_carriers(
    input: BrepCarrierInput<'_>,
) -> Result<BrepCarrierDraft, crate::curves::GeometryError> {
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
    let mut staged = BrepDraft::default();
    let mut procedural_admission =
        cadmpeg_ir::document::procedural::ProceduralAdmission::new(staged.draft.model_mut());
    let mut c3 = HashMap::new();
    let mut surfaces = HashMap::new();
    let mut child_cause = None;
    for (kind, slots) in [
        ("render", &raw.render_meshes),
        ("analysis", &raw.analysis_meshes),
    ] {
        for (index, slot) in slots.iter().enumerate() {
            let Some(slot) = slot.as_ref() else {
                continue;
            };
            let id = format!("rhino:object:tessellation#{key}.{kind}-{index}");
            match crate::mesh::decode(
                expand,
                data,
                slot.mesh.class_data_range.clone(),
                archive,
                crate::mesh::MeshDecodeOptions {
                    writer_version,
                    association: Some(association.clone()),
                    id: crate::mesh::MeshId::Ready(
                        cadmpeg_ir::tessellation::TessellationId::mint(id).map_err(|error| {
                            cadmpeg_core::CodecError::Malformed(error.to_string())
                        })?,
                    ),
                    scale,
                    userdata: &slot.userdata,
                },
                mesh_budget,
            ) {
                Ok(mut mesh) => {
                    staged.warnings.append(&mut mesh.warnings);
                    staged.draft.exactness(
                        mesh.tessellation.id.to_string(),
                        if mesh.scaled {
                            Exactness::Derived
                        } else {
                            Exactness::ByteExact
                        },
                    );
                    staged.links.push(mesh.tessellation.id.to_string());
                    staged
                        .draft
                        .model_mut()
                        .tessellations
                        .push(mesh.tessellation);
                }
                Err(error @ crate::curves::GeometryError::Codec(_)) => return Err(error),
                Err(error) => staged.mesh_cache_slot_dropped(kind, index, &error),
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
            expand.ctx(),
            data,
            child.class_uuid,
            child.class_data_range.clone(),
            scale,
            archive,
        );
        match decoded {
            Ok(crate::curves::DecodedGeometry::Curve { curve }) => {
                append_curve_warnings(
                    expand.ctx(),
                    &mut staged.warnings,
                    &curve,
                    format_args!("C3 slot {index}"),
                )?;
                let id = match stage_curve_tree_indexed(
                    &mut procedural_admission,
                    &mut staged,
                    curve,
                    &format!("c3-{index}"),
                    &BrepStageContext {
                        ctx: expand.ctx(),
                        key,
                        association,
                        unknown,
                    },
                ) {
                    Ok(id) => id,
                    Err(error @ crate::curves::GeometryError::Codec(_)) => return Err(error),
                    Err(error) => {
                        child_cause = Some(format!("C3 slot {index}: {error}"));
                        continue;
                    }
                };

                c3.insert(index, id);
            }
            Ok(_) => {
                child_cause = Some(format!("C3 slot {index} is not a curve"));
            }
            Err(error @ crate::curves::GeometryError::Codec(_)) => return Err(error),
            Err(error) => {
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
            expand.ctx(),
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
                let plane_parameterization = geometry.plane_parameterization();
                let surface_key = match IdentityKey::try_new(key.to_owned()) {
                    Ok(key) => key,
                    Err(error) => {
                        child_cause = Some(format!("surface slot {index}: {error}"));
                        continue;
                    }
                };
                let id = cadmpeg_ir::ids::SurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
                    surface_key
                        .then(cadmpeg_ir::identity_key!(".slot-"))
                        .then(index),
                );
                staged.draft.model_mut().surfaces.push(Surface {
                    id: id.clone(),
                    geometry: geometry.into_geometry(),
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

                surfaces.insert(
                    index,
                    StagedBrepSurface {
                        id,
                        plane_parameterization,
                    },
                );
            }
            Ok(crate::curves::DecodedGeometry::Surface {
                surface:
                    crate::surfaces::DecodedSurface::Procedural {
                        geometry,
                        definition,
                    },
            }) => match stage_brep_procedural_surface(
                &mut procedural_admission,
                &mut staged,
                index,
                geometry,
                definition,
                &BrepStageContext {
                    ctx: expand.ctx(),
                    key,
                    association,
                    unknown,
                },
            ) {
                Ok(id) => {
                    surfaces.insert(
                        index,
                        StagedBrepSurface {
                            id,
                            plane_parameterization: None,
                        },
                    );
                }
                Err(error @ crate::curves::GeometryError::Codec(_)) => return Err(error),
                Err(error) => {
                    child_cause = Some(format!("surface slot {index}: {error}"));
                }
            },
            Ok(_) => {
                child_cause = Some(format!("surface slot {index} is not a surface"));
            }
            Err(error @ crate::curves::GeometryError::Codec(_)) => return Err(error),
            Err(error) => {
                child_cause = Some(format!("surface slot {index}: {error}"));
            }
        }
    }
    let model = staged.draft.model_mut();
    for source in model
        .surfaces
        .iter_mut()
        .filter_map(|surface| surface.source_object.as_mut())
        .chain(
            model
                .curves
                .iter_mut()
                .filter_map(|curve| curve.source_object.as_mut()),
        )
    {
        source.geometry_role = Some(cadmpeg_ir::SourceGeometryRole::Support);
    }
    Ok(BrepCarrierDraft {
        staged,
        c3,
        surfaces,
        child_cause,
    })
}

fn stage_invalid_brep(
    input: BrepCarrierInput<'_>,
    semantic_error: &crate::curves::GeometryError,
) -> Result<BrepDraft, crate::curves::GeometryError> {
    let carriers = stage_brep_carriers(input)?;
    Ok(finish_brep_fallback(carriers.staged, semantic_error))
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
    let key = IdentityKey::try_new(key.to_owned())
        .map_err(|error| crate::curves::GeometryError::unpositioned(error.to_string()))?;
    let raw = brep.raw();
    let resolved = brep.resolved();
    let BrepCarrierDraft {
        mut staged,
        c3,
        surfaces,
        child_cause,
    } = stage_brep_carriers(BrepCarrierInput {
        expand,
        data,
        archive,
        writer_version,
        raw,
        key: key.as_str(),
        association,
        unknown,
        scale,
        mesh_budget,
    })?;
    let ctx = expand.ctx();
    if let Some(cause) = child_cause {
        return Ok(finish_brep_fallback(staged, cause));
    }
    let DecodedPcurves {
        ids: c2,
        values: pcurves,
        warnings: mut pcurve_warnings,
    } = decode_pcurves(
        expand.ctx(),
        data,
        archive,
        raw,
        resolved,
        key.as_str(),
        &surfaces,
    )?;
    staged.warnings.append(&mut pcurve_warnings);
    staged.draft.model_mut().pcurves = pcurves;
    let body_id = {
        Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::BodyId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "body"),
            key.clone(),
        ))
    }?;
    let mut vertex_ids = Vec::new();

    for (index, vertex) in raw.vertices.iter().enumerate() {
        let point_id = {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::PointId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "point"),
                key.clone()
                    .then(cadmpeg_ir::identity_key!(".vertex-"))
                    .then(index),
            ))
        }?;
        let vertex_id = {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::VertexId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "vertex"),
                key.clone()
                    .then(cadmpeg_ir::identity_key!(".slot-"))
                    .then(index),
            ))
        }?;
        let position = crate::wire::scaled_point(vertex.point.get(), scale).ok_or_else(|| {
            crate::curves::GeometryError::unpositioned("scaled Brep vertex coordinate is invalid")
        })?;
        staged.draft.model_mut().points.push(Point::new(
            point_id.clone(),
            position,
            Some(association.clone()),
        ));
        staged.draft.model_mut().vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point_id,
            tolerance: scaled_tolerance(resolved.vertices[index].tolerance, scale)?,
        });
        vertex_ids.push(vertex_id);
    }
    let mut edge_ids = Vec::new();

    for (index, edge) in raw.edges.iter().enumerate() {
        let id = {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::EdgeId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "edge"),
                key.clone()
                    .then(cadmpeg_ir::identity_key!(".slot-"))
                    .then(index),
            ))
        }?;
        let curve = c3.get(&resolved.edges[index].curve).cloned();
        let vertices = edge_vertices(edge, &resolved.edges[index]);
        staged.draft.model_mut().edges.push(Edge {
            id: id.clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(curve, Some(edge_param_range(edge)))
                .map_err(crate::curves::GeometryError::unpositioned)?,
            start: vertex_ids[vertices[0]].clone(),
            end: vertex_ids[vertices[1]].clone(),
            tolerance: scaled_tolerance(resolved.edges[index].tolerance, scale)?,
        });
        edge_ids.push(id);
    }
    let components = face_components(expand.ctx(), resolved)?;
    let grouping = region_shell_groups(expand.ctx(), raw, resolved, &components)?;
    let free_vertex_indices = brep_free_vertex_indices(expand.ctx(), resolved)?;
    if !free_vertex_indices.is_empty() && grouping.shells.len() != 1 {
        return Ok(finish_brep_fallback(
            staged,
            "Brep free vertices have no unique shell membership",
        ));
    }
    let mut free_vertex_ids = Vec::new();
    for index in &free_vertex_indices {
        free_vertex_ids.push(Ok::<_, cadmpeg_core::CodecError>(
            cadmpeg_ir::ids::VertexId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "vertex"),
                key.clone()
                    .then(cadmpeg_ir::identity_key!(".slot-"))
                    .then(*index),
            ),
        )?);
    }
    if grouping.fallback {
        staged.warnings.push(format_args!(
            "Brep 3.3 region topology was not representable; incidence-derived shells used"
        ));
    }
    let mut face_ids = Vec::new();
    let mut pending_faces = Vec::new();

    for (index, face) in raw.faces.iter().enumerate() {
        let surface = surfaces
            .get(&resolved.faces[index].surface)
            .map(|surface| surface.id.clone())
            .ok_or_else(|| {
                crate::curves::error(face.source_range.start, "surface child missing")
            })?;
        let component = grouping.face_groups[index];
        let id = {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::FaceId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "face"),
                key.clone()
                    .then(cadmpeg_ir::identity_key!(".slot-"))
                    .then(index),
            ))
        }?;
        // The face's non-loop fields are held until its loops resolve, so the
        // face is constructed once with its complete boundary.
        pending_faces.push((
            id.clone(),
            {
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::ShellId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "shell"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".component-"))
                        .then(component),
                ))
            }?,
            surface,
            face_sense(face.reversed_surface),
            face.color.map(color),
        ));
        face_ids.push(id);
    }
    let mut face_loop_ids = ctx.alloc_filled(
        raw.faces.len(),
        Vec::<cadmpeg_ir::ids::LoopId>::new(),
        "Rhino staged Brep face loop lists",
    )?;
    let mut coedge_positions = ctx.alloc_filled(
        raw.trims.len(),
        None::<usize>,
        "Rhino staged Brep coedge positions",
    )?;

    for (index, loop_record) in resolved.loops.iter().enumerate() {
        let id = {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::LoopId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "loop"),
                key.clone()
                    .then(cadmpeg_ir::identity_key!(".slot-"))
                    .then(index),
            ))
        }?;
        let face_id = face_ids[loop_record.face].clone();
        let mut coedges = Vec::new();
        for trim_index in &loop_record.trims {
            let trim = &raw.trims[*trim_index];
            let trim_refs = &resolved.trims[*trim_index];
            let coedge_id = {
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::CoedgeId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "coedge"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".slot-"))
                        .then(*trim_index),
                ))
            }?;
            let edge_id = if let Some(edge) = trim_refs.edge {
                edge_ids.get(edge).cloned().ok_or_else(|| {
                    crate::curves::error(trim.source_range.start, "trim edge missing")
                })?
            } else {
                let synthetic_id = {
                    Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::EdgeId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "edge"),
                        key.clone()
                            .then(cadmpeg_ir::identity_key!(".singular-"))
                            .then(*trim_index),
                    ))
                }?;
                if coedge_positions[*trim_index].is_none() {
                    staged.draft.model_mut().edges.push(Edge {
                        id: synthetic_id.clone(),
                        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(None),
                        start: vertex_ids[trim_refs.vertices[0]].clone(),
                        end: vertex_ids[trim_refs.vertices[0]].clone(),
                        tolerance: scaled_tolerance(trim_refs.tolerances[1], scale)?,
                    });
                }
                synthetic_id
            };
            let pcurve = if trim.trim_type == crate::brep::RawTrimKind::PointOnSurface {
                None
            } else {
                c2.get(trim_index).cloned()
            };
            coedge_positions[*trim_index] = Some(staged.draft.model().coedges.len());
            let mut pcurves = Vec::new();
            if let Some(pcurve) = pcurve {
                pcurves.push(cadmpeg_ir::topology::PcurveUse {
                    pcurve,
                    isoparametric: None,
                    parameter_range: None,
                });
            }
            staged.draft.model_mut().coedges.push(Coedge {
                id: coedge_id.clone(),
                owner_loop: id.clone(),
                edge: edge_id,
                radial_next: coedge_id.clone(),
                sense: coedge_sense(
                    trim.reversed_3d,
                    trim_refs
                        .edge
                        .is_some_and(|edge| raw.edges[edge].proxy_reversed),
                ),
                pcurves,
                use_curve: None,
            });
            coedges.push(coedge_id);
        }
        staged.draft.model_mut().loops.push(Loop {
            id: id.clone(),
            face: face_id.clone(),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::new(coedges, Vec::new()).map_err(|error| {
                    crate::curves::GeometryError::unpositioned(error.to_string())
                })?,
            ),
        });

        face_loop_ids[loop_record.face].push(id);
    }
    for (face_index, (id, shell, surface, sense, color)) in pending_faces.into_iter().enumerate() {
        staged.draft.model_mut().faces.push(Face {
            id,
            shell,
            surface,
            sense,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(std::mem::take(
                &mut face_loop_ids[face_index],
            )),
            name: None,
            color,
            tolerance: None,
        });
    }
    for edge_index in 0..resolved.edges.len() {
        let uses = &resolved.edges[edge_index].trims;
        if uses.is_empty() {
            continue;
        }
        for (offset, trim_index) in uses.iter().enumerate() {
            let next = {
                Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::CoedgeId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "coedge"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".slot-"))
                        .then(uses[(offset + 1) % uses.len()]),
                ))
            }?;
            let Some(position) = coedge_positions[*trim_index] else {
                return Err(crate::curves::GeometryError::unpositioned(
                    "Brep coedge position is missing",
                ));
            };
            staged.draft.model_mut().coedges[position].radial_next = next;
        }
    }
    let mut regions: Vec<Region> = Vec::new();

    for (component, shell) in grouping.shells.iter().enumerate() {
        let region_label = shell.region;
        let region_id = {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::RegionId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "region"),
                key.clone()
                    .then(cadmpeg_ir::identity_key!(".slot-"))
                    .then(region_label),
            ))
        }?;
        let shell_id = {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::ShellId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "shell"),
                key.clone()
                    .then(cadmpeg_ir::identity_key!(".component-"))
                    .then(component),
            ))
        }?;
        let mut shell_faces = Vec::new();
        for index in &shell.faces {
            shell_faces.push(face_ids[*index].clone());
        }
        staged.draft.model_mut().shells.push(
            Shell::new(
                shell_id.clone(),
                region_id.clone(),
                shell_faces,
                Vec::new(),
                if component == 0 {
                    std::mem::take(&mut free_vertex_ids)
                } else {
                    Vec::new()
                },
            )
            .map_err(|message| crate::curves::GeometryError::unpositioned(message.to_string()))?,
        );
        if let Some(region) = regions.iter_mut().find(|region| region.id == region_id) {
            region.shells.push(shell_id);
        } else {
            regions.push(Region {
                id: region_id,
                body: body_id.clone(),
                shells: vec![shell_id],
            });
        }
    }
    staged.draft.model_mut().regions = regions;
    let mut body_regions = Vec::new();
    for region in &staged.draft.model().regions {
        body_regions.push(region.id.clone());
    }
    let (body_kind, body_kind_substituted) = brep.body_kind(writer_version);
    if let Some(loss) = body_kind_substituted {
        staged.typed_losses.push(loss);
    }

    staged.draft.model_mut().bodies.push(Body {
        id: body_id.clone(),
        kind: match body_kind {
            crate::brep::BrepBodyKind::Solid => BodyKind::Solid,
            crate::brep::BrepBodyKind::Sheet => BodyKind::Sheet,
        },
        regions: body_regions,
        transform: None,
        name: association
            .name
            .as_deref()
            .map(std::borrow::ToOwned::to_owned),
        color: association.color,
        visible: association.visible,
    });

    for curve in &staged.draft.model().curves {
        staged.links.push(format!("{}", curve.id));
    }
    for surface in &staged.draft.model().surfaces {
        staged.links.push(format!("{}", surface.id));
    }
    staged.links.push(format!("{body_id}"));
    let derived_ids = {
        let model = staged.draft.model();
        let mut ids = Vec::new();
        macro_rules! append_ids {
            ($field:ident) => {
                for value in &model.$field {
                    ids.push(format!("{}", value.id));
                }
            };
        }
        append_ids!(bodies);
        append_ids!(regions);
        append_ids!(shells);
        append_ids!(faces);
        append_ids!(loops);
        append_ids!(coedges);
        append_ids!(edges);
        append_ids!(vertices);
        append_ids!(points);
        append_ids!(pcurves);
        ids
    };
    for id in derived_ids {
        staged.draft.exactness(id, Exactness::Derived);
    }
    scale_plane_pcurves(&mut staged, scale)?;
    Ok(staged)
}

fn finish_brep_fallback(mut staged: BrepDraft, cause: impl std::fmt::Display) -> BrepDraft {
    for id in staged
        .draft
        .model()
        .curves
        .iter()
        .map(|curve| curve.id.as_str())
        .chain(
            staged
                .draft
                .model()
                .surfaces
                .iter()
                .map(|surface| surface.id.as_str()),
        )
    {
        staged.links.push(id.to_owned());
    }
    staged.free_carrier_fallback(cause)
}

/// Projects one embedded Brep into a self-contained semantic topology value.
pub(crate) fn embedded_brep_json(
    expand: crate::mesh::MeshExpand<'_>,
    data: &[u8],
    range: std::ops::Range<usize>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    scale: MillimeterScale,
    refusal: &mut Option<cadmpeg_core::CodecError>,
) -> Option<String> {
    let parsed = match crate::brep::parse(expand.ctx(), data, range, archive, writer_version, &[]) {
        Ok(value) => value,
        Err(crate::curves::GeometryError::Codec(error)) => {
            *refusal = Some(error);
            return None;
        }
        Err(_) => return None,
    };
    let brep = match parsed {
        crate::brep::BrepParse::Valid(value) => value,
        crate::brep::BrepParse::SemanticInvalid { .. } => return None,
    };
    let association = SourceObjectAssociation {
        geometry_role: None,
        format: cadmpeg_ir::CodecFormat::Rhino,
        object_id: cadmpeg_core::text::NonBlankString::new("embedded-history-brep".to_string())?,
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let unknown = UnknownId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "history", "brep"),
        cadmpeg_ir::identity_key!("embedded"),
    );
    let mut mesh_budget = crate::mesh::MeshBudget::new();
    let staged = match stage_brep(BrepTransferInput {
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
    }) {
        Ok(value) => value,
        Err(crate::curves::GeometryError::Codec(error)) => {
            *refusal = Some(error);
            return None;
        }
        Err(_) => return None,
    };
    if staged.kind != BrepTransferKind::FullTopology {
        return None;
    }
    let snapshot = match staged.draft.model().geometry_snapshot("brep") {
        Ok(snapshot) => snapshot,
        Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => {
            *refusal = Some(error);
            return None;
        }
        Err(_) => return None,
    };
    match crate::wire::canonical_json(&snapshot) {
        Ok(text) => Some(text),
        Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => {
            *refusal = Some(error);
            None
        }
        Err(_) => None,
    }
}

/// Rhino trim curves live in the surface's native parameter space. A plane's
/// parameters are lengths, so a unit-scaled document moves the plane's
/// parameterization to millimeters while the trims stay in native units;
/// the UV poles of pcurves on plane faces scale to match. NURBS surface
/// parameters are knot-domain values and do not scale.
fn scale_plane_pcurves(
    staged: &mut BrepDraft,
    scale: MillimeterScale,
) -> Result<(), crate::curves::GeometryError> {
    fn insert_id(values: &mut BTreeSet<String>, id: &str) {
        if !values.contains(id) {
            values.insert(id.to_owned());
        }
    }
    if scale == MillimeterScale::IDENTITY {
        return Ok(());
    }
    let mut plane_surfaces = BTreeSet::new();
    for surface in &staged.draft.model().surfaces {
        if matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
        ) {
            insert_id(&mut plane_surfaces, surface.id.as_str());
        }
    }
    let mut plane_faces = BTreeSet::new();
    for face in &staged.draft.model().faces {
        if plane_surfaces.contains(face.surface.as_str()) {
            insert_id(&mut plane_faces, face.id.as_str());
        }
    }
    let mut plane_loops = BTreeSet::new();
    for value in &staged.draft.model().loops {
        if plane_faces.contains(value.face.as_str()) {
            insert_id(&mut plane_loops, value.id.as_str());
        }
    }
    let mut plane_pcurves = BTreeSet::new();
    for coedge in &staged.draft.model().coedges {
        if plane_loops.contains(coedge.owner_loop.as_str()) {
            for curve_use in &coedge.pcurves {
                insert_id(&mut plane_pcurves, curve_use.pcurve.as_str());
            }
        }
    }
    for pcurve in &mut staged.draft.model_mut().pcurves {
        if !plane_pcurves.contains(pcurve.id.as_str()) {
            continue;
        }
        if let PcurveGeometry::Nurbs { nurbs } = &mut pcurve.geometry {
            if let Err(message) = nurbs.try_map_control_points(|_, pole| {
                let pole = pole.get();
                cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(
                    pole.u * scale.value(),
                    pole.v * scale.value(),
                ))
                .ok_or("control_points contains a non-finite point")
            }) {
                return Err(crate::curves::GeometryError::unpositioned(
                    message.to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn edge_param_range(edge: &crate::brep::RawBrepEdge) -> [f64; 2] {
    edge.proxy_domain.0.get()
}

fn edge_vertices(
    edge: &crate::brep::RawBrepEdge,
    resolved: &crate::brep::ResolvedEdge,
) -> [usize; 2] {
    if edge.proxy_reversed {
        [resolved.vertices[1], resolved.vertices[0]]
    } else {
        resolved.vertices
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

fn stage_brep_procedural_surface(
    procedural_admission: &mut cadmpeg_ir::document::procedural::ProceduralAdmission,
    staged: &mut BrepDraft,
    index: usize,
    geometry: cadmpeg_ir::geometry::nurbs::NurbsSurface,
    definition: crate::surfaces::DecodedProceduralSurface,
    context: &BrepStageContext<'_>,
) -> Result<cadmpeg_ir::ids::SurfaceId, crate::curves::GeometryError> {
    let key = IdentityKey::try_new(context.key.to_owned())
        .map_err(|error| crate::curves::GeometryError::unpositioned(error.to_string()))?;
    let definition = definition.into_definition(
        |child_index, _, child| {
            stage_curve_tree_indexed(
                procedural_admission,
                staged,
                child,
                &format!("surface-{index}.child-{child_index}"),
                &BrepStageContext {
                    key: key.as_str(),
                    ..*context
                },
            )
        },
        |error| crate::curves::GeometryError::unpositioned(error.to_string()),
    )?;
    let surface_id = {
        Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::SurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
            key.clone()
                .then(cadmpeg_ir::identity_key!(".slot-"))
                .then(index),
        ))
    }?;
    staged.draft.model_mut().surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(geometry)),
        source_object: Some(context.association.clone()),
    });
    let procedural_id = cadmpeg_ir::ids::ProceduralSurfaceId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "object", "procedural-surface"),
        key.then(cadmpeg_ir::identity_key!(".slot-")).then(index),
    );
    procedural_admission
        .add_surface(
            staged.draft.model_mut(),
            &surface_id,
            ProceduralSurface::new(procedural_id.clone(), definition, None),
        )?
        .map_err(|error| crate::curves::GeometryError::unpositioned(error.to_string()))?;
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

fn stage_curve_tree_indexed(
    procedural_admission: &mut cadmpeg_ir::document::procedural::ProceduralAdmission,
    staged: &mut BrepDraft,
    curve: crate::curves::DecodedCurve,
    path: &str,
    context: &BrepStageContext<'_>,
) -> Result<cadmpeg_ir::ids::CurveId, crate::curves::GeometryError> {
    let BrepStageContext {
        ctx,
        key,
        association,
        unknown,
    } = *context;
    let _nested = ctx.enter_nested("Rhino Brep curve tree")?;
    let (geometry, definition) = match curve {
        crate::curves::DecodedCurve::Leaf { geometry, .. } => (geometry, None),
        crate::curves::DecodedCurve::Compound {
            children,
            end_parameter,
            ..
        } => {
            let mut parameters = Vec::new();
            let mut components = Vec::new();
            for (index, (parameter, child)) in children.into_iter().enumerate() {
                let parameter = parameter.get();
                parameters.push(parameter);
                components.push(cadmpeg_ir::geometry::CompoundComponent {
                    parameter,
                    component: stage_curve_tree_indexed(
                        procedural_admission,
                        staged,
                        child,
                        &format!("{path}.component-{index}"),
                        context,
                    )?,
                });
            }
            parameters.push(end_parameter.get());
            (
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                    record: Some(unknown.clone()),
                }),
                Some(ProceduralCurveDefinition::Compound(
                    cadmpeg_ir::geometry::CompoundCurveConstruction::try_new(
                        parameters, components, None,
                    )
                    .map_err(crate::curves::GeometryError::unpositioned)?,
                )),
            )
        }
    };
    let key = IdentityKey::try_new(key.to_owned())
        .map_err(|error| crate::curves::GeometryError::unpositioned(error.to_string()))?;
    let id = {
        Ok::<_, crate::curves::GeometryError>(cadmpeg_ir::ids::CurveId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "curve"),
            if path == "root" {
                key.clone()
            } else {
                key.clone().then(cadmpeg_ir::identity_key!(".")).then(
                    IdentityKey::try_new(path.to_owned()).map_err(|error| {
                        crate::curves::GeometryError::unpositioned(error.to_string())
                    })?,
                )
            },
        ))
    }?;
    staged.draft.model_mut().curves.push(Curve {
        parameter_range: None,
        id: id.clone(),
        geometry,
        source_object: Some(association.clone()),
    });
    staged.draft.exactness(id.to_string(), Exactness::Derived);
    staged.links.push(id.to_string());
    if let Some(definition) = definition {
        let procedure_key = if path == "root" {
            key.clone()
        } else {
            key.clone().then(cadmpeg_ir::identity_key!(".")).then(
                IdentityKey::try_new(path.to_owned()).map_err(|error| {
                    crate::curves::GeometryError::unpositioned(error.to_string())
                })?,
            )
        };
        let procedure_id = cadmpeg_ir::ids::ProceduralCurveId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "procedural-curve"),
            procedure_key,
        );
        staged
            .draft
            .exactness(procedure_id.to_string(), Exactness::Derived);
        staged.links.push(procedure_id.to_string());
        procedural_admission
            .add_curve(
                staged.draft.model_mut(),
                &id,
                ProceduralCurve::new(procedure_id, definition),
            )?
            .map_err(|error| crate::curves::GeometryError::unpositioned(error.to_string()))?;
    }
    Ok(id)
}

struct DecodedPcurves {
    ids: HashMap<usize, cadmpeg_ir::ids::PcurveId>,
    values: Vec<Pcurve>,
    warnings: Diagnostics,
}

fn decode_pcurves(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    archive: ArchiveVersion,
    raw: &crate::brep::RawBrep,
    resolved: &crate::brep::ResolvedBrep,
    key: &str,
    surfaces: &HashMap<usize, StagedBrepSurface>,
) -> Result<DecodedPcurves, crate::curves::GeometryError> {
    let mut ids = HashMap::new();
    let mut values = Vec::new();
    let mut warnings = Diagnostics::new();
    let key = match IdentityKey::try_new(key.to_owned()) {
        Ok(key) => key,
        Err(error) => {
            warnings.push(format_args!("Brep pcurve identity key is invalid: {error}"));
            return Ok(DecodedPcurves {
                ids,
                values,
                warnings,
            });
        }
    };

    let mut decoded_slots = HashMap::<usize, Option<NurbsCurve>>::new();
    for (index, trim) in raw.trims.iter().enumerate() {
        if trim.trim_type == crate::brep::RawTrimKind::PointOnSurface {
            continue;
        }
        let trim_refs = &resolved.trims[index];
        let Some(trim_curve) = trim_refs.curve else {
            continue;
        };
        let nurbs = if let Some(nurbs) = decoded_slots.get(&trim_curve) {
            let Some(nurbs) = nurbs else { continue };
            nurbs.clone()
        } else {
            let decoded = (|| -> Result<crate::curves::NurbsJoin, crate::curves::GeometryError> {
                let child = raw
                    .c2
                    .slots
                    .get(trim_curve)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| {
                        crate::curves::error(trim.source_range.start, "trim C2 slot missing")
                    })?;
                let decoded = crate::curves::decode_2d(
                    ctx,
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
                c2_curve_to_nurbs_join(ctx, curve, trim.source_range.start)
            })();
            match decoded {
                Ok(joined) => {
                    warnings.append_prefixed(joined.warnings, format_args!("trim {index}"));
                    let cached = joined.curve.clone();

                    decoded_slots.insert(trim_curve, Some(cached));
                    joined.curve
                }
                Err(error @ crate::curves::GeometryError::Codec(_)) => return Err(error),
                Err(error) => {
                    warnings.push_coded(
                        crate::loss::RhinoLossCode::TrimPcurveDropped,
                        format_args!("trim {index} C2 omitted: {error}"),
                    );

                    decoded_slots.insert(trim_curve, None);
                    continue;
                }
            }
        };
        let plane_parameterization = resolved
            .loops
            .get(trim_refs.loop_index)
            .and_then(|loop_record| resolved.faces.get(loop_record.face))
            .and_then(|face| surfaces.get(&face.surface))
            .and_then(|surface| surface.plane_parameterization);
        let map_point = |point: FinitePoint3| {
            let point = point.get();
            let point = Point2::new(point.x, point.y);
            FinitePoint2::new(plane_parameterization.map_or(point, |map| map.map_point(point)))
        };
        let mut invalid_point = false;
        let poles = match nurbs.pole_rows() {
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
                let mut mapped = Vec::new();
                for point in points {
                    let Some(point) = map_point(*point) else {
                        invalid_point = true;
                        break;
                    };
                    mapped.push(point);
                }
                PcurveNurbsPoles::Polynomial { points: mapped }
            }
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
                let mut mapped = Vec::new();
                for pole in points {
                    let Some(point) = map_point(pole.point) else {
                        invalid_point = true;
                        break;
                    };
                    mapped.push(WeightedPole2 {
                        point,
                        weight: pole.weight,
                    });
                }
                PcurveNurbsPoles::Rational { points: mapped }
            }
        };
        if invalid_point {
            warnings.push(format_args!("trim {index} C2 has an invalid NURBS shape: control_points contains a non-finite point"));
            continue;
        }
        let id = {
            Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::PcurveId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "pcurve"),
                key.clone()
                    .then(cadmpeg_ir::identity_key!(".trim-"))
                    .then(index),
            ))
        }?;
        let knots = nurbs.knots().clone();
        let nurbs = match PcurveNurbs::new(nurbs.degree(), knots, poles, nurbs.periodic()) {
            Ok(nurbs) => nurbs,
            Err(error) => {
                warnings.push(format_args!(
                    "trim {index} C2 has an invalid NURBS shape: {error}"
                ));
                continue;
            }
        };

        values.push(Pcurve {
            id: id.clone(),
            geometry: PcurveGeometry::Nurbs { nurbs },
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                Some(trim.proxy_reversed),
                Some(trim.domain.0),
                trim_refs.tolerances[0].fit(),
            ),
        });

        ids.insert(index, id);
    }
    Ok(DecodedPcurves {
        ids,
        values,
        warnings,
    })
}

fn c2_curve_to_nurbs_join(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    curve: crate::curves::DecodedCurve,
    offset: usize,
) -> Result<crate::curves::NurbsJoin, crate::curves::GeometryError> {
    match curve {
        crate::curves::DecodedCurve::Leaf { geometry, .. } => match geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                Ok(crate::curves::NurbsJoin {
                    curve: nurbs,
                    warnings: Diagnostics::new(),
                })
            }
            _ => Err(crate::curves::error(
                offset,
                "C2 child has no parameter-space representation",
            )),
        },
        crate::curves::DecodedCurve::Compound {
            children,
            end_parameter,
            ..
        } => {
            let mut segments = Vec::new();
            let mut warnings = Diagnostics::new();
            let mut children = children.into_iter().peekable();
            while let Some((start, child)) = children.next() {
                let end = children.peek().map_or(end_parameter, |(start, _)| *start);
                let target = [start, end];
                if target[0] >= target[1] {
                    return Err(crate::curves::error(
                        offset,
                        "C2 polycurve segment domain is invalid",
                    ));
                }
                let mut joined = c2_curve_to_nurbs_join(ctx, child, offset)?;
                warnings.append(&mut joined.warnings);
                segments.push(crate::curves::remap_nurbs_domain(
                    joined.curve,
                    target,
                    offset,
                )?);
            }
            let mut joined = crate::curves::join_nurbs_segments(ctx, segments, offset)?;
            warnings.append(&mut joined.warnings);
            joined.warnings = warnings;
            Ok(joined)
        }
    }
}

fn scaled_tolerance(
    value: crate::brep::BrepTolerance,
    scale: MillimeterScale,
) -> Result<Option<cadmpeg_ir::scalar::PositiveReal>, crate::curves::GeometryError> {
    let Some(source) = value.positive() else {
        return Ok(None);
    };
    let scaled = crate::wire::scaled_coordinate(source.get(), scale)
        .ok_or_else(|| crate::curves::GeometryError::unpositioned("scaled tolerance is invalid"))?;
    Ok(Some(
        cadmpeg_ir::scalar::PositiveReal::from_finite(scaled).ok_or_else(|| {
            crate::curves::GeometryError::unpositioned(
                "scaled tolerance must be positive and finite",
            )
        })?,
    ))
}

fn face_components(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    resolved: &crate::brep::ResolvedBrep,
) -> Result<Vec<usize>, crate::curves::GeometryError> {
    let mut parent = ctx.alloc_filled(resolved.faces.len(), 0usize, "Rhino Brep face parents")?;
    for (index, value) in parent.iter_mut().enumerate() {
        *value = index;
    }
    for edge in &resolved.edges {
        let mut faces = ctx.alloc_filled(edge.trims.len(), 0usize, "Rhino Brep edge faces")?;
        for (face, trim) in faces.iter_mut().zip(&edge.trims) {
            *face = resolved.loops[resolved.trims[*trim].loop_index].face;
        }
        for pair in faces.windows(2) {
            let left = disjoint_root(&mut parent, pair[0]);
            let right = disjoint_root(&mut parent, pair[1]);
            parent[left] = right;
        }
    }
    let mut roots = ctx.alloc_filled(parent.len(), 0usize, "Rhino Brep face roots")?;
    for (index, root) in roots.iter_mut().enumerate() {
        *root = disjoint_root(&mut parent, index);
    }
    let mut labels = ctx.alloc_filled(parent.len(), None, "Rhino Brep face labels")?;
    let mut components = ctx.alloc_filled(parent.len(), 0usize, "Rhino Brep face components")?;
    let mut next = 0;
    for (component, root) in components.iter_mut().zip(roots) {
        *component = *labels[root].get_or_insert_with(|| {
            let label = next;
            next += 1;
            label
        });
    }
    Ok(components)
}

fn brep_free_vertex_indices(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    resolved: &crate::brep::ResolvedBrep,
) -> Result<Vec<usize>, crate::curves::GeometryError> {
    let mut attached = ctx.alloc_filled(
        resolved.vertices.len(),
        false,
        "Rhino Brep free-vertex attachment flags",
    )?;
    for (index, vertex) in resolved.vertices.iter().enumerate() {
        if !vertex.edges.is_empty() {
            attached[index] = true;
        }
    }
    for trim in &resolved.trims {
        if trim.edge.is_none() {
            attached[trim.vertices[0]] = true;
        }
    }
    let mut free = Vec::new();
    for (index, attached) in attached.into_iter().enumerate() {
        if !attached {
            free.push(index);
        }
    }
    Ok(free)
}

struct ShellGrouping {
    face_groups: Vec<usize>,
    shells: Vec<ShellGroup>,
    fallback: bool,
}

struct ShellGroup {
    region: usize,
    faces: Vec<usize>,
}

fn region_shell_groups(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    raw: &crate::brep::RawBrep,
    resolved: &crate::brep::ResolvedBrep,
    components: &[usize],
) -> Result<ShellGrouping, crate::curves::GeometryError> {
    if raw.minor < 3 || raw.regions.is_empty() {
        let mut face_groups =
            ctx.alloc_filled(components.len(), 0usize, "Rhino Brep fallback face groups")?;
        let mut groups = HashMap::new();
        for (face, component) in components.iter().copied().enumerate() {
            push_group_face(&mut groups, component, face);
        }
        let groups = ordered_group_faces(groups);
        let mut shells = Vec::new();
        for (group, (_component, faces)) in groups.into_iter().enumerate() {
            for face in &faces {
                face_groups[*face] = group;
            }
            shells.push(ShellGroup {
                region: group,
                faces,
            });
        }
        return Ok(ShellGrouping {
            face_groups,
            shells,
            fallback: false,
        });
    }
    let mut face_groups =
        ctx.alloc_filled(components.len(), 0usize, "Rhino Brep region face groups")?;
    let mut grouped = HashMap::new();
    for face in 0..raw.faces.len() {
        let mut bounded_region = None;
        let mut bounded_count = 0;
        for region in resolved
            .face_sides
            .iter()
            .filter(|side| side.face == face)
            .filter_map(|side| side.region)
            .filter(|region| {
                raw.regions
                    .get(*region)
                    .is_some_and(|item| item.region_type == 1)
            })
        {
            bounded_region = Some(region);
            bounded_count += 1;
        }
        if bounded_count != 1 {
            return region_shell_groups_without_records(ctx, components);
        }
        if let Some(region) = bounded_region {
            push_group_face(&mut grouped, (region, components[face]), face);
        }
    }
    let grouped = ordered_group_faces(grouped);
    let mut shells = Vec::new();
    for (group, ((region, _component), faces)) in grouped.into_iter().enumerate() {
        for face in &faces {
            face_groups[*face] = group;
        }
        shells.push(ShellGroup { region, faces });
    }
    Ok(ShellGrouping {
        face_groups,
        shells,
        fallback: false,
    })
}

fn region_shell_groups_without_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    components: &[usize],
) -> Result<ShellGrouping, crate::curves::GeometryError> {
    let mut face_groups =
        ctx.alloc_filled(components.len(), 0usize, "Rhino Brep incidence face groups")?;
    let mut groups = HashMap::new();
    for (face, component) in components.iter().copied().enumerate() {
        push_group_face(&mut groups, component, face);
    }
    let groups = ordered_group_faces(groups);
    let mut shells = Vec::new();
    for (group, (_component, faces)) in groups.into_iter().enumerate() {
        for face in &faces {
            face_groups[*face] = group;
        }
        shells.push(ShellGroup {
            region: group,
            faces,
        });
    }
    Ok(ShellGrouping {
        face_groups,
        shells,
        fallback: true,
    })
}

fn push_group_face<K: Eq + std::hash::Hash>(
    groups: &mut HashMap<K, Vec<usize>>,
    key: K,
    face: usize,
) {
    let faces = groups.entry(key).or_default();

    faces.push(face);
}

fn ordered_group_faces<K: Ord>(groups: HashMap<K, Vec<usize>>) -> Vec<(K, Vec<usize>)> {
    let mut ordered: Vec<_> = groups.into_iter().collect();
    ordered.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    ordered
}

fn disjoint_root(parent: &mut [usize], mut value: usize) -> usize {
    while parent[value] != value {
        parent[value] = parent[parent[value]];
        value = parent[value];
    }
    value
}

fn append_curve_warnings<P: std::fmt::Display + Copy>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    destination: &mut Diagnostics,
    curve: &crate::curves::DecodedCurve,
    prefix: P,
) -> Result<(), cadmpeg_core::CodecError> {
    let _nested = ctx.enter_nested("Rhino curve warning tree")?;
    for warning in curve.warnings() {
        destination.push_coded(warning.code, format_args!("{prefix}: {}", warning.message));
    }
    if let crate::curves::DecodedCurve::Compound { children, .. } = curve {
        for (_, child) in children {
            append_curve_warnings(ctx, destination, child, prefix)?;
        }
    }
    Ok(())
}

struct CurveCommitSource<'a> {
    key: &'a str,
    association: &'a SourceObjectAssociation,
    record: Option<UnknownId>,
    path: &'a str,
}

fn commit_curve_tree(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut impl ExactnessTarget,
    curve: crate::curves::DecodedCurve,
    source: CurveCommitSource<'_>,
) -> Result<cadmpeg_ir::ids::CurveId, CandidateError> {
    let mut procedural_admission =
        cadmpeg_ir::document::procedural::ProceduralAdmission::new(&ir.model);
    commit_curve_tree_indexed(
        ctx,
        &mut procedural_admission,
        ir,
        annotations,
        curve,
        source,
    )
}

fn commit_curve_tree_indexed(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    procedural_admission: &mut cadmpeg_ir::document::procedural::ProceduralAdmission,
    ir: &mut CadIr,
    annotations: &mut impl ExactnessTarget,
    curve: crate::curves::DecodedCurve,
    source: CurveCommitSource<'_>,
) -> Result<cadmpeg_ir::ids::CurveId, CandidateError> {
    let _nested = ctx.enter_nested("Rhino committed curve tree")?;
    let (geometry, definition) = match curve {
        crate::curves::DecodedCurve::Leaf { geometry, .. } => (geometry, None),
        crate::curves::DecodedCurve::Compound {
            children,
            end_parameter,
            ..
        } => {
            let mut parameters = Vec::new();
            let mut components = Vec::new();
            for (index, (parameter, child)) in children.into_iter().enumerate() {
                let parameter = parameter.get();
                parameters.push(parameter);
                let child_path = format!("{}.component-{index}", source.path);
                components.push(cadmpeg_ir::geometry::CompoundComponent {
                    parameter,
                    component: commit_curve_tree_indexed(
                        ctx,
                        procedural_admission,
                        ir,
                        annotations,
                        child,
                        CurveCommitSource {
                            key: source.key,
                            association: source.association,
                            record: None,
                            path: &child_path,
                        },
                    )?,
                });
            }
            parameters.push(end_parameter.get());
            (
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                    record: source.record,
                }),
                Some(ProceduralCurveDefinition::Compound(
                    cadmpeg_ir::geometry::CompoundCurveConstruction::try_new(
                        parameters, components, None,
                    )
                    .map_err(str::to_owned)?,
                )),
            )
        }
    };
    let key = IdentityKey::try_new(source.key.to_owned()).map_err(|error| error.to_string())?;

    let curve_key = if source.path == "root" {
        key.clone()
    } else {
        key.clone()
            .then(cadmpeg_ir::identity_key!("."))
            .then(IdentityKey::try_new(source.path.to_owned()).map_err(|error| error.to_string())?)
    };
    let id = {
        Ok::<_, cadmpeg_core::CodecError>(cadmpeg_ir::ids::CurveId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "curve"),
            curve_key.clone(),
        ))
    }?;
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: id.clone(),
        geometry,
        source_object: Some(source.association.clone()),
    });
    set_exactness(annotations, &id, Exactness::Derived);
    if let Some(definition) = definition {
        let procedure_id = cadmpeg_ir::ids::ProceduralCurveId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "procedural-curve"),
            curve_key,
        );
        procedural_admission
            .add_curve(
                &mut ir.model,
                &id,
                ProceduralCurve::new(procedure_id, definition),
            )?
            .map_err(|error| error.to_string())?;
    }
    Ok(id)
}

fn hatch_loop_ids(
    key: &str,
    kinds: impl Iterator<Item = crate::hatch::LoopKind>,
) -> Vec<(crate::hatch::LoopKind, String)> {
    kinds
        .enumerate()
        .map(|(index, kind)| (kind, format!("rhino:object:curve#{key}.hatch-loop-{index}")))
        .collect()
}

fn hatch_source_links(
    loop_ids: Vec<(crate::hatch::LoopKind, String)>,
    feature_id: &cadmpeg_ir::features::FeatureId,
) -> Vec<String> {
    let mut links: Vec<String> = loop_ids.into_iter().map(|(_, id)| id).collect();
    links.push(feature_id.as_str().to_owned());
    links
}

/// The hatch plane's placement, scaled into millimetres.
///
/// Both the plane axes and `scale` come off the document, so a scale that
/// drives a coefficient non-finite is a source the transform carrier refuses,
/// not an impossible state. `record` names the hatch the plane came from.
fn hatch_plane_transform(
    plane: &crate::settings::Plane,
    scale: MillimeterScale,
    record: &str,
) -> Result<Transform, cadmpeg_core::CodecError> {
    let origin = plane.origin.get();
    let x = plane.xaxis.get();
    let y = plane.yaxis.get();
    let z = plane.zaxis.get();
    let scale = scale.value();
    let rows = [
        [x[0] * scale, y[0] * scale, z[0] * scale, origin[0] * scale],
        [x[1] * scale, y[1] * scale, z[1] * scale, origin[1] * scale],
        [x[2] * scale, y[2] * scale, z[2] * scale, origin[2] * scale],
    ];
    Transform::affine(rows).ok_or_else(|| {
        let offending = rows
            .iter()
            .flatten()
            .copied()
            .find(|value| !value.is_finite())
            .unwrap_or(f64::NAN);
        cadmpeg_core::CodecError::malformed(format!(
            "{record}: the hatch plane scaled by {scale} states the non-finite \
             transform coefficient {offending}"
        ))
    })
}

fn transform_decoded_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    curve: &mut crate::curves::DecodedCurve,
    transform: Transform,
) -> Result<(), ReferenceFailure> {
    match curve {
        crate::curves::DecodedCurve::Compound { children, .. } => {
            for (_, child) in children {
                transform_decoded_curve(ctx, child, transform)?;
            }
            Ok(())
        }
        crate::curves::DecodedCurve::Leaf {
            geometry,
            warnings: _,
        } => {
            let source = std::mem::replace(
                geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            );
            let mut carrier = Curve {
                parameter_range: None,
                id: cadmpeg_ir::ids::CurveId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "hatch", "curve"),
                    cadmpeg_ir::identity_key!("placement"),
                ),
                geometry: source,
                source_object: None,
            };
            transform_curve(ctx, &mut carrier, transform)?;
            *geometry = carrier.geometry;
            Ok(())
        }
    }
}

const NON_FINITE_PLACEMENT: &str = "instance transform produced a non-finite coordinate";

/// Places a point, refusing a placement the transform sends out of the finite
/// range.
fn placed_point(transform: Transform, point: Point3) -> Result<Point3, String> {
    transform
        .apply_point(point)
        .map(FinitePoint3::get)
        .ok_or_else(|| NON_FINITE_PLACEMENT.to_string())
}

/// Places an admitted point, refusing a placement the transform sends out of
/// the finite range. The placed point stays admitted.
fn placed_finite_point(transform: Transform, point: FinitePoint3) -> Result<FinitePoint3, String> {
    transform
        .apply_point(point.get())
        .ok_or_else(|| NON_FINITE_PLACEMENT.to_string())
}

fn transform_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    curve: &mut Curve,
    transform: Transform,
) -> Result<(), ReferenceFailure> {
    let geometry = std::mem::replace(
        &mut curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
    );
    curve.geometry = match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(mut nurbs)) => {
            if let Err(message) = nurbs.try_map_control_points(|_, pole| {
                transform
                    .apply_point(pole.get())
                    .ok_or("instance control point transform produced a non-finite coordinate")
            }) {
                return Err(ReferenceFailure::Semantic(message.to_owned()));
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let decoded = crate::curves::DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)),
                Diagnostics::new(),
            );
            let mut nurbs =
                crate::curves::exact_nurbs(ctx, &decoded, 0).map_err(|error| match error {
                    crate::curves::GeometryError::Codec(error) => ReferenceFailure::Codec(error),
                    other => ReferenceFailure::Semantic(format!(
                        "analytic instance curve conversion failed: {other}"
                    )),
                })?;
            if let Err(message) = nurbs.try_map_control_points(|_, pole| {
                transform
                    .apply_point(pole.get())
                    .ok_or("instance control point transform produced a non-finite coordinate")
            }) {
                return Err(ReferenceFailure::Semantic(message.to_owned()));
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin();
            let direction = *line_curve.direction().as_raw();
            let transformed_origin = placed_finite_point(transform, origin)?;
            let endpoint = placed_point(
                transform,
                Point3::new(
                    origin.x + direction.x,
                    origin.y + direction.y,
                    origin.z + direction.z,
                ),
            )?;
            let value = cadmpeg_ir::math::Vector3::new(
                endpoint.x - transformed_origin.x,
                endpoint.y - transformed_origin.y,
                endpoint.z - transformed_origin.z,
            );
            let norm = PositiveReal::new(value.norm())
                .ok_or_else(|| "instance line transform collapsed its direction".to_string())?;
            CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::new(
                    transformed_origin,
                    UnitVector3::normalized_with_admitted_length(value, norm)
                        .ok_or_else(|| "LineCurve.direction must have unit length".to_string())?,
                ),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(degenerate_curve)) => {
            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
                cadmpeg_ir::geometry::analytic::DegenerateCurve::new(placed_finite_point(
                    transform,
                    degenerate_curve.point(),
                )?),
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record }) => {
            curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record });
            return Err("unknown free curve cannot be transformed exactly"
                .to_string()
                .into());
        }
        other => {
            curve.geometry = other;
            return Err(
                "analytic curve family has no exact general-affine instance conversion"
                    .to_string()
                    .into(),
            );
        }
    };
    Ok(())
}

fn transform_surface(surface: &mut Surface, transform: Transform) -> Result<(), ReferenceFailure> {
    let geometry = std::mem::replace(
        &mut surface.geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
    );
    surface.geometry = match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(mut nurbs)) => {
            if let Err(message) = nurbs.try_map_control_points(|_, pole| {
                transform
                    .apply_point(pole.get())
                    .ok_or("instance control point transform produced a non-finite coordinate")
            }) {
                return Err(ReferenceFailure::Semantic(message.to_owned()));
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let source_origin = plane_surface.origin().get();
            let u_axis = *plane_surface.frame().reference().as_raw();
            let origin = placed_finite_point(transform, plane_surface.origin())?;
            let unit_normal = transform
                .apply_unit_normal(*plane_surface.frame().axis())
                .ok_or_else(|| {
                    "instance plane normal transform could not produce a finite unit normal"
                        .to_string()
                })?;
            let normal = *unit_normal.as_raw();
            let endpoint = placed_point(
                transform,
                Point3::new(
                    source_origin.x + u_axis.x,
                    source_origin.y + u_axis.y,
                    source_origin.z + u_axis.z,
                ),
            )?;
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
            let length = PositiveReal::new(value.norm())
                .ok_or_else(|| "instance plane transform collapsed its frame".to_string())?;
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::new(
                    origin,
                    UnitVector3::normalized_with_admitted_length(value, length)
                        .and_then(|u_axis| OrthonormalFrame3::from_units(unit_normal, u_axis))
                        .ok_or_else(|| {
                            "PlaneSurface.normal/u_axis must form an orthonormal frame".to_string()
                        })?,
                ),
            ))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record }) => {
            surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record });
            return Err("unknown free surface cannot be transformed exactly"
                .to_string()
                .into());
        }
        other => {
            surface.geometry = other;
            return Err(
                "analytic surface family has no exact general-affine instance conversion"
                    .to_string()
                    .into(),
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
) -> Result<SourceObjectAssociation, cadmpeg_core::CodecError> {
    let object_id = format!("{}", identity.object_id);
    let object_id = cadmpeg_core::text::NonBlankString::new(object_id)
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("Rhino object UUID is blank"))?;
    let name = (!identity.name.is_empty()).then(|| identity.name.clone());
    let layer = identity.layer.as_ref().map(|layer| match layer.id {
        Some(id) => format!("{id}"),
        None => layer.name.clone(),
    });
    let mut admitted_path = Vec::new();
    for segment in instance_path {
        admitted_path.push(segment.to_owned());
    }
    Ok(SourceObjectAssociation {
        geometry_role: None,
        format: cadmpeg_ir::CodecFormat::Rhino,
        object_id,
        name,
        color: identity.effective_color.map(color).or(parent_color),
        visible: Some(parent_visible.unwrap_or(true) && identity.effective_visible),
        layer,
        instance_path: admitted_path,
    })
}

fn color(value: [u8; 4]) -> Color {
    Color::from_rgba8(value[0], value[1], value[2], value[3]).invert_alpha()
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

fn loss_provenance(
    class: &str,
    outcome: &ClassOutcome<'_>,
) -> cadmpeg_ir::Provenance<cadmpeg_ir::SourceLocation> {
    let tag = format!(
        "OBJECT_RECORD/class={class}/type=0x{:08x}",
        outcome
            .first_object
            .framed()
            .map_or(0, |object| object.object_type)
    );
    SourceProvenance::root(
        "rhino",
        cadmpeg_core::decode::u64_from_index(outcome.first_object.range().start),
    )
    .with_tag(tag)
}

/// Builds the metadata-only Rhino decode transaction.
pub(crate) fn decode(
    scan: &Scan<'_>,
    expand: crate::mesh::MeshExpand<'_>,
) -> Result<Decoded, cadmpeg_core::CodecError> {
    let mut context = DecodeContext::new(scan, expand)?;
    context.decode_geometry()?;
    context.decode_dimensions()?;
    context.retain_unbound_history_geometry()?;
    let geometry_context = context.neutral_scale().map(|scale| {
        (
            expand,
            scan.archive,
            scan.metadata.properties.writer_version,
            scale,
        )
    });
    let mut history_warnings = Diagnostics::new();
    let untyped = context.validate_candidate_fallible(|candidate, _annotations| {
        crate::history::project(
            expand.ctx(),
            &scan.history,
            geometry_context,
            candidate,
            &mut history_warnings,
        )
    });
    context.report.phase_warnings.append(&mut history_warnings);
    match untyped {
        Ok((0, 0, 0, 0)) => {}
        Ok((untyped, failed, dropped_dependencies, redundant_repairs)) => {
            if untyped != 0 {
                context.report.typed_losses.push(
                    RhinoLossCode::HistoryGeometryNotTransferred.note(format_args!(
                        "{untyped} history value(s) decoded without a neutral carrier"
                    )),
                );
            }
            if failed != 0 {
                context.report.typed_losses.push(
                    RhinoLossCode::HistoryEmbeddedGeometryDropped.note(format_args!(
                        "{failed} embedded history geometry value(s) could not be decoded"
                    )),
                );
            }
            if dropped_dependencies != 0 {
                context.report.typed_losses.push(RhinoLossCode::HistoryDependencyDropped.note(format_args!("{dropped_dependencies} history dependency edge(s) point to later or ambiguous producers")));
            }
            if redundant_repairs != 0 {
                context
                    .report
                    .typed_losses
                    .push(RhinoLossCode::RedundantFieldRepaired.note(format_args!(
                        "{redundant_repairs} history geometry optional channel repair(s)"
                    )));
            }
        }
        Err(CandidateError::Codec(error)) => return Err(error),
        Err(error) => context.scan_warnings_for_class(
            "history",
            format_args!("history projection rejected atomically by IR validation: {error}"),
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
pub(crate) fn decode_for_test(scan: &Scan<'_>) -> cadmpeg_ir::codec::DecodeResult {
    with_expand(scan, |expand| {
        seal_for_test(
            decode(scan, expand).expect("decode install invariants"),
            false,
        )
    })
}

#[cfg(test)]
pub(crate) fn seal_for_test(
    decoded: Decoded,
    container_only: bool,
) -> cadmpeg_ir::codec::DecodeResult {
    use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, FormatId};

    #[derive(Clone)]
    struct TestBackend(Decoded);

    impl CodecBackend for TestBackend {
        const FORMAT: FormatId = FormatId::new(crate::dialect::FORMAT);

        fn detect_impl(
            &self,
            _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
            _prefix: cadmpeg_core::decode::View<'_>,
        ) -> Result<Confidence, cadmpeg_core::CodecError> {
            Ok(Confidence::High)
        }

        fn inspect_impl(
            &self,
            _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
            _root: cadmpeg_core::decode::View<'_>,
        ) -> Result<cadmpeg_ir::ContainerSummary, cadmpeg_core::CodecError> {
            unreachable!("test backend is decode-only")
        }

        fn decode_impl(
            &self,
            _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
            _root: cadmpeg_core::decode::View<'_>,
        ) -> Result<Decoded, cadmpeg_core::CodecError> {
            Ok(self.0.clone())
        }
    }

    Codec::decode(
        &TestBackend(decoded),
        &mut std::io::Cursor::new(Vec::<u8>::new()),
        &DecodeOptions {
            container_only,
            ..DecodeOptions::default()
        },
    )
    .expect("test decode result satisfies the sealed codec contract")
}

/// Admits an archive tolerance with a recorded default repair.
pub(crate) fn admitted_tolerance<T: Copy + Into<f64>>(
    admitted: Option<T>,
    value: f64,
    default: T,
    field: &str,
    losses: &mut Vec<LossNote>,
) -> T {
    let Some(value_admitted) = admitted else {
        losses.push(RhinoLossCode::RedundantFieldRepaired.note(format_args!(
            "{field} tolerance {value} replaced with default {}",
            default.into()
        )));
        return default;
    };
    value_admitted
}

fn build_ir(scan: &Scan<'_>) -> CadIr {
    let mut ir = CadIr::empty();
    if let Some(source_units) = &scan.metadata.settings.units {
        if let Some(linear) = source_units.absolute_tolerance_millimeters() {
            ir.tolerances.linear = linear;
        }
        ir.tolerances.angular = source_units.angular_tolerance;
    }
    ir
}

fn insert_full_source_attribute(
    attributes: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: std::fmt::Arguments<'_>,
    value: std::fmt::Arguments<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let key = key.to_string();
    let key = cadmpeg_core::text::NonBlankString::new(key).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("generated Rhino source attribute key is blank")
    })?;
    let value = value.to_string();
    attributes.insert(key, value);
    Ok(())
}

struct LayerAttributePrefix {
    index: i32,
    duplicate: Option<(usize, usize)>,
}

impl std::fmt::Display for LayerAttributePrefix {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "layer.{}", self.index)?;
        if let Some((occurrence, offset)) = self.duplicate {
            write!(formatter, ".record-{occurrence:06}-offset-{offset}")?;
        }
        Ok(())
    }
}

/// Builds the path-specific facts available after full decoding.
fn full_source_attributes(
    scan: &Scan<'_>,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, cadmpeg_core::CodecError> {
    let mut attributes = BTreeMap::new();
    macro_rules! attribute {
        ($key:literal, $value:expr) => {
            insert_full_source_attribute(
                &mut attributes,
                format_args!($key),
                format_args!("{}", $value),
            )?
        };
    }
    let settings = &scan.metadata.settings;
    if let Some(units) = &settings.units {
        attribute!("unit_value", units.unit.value());
        match &units.unit {
            crate::settings::UnitSystem::None => attribute!("unit_system", "none"),
            crate::settings::UnitSystem::Unset => attribute!("unit_system", "unset"),
            crate::settings::UnitSystem::Standard(value) => {
                attribute!("unit_system", format_args!("standard:{}", value.value()));
            }
            crate::settings::UnitSystem::Custom(unit) => {
                attribute!("unit_system", format_args!("custom:{}", unit.name()));
            }
        }
        if let crate::settings::UnitSystem::Custom(unit) = &units.unit {
            attribute!("custom_unit_name", unit.name());
            attribute!("custom_meters_per_unit", unit.meters_per_unit().get());
        }
        if let Some(scale) = units.millimeters_per_unit() {
            attribute!("millimeters_per_unit", scale);
        }
        attribute!("absolute_tolerance_native", units.absolute_tolerance.get());
        if let Some(value) = units.absolute_tolerance_millimeters() {
            attribute!("absolute_tolerance_millimeters", value.get());
        } else {
            attribute!("absolute_tolerance_millimeters", "unresolved");
        }
        attribute!("angular_tolerance", units.angular_tolerance.get());
        attribute!("relative_tolerance", units.relative_tolerance.get());
        if let Some(display) = units.distance_display {
            attribute!("distance_display_mode", display.mode);
            attribute!("distance_display_precision", display.precision);
        }
    }
    if let Some(application) = &scan.metadata.properties.application {
        attribute!("application_name", application.name);
        attribute!("application_url", application.url);
        attribute!("application_details", application.details);
    }
    if let Some(current) = settings.current_layer {
        attribute!("current_layer", current);
    }
    if let Some(current) = settings.current_material {
        attribute!("current_material", current.value);
        attribute!("current_material_source", current.source);
    }
    if let Some(current) = settings.current_color {
        attribute!(
            "current_color",
            format_args!(
                "{},{},{},{}",
                current.value[0], current.value[1], current.value[2], current.value[3]
            )
        );
        attribute!("current_color_source", current.source);
    }
    if let Some(current) = settings.current_wire_density {
        attribute!("current_wire_density", current);
    }
    if let Some(current) = settings.current_font {
        attribute!("current_font", current);
    }
    if let Some(current) = settings.current_dimstyle {
        attribute!("current_dimstyle", current);
    }
    if let Some(url) = &settings.model_url {
        attribute!("model_url", url);
    }
    let mut layer_index_counts = BTreeMap::<i32, usize>::new();
    for layer in &scan.metadata.layers {
        *layer_index_counts.entry(layer.index).or_default() += 1;
    }
    let mut layer_index_occurrences = BTreeMap::<i32, usize>::new();
    for layer in &scan.metadata.layers {
        let duplicate = if layer_index_counts.get(&layer.index) == Some(&1) {
            None
        } else {
            let occurrence = layer_index_occurrences.entry(layer.index).or_default();
            let current = *occurrence;
            *occurrence += 1;
            Some((current, layer.source.range.start))
        };
        let prefix = LayerAttributePrefix {
            index: layer.index,
            duplicate,
        };
        if duplicate.is_some() {
            attribute!("{prefix}.index", layer.index);
        }
        attribute!("{prefix}.name", layer.name);
        attribute!("{prefix}.visible", layer.visible);
        attribute!("{prefix}.locked", layer.locked);
        if let Some(id) = layer.id {
            attribute!("{prefix}.uuid", id);
        }
    }
    Ok(attributes)
}

#[cfg(test)]
mod tests;
