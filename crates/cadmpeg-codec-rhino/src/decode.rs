// SPDX-License-Identifier: Apache-2.0
//! Decode Rhino metadata and retain object records for later geometry phases.

use crate::loss::{Diagnostics, ScratchDiagnostics};
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
use cadmpeg_ir::hash::digest::Sha256Digest;
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

fn push_report_loss(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    code: RhinoLossCode,
    message: std::fmt::Arguments<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.reserve_vec(losses, 1, "Rhino typed decode losses")?;
    losses.push(crate::wire::admitted_loss(
        ctx,
        code,
        message,
        "Rhino typed decode loss message",
    )?);
    Ok(())
}

fn append_class_diagnostic(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    destination: &mut Diagnostics,
    class: crate::wire::Uuid,
    diagnostic: &crate::loss::RhinoDiagnostic,
) -> Result<(), cadmpeg_core::CodecError> {
    destination.push_coded_admitted(
        ctx,
        diagnostic.code,
        format_args!("{class}: {}", diagnostic.message),
    )
}

fn append_report_losses(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    destination: &mut Vec<LossNote>,
    source: crate::loss::ScratchVec<'_, LossNote>,
) -> Result<(), cadmpeg_core::CodecError> {
    source.append_admitted(ctx, destination, "Rhino typed decode losses")
}

fn instance_members_are_unique(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    members: &[crate::wire::Uuid],
) -> Result<bool, cadmpeg_core::CodecError> {
    let mut lookup_storage = ctx.reserve_scoped(0, "Rhino instance member lookup")?;
    let mut unique_members = BTreeSet::new();
    ctx.all_by(
        members,
        |member| {
            lookup_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut unique_members,
                    *member,
                    "Rhino instance unique members",
                )
            })
        },
        "Rhino instance members are unique traversal",
    )
}

fn insert_feature_property(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: std::fmt::Arguments<'_>,
    value: std::fmt::Arguments<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let value = ctx.format_retained(value, "Rhino feature property value")?;
    insert_feature_property_owned(ctx, properties, key, value)
}

fn insert_feature_property_owned(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    properties: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: std::fmt::Arguments<'_>,
    value: String,
) -> Result<(), cadmpeg_core::CodecError> {
    let key = ctx.format_retained(key, "Rhino feature property key")?;
    let key = cadmpeg_core::text::NonBlankString::for_decode(ctx, key, "validate nonblank text")?
        .ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("blank generated Rhino property key")
    })?;
    ctx.insert_btree_map(properties, key, value, "Rhino feature property entries")?;
    Ok(())
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

/// The lower of a session limit and a rhino ceiling.
///
/// [`ResourceLimits`](cadmpeg_core::decode::ResourceLimits) states its limits
/// in `u64`; the ceilings below count in-memory items and are `usize`. A
/// session limit the address space cannot name is above every ceiling, so the
/// ceiling is the answer in that case.
pub(crate) fn session_ceiling(limit: u64, ceiling: usize) -> usize {
    match usize::try_from(limit) {
        Ok(limit) => limit.min(ceiling),
        Err(_) => ceiling,
    }
}

struct InstanceRowSnapshot {
    status: Option<GeometryOutcome>,
    sorted_at_checkpoint: bool,
    additions: BTreeSet<String>,
}

struct InstanceJournal<'a> {
    rows: BTreeMap<usize, InstanceRowSnapshot>,
    storage: cadmpeg_core::decode::ScopedReservation<'a>,
    field_text_storage: cadmpeg_core::decode::ScopedReservation<'a>,
}

impl<'a> InstanceJournal<'a> {
    fn new(
        ctx: &'a cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            rows: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "Rhino instance touched rows")?,
            field_text_storage: ctx.reserve_scoped(0, "Rhino instance link field text")?,
        })
    }

    fn record_link(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        source_order: usize,
        link: &str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let row = ctx
            .get_mut_btree_map(
                &mut self.rows,
                &source_order,
                "Rhino instance added link row",
            )?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino instance row checkpoint is missing")
            })?;
        let key =
            ctx.copy_scoped_text(link, &mut self.storage, "Rhino instance added link copy")?;
        self.storage.with_storage(|| {
            ctx.insert_btree_set(&mut row.additions, key, "Rhino instance added links")
                .map(|_| ())
        })?;
        Ok(())
    }
}

struct InstanceLinks<'a> {
    values: Vec<String>,
    _backing: cadmpeg_core::decode::ScopedReservation<'a>,
}

#[derive(Default)]
struct SourceLinkIndex {
    links: BTreeSet<String>,
    sorted: bool,
    seeded: bool,
    pending: bool,
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

    fn charge(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        value: &mut usize,
        amount: usize,
        limit: usize,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let requested = value
            .checked_add(amount)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64_from_index(limit), u64::MAX))?;
        if requested > limit {
            return Err(ctx.refuse_codec_limit(
                operation,
                u64_from_index(limit),
                u64_from_index(requested),
            ));
        }
        *value = requested;
        Ok(())
    }

    fn reference(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        Self::charge(
            ctx,
            &mut self.references,
            1,
            self.limits[0],
            "Rhino instance reference limit",
        )
    }

    fn member(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        Self::charge(
            ctx,
            &mut self.members,
            1,
            self.limits[1],
            "Rhino instance member limit",
        )
    }

    fn entities(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        amount: usize,
    ) -> Result<(), cadmpeg_core::CodecError> {
        Self::charge(
            ctx,
            &mut self.entities,
            amount,
            self.limits[2],
            "Rhino instance entity limit",
        )
    }
}

#[derive(Debug)]
struct InstanceSelection<'ctx> {
    source_order: usize,
    key: IdentityKey,
    path: Vec<String>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
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

impl<'ctx> InstanceSelection<'ctx> {
    fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        source_order: usize,
        path: &[String],
        member: crate::wire::Uuid,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut copied = ctx.collect_scoped_texts(
            path.iter().map(String::as_str),
            "Rhino instance selection scratch",
        )?;
        let key = ctx.format_scoped_text(
            &mut copied.1,
            format_args!(
                "{}",
                InstanceKey {
                    path: &copied.0,
                    member
                }
            ),
            "Rhino instance selection key",
        )?;
        let key = IdentityKey::try_new(key).map_err(cadmpeg_core::CodecError::malformed)?;
        Ok(Self {
            source_order,
            key,
            path: copied.0,
            _storage: copied.1,
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
    unknown_record_storage: Option<cadmpeg_core::decode::ScopedReservation<'a>>,
    statuses: Vec<Option<GeometryOutcome>>,
    status_storage: Option<cadmpeg_core::decode::ScopedReservation<'a>>,
    retained_bytes: usize,
    retention_limits: [usize; 2],
    mesh_budget: crate::mesh::MeshBudget,
    geometry_transferred: bool,
    /// Transactional report buckets produced by semantic decode phases.
    report: ReportBuckets,
    instance_selection: Option<InstanceSelection<'a>>,
    instance_display: Option<InstanceDisplay>,
    instance_journal: Option<InstanceJournal<'a>>,
    link_indices: BTreeMap<usize, SourceLinkIndex>,
    pending_seeded_link_rows: Vec<usize>,
    pending_seeded_link_cursor: usize,
    link_index_storage: Option<cadmpeg_core::decode::ScopedReservation<'a>>,
    object_candidates: HashMap<crate::wire::Uuid, Vec<usize>>,
    definition_candidates: HashMap<crate::wire::Uuid, usize>,
    expansion_budget: ExpansionBudget,
    lookup_storage: std::rc::Rc<std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'a>>>,
}

impl<'a> DecodeContext<'a> {
    /// Starts a transaction from a completed Rhino scan.
    pub(crate) fn new(
        scan: &'a Scan<'a>,
        expand: crate::mesh::MeshExpand<'a>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let session = expand.ctx();
        let mut lookup_storage = session.reserve_scoped(0, "Rhino transaction lookup storage")?;
        let mut object_candidates = HashMap::new();
        let mut objects = scan.objects.iter();
        for source_order in 0..scan.objects.len() {
            let object = session
                .next_charged(&mut objects, "Rhino new traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino object source ended early")
                })?;
            if let Some(identity) = object.identity() {
                let positions = lookup_storage
                    .with_storage(|| {
                        session.entry_hash_map(
                            &mut object_candidates,
                            identity.object_id,
                            "Rhino object candidate keys",
                        )
                    })?
                    .or_default();
                lookup_storage.with_storage(|| {
                    session.reserve_vec(positions, 1, "Rhino object candidate positions")
                })?;
                positions.push(source_order);
            }
        }
        let mut definition_candidates = HashMap::new();
        let mut definitions = scan.definitions.definitions().iter();
        for index in 0..scan.definitions.definitions().len() {
            let definition = session
                .next_charged(&mut definitions, "Rhino new traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino definition source ended early")
                })?;
            let id = definition.id();
            lookup_storage.with_storage(|| {
                session.insert_hash_map(
                    &mut definition_candidates,
                    id,
                    index,
                    "Rhino definition candidate keys",
                )
            })?;
        }
        let report = ReportBuckets::default();
        let link_index_storage = Some(session.reserve_scoped(0, "Rhino source link index")?);
        let ir = build_ir(scan);
        let mut context = Self {
            scan,
            expand,
            session: cadmpeg_ir::draft::CommitSession::new(ir, session, Some("rhino"))?,
            annotations: cadmpeg_ir::Annotations::default(),
            opaque_records: Vec::new(),
            unknown_record_storage: None,
            statuses: Vec::new(),
            status_storage: None,
            retained_bytes: 0,
            retention_limits: [RETAINED_RECORD_CAP, RETAINED_DOCUMENT_CAP],
            mesh_budget: crate::mesh::MeshBudget::from_session(expand.ctx()),
            geometry_transferred: false,
            report,
            instance_selection: None,
            instance_display: None,
            instance_journal: None,
            link_indices: BTreeMap::new(),
            pending_seeded_link_rows: Vec::new(),
            pending_seeded_link_cursor: 0,
            link_index_storage,
            object_candidates,
            definition_candidates,
            expansion_budget: ExpansionBudget::new(),
            lookup_storage: std::rc::Rc::new(std::cell::RefCell::new(lookup_storage)),
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
    pub(crate) fn set_retention_limits(
        &mut self,
        record: usize,
        document: usize,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.retention_limits = [record, document];
        self.session.replace_unknowns(Vec::new())?;
        self.unknown_record_storage = None;
        self.opaque_records.clear();
        self.statuses.clear();
        self.retained_bytes = 0;
        self.link_indices.clear();
        drop(std::mem::take(&mut self.pending_seeded_link_rows));
        self.pending_seeded_link_cursor = 0;
        drop(self.link_index_storage.take());
        self.link_index_storage = Some(
            self.expand
                .ctx()
                .reserve_scoped(0, "Rhino source link index")?,
        );
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
        self.link_indices.remove(&source_order);
        self.pending_seeded_link_rows
            .retain(|pending| *pending != source_order);
        self.pending_seeded_link_cursor = 0;
        self.session
            .unknown_links_mut(source_order)
            .map(|(_, links)| links)
    }

    #[cfg(test)]
    fn unknown_count(&self) -> usize {
        self.session.unknowns().len()
    }

    fn checkpoint_instance_row(
        &mut self,
        source_order: usize,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let Some(journal) = &mut self.instance_journal else {
            return Ok(());
        };
        let ctx = self.expand.ctx();
        if ctx.contains_key_btree_map(
            &journal.rows,
            &source_order,
            "Rhino instance row checkpoint lookup",
        )? {
            return Ok(());
        }
        if self.session.unknowns().get(source_order).is_none() {
            return Ok(());
        }
        let status = self.statuses.get(source_order).copied().ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("Rhino instance status is missing")
        })?;
        let has_links = self
            .session
            .unknowns()
            .get(source_order)
            .map(|record| !record.links().is_empty())
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino instance source record is missing")
            })?;
        if has_links
            && !ctx.contains_key_btree_map(
                &self.link_indices,
                &source_order,
                "Rhino source link index row lookup",
            )?
        {
            let (_, links) = self
                .session
                .unknown_links_mut(source_order)
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino instance source record is missing")
                })?;
            Self::ensure_source_link_index(
                ctx,
                &mut self.link_indices,
                &mut self.link_index_storage,
                source_order,
                links,
            )?;
        }
        let sorted_at_checkpoint = ctx
            .get_btree_map(
                &self.link_indices,
                &source_order,
                "Rhino instance link order checkpoint",
            )?
            .is_none_or(|index| index.sorted);
        journal.storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut journal.rows,
                source_order,
                InstanceRowSnapshot {
                    status,
                    sorted_at_checkpoint,
                    additions: BTreeSet::new(),
                },
                "Rhino instance touched rows",
            )
        })?;
        Ok(())
    }

    fn rollback_instance_rows(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let journal = self.instance_journal.take().ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("Rhino instance journal is missing")
        })?;
        let ctx = self.expand.ctx();
        let row_count = journal.rows.len();
        let mut rows = journal.rows.into_iter();
        for _ in 0..row_count {
            let (position, row) = ctx
                .next_charged(&mut rows, "Rhino instance row rollback traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino instance journal ended early")
                })?;
            {
                let (_, links) = self.session.unknown_links_mut(position).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Rhino source record disappeared during instance expansion",
                    )
                })?;
                ctx.retain_vec(
                    links,
                    |link| {
                        Ok(!ctx.contains_btree_set(
                            &row.additions,
                            link.as_str(),
                            "Rhino instance rollback link lookup",
                        )?)
                    },
                    "Rhino instance rollback links",
                )?;
            }
            let index = ctx.get_mut_btree_map(
                &mut self.link_indices,
                &position,
                "Rhino source link rollback index",
            )?;
            if let Some(index) = index {
                let addition_count = row.additions.len();
                let mut additions = row.additions.into_iter();
                for _ in 0..addition_count {
                    let link = ctx
                        .next_charged(&mut additions, "Rhino source link rollback index traversal")?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Rhino instance link journal ended early",
                            )
                        })?;
                    ctx.remove_btree_set(
                        &mut index.links,
                        link.as_str(),
                        "Rhino source link rollback index removal",
                    )?;
                }
                index.sorted = row.sorted_at_checkpoint;
                if index.seeded && !index.sorted && !index.pending {
                    let storage = self.link_index_storage.as_mut().ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Rhino source link index storage is missing",
                        )
                    })?;
                    ctx.push_scoped_vec(
                        storage,
                        &mut self.pending_seeded_link_rows,
                        position,
                        "Rhino pending seeded source link rows",
                    )?;
                    index.pending = true;
                }
            }
            let status = self.statuses.get_mut(position).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(
                    "Rhino instance status disappeared during expansion",
                )
            })?;
            *status = row.status;
        }
        Ok(())
    }

    fn ensure_source_link_index(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        link_indices: &mut BTreeMap<usize, SourceLinkIndex>,
        link_index_storage: &mut Option<cadmpeg_core::decode::ScopedReservation<'a>>,
        source_order: usize,
        links: &mut Vec<String>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        if ctx.contains_key_btree_map(
            link_indices,
            &source_order,
            "Rhino source link index row lookup",
        )? {
            return Ok(());
        }

        if !ctx.is_sorted_by(
            &links[..],
            String::as_str,
            Ord::cmp,
            "Rhino source link order check",
        )? {
            ctx.sort_unstable_by(
                links,
                String::as_str,
                Ord::cmp,
                "Rhino unknown record links",
            )?;
        }
        let mut index = SourceLinkIndex {
            sorted: true,
            seeded: !links.is_empty(),
            ..SourceLinkIndex::default()
        };
        let storage = link_index_storage.as_mut().ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("Rhino source link index storage is missing")
        })?;
        ctx.fold(
            &links[..],
            (),
            |(), link| {
                if ctx.contains_btree_set(
                    &index.links,
                    link,
                    "Rhino source link index seed lookup",
                )? {
                    return Ok(());
                }
                let key = ctx.copy_scoped_text(link, storage, "Rhino source link index key")?;
                storage.with_storage(|| {
                    ctx.insert_btree_set(&mut index.links, key, "Rhino source link index entries")
                        .map(|_| ())
                })?;
                Ok(())
            },
            "Rhino source link index seed traversal",
        )?;
        storage.with_storage(|| {
            ctx.insert_btree_map(
                link_indices,
                source_order,
                index,
                "Rhino source link index rows",
            )
        })?;
        Ok(())
    }

    fn index_source_link(
        &mut self,
        source_order: usize,
        link: &str,
        remains_sorted: bool,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let ctx = self.expand.ctx();
        let (indices, pending, storage) = (
            &mut self.link_indices,
            &mut self.pending_seeded_link_rows,
            &mut self.link_index_storage,
        );
        let storage = storage.as_mut().ok_or_else(|| {
            cadmpeg_core::CodecError::malformed("Rhino source link index storage is missing")
        })?;
        let index = ctx
            .get_mut_btree_map(indices, &source_order, "Rhino source link index rows")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino source link index row is missing")
            })?;
        let key = ctx.copy_scoped_text(link, storage, "Rhino source link index key")?;
        storage.with_storage(|| {
            ctx.insert_btree_set(&mut index.links, key, "Rhino source link index entries")
                .map(|_| ())
        })?;
        let was_sorted = index.sorted;
        index.sorted &= remains_sorted;
        if index.seeded && was_sorted && !index.sorted && !index.pending {
            ctx.push_scoped_vec(
                storage,
                pending,
                source_order,
                "Rhino pending seeded source link rows",
            )?;
            index.pending = true;
        }
        Ok(())
    }

    fn set_source_link_order(
        &mut self,
        source_order: usize,
        sorted: bool,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let ctx = self.expand.ctx();
        let enqueue = ctx
            .get_mut_btree_map(
                &mut self.link_indices,
                &source_order,
                "Rhino source link order update",
            )?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino source link index row is missing")
            })
            .map(|index| {
                index.sorted = sorted;
                index.seeded && !sorted && !index.pending
            })?;
        if enqueue {
            let storage = self.link_index_storage.as_mut().ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino source link index storage is missing")
            })?;
            ctx.push_scoped_vec(
                storage,
                &mut self.pending_seeded_link_rows,
                source_order,
                "Rhino pending seeded source link rows",
            )?;
            let index = ctx
                .get_mut_btree_map(
                    &mut self.link_indices,
                    &source_order,
                    "Rhino source link order update",
                )?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino source link index row is missing")
                })?;
            index.pending = true;
        }
        Ok(())
    }

    fn flush_source_links(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let ctx = self.expand.ctx();
        let count = self.session.unknowns().len();
        let mut source_orders = 0..count;
        for _ in 0..count {
            let source_order = ctx
                .next_charged(&mut source_orders, "Rhino source link flush traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino source link row source ended early")
                })?;
            let index = ctx.get_btree_map(
                &self.link_indices,
                &source_order,
                "Rhino source link flush row lookup",
            )?;
            if index.is_some_and(|index| index.sorted) {
                continue;
            }
            let has_links = self
                .session
                .unknowns()
                .get(source_order)
                .is_some_and(|record| !record.links().is_empty());
            if !has_links {
                continue;
            }
            if index.is_none() {
                let (_, links) = self
                    .session
                    .unknown_links_mut(source_order)
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Rhino source record disappeared during link indexing",
                        )
                    })?;
                Self::ensure_source_link_index(
                    ctx,
                    &mut self.link_indices,
                    &mut self.link_index_storage,
                    source_order,
                    links,
                )?;
                continue;
            }
            {
                let (_, links) = self
                    .session
                    .unknown_links_mut(source_order)
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Rhino source record disappeared during link sorting",
                        )
                    })?;
                ctx.sort_unstable_by(
                    links,
                    String::as_str,
                    Ord::cmp,
                    "Rhino unknown record links",
                )?;
            }
            self.set_source_link_order(source_order, true)?;
        }
        Ok(())
    }

    fn flush_seeded_source_links(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let start = self.pending_seeded_link_cursor;
        let end = self.pending_seeded_link_rows.len();
        if start == end {
            return Ok(());
        }
        let ctx = self.expand.ctx();
        self.pending_seeded_link_cursor = end;
        let mut pending = start..end;
        for _ in start..end {
            let row = ctx
                .next_charged(&mut pending, "Rhino seeded source link flush traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino pending link queue ended early")
                })?;
            let source_order = self.pending_seeded_link_rows[row];
            let index = ctx
                .get_mut_btree_map(
                    &mut self.link_indices,
                    &source_order,
                    "Rhino seeded source link flush row lookup",
                )?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino source link index row is missing")
                })?;
            index.pending = false;
            let needs_sort = index.seeded && !index.sorted;
            if !needs_sort {
                continue;
            }
            let (_, links) = self
                .session
                .unknown_links_mut(source_order)
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Rhino seeded source record disappeared during link sorting",
                    )
                })?;
            ctx.sort_unstable_by(
                links,
                String::as_str,
                Ord::cmp,
                "Rhino unknown record links",
            )?;
            self.set_source_link_order(source_order, true)?;
        }
        Ok(())
    }

    fn append_source_link(
        &mut self,
        source_order: usize,
        link: &str,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let ctx = self.expand.ctx();
        let Some(record) = self.session.unknowns().get(source_order) else {
            return Ok(false);
        };
        let id = record.id().as_str();
        if ctx.equal(link, id, "Rhino source link equality")? {
            return Ok(false);
        }
        let (already_present, was_sorted) = if let Some(index) = ctx.get_btree_map(
            &self.link_indices,
            &source_order,
            "Rhino source link index row lookup",
        )? {
            (
                ctx.contains_btree_set(&index.links, link, "Rhino source link index lookup")?,
                index.sorted,
            )
        } else {
            let (_, links) = self
                .session
                .unknown_links_mut(source_order)
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Rhino source record disappeared during link indexing",
                    )
                })?;
            Self::ensure_source_link_index(
                ctx,
                &mut self.link_indices,
                &mut self.link_index_storage,
                source_order,
                links,
            )?;
            let index = ctx
                .get_btree_map(
                    &self.link_indices,
                    &source_order,
                    "Rhino source link index row lookup",
                )?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino source link index row is missing")
                })?;
            (
                ctx.contains_btree_set(&index.links, link, "Rhino source link index lookup")?,
                index.sorted,
            )
        };
        if already_present {
            return Ok(true);
        }
        let links = self
            .session
            .unknowns()
            .get(source_order)
            .map(UnknownRecord::links)
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(
                    "Rhino source record disappeared during link append",
                )
            })?;
        let position = links.len();
        let remains_sorted = if !was_sorted || position == 0 {
            was_sorted
        } else {
            let previous = links.last().ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino source link position is invalid")
            })?;
            !ctx.compare(previous.as_str(), link, "Rhino source link order check")?
                .is_gt()
        };

        {
            let (_, links) = self
                .session
                .unknown_links_mut(source_order)
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Rhino source record disappeared during link append",
                    )
                })?;
            ctx.reserve_vec(links, 1, "Rhino unknown record links")?;
        }
        let copy = if let Some(journal) = &mut self.instance_journal {
            let copy = ctx.copy_scoped_text(
                link,
                &mut journal.field_text_storage,
                "Rhino unknown record link copy",
            )?;
            journal.record_link(ctx, source_order, link)?;
            self.index_source_link(source_order, link, remains_sorted)?;
            copy
        } else {
            let copy = ctx.copy_retained_text(link, "Rhino unknown record link copy")?;
            self.index_source_link(source_order, link, remains_sorted)?;
            copy
        };
        let (_, links) = self
            .session
            .unknown_links_mut(source_order)
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed(
                    "Rhino source record disappeared during link append",
                )
            })?;
        links.push(copy);
        Ok(true)
    }

    /// Restores canonical link order before final document validation.
    /// Generated links are typed identities for entities already admitted to
    /// the document. Canonicalize seeded links when indexing them, then sort a
    /// seeded row again before validation if a generated append changed order.
    /// Generated-only rows need canonical order only at final serialization.
    fn sort_source_links(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        self.flush_source_links()?;
        self.link_indices.clear();
        drop(std::mem::take(&mut self.pending_seeded_link_rows));
        self.pending_seeded_link_cursor = 0;
        drop(self.link_index_storage.take());
        Ok(())
    }

    /// Appends a later geometry-phase link to an object record.
    fn append_link(
        &mut self,
        source_order: usize,
        link: &str,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        self.checkpoint_instance_row(source_order)?;
        self.append_source_link(source_order, link)
    }

    fn append_links(
        &mut self,
        source_order: usize,
        incoming: &[String],
    ) -> Result<bool, cadmpeg_core::CodecError> {
        self.checkpoint_instance_row(source_order)?;
        if self.session.unknowns().get(source_order).is_none() {
            return Ok(false);
        }
        let ctx = self.expand.ctx();
        let mut links = incoming.iter();
        for _ in 0..incoming.len() {
            let link = ctx
                .next_charged(&mut links, "Rhino incoming source link scan")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino incoming link source ended early")
                })?;
            self.append_source_link(source_order, link.as_str())?;
        }
        Ok(true)
    }

    #[cfg(test)]
    fn validate_candidate<T>(
        &mut self,
        apply: impl FnOnce(&mut CadIr, &mut cadmpeg_ir::Annotations) -> T,
    ) -> Result<T, CandidateError> {
        self.validate_candidate_fallible(|ir, annotations, _arena_storage| {
            Ok::<_, String>(apply(ir, annotations))
        })
    }

    fn validate_candidate_fallible<T, E: Into<CandidateError>>(
        &mut self,
        apply: impl FnOnce(
            &mut CadIr,
            &mut cadmpeg_ir::Annotations,
            &mut cadmpeg_core::decode::ScopedReservation<'a>,
        ) -> Result<T, E>,
    ) -> Result<T, CandidateError> {
        let mut arena_storage = self
            .expand
            .ctx()
            .reserve_scoped(0, "Rhino candidate arena scratch")?;
        let annotations = self
            .annotations
            .copy_transaction(self.expand.ctx(), "Rhino speculative annotations")?;
        let ((candidate, value), annotations) = annotations.update(|annotations| {
            let mut candidate = CadIr::empty();
            let value =
                apply(&mut candidate, annotations, &mut arena_storage).map_err(Into::into)?;
            Ok::<_, CandidateError>((candidate, value))
        })?;
        let entity_count = candidate.model.entity_count();
        let mut budget = self.expansion_budget;
        let session = self.expand.ctx();
        self.flush_seeded_source_links()
            .map_err(CandidateError::Codec)?;
        let appended =
            self.session
                .try_append(candidate.model, candidate.native, |combined, unknowns| {
                    drop(arena_storage);
                    let validation = match cadmpeg_ir::validate::admit::admit_with_native_unknowns(
                        session,
                        combined,
                        ("rhino", unknowns),
                        Some(annotations.annotations()),
                        cadmpeg_ir::RHINO_DRAFT_CHECKS,
                        Vec::new(),
                    )? {
                        Ok(report) => report,
                        Err(error) => {
                            return Ok(Err(CandidateError::Admission(session.format_retained(
                                format_args!("{error}"),
                                "Rhino native admission message",
                            )?)))
                        }
                    };
                    if session.any_by(
                        &validation.findings,
                        |finding| Ok(finding.severity >= Severity::Error),
                        "Rhino candidate acceptance traversal",
                    )? {
                        return Ok(Err(CandidateError::Validation(validation_findings(
                            session,
                            &validation,
                        )?)));
                    }
                    budget.entities(session, entity_count)?;
                    session
                        .charge_entities(u64_from_index(entity_count), "rhino_instance_entities")?;
                    Ok(Ok((value, annotations.into_retained()?)))
                });
        let (value, annotations) = appended??;
        self.annotations = annotations;
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
    fn resolve_object(
        &self,
        id: crate::wire::Uuid,
    ) -> Result<ObjectReference, cadmpeg_core::CodecError> {
        Ok(
            match self
                .expand
                .ctx()
                .get_hash_map(
                    &self.object_candidates,
                    &id,
                    "Rhino object candidate lookup",
                )?
                .map_or(&[][..], Vec::as_slice)
            {
                [order] => ObjectReference::Resolved(*order),
                [] => ObjectReference::Missing,
                _ => ObjectReference::Ambiguous,
            },
        )
    }

    /// Resolves a foreign object UUID to its native record identity.
    /// Non-nil UUIDs that do not resolve are charged against `role`.
    fn resolve_object_record(
        &mut self,
        source_order: usize,
        role: &str,
        id: crate::wire::Uuid,
        diagnostic_storage: Option<&mut cadmpeg_core::decode::ScopedReservation<'_>>,
    ) -> Result<Option<String>, cadmpeg_core::CodecError> {
        if id.is_nil() {
            return Ok(None);
        }
        let code = match self.resolve_object(id)? {
            ObjectReference::Resolved(order) => {
                return Ok(Some(self.expand.ctx().format_retained(
                    format_args!("{}", Self::mint_unknown_id(order)),
                    "Rhino resolved object record ID",
                )?));
            }
            ObjectReference::Missing => RhinoLossCode::ReferenceMemberUnresolved,
            ObjectReference::Ambiguous => RhinoLossCode::ReferenceMemberAmbiguous,
        };
        let mut append = || {
            self.expand.ctx().reserve_vec(
                &mut self.report.typed_losses,
                1,
                "Rhino typed decode losses",
            )?;
            self.report.typed_losses.push(crate::wire::admitted_loss(
                self.expand.ctx(),
                code,
                format_args!("{role} in object record {source_order} references object {id}"),
                "Rhino unresolved object reference loss",
            )?);
            Ok::<_, cadmpeg_core::CodecError>(())
        };
        if let Some(storage) = diagnostic_storage {
            storage.with_storage(append)?;
        } else {
            append()?;
        }
        Ok(None)
    }

    /// Decode and atomically commit supported simple geometry.
    pub(crate) fn decode_geometry(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        if !self.archive().is_chunked() {
            return Ok(());
        }
        let mut source_orders = self
            .instance_selection
            .as_ref()
            .map_or(0..self.scan.objects.len(), |selected| {
                selected.source_order..selected.source_order + 1
            });
        let source_order_count = source_orders.len();
        for _ in 0..source_order_count {
            let source_order = self
                .expand
                .ctx()
                .next_charged(&mut source_orders, "Rhino object dispatch")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino object dispatch source ended early")
                })?;
            let Some(object) = self.scan.objects[source_order].framed() else {
                continue;
            };
            if self.instance_selection.is_none() && self.is_definition_member(object)? {
                continue;
            }
            if self.instance_selection.is_some() {
                self.checkpoint_instance_row(source_order)?;
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
                self.scan_unbound_unit_warning(source_order, "simple geometry")?;
                continue;
            };
            if crate::mesh::supported_class(object.class_uuid) {
                let identity = &object.identity;
                let Some((key_buffer, _key_storage)) =
                    self.checked_object_key(identity, source_order)?
                else {
                    continue;
                };
                let key = key_buffer;
                let decoded = crate::mesh::decode(
                    self.expand,
                    self.scan.data,
                    object.class_data_range.clone(),
                    self.archive(),
                    crate::mesh::MeshDecodeOptions {
                        writer_version: self.scan.metadata.properties.writer_version,
                        association: None,
                        id: crate::mesh::MeshId::Ready({
                            let ctx = self.expand.ctx();
                            let id = ctx.format_retained(
                                format_args!("rhino:object:tessellation#{key}"),
                                "Rhino mesh tessellation identity",
                            )?;
                            cadmpeg_ir::tessellation::TessellationId::mint(id).map_err(|error| {
                                ctx.format_retained(
                                    format_args!("{error}"),
                                    "Rhino mesh tessellation identity error",
                                )
                                .map_or_else(
                                    std::convert::identity,
                                    cadmpeg_core::CodecError::Malformed,
                                )
                            })?
                        }),
                        scale,
                        userdata: &object.userdata,
                    },
                    &mut self.mesh_budget,
                );
                match decoded {
                    Ok(mesh) => {
                        let proxy = self
                            .expand
                            .ctx()
                            .find_map(
                                &object.userdata[..],
                                |raw| {
                                    let Some(extra) = UserdataDescriptor::known(raw) else {
                                        return Ok(None);
                                    };
                                    Ok((extra.class_uuid == crate::subd::SUBD_MESH_PROXY_USERDATA
                                        && extra.item_uuid
                                            == crate::subd::SUBD_MESH_PROXY_USERDATA)
                                        .then_some(extra))
                                },
                                "Rhino decode geometry traversal",
                            )?
                            .cloned();
                        let mut proxy_transferred = false;
                        if let (Some(extra), Some(fingerprint)) = (proxy, mesh.proxy_fingerprint) {
                            let subd_id = cadmpeg_ir::ids::SubdId::compose(
                                &cadmpeg_ir::identity_namespace!("rhino", "object", "subd"),
                                key.clone(),
                            );
                            match crate::subd::decode_mesh_proxy(
                                self.expand.ctx(),
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
                                        self.scan_warning(source_order, format_args!("valid SubD mesh proxy rejected by IR validation; parent mesh retained"))?;
                                    }
                                }
                                Ok(None) => self.scan_warning(source_order, format_args!("SubD mesh proxy failed its validity or parent-mesh identity checks; parent mesh retained"))?,
                                Err(crate::subd::SubdError::Resource(limit)) => {
                                    return Err(cadmpeg_core::CodecError::ResourceLimit(limit));
                                }
                                Err(error) => self.scan_warning(source_order, format_args!("SubD mesh proxy dropped: {error}; parent mesh retained"))?,
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
                        )?;
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
                        )?;
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
                    )?;
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
        let mut source_orders = 0..self.scan.objects.len();
        let source_order_count = source_orders.len();
        for _ in 0..source_order_count {
            let source_order = self
                .expand
                .ctx()
                .next_charged(&mut source_orders, "Rhino object traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino object traversal source ended early")
                })?;
            let Some(object) = self.scan.objects[source_order].framed() else {
                continue;
            };
            if !crate::dimensions::supported_class(object.class_uuid) {
                continue;
            }
            if self.is_definition_member(object)? {
                self.scan_warning(source_order, format_args!("definition-member dimension retained because annotation instance expansion is unsupported"))?;
                continue;
            }
            let Some(scale) = self.neutral_scale() else {
                self.scan_unbound_unit_warning(source_order, "dimension")?;
                continue;
            };
            let identity = &object.identity;
            let Some((key_buffer, _key_storage)) =
                self.checked_object_key(identity, source_order)?
            else {
                continue;
            };
            let key = key_buffer;
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
                            let count = duplicate_userdata_count(
                                self.expand.ctx(),
                                &object.userdata,
                                class,
                            )?;
                            if count > 1 {
                                push_report_loss(
                                    self.expand.ctx(),
                                    &mut self.report.typed_losses,
                                    RhinoLossCode::DuplicateRecordResolved,
                                    format_args!(
                                        "{label} object at offset {} has {count} matching userdata records; first serialized record wins",
                                        object.range.start
                                    ),
                                )?;
                            }
                        }
                        if let Err(error) = crate::dimensions::apply_userdata(
                            self.expand.ctx(),
                            self.scan.data,
                            &object.userdata,
                            self.archive(),
                            scale,
                            &mut dimension,
                        ) {
                            if let crate::chunks::FramingError::Resource(limit) = &error {
                                return Err(cadmpeg_core::CodecError::ResourceLimit(*limit));
                            }
                            self.scan_warning(
                                source_order,
                                format_args!("dimension extension retained: {error}"),
                            )?;
                            continue;
                        }
                    }
                    // `SemanticAnnotation::order` must be globally unique and
                    // is a `u32`. The arena length is the dense next index and
                    // rolls back with the arena, unlike a standalone counter.
                    let Ok(order) =
                        u32::try_from(self.session.document().model.semantic_annotations.len())
                    else {
                        self.scan_warning(source_order, format_args!("dimension retained because the annotation arena exceeds u32 ordinals"))?;
                        continue;
                    };
                    let object = self.session.unknowns()[source_order].id().as_str();
                    let (annotation, unresolved) = match crate::dimensions::project(
                        self.expand.ctx(),
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
                            self.scan_warning(source_order, format_args!("{error}"))?;
                            continue;
                        }
                    };
                    if dimension.override_present {
                        push_report_loss(
                            self.expand.ctx(),
                            &mut self.report.typed_losses,
                            RhinoLossCode::DimensionOverrideDropped,
                            format_args!(
                                "dimension object at offset {} has an unapplied style override",
                                dimension.source_range.start
                            ),
                        )?;
                    }
                    let session = self.expand.ctx();
                    let mut link_storage =
                        session.reserve_scoped(0, "Rhino annotation link scratch")?;
                    let link = session.copy_scoped_text(
                        annotation.id.as_str(),
                        &mut link_storage,
                        "Rhino annotation link",
                    )?;
                    let result = self.validate_candidate_fallible(
                        |candidate, _annotations, arena_storage| {
                            session.push_scoped_vec(
                                arena_storage,
                                &mut candidate.model.semantic_annotations,
                                annotation,
                                "Rhino candidate semantic annotations",
                            )
                        },
                    );
                    match result {
                        Ok(()) => {
                            self.append_links(source_order, &[link])?;
                            self.mark_decoded(source_order);
                            let unresolved_count = unresolved.len();
                            let mut unresolved = unresolved.into_iter();
                            for _ in 0..unresolved_count {
                                let code = self
                                    .expand
                                    .ctx()
                                    .next_charged(
                                        &mut unresolved,
                                        "Rhino unresolved dimension reference traversal",
                                    )?
                                    .ok_or_else(|| {
                                        cadmpeg_core::CodecError::malformed(
                                            "Rhino unresolved dimension source ended early",
                                        )
                                    })?;
                                push_report_loss(
                                    self.expand.ctx(),
                                    &mut self.report.typed_losses,
                                    code,
                                    format_args!(
                                        "dimension record {source_order} reference is not resolved to a \
                                         decoded record"
                                    ),
                                )?;
                            }
                        }
                        Err(CandidateError::Codec(error)) => return Err(error),
                        Err(error) => self.scan_warning(
                            source_order,
                            format_args!("dimension candidate rejected: {error}"),
                        )?,
                    }
                }
                Err(crate::chunks::FramingError::Resource(limit)) => {
                    return Err(cadmpeg_core::CodecError::ResourceLimit(limit));
                }
                Err(error) => {
                    self.scan_warning(source_order, format_args!("dimension retained: {error}"))?;
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

        let ctx = self.expand.ctx();

        let Some(scale) = self.neutral_scale() else {
            self.scan_unbound_unit_warning(source_order, "hatch")?;
            return Ok(());
        };
        let identity = &object.identity;
        // Hatch owns values allocated by the userdata install. Declare its
        // backing guard first so `hatch` drops before the guard on every exit.
        let _gradient_storage;
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
                )?;
                if !future {
                    self.mark_failed(source_order);
                }
                return Ok(());
            }
        };
        let duplicate_count = duplicate_userdata_count(
            self.expand.ctx(),
            &object.userdata,
            crate::hatch::V5_HATCH_EXTRA,
        )?;
        if duplicate_count > 1 {
            push_report_loss(
                self.expand.ctx(),
                &mut self.report.typed_losses,
                RhinoLossCode::DuplicateRecordResolved,
                format_args!(
                    "hatch object at offset {} has {duplicate_count} matching userdata records; last valid serialized record wins",
                    object.range.start
                ),
            )?;
        }
        let crate::hatch::HatchUserdataInstall {
            errors,
            _gradient_storage: gradient_storage,
        } = crate::hatch::apply_userdata(
            ctx,
            self.scan.data,
            &object.userdata,
            scale,
            self.archive(),
            &mut hatch,
        )?;
        _gradient_storage = gradient_storage;
        if !errors.is_empty() {
            let class = self.scan.objects[source_order]
                .class_uuid()
                .unwrap_or_else(crate::wire::Uuid::nil);
            ctx.fold(
                &errors[..],
                (),
                |(), error| {
                    self.report.phase_warnings.push_coded_admitted(
                        self.expand.ctx(),
                        RhinoLossCode::ObjectDecodeDiagnostic,
                        format_args!("{class}: hatch userdata extension failed: {error}"),
                    )?;
                    Ok(())
                },
                "Rhino hatch userdata error traversal",
            )?;
        }
        drop(errors);
        let Some((key_buffer, _key_storage)) = self.checked_object_key(identity, source_order)?
        else {
            return Ok(());
        };
        let key = key_buffer;
        let (association_buffer, _association_storage) = self
            .expand
            .ctx()
            .with_scoped_storage("Rhino borrowed source association", || {
                self.source_association(identity)
            })?;
        let association = association_buffer;
        let feature_id = FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "hatch", "feature"),
            key.clone(),
        );
        let (record_buffer, _record_storage) = ctx.format_scoped(
            format_args!("rhino hatch record #{key}"),
            "Rhino hatch record label",
        )?;
        let record = record_buffer;
        let transform = match hatch_plane_transform(ctx, &hatch.plane, scale, &record) {
            Ok(transform) => transform,
            Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("hatch placement failed: {error}"),
                )?;
                self.mark_failed(source_order);
                return Ok(());
            }
        };
        let hatch_loop_count = hatch.loops.len();
        let mut hatch_loops = hatch.loops.iter_mut();
        for _ in 0..hatch_loop_count {
            let hatch_loop = ctx
                .next_charged(&mut hatch_loops, "Rhino hatch placement traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino hatch loop source ended early")
                })?;
            match transform_decoded_curve(self.expand.ctx(), &mut hatch_loop.curve, transform) {
                Ok(()) => {}
                Err(ReferenceFailure::Codec(error)) => return Err(error),
                Err(ReferenceFailure::Semantic(error)) => {
                    self.scan_warning(
                        source_order,
                        format_args!("hatch loop placement failed: {error}"),
                    )?;
                    self.mark_failed(source_order);
                    return Ok(());
                }
            }
        }
        let mut link_storage = ctx.reserve_scoped(0, "Rhino hatch link scratch")?;
        let loop_ids = hatch_loop_ids(
            self.expand.ctx(),
            key.as_str(),
            hatch.loops.iter().map(|hatch_loop| hatch_loop.kind),
            &mut link_storage,
        )?;
        let (feature, feature_storage) =
            ctx.with_scoped_storage("Rhino speculative feature fields", || {
                let mut parameters = BTreeMap::new();
                insert_feature_property(
                    self.expand.ctx(),
                    &mut parameters,
                    format_args!("pattern_index"),
                    format_args!("{}", hatch.pattern_index),
                )?;
                insert_feature_property(
                    self.expand.ctx(),
                    &mut parameters,
                    format_args!("pattern_scale"),
                    format_args!("{}", hatch.pattern_scale.get()),
                )?;
                insert_feature_property(
                    self.expand.ctx(),
                    &mut parameters,
                    format_args!("pattern_rotation"),
                    format_args!("{}", hatch.pattern_rotation.get()),
                )?;
                insert_feature_property(
                    self.expand.ctx(),
                    &mut parameters,
                    format_args!("basepoint"),
                    format_args!("{},{}", hatch.basepoint[0].get(), hatch.basepoint[1].get()),
                )?;
                if let Some(gradient) = hatch.gradient.as_ref() {
                    let gradient = crate::hatch::gradient_json(self.expand.ctx(), gradient)?;
                    insert_feature_property_owned(
                        self.expand.ctx(),
                        &mut parameters,
                        format_args!("gradient"),
                        gradient,
                    )?;
                }
                let mut hatch_loop_ids = loop_ids.iter().enumerate();
                for _ in 0..loop_ids.len() {
                    let (index, (kind, id)) = ctx
                        .next_charged(&mut hatch_loop_ids, "Rhino decode hatch traversal")?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Rhino hatch loop source ended early",
                            )
                        })?;
                    insert_feature_property(
                        self.expand.ctx(),
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
                let mut source_tag =
                    ctx.retained_string("RhinoHatch".len(), "Rhino feature source tag")?;
                source_tag.push_str("RhinoHatch");
                let mut native_kind =
                    ctx.retained_string("hatch".len(), "Rhino feature native kind")?;
                native_kind.push_str("hatch");
                let feature = Feature {
                    id: feature_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    ordinal: cadmpeg_core::decode::u64_from_index(hatch.source_range.start),
                    name: (!identity.name.is_empty())
                        .then(|| {
                            ctx.copy_retained_text(&identity.name, "Rhino decode_hatch text copy")
                        })
                        .transpose()?,
                    suppressed: Some(false),
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    source_properties: BTreeMap::new(),
                    source_tag: Some(source_tag),
                    source_text: None,
                    source_content: cadmpeg_ir::features::FeatureContent::default(),

                    evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: native_kind.into(),
                            parameters,
                        }),
                    ),
                    native_ref: Some(self.expand.ctx().copy_retained_text(
                        self.session.unknowns()[source_order].id().as_str(),
                        "Rhino source native reference copy",
                    )?),
                };
                Ok::<_, cadmpeg_core::CodecError>(feature)
            })?;
        let hatch_loops = hatch.loops;
        let hatch_loop_count = hatch_loops.len();
        let mut hatch_loops = hatch_loops.into_iter();
        let session = self.expand.ctx();
        let result =
            self.validate_candidate_fallible(|candidate, candidate_annotations, arena_storage| {
                for index in 0..hatch_loop_count {
                    let hatch_loop = session
                        .next_charged(&mut hatch_loops, "Rhino hatch curve traversal")?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Rhino hatch curve source ended early",
                            )
                        })?;
                    commit_curve_tree(
                        session,
                        candidate,
                        candidate_annotations,
                        hatch_loop.curve,
                        CurveCommitSource {
                            key: key.as_str(),
                            association: &association,
                            record: None,
                            path: &session
                                .format_scoped(
                                    format_args!("hatch-loop-{index}"),
                                    "Rhino hatch loop path",
                                )?
                                .0,
                        },
                        &mut *arena_storage,
                    )?;
                }
                ctx.push_scoped_vec(
                    arena_storage,
                    &mut candidate.model.features,
                    feature,
                    "Rhino candidate features",
                )?;
                Ok::<(), CandidateError>(())
            });
        match result {
            Ok(()) => {
                feature_storage.commit()?;
                ctx.fold(
                    &hatch.warnings[..],
                    (),
                    |(), warning| self.scan_diagnostic(source_order, warning),
                    "Rhino hatch warning traversal",
                )?;
                let links = hatch_source_links(
                    self.expand.ctx(),
                    loop_ids,
                    &feature_id,
                    &mut link_storage,
                )?;
                self.append_links(source_order, &links)?;
                self.geometry_transferred = true;
                self.mark_native_retained(source_order, RhinoLossCode::HatchFillNotTransferred);
            }
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("hatch candidate rejected: {error}"),
                )?;
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

        let ctx = self.expand.ctx();

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
                self.scan_warning(source_order, format_args!("polyedge retained: {error}"))?;
                self.mark_failed(source_order);
                return Ok(());
            }
        };
        let (construction, mut construction_storage) = ctx
            .with_scoped_storage("Rhino polyedge construction scratch", || {
                crate::polyedge::semantic_json(ctx, &polyedge)
            })?;
        let Some(construction) = construction else {
            self.scan_warning(
                source_order,
                format_args!("polyedge semantic serialization failed"),
            )?;
            return Ok(());
        };
        let Some((key_buffer, _key_storage)) = self.checked_object_key(identity, source_order)?
        else {
            return Ok(());
        };
        let key = key_buffer;
        let id = FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "polyedge", "feature"),
            key.clone(),
        );
        let mut feature_storage = ctx.reserve_scoped(0, "Rhino speculative feature fields")?;
        feature_storage.absorb(&mut construction_storage)?;
        let mut parameters = BTreeMap::new();
        ctx.fold(
            &polyedge.segments,
            0_usize,
            |index, segment| {
                let (record, mut record_storage) =
                    ctx.with_scoped_storage("Rhino polyedge reference scratch", || {
                        self.resolve_object_record(
                            source_order,
                            "polyedge segment",
                            segment.reference.object_id,
                            None,
                        )
                    })?;
                if let Some(record) = record {
                    feature_storage.absorb(&mut record_storage)?;
                    feature_storage.with_storage(|| {
                        insert_feature_property_owned(
                            self.expand.ctx(),
                            &mut parameters,
                            format_args!("segment_{index}_object"),
                            record,
                        )?;
                        Ok::<_, cadmpeg_core::CodecError>(())
                    })?;
                } else {
                    record_storage.commit()?;
                }
                Ok(index + 1)
            },
            "Rhino decode polyedge traversal",
        )?;
        let feature = feature_storage.with_storage(|| {
            let mut source_properties = BTreeMap::new();
            insert_feature_property_owned(
                self.expand.ctx(),
                &mut source_properties,
                format_args!("construction"),
                construction,
            )?;
            let name = (!identity.name.is_empty())
                .then(|| ctx.copy_retained_text(&identity.name, "Rhino decode_polyedge text copy"))
                .transpose()?;
            let mut source_tag =
                ctx.retained_string("RhinoPolyEdgeReference".len(), "Rhino feature source tag")?;
            source_tag.push_str("RhinoPolyEdgeReference");
            let mut native_kind =
                ctx.retained_string("polyedge_reference".len(), "Rhino feature native kind")?;
            native_kind.push_str("polyedge_reference");
            let feature = Feature {
                id: id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                ordinal: cadmpeg_core::decode::u64_from_index(source_order),
                name,
                suppressed: Some(false),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties,
                source_tag: Some(source_tag),
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),

                evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: native_kind.into(),
                        parameters,
                    }),
                ),
                native_ref: Some(ctx.format_retained(
                    format_args!("{}", Self::mint_unknown_id(source_order)),
                    "Rhino decode_polyedge text",
                )?),
            };
            Ok::<_, cadmpeg_core::CodecError>(feature)
        })?;
        match self.validate_candidate_fallible(|candidate, _annotations, arena_storage| {
            ctx.push_scoped_vec(
                arena_storage,
                &mut candidate.model.features,
                feature,
                "Rhino candidate features",
            )
        }) {
            Ok(()) => {
                feature_storage.commit()?;
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
            )?,
        }
        Ok(())
    }

    fn decode_detail(
        &mut self,
        source_order: usize,
        object: &ObjectDescriptor,
    ) -> Result<(), cadmpeg_core::CodecError> {
        use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

        let ctx = self.expand.ctx();

        let identity = &object.identity;
        let detail = match crate::detail::decode(
            self.expand.ctx(),
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
                )?;
                if !future {
                    self.mark_failed(source_order);
                }
                return Ok(());
            }
        };
        let Some((key_buffer, _key_storage)) = self.checked_object_key(identity, source_order)?
        else {
            return Ok(());
        };
        let key = key_buffer;
        let mut link_storage = ctx.reserve_scoped(0, "Rhino source link scratch")?;
        let (association_buffer, _association_storage) = self
            .expand
            .ctx()
            .with_scoped_storage("Rhino borrowed source association", || {
                self.source_association(identity)
            })?;
        let association = association_buffer;
        let curve_id = ctx.format_scoped_text(
            &mut link_storage,
            format_args!("rhino:object:curve#{key}.detail-boundary"),
            "Rhino decode_detail text",
        )?;
        let feature_id = FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "detail", "feature"),
            key.clone(),
        );
        let view = &self.scan.data[detail.view_range.clone()];
        let (feature, feature_storage) =
            ctx.with_scoped_storage("Rhino speculative feature fields", || {
                let mut source_properties = BTreeMap::new();
                insert_feature_property(
                    self.expand.ctx(),
                    &mut source_properties,
                    format_args!("view_bytes"),
                    format_args!("{}", view.len()),
                )?;
                let digest =
                    Sha256Digest::digest_for_decode(ctx, view, "Rhino detail view digest")?;
                insert_feature_property_owned(
                    ctx,
                    &mut source_properties,
                    format_args!("view_sha256"),
                    digest.into(),
                )?;
                let mut parameters = BTreeMap::new();
                insert_feature_property(
                    self.expand.ctx(),
                    &mut parameters,
                    format_args!("boundary"),
                    format_args!("{curve_id}"),
                )?;
                insert_feature_property(
                    self.expand.ctx(),
                    &mut parameters,
                    format_args!("page_per_model_ratio"),
                    format_args!("{}", detail.page_per_model_ratio.get()),
                )?;
                let mut source_tag =
                    ctx.retained_string("RhinoDetailView".len(), "Rhino feature source tag")?;
                source_tag.push_str("RhinoDetailView");
                let mut native_kind =
                    ctx.retained_string("detail_view".len(), "Rhino feature native kind")?;
                native_kind.push_str("detail_view");
                let feature = Feature {
                    id: feature_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    ordinal: cadmpeg_core::decode::u64_from_index(detail.source_range.start),
                    name: (!identity.name.is_empty())
                        .then(|| {
                            ctx.copy_retained_text(&identity.name, "Rhino decode_detail text copy")
                        })
                        .transpose()?,
                    suppressed: Some(false),
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    source_properties,
                    source_tag: Some(source_tag),
                    source_text: None,
                    source_content: cadmpeg_ir::features::FeatureContent::default(),

                    evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: native_kind.into(),
                            parameters,
                        }),
                    ),
                    native_ref: Some(self.expand.ctx().copy_retained_text(
                        self.session.unknowns()[source_order].id().as_str(),
                        "Rhino source native reference copy",
                    )?),
                };
                Ok::<_, cadmpeg_core::CodecError>(feature)
            })?;
        let session = self.expand.ctx();
        let result =
            self.validate_candidate_fallible(|candidate, candidate_annotations, arena_storage| {
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
                    &mut *arena_storage,
                )?;
                ctx.push_scoped_vec(
                    arena_storage,
                    &mut candidate.model.features,
                    feature,
                    "Rhino candidate features",
                )?;
                Ok::<(), CandidateError>(())
            });
        match result {
            Ok(()) => {
                feature_storage.commit()?;
                self.append_links(
                    source_order,
                    &[
                        curve_id,
                        ctx.format_scoped_text(
                            &mut link_storage,
                            format_args!("{feature_id}"),
                            "Rhino decode_detail text",
                        )?,
                    ],
                )?;
                self.geometry_transferred = true;
                self.mark_native_retained(source_order, RhinoLossCode::DetailViewNotTransferred);
            }
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("detail candidate rejected: {error}"),
                )?;
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

        let ctx = self.expand.ctx();

        let Some(scale) = self.neutral_scale() else {
            self.scan_unbound_unit_warning(source_order, "NURBS cage")?;
            return Ok(());
        };
        let identity = &object.identity;
        let mut cage_storage = ctx.reserve_scoped(0, "Rhino cage parse scratch")?;
        let cage = match cage_storage.with_storage(|| {
            crate::cage::decode(
                self.expand,
                object.class_data_range.clone(),
                scale,
                self.archive(),
            )
        }) {
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
                )?;
                if !future {
                    self.mark_failed(source_order);
                }
                return Ok(());
            }
        };
        let Some((key_buffer, _key_storage)) = self.checked_object_key(identity, source_order)?
        else {
            return Ok(());
        };
        let key = key_buffer;
        let feature_id = FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "cage", "feature"),
            key.clone(),
        );
        let (feature, feature_storage) =
            ctx.with_scoped_storage("Rhino speculative feature fields", || {
                let mut properties = BTreeMap::new();
                for (axis, knots) in ["u", "v", "w"].into_iter().zip(&cage.knots) {
                    insert_feature_property_owned(
                        self.expand.ctx(),
                        &mut properties,
                        format_args!("{axis}_knots"),
                        ctx.join_display_retained(
                            knots.iter().map(|value| value.get()),
                            ",",
                            "Rhino cage knot text",
                        )?,
                    )?;
                }
                insert_feature_property_owned(
                    self.expand.ctx(),
                    &mut properties,
                    format_args!("control_points"),
                    ctx.fold(
                        &cage.control_points,
                        String::new(),
                        |mut text, point| {
                            if !text.is_empty() {
                                ctx.append_retained(&mut text, ";", "Rhino cage point text")?;
                            }
                            ctx.fold(
                                point,
                                0_usize,
                                |index, coordinate| {
                                    if index > 0 {
                                        ctx.append_retained(
                                            &mut text,
                                            ",",
                                            "Rhino cage coordinate text",
                                        )?;
                                    }
                                    ctx.append_formatted_retained(
                                        &mut text,
                                        format_args!("{}", coordinate.get()),
                                        "Rhino cage coordinate text",
                                    )?;
                                    Ok(index + 1)
                                },
                                "Rhino cage coordinate traversal",
                            )?;
                            Ok(text)
                        },
                        "Rhino cage point traversal",
                    )?,
                )?;
                if let Some(weights) = &cage.weights {
                    insert_feature_property_owned(
                        self.expand.ctx(),
                        &mut properties,
                        format_args!("weights"),
                        ctx.join_display_retained(
                            weights.iter().map(|value| value.get()),
                            ",",
                            "Rhino cage weight text",
                        )?,
                    )?;
                }
                let mut parameters = BTreeMap::new();
                insert_feature_property(
                    self.expand.ctx(),
                    &mut parameters,
                    format_args!("dimension"),
                    format_args!("{}", cage.dimension),
                )?;
                insert_feature_property(
                    self.expand.ctx(),
                    &mut parameters,
                    format_args!("rational"),
                    format_args!("{}", cage.rational()),
                )?;
                insert_feature_property(
                    self.expand.ctx(),
                    &mut parameters,
                    format_args!("orders"),
                    format_args!("{},{},{}", cage.orders[0], cage.orders[1], cage.orders[2]),
                )?;
                insert_feature_property(
                    self.expand.ctx(),
                    &mut parameters,
                    format_args!("counts"),
                    format_args!("{},{},{}", cage.counts[0], cage.counts[1], cage.counts[2]),
                )?;
                let mut source_tag =
                    ctx.retained_string("RhinoNurbsCage".len(), "Rhino feature source tag")?;
                source_tag.push_str("RhinoNurbsCage");
                let mut native_kind =
                    ctx.retained_string("nurbs_cage".len(), "Rhino feature native kind")?;
                native_kind.push_str("nurbs_cage");
                let feature = Feature {
                    id: feature_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    ordinal: cadmpeg_core::decode::u64_from_index(cage.source_range.start),
                    name: (!identity.name.is_empty())
                        .then(|| {
                            ctx.copy_retained_text(&identity.name, "Rhino decode_cage text copy")
                        })
                        .transpose()?,
                    suppressed: Some(false),
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    source_properties: properties,
                    source_tag: Some(source_tag),
                    source_text: None,
                    source_content: cadmpeg_ir::features::FeatureContent::default(),

                    evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: native_kind.into(),
                            parameters,
                        }),
                    ),
                    native_ref: Some(self.expand.ctx().copy_retained_text(
                        self.session.unknowns()[source_order].id().as_str(),
                        "Rhino source native reference copy",
                    )?),
                };
                Ok::<_, cadmpeg_core::CodecError>(feature)
            })?;
        match self.validate_candidate_fallible(|candidate, _annotations, arena_storage| {
            ctx.push_scoped_vec(
                arena_storage,
                &mut candidate.model.features,
                feature,
                "Rhino candidate features",
            )
        }) {
            Ok(()) => {
                feature_storage.commit()?;
                self.append_link(source_order, feature_id.as_str())?;
                self.geometry_transferred = true;
                self.mark_native_retained(source_order, RhinoLossCode::CageLatticeNotTransferred);
            }
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("NURBS cage candidate rejected: {error}"),
                )?;
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
            self.scan_unbound_unit_warning(source_order, "morph control")?;
            return Ok(());
        };
        let ctx = self.expand.ctx();
        let identity = &object.identity;
        let mut morph_storage = ctx.reserve_scoped(0, "Rhino morph parse scratch")?;
        let morph = match morph_storage.with_storage(|| {
            crate::morph::decode(
                self.expand,
                object.class_data_range.clone(),
                scale,
                self.archive(),
            )
        }) {
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
                )?;
                if !future {
                    self.mark_failed(source_order);
                }
                return Ok(());
            }
        };
        let Some((key_buffer, _key_storage)) = self.checked_object_key(identity, source_order)?
        else {
            return Ok(());
        };
        let key = key_buffer;
        let mut diagnostic_storage = ctx.reserve_scoped(0, "Rhino morph reference diagnostics")?;
        let projected = ctx.with_scoped_storage("Rhino speculative feature fields", || {
            crate::morph::project(
                self.expand.ctx(),
                &morph,
                key.as_str(),
                (!identity.name.is_empty())
                    .then(|| {
                        self.expand
                            .ctx()
                            .copy_retained_text(&identity.name, "Rhino decode_morph text copy")
                    })
                    .transpose()?,
                self.expand.ctx().copy_retained_text(
                    self.session.unknowns()[source_order].id().as_str(),
                    "Rhino source native reference copy",
                )?,
                |id| {
                    self.resolve_object_record(
                        source_order,
                        "morph captive",
                        id,
                        Some(&mut diagnostic_storage),
                    )
                },
            )
        });
        diagnostic_storage.commit()?;
        let (feature, feature_storage) = match projected {
            Ok(value) => value,
            Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                self.scan_warning(source_order, format_args!("morph control failed: {error}"))?;
                self.mark_failed(source_order);
                return Ok(());
            }
        };
        let mut link_storage = ctx.reserve_scoped(0, "Rhino source link scratch")?;
        let feature_id = self.expand.ctx().format_scoped_text(
            &mut link_storage,
            format_args!("{}", feature.id),
            "Rhino decode_morph text",
        )?;
        match self.validate_candidate_fallible(|candidate, _annotations, arena_storage| {
            ctx.push_scoped_vec(
                arena_storage,
                &mut candidate.model.features,
                feature,
                "Rhino candidate features",
            )
        }) {
            Ok(()) => {
                feature_storage.commit()?;
                self.append_link(source_order, &feature_id)?;
                self.geometry_transferred = true;
                self.mark_native_retained(source_order, RhinoLossCode::MorphDeformationNotApplied);
            }
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("morph candidate rejected: {error}"),
                )?;
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

        let ctx = self.expand.ctx();

        let Some(scale) = self.neutral_scale() else {
            self.scan_unbound_unit_warning(source_order, "curve-on-surface")?;
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
                )?;
                if !future {
                    self.mark_failed(source_order);
                }
                return Ok(());
            }
        };
        let Some((key_buffer, _key_storage)) = self.checked_object_key(identity, source_order)?
        else {
            return Ok(());
        };
        let key = key_buffer;
        let mut link_storage = ctx.reserve_scoped(0, "Rhino source link scratch")?;
        let association = self.source_association(identity)?;
        let parameter_id = ctx.format_scoped_text(
            &mut link_storage,
            format_args!("rhino:object:curve#{key}.curve-on-surface-c2"),
            "Rhino decode_curve_on_surface text",
        )?;
        let model_id = construction
            .model_curve
            .as_ref()
            .map(|_| {
                ctx.format_scoped_text(
                    &mut link_storage,
                    format_args!("rhino:object:curve#{key}.curve-on-surface-c3"),
                    "Rhino decode_curve_on_surface text",
                )
            })
            .transpose()?;
        let surface_id = cadmpeg_ir::ids::SurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
            key.clone()
                .then(cadmpeg_ir::identity_key!(".curve-on-surface-support")),
        );
        let feature_id = FeatureId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "curve-on-surface", "feature"),
            key.clone(),
        );
        let (feature, feature_storage) =
            ctx.with_scoped_storage("Rhino speculative feature fields", || {
                let mut source_properties = BTreeMap::new();
                if let Some(id) = model_id.as_ref() {
                    insert_feature_property(
                        self.expand.ctx(),
                        &mut source_properties,
                        format_args!("model_curve"),
                        format_args!("{id}"),
                    )?;
                }
                let mut parameters = BTreeMap::new();
                insert_feature_property(
                    self.expand.ctx(),
                    &mut parameters,
                    format_args!("parameter_curve"),
                    format_args!("{parameter_id}"),
                )?;
                insert_feature_property(
                    self.expand.ctx(),
                    &mut parameters,
                    format_args!("support_surface"),
                    format_args!("{surface_id}"),
                )?;
                let mut source_tag =
                    ctx.retained_string("RhinoCurveOnSurface".len(), "Rhino feature source tag")?;
                source_tag.push_str("RhinoCurveOnSurface");
                let mut native_kind =
                    ctx.retained_string("curve_on_surface".len(), "Rhino feature native kind")?;
                native_kind.push_str("curve_on_surface");
                let feature = Feature {
                    id: feature_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    ordinal: cadmpeg_core::decode::u64_from_index(construction.source_range.start),
                    name: (!identity.name.is_empty())
                        .then(|| {
                            ctx.copy_retained_text(
                                &identity.name,
                                "Rhino decode_curve_on_surface text copy",
                            )
                        })
                        .transpose()?,
                    suppressed: Some(false),
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    source_properties,
                    source_tag: Some(source_tag),
                    source_text: None,
                    source_content: cadmpeg_ir::features::FeatureContent::default(),

                    evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: native_kind.into(),
                            parameters,
                        }),
                    ),
                    native_ref: Some(self.expand.ctx().copy_retained_text(
                        self.session.unknowns()[source_order].id().as_str(),
                        "Rhino source native reference copy",
                    )?),
                };
                Ok::<_, cadmpeg_core::CodecError>(feature)
            })?;
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
        let result =
            self.validate_candidate_fallible(|candidate, candidate_annotations, arena_storage| {
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
                    &mut *arena_storage,
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
                        &mut *arena_storage,
                    )?;
                }
                ctx.push_scoped_vec(
                    arena_storage,
                    &mut candidate.model.surfaces,
                    Surface {
                        id: surface_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        geometry: surface_geometry,
                        source_object: Some(association),
                    },
                    "Rhino candidate surfaces",
                )?;
                set_exactness(
                    ctx,
                    candidate_annotations,
                    &surface_id,
                    if surface_derived {
                        Exactness::Derived
                    } else {
                        Exactness::ByteExact
                    },
                )?;
                ctx.push_scoped_vec(
                    arena_storage,
                    &mut candidate.model.features,
                    feature,
                    "Rhino candidate features",
                )?;
                Ok::<(), CandidateError>(())
            });
        match result {
            Ok(()) => {
                feature_storage.commit()?;
                let class = self.scan.objects[source_order]
                    .class_uuid()
                    .unwrap_or_else(crate::wire::Uuid::nil);
                let phase_warnings = &mut self.report.phase_warnings;
                ctx.fold(
                    &construction.warnings[..],
                    (),
                    |(), warning| append_class_diagnostic(ctx, phase_warnings, class, warning),
                    "Rhino curve-on-surface warning traversal",
                )?;
                let mut links = link_storage.with_storage(|| {
                    ctx.collection_vec(
                        3 + usize::from(model_id.is_some()),
                        "Rhino curve-on-surface links",
                    )
                })?;
                links.extend([
                    parameter_id,
                    ctx.format_scoped_text(
                        &mut link_storage,
                        format_args!("{surface_id}"),
                        "Rhino decode_curve_on_surface text",
                    )?,
                    ctx.format_scoped_text(
                        &mut link_storage,
                        format_args!("{feature_id}"),
                        "Rhino decode_curve_on_surface text",
                    )?,
                ]);
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
                )?;
                self.mark_failed(source_order);
            }
        }
        Ok(())
    }

    fn is_definition_member(
        &self,
        object: &ObjectDescriptor,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let identity = &object.identity;
        self.scan
            .definitions
            .contains_member(self.expand.ctx(), identity.object_id)
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
    ) -> Result<
        Option<(IdentityKey, cadmpeg_core::decode::ScopedReservation<'a>)>,
        cadmpeg_core::CodecError,
    > {
        let ctx = self.expand.ctx();
        let (value_buffer, storage) =
            ctx.with_scoped_storage("Rhino object key scratch", || {
                Ok::<_, cadmpeg_core::CodecError>(
                    if let Some(selected) = &self.instance_selection {
                        ctx.format_retained(
                            format_args!("{}", selected.key.as_str()),
                            "Rhino object key copy",
                        )?
                    } else if let Some((_, key)) =
                        ctx.rsplit_once(identity.source_id.as_str(), "#", "Rhino object key scan")?
                    {
                        ctx.format_retained(format_args!("{key}"), "Rhino object key copy")?
                    } else {
                        ctx.format_retained(
                            format_args!("{source_order}"),
                            "Rhino object key copy",
                        )?
                    },
                )
            })?;
        let value = value_buffer;
        match IdentityKey::try_new(value) {
            Ok(key) => Ok(Some((key, storage))),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("object identity key is invalid: {error}"),
                )?;
                self.mark_failed(source_order);
                Ok(None)
            }
        }
    }

    fn reference_segment(
        &self,
        source_order: usize,
        identity: &crate::objects::SourceIdentity,
        scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ) -> Result<String, cadmpeg_core::CodecError> {
        if !identity.object_id.is_nil()
            && self.resolve_object(identity.object_id)? == ObjectReference::Resolved(source_order)
        {
            self.expand.ctx().format_scoped_text(
                scratch,
                format_args!("{}", identity.object_id),
                "Rhino instance path segment",
            )
        } else {
            let object = self.scan.objects.get(source_order).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino reference object is missing")
            })?;
            self.expand.ctx().format_scoped_text(
                scratch,
                format_args!("record-{source_order:06}-offset-{}", object.range().start),
                "Rhino instance path segment",
            )
        }
    }

    fn source_association(
        &self,
        identity: &crate::objects::SourceIdentity,
    ) -> Result<SourceObjectAssociation, cadmpeg_core::CodecError> {
        source_association(
            self.expand.ctx(),
            identity,
            self.instance_selection
                .as_ref()
                .map_or(&[], |selected| selected.path.as_slice()),
            self.instance_display.and_then(|display| display.color),
            self.instance_display.map(|display| display.visible),
        )
    }

    fn expand_reference(&mut self, source_order: usize) -> Result<bool, cadmpeg_core::CodecError> {
        let original_model =
            ModelCheckpoint::capture(&self.session.document().model, self.expand.ctx())?;
        let annotation_checkpoint = self
            .annotations
            .copy_transaction(self.expand.ctx(), "Rhino annotation checkpoint")?;
        let session = self.expand.ctx();
        self.instance_journal = Some(InstanceJournal::new(session)?);
        let original_geometry_transferred = self.geometry_transferred;
        let report_checkpoint = self.report.checkpoint();
        let original_selection = self.instance_selection.take();
        let original_display = self.instance_display;
        let original_expansion_budget = self.expansion_budget;
        let attempt = {
            let initial_path = original_selection
                .as_ref()
                .map_or(&[][..], |selected| selected.path.as_slice());
            let mut traversal = session.collect_scoped_texts(
                initial_path.iter().map(String::as_str),
                "Rhino instance traversal scratch",
            )?;
            let mut stack = Vec::new();
            let outcome = self.expand_reference_inner(
                source_order,
                Transform::identity(),
                &mut traversal.0,
                &mut stack,
                &mut traversal.1,
            );
            self.instance_selection = original_selection;
            // ModelCheckpoint truncates appended entities and keeps grown arena
            // capacity. Mesh and decompressed totals remain cumulative.
            match outcome {
                Ok(links) => {
                    self.flush_seeded_source_links()?;
                    let validation = cadmpeg_ir::validate::admit::admit_with_native_unknowns(
                        session,
                        self.session.document(),
                        ("rhino", self.session.unknowns()),
                        None,
                        cadmpeg_ir::RHINO_INSTANCE_CHECKS,
                        Vec::new(),
                    );
                    let accepted = match &validation {
                        Ok(Ok(report)) => !session.any_by(
                            &report.findings,
                            |finding| Ok(finding.severity >= Severity::Error),
                            "Rhino instance acceptance traversal",
                        )?,
                        _ => false,
                    };
                    if accepted {
                        self.append_links(source_order, &links.values)?;
                        self.mark_decoded(source_order);
                        self.geometry_transferred = true;
                        let journal = self.instance_journal.take().ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed("Rhino instance journal is missing")
                        })?;
                        journal.field_text_storage.commit()?;
                        Ok(Ok(()))
                    } else {
                        let findings = match validation {
                            Ok(Ok(report)) => validation_findings(session, &report)?,
                            Ok(Err(error)) => session.format_retained(
                                format_args!("{error}"),
                                "Rhino native admission message",
                            )?,
                            Err(error) => return Err(error),
                        };
                        Ok(Err(session.format_retained(
                            format_args!(
                                "instance expansion rejected atomically by IR admission: {findings}"
                            ),
                            "Rhino instance rejection message",
                        )?))
                    }
                }
                Err(ReferenceFailure::Codec(error)) => Err(error),
                Err(ReferenceFailure::Semantic(message)) => Ok(Err(session.format_retained(
                    format_args!("instance retained: {message}"),
                    "Rhino instance rejection message",
                )?)),
            }
        }?;
        if attempt.is_ok() {
            return Ok(true);
        }
        let rejection_warning = attempt.expect_err("rejected instance has a reason");

        original_model
            .0
            .discard_appended(&mut self.session.document_mut()?.model, self.expand.ctx())?;
        self.annotations = annotation_checkpoint.into_retained()?;
        self.rollback_instance_rows()?;
        self.geometry_transferred = original_geometry_transferred;
        self.report.rollback(report_checkpoint);
        self.instance_display = original_display;
        self.expansion_budget = original_expansion_budget;
        self.scan_warning(source_order, format_args!("{rejection_warning}"))?;
        Ok(false)
    }

    fn expand_reference_inner(
        &mut self,
        source_order: usize,
        parent: Transform,
        path: &mut Vec<String>,
        stack: &mut Vec<crate::wire::Uuid>,
        scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ) -> Result<InstanceLinks<'a>, ReferenceFailure> {
        const MAX_INSTANCE_DEPTH: usize = 64;
        let _nested = self.expand.ctx().enter_nested("rhino_instance_nesting")?;
        self.expansion_budget.reference(self.expand.ctx())?;
        self.expand
            .ctx()
            .charge_collection_items(1, "rhino_instance_reference")?;
        let depth_limit = session_ceiling(
            self.expand.ctx().policy().limits.max_recursion_depth,
            MAX_INSTANCE_DEPTH,
        );
        if stack.len() >= depth_limit {
            return Err(self
                .expand
                .ctx()
                .refuse_codec_limit(
                    "Rhino instance depth limit",
                    u64_from_index(depth_limit),
                    u64_from_index(stack.len()) + 1,
                )
                .into());
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
        let reference = crate::instances::parse_reference(
            self.expand.ctx(),
            self.scan.data,
            object.class_data_range.clone(),
        )
        .map_err(|error| {
            self.expand
                .ctx()
                .format_retained(format_args!("{error}"), "Rhino expand_reference_inner text")
                .map_or_else(Into::into, ReferenceFailure::Semantic)
        })?;
        if self
            .scan
            .definitions
            .is_ambiguous(self.expand.ctx(), reference.definition_id())?
        {
            return Err(ReferenceFailure::Semantic(
                self.expand.ctx().format_retained(
                    format_args!("definition {} is duplicated", reference.definition_id()),
                    "Rhino expand_reference_inner text",
                )?,
            ));
        }
        let definitions = self.scan.definitions.definitions();
        let definition = self
            .expand
            .ctx()
            .get_hash_map(
                &self.definition_candidates,
                &reference.definition_id(),
                "Rhino definition candidate lookup",
            )?
            .and_then(|index| definitions.get(*index))
            .map_or_else(
                || {
                    Err(ReferenceFailure::Semantic(
                        self.expand.ctx().format_retained(
                            format_args!("definition {} is missing", reference.definition_id()),
                            "Rhino expand_reference_inner text",
                        )?,
                    ))
                },
                Ok,
            )?;
        if matches!(definition.kind, crate::instances::DefinitionKind::Linked)
            && definition.members.is_empty()
        {
            return Err(ReferenceFailure::Semantic(
                self.expand.ctx().format_retained(
                    format_args!(
                        "linked external definition {} has no local members",
                        definition.id()
                    ),
                    "Rhino expand_reference_inner text",
                )?,
            ));
        }
        if matches!(definition.kind, crate::instances::DefinitionKind::Unset) {
            return Err(ReferenceFailure::Semantic(
                self.expand.ctx().format_retained(
                    format_args!("definition {} has unset type", definition.id()),
                    "Rhino expand_reference_inner text",
                )?,
            ));
        }
        if !instance_members_are_unique(self.expand.ctx(), &definition.members)? {
            return Err(ReferenceFailure::Semantic(
                self.expand.ctx().format_retained(
                    format_args!(
                        "definition {} contains duplicate member UUIDs",
                        definition.id()
                    ),
                    "Rhino expand_reference_inner text",
                )?,
            ));
        }
        if self
            .expand
            .ctx()
            .contains(stack, &definition.id(), "Rhino instance cycle lookup")?
        {
            return Err(ReferenceFailure::Semantic(
                self.expand.ctx().format_retained(
                    format_args!("definition cycle reaches {}", definition.id()),
                    "Rhino expand_reference_inner text",
                )?,
            ));
        }
        let binding = self.unit_binding();
        let crate::settings::UnitBinding::Millimeters(scale) = binding else {
            return Err(ReferenceFailure::Semantic(
                self.expand.ctx().format_retained(
                    format_args!(
                        "document has no physical millimetre binding ({})",
                        binding.label()
                    ),
                    "Rhino expand_reference_inner text",
                )?,
            ));
        };
        let local = crate::instances::scale_translation(reference.transform(), scale)
            .ok_or_else(|| "scaled instance transform is invalid".to_string())?;
        let transform = parent.compose(local).map_err(|error| error.to_string())?;
        let definition_id = definition.id();
        let definition_members = &definition.members;
        self.expand
            .ctx()
            .reserve_scoped_vec(scratch, stack, 1, "Rhino instance stack slots")?;
        stack.push(definition_id);
        self.expand
            .ctx()
            .reserve_scoped_vec(scratch, path, 1, "Rhino instance path slots")?;
        let mut segment_storage = self
            .expand
            .ctx()
            .reserve_scoped(0, "Rhino instance segment scratch")?;
        path.push(self.reference_segment(source_order, identity, &mut segment_storage)?);
        let previous_display = self.instance_display;
        self.instance_display = Some(InstanceDisplay {
            color: identity
                .effective_color
                .map(color)
                .or(previous_display.and_then(|display| display.color)),
            visible: previous_display.is_none_or(|display| display.visible)
                && identity.effective_visible,
        });
        let result = (|| {
            let mut backing = self
                .expand
                .ctx()
                .reserve_scoped(0, "Rhino instance link slots")?;
            let mut links = Vec::new();
            let mut members = definition_members.iter();
            for _ in 0..definition_members.len() {
                let &member_id = self
                    .expand
                    .ctx()
                    .next_charged(&mut members, "Rhino instance definition members")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Rhino instance definition source ended early",
                        )
                    })?;
                self.expansion_budget.member(self.expand.ctx())?;
                self.expand
                    .ctx()
                    .charge_collection_items(1, "rhino_instance_member")?;
                let member_order = match self.resolve_object(member_id)? {
                    ObjectReference::Resolved(order) => order,
                    ObjectReference::Missing => {
                        return Err(ReferenceFailure::Semantic(
                            self.expand.ctx().format_retained(
                                format_args!("definition member {member_id} is missing"),
                                "Rhino expand_reference_inner text",
                            )?,
                        ));
                    }
                    ObjectReference::Ambiguous => {
                        return Err(ReferenceFailure::Semantic(
                            self.expand.ctx().format_retained(
                                format_args!("definition member {member_id} is ambiguous"),
                                "Rhino expand_reference_inner text",
                            )?,
                        ));
                    }
                };
                let member = &self.scan.objects[member_order];
                if member
                    .class_uuid()
                    .is_some_and(crate::instances::is_reference_class)
                {
                    let nested =
                        self.expand_reference_inner(member_order, transform, path, stack, scratch)?;
                    self.append_links(member_order, &nested.values)?;
                    self.mark_decoded(member_order);
                    backing.with_storage(|| {
                        self.expand.ctx().extend_vec(
                            &mut links,
                            nested.values,
                            "Rhino instance link slots",
                        )
                    })?;
                    continue;
                }
                let before =
                    ModelCheckpoint::capture(&self.session.document().model, self.expand.ctx())?;
                let selection =
                    InstanceSelection::new(self.expand.ctx(), member_order, path, member_id)?;
                let previous_selection = self.instance_selection.replace(selection);
                let decoded = self.decode_geometry();
                self.instance_selection = previous_selection;
                decoded?;
                let after =
                    ModelCheckpoint::capture(&self.session.document().model, self.expand.ctx())?;
                if before.0.same_state(&after.0, self.expand.ctx())? {
                    return Err(ReferenceFailure::Semantic(
                        self.expand.ctx().format_retained(
                            format_args!("definition member {member_id} did not decode"),
                            "Rhino expand_reference_inner text",
                        )?,
                    ));
                }
                let transformed = self.transform_new_entities(&before.0, transform, scratch)?;
                backing.with_storage(|| {
                    self.expand.ctx().extend_vec(
                        &mut links,
                        transformed.values,
                        "Rhino instance link slots",
                    )
                })?;
            }
            Ok(InstanceLinks {
                values: links,
                _backing: backing,
            })
        })();
        self.instance_display = previous_display;
        path.pop();
        stack.pop();
        result
    }

    fn transform_new_entities(
        &mut self,
        before: &ModelCheckpoint,
        transform: Transform,
        scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ) -> Result<InstanceLinks<'a>, ReferenceFailure> {
        let ctx = self.expand.ctx();
        let ir = self.session.document_mut()?;
        let mut backing = ctx.reserve_scoped(0, "Rhino transformed instance links")?;
        let mut links = Vec::new();
        let mut derived_storage = ctx.reserve_scoped(0, "Rhino transformed annotation scratch")?;
        let mut derived_ids = Vec::new();
        let new_bodies = ir
            .model
            .bodies
            .get(before.arena_len::<Body>()..)
            .ok_or_else(|| "instance decode removed existing bodies".to_string())?;
        ctx.fold(
            new_bodies,
            (),
            |(), body| {
                ctx.reserve_scoped_vec(
                    &mut backing,
                    &mut links,
                    1,
                    "Rhino transformed instance links",
                )?;
                let id = ctx.format_scoped_text(
                    scratch,
                    format_args!("{}", body.id.as_str()),
                    "Rhino transformed instance links",
                )?;
                links.push(id);
                ctx.push_scoped_vec(
                    &mut derived_storage,
                    &mut derived_ids,
                    body.id.as_str(),
                    "Rhino transformed instance annotations",
                )?;
                Ok(())
            },
            "Rhino transformed entity traversal",
        )?;
        let mut point_source = ir
            .model
            .points
            .get_mut(before.arena_len::<Point>()..)
            .ok_or_else(|| "instance decode removed existing points".to_string())?
            .iter_mut();
        let point_count = point_source.len();
        for _ in 0..point_count {
            let point = ctx
                .next_charged(&mut point_source, "Rhino transformed entity traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Rhino transformed point source ended early",
                    )
                })?;
            let placed = placed_finite_point(transform, point.position())?;
            point.set_position(placed);
            ctx.push_scoped_vec(
                &mut derived_storage,
                &mut derived_ids,
                point.id.as_str(),
                "Rhino transformed instance annotations",
            )?;
        }
        let mut curve_source = ir
            .model
            .curves
            .get_mut(before.arena_len::<Curve>()..)
            .ok_or_else(|| "instance decode removed existing curves".to_string())?
            .iter_mut();
        let curve_count = curve_source.len();
        for _ in 0..curve_count {
            let curve = ctx
                .next_charged(&mut curve_source, "Rhino transformed entity traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Rhino transformed curve source ended early",
                    )
                })?;
            if let CurveGeometry::Procedural { cache, .. } = &mut curve.geometry {
                if let Some(cache) = cache.take() {
                    curve.geometry = CurveGeometry::Solved(cache);
                }
            }
            transform_curve(self.expand.ctx(), curve, transform)?;
            ctx.reserve_scoped_vec(
                &mut backing,
                &mut links,
                1,
                "Rhino transformed instance links",
            )?;
            let id = ctx.format_scoped_text(
                scratch,
                format_args!("{}", curve.id.as_str()),
                "Rhino transformed instance links",
            )?;
            links.push(id);
            ctx.push_scoped_vec(
                &mut derived_storage,
                &mut derived_ids,
                curve.id.as_str(),
                "Rhino transformed instance annotations",
            )?;
        }
        let mut surface_source = ir
            .model
            .surfaces
            .get_mut(before.arena_len::<Surface>()..)
            .ok_or_else(|| "instance decode removed existing surfaces".to_string())?
            .iter_mut();
        let surface_count = surface_source.len();
        for _ in 0..surface_count {
            let surface = ctx
                .next_charged(&mut surface_source, "Rhino transformed entity traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Rhino transformed surface source ended early",
                    )
                })?;
            if let SurfaceGeometry::Procedural { cache, .. } = &mut surface.geometry {
                if let Some(cache) = cache.take() {
                    surface.geometry = SurfaceGeometry::Solved(cache);
                }
            }
            transform_surface(ctx, surface, transform)?;
            ctx.reserve_scoped_vec(
                &mut backing,
                &mut links,
                1,
                "Rhino transformed instance links",
            )?;
            let id = ctx.format_scoped_text(
                scratch,
                format_args!("{}", surface.id.as_str()),
                "Rhino transformed instance links",
            )?;
            links.push(id);
            ctx.push_scoped_vec(
                &mut derived_storage,
                &mut derived_ids,
                surface.id.as_str(),
                "Rhino transformed instance annotations",
            )?;
        }
        let mut mesh_source = ir
            .model
            .tessellations
            .get_mut(before.arena_len::<Tessellation>()..)
            .ok_or_else(|| "instance decode removed existing tessellations".to_string())?
            .iter_mut();
        let mesh_count = mesh_source.len();
        for _ in 0..mesh_count {
            let mesh = ctx
                .next_charged(&mut mesh_source, "Rhino transformed entity traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino transformed mesh source ended early")
                })?;
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
            .map_err(|error| {
                ctx.format_retained(format_args!("{error}"), "Rhino transform_new_entities text")
                    .map_or_else(Into::into, ReferenceFailure::Semantic)
            })?;
            let has_vertex_normals = match mesh.mesh() {
                cadmpeg_ir::tessellation::TessellationMesh::ShadedList { vertices, .. } => {
                    !vertices.is_empty()
                }
                cadmpeg_ir::tessellation::TessellationMesh::ShadedStrips { strips } => {
                    !strips.as_slice().is_empty()
                }
                _ => false,
            };
            if has_vertex_normals {
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
                .map_err(|error| {
                    ctx.format_retained(
                        format_args!("{error}"),
                        "Rhino transform_new_entities text",
                    )
                    .map_or_else(Into::into, ReferenceFailure::Semantic)
                })?;
            }
            ctx.reserve_scoped_vec(
                &mut backing,
                &mut links,
                1,
                "Rhino transformed instance links",
            )?;
            let id = ctx.format_scoped_text(
                scratch,
                format_args!("{}", mesh.id.as_str()),
                "Rhino transformed instance links",
            )?;
            links.push(id);
            ctx.push_scoped_vec(
                &mut derived_storage,
                &mut derived_ids,
                mesh.id.as_str(),
                "Rhino transformed instance annotations",
            )?;
        }
        let mut subd_source = ir
            .model
            .subds
            .get_mut(before.arena_len::<cadmpeg_ir::SubdSurface>()..)
            .ok_or_else(|| "instance decode removed existing subdivision surfaces".to_string())?
            .iter_mut();
        let subd_count = subd_source.len();
        for _ in 0..subd_count {
            let subd = ctx
                .next_charged(&mut subd_source, "Rhino transformed entity traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Rhino transformed subdivision source ended early",
                    )
                })?;
            subd.cage
                .edit_vertices(
                    |vertices| {
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
                    },
                    ctx,
                )?
                .map_err(|error| {
                    ctx.format_retained(
                        format_args!("{error}"),
                        "Rhino transform_new_entities text",
                    )
                    .map_or_else(Into::into, ReferenceFailure::Semantic)
                })?;
            ctx.reserve_scoped_vec(
                &mut backing,
                &mut links,
                1,
                "Rhino transformed instance links",
            )?;
            let id = ctx.format_scoped_text(
                scratch,
                format_args!("{}", subd.id.as_str()),
                "Rhino transformed instance links",
            )?;
            links.push(id);
            ctx.push_scoped_vec(
                &mut derived_storage,
                &mut derived_ids,
                subd.id.as_str(),
                "Rhino transformed instance annotations",
            )?;
        }
        let procedural_curve_start = before.arena_len::<ProceduralCurve>();
        let procedural_surface_start = before.arena_len::<ProceduralSurface>();
        if ir.model.procedural_curves.len() > procedural_curve_start
            || ir.model.procedural_surfaces.len() > procedural_surface_start
        {
            let mut annotations = AnnotationBuilder::resume(std::mem::take(&mut self.annotations));
            ctx.fold(
                &ir.model.procedural_curves[procedural_curve_start..],
                (),
                |(), procedure| {
                    annotations.remove_entity(ctx, procedure.id.as_str())?;
                    Ok(())
                },
                "Rhino transform new entities traversal",
            )?;
            ctx.fold(
                &ir.model.procedural_surfaces[procedural_surface_start..],
                (),
                |(), procedure| {
                    annotations.remove_entity(ctx, procedure.id.as_str())?;
                    Ok(())
                },
                "Rhino transform new entities traversal",
            )?;
            self.annotations = annotations.build();
            ir.model.procedural_curves.truncate(procedural_curve_start);
            ir.model
                .procedural_surfaces
                .truncate(procedural_surface_start);
            self.report.phase_warnings.push_admitted(
                self.expand.ctx(),
                format_args!("instance: transformed procedural definition omitted; exact solved carrier retained"),
            )?;
        }
        let derived_id_count = derived_ids.len();
        let mut derived_ids = derived_ids.into_iter();
        for _ in 0..derived_id_count {
            let id = ctx
                .next_charged(&mut derived_ids, "Rhino derived identity traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino derived identity source ended early")
                })?;
            annotate_derived(self.expand.ctx(), &mut self.annotations, id)?;
        }
        Ok(InstanceLinks {
            values: links,
            _backing: backing,
        })
    }

    fn decode_subd(
        &mut self,
        source_order: usize,
        object: &ObjectDescriptor,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let Some(scale) = self.neutral_scale() else {
            self.scan_unbound_unit_warning(source_order, "SubD")?;
            return Ok(());
        };
        let identity = &object.identity;
        let Some((key_buffer, _key_storage)) = self.checked_object_key(identity, source_order)?
        else {
            return Ok(());
        };
        let key = key_buffer;
        let id = cadmpeg_ir::ids::SubdId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "subd"),
            key,
        );
        match crate::subd::decode(
            self.expand.ctx(),
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
                    )?;
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
                )?;
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
        let ctx = self.expand.ctx();
        let crate::subd::DecodedSubd {
            mut surface,
            neutral_metadata,
            enum_diagnostics,
            warnings,
        } = decoded;
        let class = self.scan.objects[source_order]
            .class_uuid()
            .unwrap_or_else(crate::wire::Uuid::nil);
        let phase_warnings = &mut self.report.phase_warnings;
        ctx.fold(
            &warnings[..],
            (),
            |(), warning| append_class_diagnostic(ctx, phase_warnings, class, warning),
            "Rhino decoded warning traversal",
        )?;
        let typed_losses = &mut self.report.typed_losses;
        ctx.fold(
            &enum_diagnostics[..],
            (),
            |(), diagnostic| {
                push_report_loss(
                    ctx,
                    typed_losses,
                    RhinoLossCode::EnumerationValueDegraded,
                    format_args!("{diagnostic}"),
                )
            },
            "Rhino SubD enumeration traversal",
        )?;
        if neutral_metadata {
            self.scan_warning(source_order, format_args!("SubD cache, texture, symmetry, or packing metadata is retained without a neutral-IR mapping"))?;
        }
        let Some(object) = self.scan.objects.get(source_order) else {
            return Ok(false);
        };
        let Some(identity) = object.identity() else {
            return Ok(false);
        };
        surface.source_object = Some(self.source_association(identity)?);
        let (id_buffer, _link_storage) = ctx.format_scoped(
            format_args!("{}", surface.id),
            "Rhino commit_subd_surface text",
        )?;
        let id = id_buffer;
        let result =
            self.validate_candidate_fallible(|candidate, candidate_annotations, arena_storage| {
                ctx.push_scoped_vec(
                    arena_storage,
                    &mut candidate.model.subds,
                    surface,
                    "Rhino candidate subds",
                )?;
                set_exactness(
                    ctx,
                    candidate_annotations,
                    &id,
                    if scaled {
                        Exactness::Derived
                    } else {
                        Exactness::ByteExact
                    },
                )?;
                Ok::<(), cadmpeg_core::CodecError>(())
            });
        match result {
            Ok(()) => {}
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(findings) => {
                self.scan_warning(
                    source_order,
                    format_args!("SubD validation rejected candidate: {findings}"),
                )?;
                return Ok(false);
            }
        }
        self.append_link(source_order, &id)?;
        self.geometry_transferred = true;
        Ok(true)
    }

    fn decode_extrusion(
        &mut self,
        source_order: usize,
        object: &ObjectDescriptor,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let ctx = self.expand.ctx();
        let Some(scale) = self.neutral_scale() else {
            self.scan_unbound_unit_warning(source_order, "extrusion")?;
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
                let class = self.scan.objects[source_order]
                    .class_uuid()
                    .unwrap_or_else(crate::wire::Uuid::nil);
                let phase_warnings = &mut self.report.phase_warnings;
                ctx.fold(
                    &(extrusion.warnings)[..],
                    (),
                    |(), warning| append_class_diagnostic(ctx, phase_warnings, class, warning),
                    "Rhino decode extrusion traversal",
                )?;
                if self.commit_extrusion(source_order, extrusion)? {
                    self.mark_decoded(source_order);
                } else {
                    self.scan_warning(
                        source_order,
                        format_args!("extrusion candidate rejected atomically"),
                    )?;
                    self.commit_unknown_surface(source_order)?;
                }
            }
            Err(crate::curves::GeometryError::Codec(error)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("extrusion degraded and retained: {error}"),
                )?;
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
        let phase_losses = &mut self.report.phase_losses;
        ctx.fold(
            &self.scan.metadata.losses[..],
            (),
            |(), loss| {
                ctx.reserve_vec(phase_losses, 1, "Rhino phase decode losses")?;
                phase_losses.push(loss.try_clone_for_decode(ctx, "Rhino phase decode loss copy")?);
                Ok(())
            },
            "Rhino commit traversal",
        )?;
        append_report_losses(
            ctx,
            &mut self.report.typed_losses,
            crate::annotations::install(ctx, self.scan, self.session.document_mut()?)?,
        )?;
        let document_data =
            crate::document_data::install(ctx, self.scan, self.session.document_mut()?)?;
        append_report_losses(ctx, &mut self.report.typed_losses, document_data.losses)?;
        ctx.fold(
            &document_data.opaque_records[..],
            (),
            |(), source| self.retain_opaque_record(source),
            "Rhino document source traversal",
        )?;
        let presentation =
            crate::presentation::install(ctx, self.scan, self.session.document_mut()?)?;
        append_report_losses(ctx, &mut self.report.typed_losses, presentation.losses)?;
        ctx.fold(
            &presentation.opaque_records[..],
            (),
            |(), source| self.retain_opaque_record(source),
            "Rhino presentation source traversal",
        )?;
        append_report_losses(
            ctx,
            &mut self.report.typed_losses,
            crate::product::install(ctx, self.scan, self.session.document_mut()?)?,
        )?;
        let views = crate::views::install(ctx, self.scan, self.session.document_mut()?)?;
        append_report_losses(ctx, &mut self.report.typed_losses, views.losses)?;
        ctx.fold(
            &views.opaque_records[..],
            (),
            |(), source| self.retain_opaque_record(source),
            "Rhino view source traversal",
        )?;
        self.sort_source_links()?;
        self.session.document_mut()?.finalize(ctx)?;
        let mut losses: Vec<LossNote> = Vec::new();
        let outcomes = self.class_outcomes()?;
        let decoded = ctx
            .admit_iter(&outcomes[..], "Rhino commit traversal")
            .map_err(cadmpeg_core::CodecError::from)?
            .map(|(_, outcome)| outcome.decoded)
            .sum::<usize>();
        let total = self.scan.objects.len();
        ctx.reserve_vec(&mut losses, 1, "Rhino final decode losses")?;
        losses.push(crate::wire::admitted_loss(
            ctx,
            RhinoLossCode::ObjectRecordCensus,
            format_args!("decoded {decoded}/{total} Rhino object records"),
            "Rhino final decode loss message",
        )?);
        let mut omissions: Vec<LossNote> = Vec::new();
        ctx.fold(
            &outcomes[..],
            (),
            |(), (class, outcome)| {
                if outcome.retained > 0 {
                    ctx.reserve_vec(&mut omissions, 1, "Rhino class omission losses")?;
                    omissions.push(
                        crate::wire::admitted_loss(
                            ctx,
                            RhinoLossCode::ObjectFamilyNotTransferred,
                            format_args!(
                            "retained {} object record(s) for class {class}; geometry is not decoded",
                            outcome.retained
                            ),
                            "Rhino final decode loss message",
                        )?
                        .with_provenance(loss_provenance(ctx, class, outcome)?),
                    );
                }
                if let Some((code, count)) = outcome.native {
                    ctx.reserve_vec(&mut omissions, 1, "Rhino class omission losses")?;
                    omissions.push(
                        crate::wire::admitted_loss(
                            ctx,
                            code,
                            format_args!(
                            "framed and read {} object record(s) for class {class}; construction \
                         state is retained as native passthrough",
                            count.get()
                        ),
                        "Rhino final decode loss message",
                    )?
                    .with_provenance(loss_provenance(ctx, class, outcome)?),
                    );
                }
                if outcome.attribute_degraded > 0 {
                    ctx.reserve_vec(&mut losses, 1, "Rhino final decode losses")?;
                    losses.push(
                        crate::wire::admitted_loss(
                            ctx,
                            RhinoLossCode::ObjectAttributesDegraded,
                            format_args!(
                            "{} object record(s) for class {class} have degraded attributes",
                            outcome.attribute_degraded
                        ),
                        "Rhino final decode loss message",
                    )?
                    .with_provenance(loss_provenance(ctx, class, outcome)?),
                    );
                }
                if outcome.failed_framed > 0 {
                    ctx.reserve_vec(&mut losses, 1, "Rhino final decode losses")?;
                    losses.push(
                        crate::wire::admitted_loss(
                            ctx,
                            RhinoLossCode::ObjectFramingUndecodable,
                            format_args!(
                            "{} framed object record(s) for class {class} could not be decoded",
                            outcome.failed_framed
                        ),
                        "Rhino final decode loss message",
                    )?
                    .with_provenance(loss_provenance(ctx, class, outcome)?),
                    );
                }
                Ok(())
            },
            "Rhino commit traversal",
        )?;
        ctx.extend_vec(
            &mut self.report.typed_losses,
            omissions,
            "Rhino typed decode losses",
        )?;
        ctx.fold(
            self.scan.definitions.diagnostics(),
            (),
            |(), diagnostic| {
                ctx.reserve_vec(&mut losses, 1, "Rhino final decode losses")?;
                losses.push(diagnostic.to_loss(ctx)?);
                Ok(())
            },
            "Rhino commit view traversal",
        )?;
        ctx.append_vec(
            &mut losses,
            &mut self.report.typed_losses,
            "Rhino final decode losses",
        )?;
        ctx.fold(
            &self.scan.warnings[..],
            (),
            |(), diagnostic| {
                ctx.reserve_vec(&mut losses, 1, "Rhino final decode losses")?;
                losses.push(crate::wire::admitted_loss(
                    ctx,
                    diagnostic
                        .code
                        .unwrap_or(RhinoLossCode::ContainerScanDiagnostic),
                    format_args!("{}", diagnostic.message),
                    "Rhino final decode loss message",
                )?);
                Ok(())
            },
            "Rhino commit traversal",
        )?;
        ctx.append_vec(
            &mut losses,
            &mut self.report.phase_losses,
            "Rhino final decode losses",
        )?;
        let mut phase_storage = ctx.reserve_scoped(0, "Rhino warning family scratch")?;
        let mut phase_families = BTreeMap::<&str, (usize, &str)>::new();
        ctx.fold(
            &self.report.phase_warnings[..],
            (),
            |(), diagnostic| {
                if let Some(code) = diagnostic.code {
                    ctx.reserve_vec(&mut losses, 1, "Rhino final decode losses")?;
                    losses.push(crate::wire::admitted_loss(
                        ctx,
                        code,
                        format_args!("{}", diagnostic.message),
                        "Rhino final decode loss message",
                    )?);
                    return Ok(());
                }
                let warning = &diagnostic.message;
                let (family, detail) =
                    match ctx.split_once(warning.as_str(), ":", "Rhino warning family split")? {
                        Some((family, detail)) => {
                            (family, ctx.trim_text(detail, "Rhino warning detail trim")?)
                        }
                        None => ("rhino", warning.as_str()),
                    };
                let entry = phase_storage
                    .with_storage(|| {
                        ctx.entry_btree_map(
                            &mut phase_families,
                            family,
                            "Rhino warning family groups",
                        )
                    })?
                    .or_insert((0, detail));
                entry.0 += 1;
                Ok(())
            },
            "Rhino commit traversal",
        )?;
        let phase_family_count = phase_families.len();
        let mut phase_families = phase_families.into_iter();
        for _ in 0..phase_family_count {
            let (family, (count, first)) = ctx
                .next_charged(&mut phase_families, "Rhino warning family traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino warning family source ended early")
                })?;
            ctx.reserve_vec(&mut losses, 1, "Rhino final decode losses")?;
            let loss = if count == 1 {
                crate::wire::admitted_loss(
                    ctx,
                    RhinoLossCode::ObjectDecodeDiagnostic,
                    format_args!("{family}: {first}"),
                    "Rhino final decode loss message",
                )?
            } else {
                crate::wire::admitted_loss(
                    ctx,
                    RhinoLossCode::ObjectDecodeDiagnostic,
                    format_args!("{family}: {count} decode warnings; first: {first}"),
                    "Rhino final decode loss message",
                )?
            };
            losses.push(loss);
        }
        let byte_records = ctx
            .admit_iter(self.session.unknowns(), "Rhino commit traversal")
            .map_err(cadmpeg_core::CodecError::from)?
            .filter(|record| record.data().is_some())
            .count()
            + ctx
                .admit_iter(&self.opaque_records[..], "Rhino commit traversal")
                .map_err(cadmpeg_core::CodecError::from)?
                .filter(|record| record.data().is_some())
                .count();
        let note = if self.opaque_records.is_empty() {
            ctx.format_retained(format_args!(
                "decoded {decoded}/{total} Rhino object records; retained metadata/digests for {} \
                 records and complete bytes for {byte_records}; document cap {} bytes, per-record cap {} bytes",
                self.session.unknowns().len(),
                RETAINED_DOCUMENT_CAP,
                RETAINED_RECORD_CAP
            ), "Rhino final decode note")?
        } else {
            ctx.format_retained(
                format_args!(
                "decoded {decoded}/{total} Rhino object records; retained metadata/digests for {} \
                 object records and {} opaque records, with complete bytes for {byte_records}; \
                 document cap {} bytes, per-record cap {} bytes",
                self.session.unknowns().len(),
                self.opaque_records.len(),
                RETAINED_DOCUMENT_CAP,
                RETAINED_RECORD_CAP
            ),
                "Rhino final decode note",
            )?
        };
        let mut notes = ctx.collection_vec(1, "Rhino final decode notes")?;
        notes.push(note);
        let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(self.annotations);
        let (mut ir, unknowns) = self.session.into_parts();
        source_fidelity.attach_native_unknown_records(&mut ir, "rhino", unknowns, ctx)?;
        source_fidelity.retain_unknown_records("rhino", self.opaque_records)?;
        let primary = crate::container::dialect_match(self.scan);
        // Charged from the admission the source records, so the document-level
        // residual admission and its loss cannot be reported apart.
        if let Some(loss) = crate::dialect::admission_loss(ctx, &primary)? {
            ctx.reserve_vec(&mut losses, 1, "Rhino final decode losses")?;
            losses.push(loss);
        }
        let attributes = full_source_attributes(self.expand.ctx(), self.scan)?;
        ir.source = Some(crate::container::source_meta(
            self.expand.ctx(),
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
        let mut records_buffer = Vec::new();
        let storage = self.expand.ctx().reserve_temporary_vec(
            &mut records_buffer,
            self.scan.objects.len(),
            "Rhino object unknown records",
        )?;
        let mut records = records_buffer;
        let (statuses_buffer, status_storage) = self
            .expand
            .ctx()
            .temporary_vec(self.scan.objects.len(), "Rhino object statuses")?;
        let mut statuses = statuses_buffer;
        let mut source_orders = 0..self.scan.objects.len();
        let source_order_count = source_orders.len();
        for _ in 0..source_order_count {
            let source_order = self
                .expand
                .ctx()
                .next_charged(&mut source_orders, "Rhino object traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino object record source ended early")
                })?;
            let object = &self.scan.objects[source_order];
            let range = object.range();
            let degraded = object.is_degraded();
            let id = Self::mint_unknown_id(source_order);
            let record = self.source_record(id, range)?;
            records.push(record);
            statuses.push(degraded.then_some(GeometryOutcome::Failed));
        }
        self.session.replace_unknowns(records)?;
        self.unknown_record_storage = Some(storage);
        self.statuses = statuses;
        self.status_storage = Some(status_storage);
        Ok(())
    }

    fn retain_opaque_records(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let ctx = self.expand.ctx();
        let mut indices = 0..self.scan.opaque_records.len();
        for _ in 0..self.scan.opaque_records.len() {
            let index = ctx
                .next_charged(&mut indices, "Rhino opaque source traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino opaque source ended early")
                })?;
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
        let ctx = self.expand.ctx();
        let mut indices = 0..self.scan.history.len();
        for _ in 0..self.scan.history.len() {
            let index = ctx
                .next_charged(&mut indices, "Rhino history source traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino history source ended early")
                })?;
            let record = &self.scan.history[index];
            if !ctx.any_by(&record.values[..], |value| Ok(matches!(&value.value, crate::history::Value::Geometries(values) if !values.is_empty())), "Rhino retain unbound history geometry traversal")? {
                continue;
            }
            let range = record.source_range.clone();
            let id = UnknownId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "history", "source"),
                IdentityKey::zero_padded(cadmpeg_core::decode::u64_from_index(range.start), 12),
            );
            ctx.reserve_vec(&mut self.opaque_records, 1, "Rhino history source records")?;
            let retained = self.source_record(id, range.clone())?;
            self.opaque_records.push(retained);
            self.report.phase_warnings.push_coded_admitted(
                ctx,
                RhinoLossCode::HistoryGeometryNotTransferred,
                format_args!(
                    "history record at source range {}..{} retained as complete source for {} unit binding",
                    range.start,
                    range.end,
                    binding.label()
                ),
            )?;
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
        self.expand.ctx().reserve_vec(
            &mut self.opaque_records,
            1,
            "Rhino opaque source records",
        )?;
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
                let data = self
                    .expand
                    .ctx()
                    .copy_retained(bytes, "Rhino source record bytes")?;
                self.retained_bytes = end;
                Ok(UnknownRecord::retained(id, offset, data, Vec::new()))
            }
            None => {
                let digest = Sha256Digest::digest_for_decode(
                    self.expand.ctx(),
                    bytes,
                    "Rhino source record digest",
                )?;
                Ok(UnknownRecord::unavailable(
                    id,
                    offset,
                    byte_len,
                    String::from(digest),
                    Vec::new(),
                ))
            }
        }
    }

    fn scan_warning(
        &mut self,
        source_order: usize,
        message: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let class = self.scan.objects[source_order]
            .class_uuid()
            .unwrap_or_else(crate::wire::Uuid::nil);
        self.report
            .phase_warnings
            .push_admitted(self.expand.ctx(), format_args!("{class}: {message}"))
    }

    fn scan_unbound_unit_warning(
        &mut self,
        source_order: usize,
        kind: &str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let binding = self.unit_binding();
        self.scan_warning(
            source_order,
            format_args!(
                "{kind} retained because the document has no physical millimetre binding ({})",
                binding.label()
            ),
        )
    }

    fn scan_diagnostic(
        &mut self,
        source_order: usize,
        diagnostic: &crate::loss::RhinoDiagnostic,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let class = self.scan.objects[source_order]
            .class_uuid()
            .unwrap_or_else(crate::wire::Uuid::nil);
        self.report.phase_warnings.push_coded_admitted(
            self.expand.ctx(),
            diagnostic.code,
            format_args!("{class}: {}", diagnostic.message),
        )
    }

    fn scan_warnings_for_class(
        &mut self,
        class: &str,
        message: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.report
            .phase_warnings
            .push_admitted(self.expand.ctx(), format_args!("{class}: {message}"))
    }

    fn charge_entities(&mut self, amount: usize) -> Result<(), cadmpeg_core::CodecError> {
        let mut budget = self.expansion_budget;
        budget.entities(self.expand.ctx(), amount)?;
        self.expand
            .ctx()
            .charge_entities(u64_from_index(amount), "rhino_instance_entities")?;
        self.expansion_budget = budget;
        Ok(())
    }

    fn commit_geometry(
        &mut self,
        source_order: usize,
        decoded: crate::curves::DecodedGeometry,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let ctx = self.expand.ctx();
        let Some(object) = self.scan.objects.get(source_order) else {
            return Ok(false);
        };
        let Some(identity) = object.identity() else {
            return Ok(false);
        };
        let Some((key_buffer, _key_storage)) = self.checked_object_key(identity, source_order)?
        else {
            return Ok(false);
        };
        let key = key_buffer;
        let (association_buffer, association_storage) = self
            .expand
            .ctx()
            .with_scoped_storage("Rhino borrowed source association", || {
                self.source_association(identity)
            })?;
        let association = association_buffer;
        let Some(unknown) = self
            .session
            .unknowns()
            .get(source_order)
            .map(|record| {
                record
                    .id()
                    .try_clone_for_decode(self.expand.ctx(), "Rhino unknown identity copy")
            })
            .transpose()?
        else {
            return Ok(false);
        };
        match decoded {
            crate::curves::DecodedGeometry::Point { position, scaled } => {
                self.charge_entities(5)?;
                let body_id = cadmpeg_ir::ids::BodyId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "body"),
                    key.clone(),
                );
                let region_id = cadmpeg_ir::ids::RegionId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "region"),
                    key.clone(),
                );
                let shell_id = cadmpeg_ir::ids::ShellId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "shell"),
                    key.clone(),
                );
                let point_id = cadmpeg_ir::ids::PointId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "point"),
                    key.clone(),
                );
                let vertex_id = cadmpeg_ir::ids::VertexId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "vertex"),
                    key.clone(),
                );
                self.expand.ctx().push_vec(
                    &mut self.session.document_mut()?.model.points,
                    Point::new(
                        point_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        position,
                        Some(
                            association
                                .try_clone_for_decode(ctx, "Rhino source association copy")?,
                        ),
                    ),
                    "Rhino committed points",
                )?;
                self.expand.ctx().push_vec(
                    &mut self.session.document_mut()?.model.vertices,
                    Vertex {
                        id: vertex_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        point: point_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        tolerance: None,
                    },
                    "Rhino committed vertices",
                )?;
                self.expand.ctx().push_vec(
                    &mut self.session.document_mut()?.model.shells,
                    {
                        let mut members = ctx.collection_vec(1, "Rhino point shell vertices")?;
                        members.push(
                            vertex_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        );
                        Shell::new(
                            shell_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                            region_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                            Vec::new(),
                            Vec::new(),
                            members,
                        )
                        .map_err(cadmpeg_core::CodecError::from)?
                    },
                    "Rhino committed shells",
                )?;
                self.expand.ctx().push_vec(
                    &mut self.session.document_mut()?.model.regions,
                    Region {
                        id: region_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        body: body_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        shells: {
                            let mut ids = ctx.collection_vec(1, "Rhino point region shells")?;
                            ids.push(
                                shell_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                            );
                            ids
                        },
                    },
                    "Rhino committed regions",
                )?;
                self.expand.ctx().push_vec(
                    &mut self.session.document_mut()?.model.bodies,
                    body(
                        ctx,
                        identity,
                        body_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        {
                            let mut ids = ctx.collection_vec(1, "Rhino point body regions")?;
                            ids.push(
                                region_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                            );
                            ids
                        },
                        &association,
                    )?,
                    "Rhino committed bodies",
                )?;
                self.annotate_point_topology(
                    &point_id, &vertex_id, &shell_id, &region_id, &body_id, scaled,
                )?;
                self.append_link(source_order, body_id.as_str())?;
            }
            crate::curves::DecodedGeometry::PointCloud(cloud) => {
                let crate::curves::PointCloud {
                    points,
                    scaled,
                    warnings,
                } = cloud;
                self.report.phase_warnings.append_prefixed_admitted(
                    self.expand.ctx(),
                    warnings,
                    format_args!("{}", identity.source_id),
                )?;
                let Some(entity_count) = points
                    .len()
                    .checked_mul(2)
                    .and_then(|count| count.checked_add(3))
                else {
                    self.scan_warning(
                        source_order,
                        format_args!("point-cloud entity count overflow"),
                    )?;
                    return Ok(false);
                };
                self.charge_entities(entity_count)?;
                let body_id = cadmpeg_ir::ids::BodyId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "body"),
                    key.clone(),
                );
                let region_id = cadmpeg_ir::ids::RegionId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "region"),
                    key.clone(),
                );
                let shell_id = cadmpeg_ir::ids::ShellId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "shell"),
                    key.clone(),
                );
                self.expand.ctx().charge_collection_items(
                    u64_from_index(points.len()),
                    "Rhino point-cloud vertices",
                )?;
                let mut vertices = Vec::new();
                ctx.reserve_capacity(&mut vertices, points.len(), "Rhino point-cloud vertices")?;
                let point_count = points.len();
                let mut points = points.into_iter().enumerate();
                for _ in 0..point_count {
                    let (index, position) = ctx
                        .next_charged(&mut points, "Rhino point-cloud point traversal")?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Rhino point-cloud source ended early",
                            )
                        })?;
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
                    set_exactness(
                        ctx,
                        &mut self.annotations,
                        &point_id,
                        if scaled {
                            Exactness::Derived
                        } else {
                            Exactness::ByteExact
                        },
                    )?;
                    self.expand.ctx().push_vec(
                        &mut self.session.document_mut()?.model.points,
                        Point::new(
                            point_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                            position,
                            Some(
                                association
                                    .try_clone_for_decode(ctx, "Rhino source association copy")?,
                            ),
                        ),
                        "Rhino committed points",
                    )?;
                    self.expand.ctx().push_vec(
                        &mut self.session.document_mut()?.model.vertices,
                        Vertex {
                            id: vertex_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                            point: point_id,
                            tolerance: None,
                        },
                        "Rhino committed vertices",
                    )?;
                    vertices.push(vertex_id);
                }
                let shell = match Shell::new(
                    shell_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    region_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    Vec::new(),
                    Vec::new(),
                    vertices,
                ) {
                    Ok(shell) => shell,
                    Err(error) => {
                        self.scan_warning(source_order, format_args!("{error}"))?;
                        return Ok(false);
                    }
                };
                self.expand.ctx().push_vec(
                    &mut self.session.document_mut()?.model.shells,
                    shell,
                    "Rhino committed shells",
                )?;
                self.expand.ctx().push_vec(
                    &mut self.session.document_mut()?.model.regions,
                    Region {
                        id: region_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        body: body_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        shells: {
                            let mut ids =
                                ctx.collection_vec(1, "Rhino point-cloud region shells")?;
                            ids.push(shell_id);
                            ids
                        },
                    },
                    "Rhino committed regions",
                )?;
                self.expand.ctx().push_vec(
                    &mut self.session.document_mut()?.model.bodies,
                    body(
                        ctx,
                        identity,
                        body_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        {
                            let mut ids =
                                ctx.collection_vec(1, "Rhino point-cloud body regions")?;
                            ids.push(region_id);
                            ids
                        },
                        &association,
                    )?,
                    "Rhino committed bodies",
                )?;
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
                let parent_id = match self.validate_candidate_fallible(
                    |candidate, annotations, arena_storage| {
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
                            &mut *arena_storage,
                        )
                    },
                ) {
                    Ok(id) => id,
                    Err(CandidateError::Codec(error)) => return Err(error),
                    Err(error) => {
                        self.report.phase_warnings.push_admitted(
                            ctx,
                            format_args!("curve candidate rejected: {error}"),
                        )?;
                        return Ok(false);
                    }
                };
                self.append_link(source_order, parent_id.as_str())?;
            }
            crate::curves::DecodedGeometry::Surface { surface } => match surface {
                crate::surfaces::DecodedSurface::Typed {
                    geometry, derived, ..
                } => {
                    self.charge_entities(1)?;
                    let surface_id = cadmpeg_ir::ids::SurfaceId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
                        key.clone(),
                    );
                    self.expand.ctx().push_vec(
                        &mut self.session.document_mut()?.model.surfaces,
                        Surface {
                            id: surface_id
                                .try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                            geometry: geometry.into_geometry(),
                            source_object: Some(
                                association
                                    .try_clone_for_decode(ctx, "Rhino source association copy")?,
                            ),
                        },
                        "Rhino committed surfaces",
                    )?;
                    set_exactness(
                        ctx,
                        &mut self.annotations,
                        &surface_id,
                        if derived {
                            Exactness::Derived
                        } else {
                            Exactness::ByteExact
                        },
                    )?;
                    self.append_link(source_order, surface_id.as_str())?;
                }
                crate::surfaces::DecodedSurface::Procedural {
                    geometry,
                    definition,
                } => {
                    let association = association_storage.commit_value(association)?;
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
        let ctx = self.expand.ctx();
        let Some(unknown) = self
            .session
            .unknowns()
            .get(source_order)
            .map(|record| {
                record
                    .id()
                    .try_clone_for_decode(self.expand.ctx(), "Rhino unknown identity copy")
            })
            .transpose()?
        else {
            return Ok(false);
        };
        let session = self.expand.ctx();
        let result =
            self.validate_candidate_fallible(|candidate, candidate_annotations, arena_storage| {
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
                                record: Some(
                                    unknown
                                        .try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                                ),
                                path,
                            },
                            &mut *arena_storage,
                        )
                    },
                    |error| CandidateError::Admission(error.to_string()),
                )?;
                let key = IdentityKey::try_new(key.to_owned()).map_err(|error| {
                    ctx.format_retained(
                        format_args!("{error}"),
                        "Rhino commit_procedural_surface text",
                    )
                    .map_or_else(Into::into, CandidateError::Admission)
                })?;
                let surface_id = cadmpeg_ir::ids::SurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
                    key.clone(),
                );
                ctx.push_scoped_vec(
                    arena_storage,
                    &mut candidate.model.surfaces,
                    Surface {
                        id: surface_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(geometry)),
                        source_object: Some(association),
                    },
                    "Rhino candidate surfaces",
                )?;
                let procedural_id = cadmpeg_ir::ids::ProceduralSurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "procedural-surface"),
                    key.clone(),
                );
                candidate
                    .model
                    .add_procedural_surface(
                        ctx,
                        &surface_id,
                        ProceduralSurface::new(
                            procedural_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                            ir_definition,
                            None,
                        ),
                    )?
                    .map_err(|error| {
                        ctx.format_retained(
                            format_args!("{error}"),
                            "Rhino commit_procedural_surface text",
                        )
                        .map_or_else(Into::into, CandidateError::Admission)
                    })?;
                for id in [surface_id.as_str(), procedural_id.as_str()] {
                    set_exactness(ctx, candidate_annotations, id, Exactness::Derived)?;
                }
                Ok::<_, CandidateError>(surface_id)
            });
        let link = match result {
            Ok(link) => link,
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(findings) => {
                self.report.phase_warnings.push_admitted(
                    self.expand.ctx(),
                    format_args!(
                        "procedural-surface: candidate rejected by IR validation: {findings}"
                    ),
                )?;
                return Ok(false);
            }
        };
        self.append_link(source_order, link.as_str())?;
        self.geometry_transferred = true;
        Ok(true)
    }

    fn commit_extrusion(
        &mut self,
        source_order: usize,
        mut extrusion: crate::extrusion::DecodedExtrusion<'_>,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let ctx = self.expand.ctx();
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
            .map(|record| {
                record
                    .id()
                    .try_clone_for_decode(self.expand.ctx(), "Rhino unknown identity copy")
            })
            .transpose()?
        else {
            return Ok(false);
        };
        let Some((key_buffer, _key_storage)) = self.checked_object_key(identity, source_order)?
        else {
            return Ok(false);
        };
        let key = key_buffer;
        if extrusion.boundaries.is_empty() {
            return Ok(false);
        }
        let (association_buffer, _association_storage) = self
            .expand
            .ctx()
            .with_scoped_storage("Rhino borrowed source association", || {
                self.source_association(identity)
            })?;
        let association = association_buffer;
        let session = self.expand.ctx();
        let mut source_boundaries = std::mem::take(&mut extrusion.boundaries);
        let mut link_storage = ctx.reserve_scoped(0, "Rhino extrusion link scratch")?;
        let result = self.validate_candidate_fallible(|candidate, candidate_annotations, arena_storage| {
            let mut links = Vec::new();
            let (boundaries_buffer, _boundary_storage) = session.temporary_vec(source_boundaries.len(), "Rhino committed extrusion boundaries")?;
            let mut boundaries = boundaries_buffer;
            let source_profile_count = source_boundaries.len();
            let mut source_profiles = source_boundaries.iter_mut().enumerate();
            for _ in 0..source_profile_count {
                let (index, boundary) = session
                    .next_charged(&mut source_profiles, "Rhino extrusion profile traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Rhino extrusion profile source ended early",
                        )
                    })?;
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
                        record: Some(unknown.try_clone_for_decode(ctx, "Rhino typed identity copy")?),
                        path: &ctx.format_scoped(format_args!("profile-{index}.start"), "Rhino extrusion profile path")?.0,
                    }, &mut *arena_storage,)?;
                boundaries.push(CommittedExtrusionBoundary {
                    boundary,
                    directrix: id,
                });
            }
            let mut committed_boundaries = boundaries.iter().enumerate();
            for _ in 0..boundaries.len() {
                let (index, boundary) = ctx
                    .next_charged(
                        &mut committed_boundaries,
                        "Rhino committed extrusion boundary traversal",
                    )?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Rhino committed extrusion boundary source ended early",
                        )
                    })?;
                let surface_id = cadmpeg_ir::ids::SurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".lateral-"))
                        .then(index),
                );
                let procedure_id = cadmpeg_ir::ids::ProceduralSurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "procedural-surface"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".lateral-"))
                        .then(index),
                );
                ctx.push_scoped_vec(arena_storage, &mut candidate.model.surfaces, Surface {
                    id: surface_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                        boundary.boundary.lateral.try_clone_for_decode(ctx, "Rhino extrusion lateral surface copy")?,
                    )),
                    source_object: Some(association.try_clone_for_decode(ctx, "Rhino source association copy")?),
                }, "Rhino candidate surfaces")?;
                candidate
                    .model
                    .add_procedural_surface(ctx, &surface_id, cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                            boundary.directrix.try_clone_for_decode(ctx, "Rhino extrusion directrix identity copy")?,
                            None,
                            extrusion.direction,
                            None,
                            cadmpeg_ir::geometry::CacheContract::from_form(None),
                        )
                        .map_err(|error| ctx.format_retained(format_args!("{error}"), "Rhino commit_extrusion text").map_or_else(Into::into, CandidateError::Admission))
                        .and_then(|admitted_payload| {
                            Ok::<_, CandidateError>(ProceduralSurface::new(
                                procedure_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                                ProceduralSurfaceDefinition::Extrusion(admitted_payload),
                                None,
                            ))
                        })
                        ?)?
                    .map_err(|error| ctx.format_retained(format_args!("{error}"), "Rhino commit_extrusion text").map_or_else(Into::into, CandidateError::Admission))?;
                {
                    let (identity_text_buffer, _identity_storage) = ctx.format_scoped(format_args!("{surface_id}"), "Rhino commit_extrusion text")?;
                    let identity_text = identity_text_buffer;
                    annotate_derived(ctx, candidate_annotations, &identity_text)?;
                }
                {
                    let (identity_text_buffer, _identity_storage) = ctx.format_scoped(format_args!("{procedure_id}"), "Rhino commit_extrusion text")?;
                    let identity_text = identity_text_buffer;
                    annotate_derived(ctx, candidate_annotations, &identity_text)?;
                }
                let link = ctx.format_scoped_text(&mut link_storage, format_args!("{surface_id}"), "Rhino commit_extrusion text")?;
                ctx.push_scoped_vec(&mut link_storage, &mut links, link, "Rhino extrusion links")?;
            }
            if extrusion.caps[0] || extrusion.caps[1] {
                let cap_id = stage_extrusion_caps((session, &mut *arena_storage),
                    candidate,
                    candidate_annotations,
                    key.as_str(),
                    &association,
                    &extrusion,
                    &boundaries,
                )?;
                let link = ctx.format_scoped_text(&mut link_storage, format_args!("{cap_id}"), "Rhino extrusion cap link")?;
                ctx.push_scoped_vec(&mut link_storage, &mut links, link, "Rhino extrusion links")?;
            }
            let mut meshes = extrusion.meshes.into_iter().enumerate();
            for _ in 0..meshes.len() {
                let (index, mut mesh) = ctx
                    .next_charged(&mut meshes, "Rhino extrusion mesh traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Rhino extrusion mesh source ended early",
                        )
                    })?;
                let id = ctx.format_retained(
                    format_args!("rhino:object:tessellation#{key}.cache-{index}"),
                    "Rhino extrusion cache tessellation identity",
                )?;
                                mesh.tessellation.id = cadmpeg_ir::tessellation::TessellationId::mint(id)
                    .map_err(|error| {
                        ctx.format_retained(
                            format_args!("{error}"),
                            "Rhino extrusion cache tessellation identity error",
                        )
                        .map_or_else(CandidateError::Codec, CandidateError::Admission)
                    })?;
                mesh.tessellation.source_object = Some(association.try_clone_for_decode(ctx, "Rhino source association copy")?);
                annotate_derived(ctx, candidate_annotations, mesh.tessellation.id.as_str())?;
                let link = ctx.format_scoped_text(&mut link_storage, format_args!("{}", mesh.tessellation.id), "Rhino commit_extrusion text")?;
                ctx.push_scoped_vec(&mut link_storage, &mut links, link, "Rhino extrusion links")?;
                ctx.push_scoped_vec(arena_storage, &mut candidate.model.tessellations, mesh.tessellation, "Rhino candidate tessellations")?;
            }
            Ok::<_, CandidateError>(links)
        });
        let links = match result {
            Ok(links) => links,
            Err(CandidateError::Admission(error)) => {
                self.scan_warning(source_order, format_args!("{error}"))?;
                return Ok(false);
            }
            Err(CandidateError::Validation(findings)) => {
                self.scan_warning(
                    source_order,
                    format_args!("extrusion candidate rejected by IR validation: {findings}"),
                )?;
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
        let ctx = self.expand.ctx();
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
            .map(|record| {
                record
                    .id()
                    .try_clone_for_decode(self.expand.ctx(), "Rhino unknown identity copy")
            })
            .transpose()?
        else {
            return Ok(());
        };
        let Some((key_buffer, _key_storage)) = self.checked_object_key(identity, source_order)?
        else {
            return Ok(());
        };
        let key = key_buffer;
        let id = cadmpeg_ir::ids::SurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
            key,
        );
        let association = self.source_association(identity)?;
        let validation =
            self.validate_candidate_fallible(|candidate, candidate_annotations, arena_storage| {
                ctx.push_scoped_vec(
                    arena_storage,
                    &mut candidate.model.surfaces,
                    Surface {
                        id: id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                            record: Some(
                                unknown.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                            ),
                        }),
                        source_object: Some(association),
                    },
                    "Rhino candidate surfaces",
                )?;
                set_exactness(ctx, candidate_annotations, &id, Exactness::Unknown)?;
                Ok::<_, CandidateError>(())
            });
        match validation {
            Ok(()) => {
                self.append_link(source_order, id.as_str())?;
            }
            Err(CandidateError::Codec(error)) => return Err(error),
            Err(findings) => self.scan_warning(
                source_order,
                format_args!("unknown surface validation rejected candidate: {findings}"),
            )?,
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
    ) -> Result<(), cadmpeg_core::CodecError> {
        let point_exactness = if scaled {
            Exactness::Derived
        } else {
            Exactness::ByteExact
        };
        set_exactness(
            self.expand.ctx(),
            &mut self.annotations,
            point,
            point_exactness,
        )?;
        for id in [
            vertex.as_str(),
            shell.as_str(),
            region.as_str(),
            body.as_str(),
        ] {
            set_exactness(
                self.expand.ctx(),
                &mut self.annotations,
                id,
                Exactness::Derived,
            )?;
        }
        Ok(())
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
        let ctx = self.expand.ctx();
        self.charge_entities(1)?;
        let mut mesh_losses = mesh.losses.into_iter();
        for _ in 0..mesh_losses.len() {
            let mut loss = ctx
                .next_charged(&mut mesh_losses, "Rhino mesh loss traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino mesh loss source ended early")
                })?;
            ctx.reserve_vec(
                &mut self.report.phase_losses,
                1,
                "Rhino phase decode losses",
            )?;
            loss.message = ctx.format_retained(
                format_args!("{}: {}", identity.source_id, loss.message),
                "Rhino phase decode loss message",
            )?;
            self.report.phase_losses.push(loss);
        }
        self.report.phase_warnings.append_prefixed_admitted(
            ctx,
            mesh.warnings,
            format_args!("{}", identity.source_id),
        )?;
        let (id_buffer, _link_storage) = self.expand.ctx().format_scoped(
            format_args!("{}", mesh.tessellation.id),
            "Rhino commit_mesh text",
        )?;
        let id = id_buffer;
        let mut tessellation = mesh.tessellation;
        tessellation.source_object = Some(self.source_association(identity)?);
        self.expand.ctx().push_vec(
            &mut self.session.document_mut()?.model.tessellations,
            tessellation,
            "Rhino committed tessellations",
        )?;
        set_exactness(
            self.expand.ctx(),
            &mut self.annotations,
            &id,
            if mesh.scaled || mesh.quad_count != 0 {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        )?;
        if mesh.ngon_count != 0 {
            push_report_loss(
                self.expand.ctx(),
                &mut self.report.typed_losses,
                RhinoLossCode::MeshNgonGroupingDropped,
                format_args!(
                    "{} n-gon grouping record(s) were not transferred for mesh {id}",
                    mesh.ngon_count
                ),
            )?;
        }
        if mesh.quad_count != 0 {
            push_report_loss(
                self.expand.ctx(),
                &mut self.report.typed_losses,
                RhinoLossCode::MeshQuadTopologyTriangulated,
                format_args!(
                    "{} quadrilateral face(s) were triangulated for mesh {id}",
                    mesh.quad_count
                ),
            )?;
        }
        self.append_link(source_order, &id)?;
        Ok(true)
    }

    fn decode_brep(
        &mut self,
        source_order: usize,
        object: &ObjectDescriptor,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let ctx = self.expand.ctx();
        let parsed = crate::brep::parse(
            ctx,
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
                )?;
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
        let class = object.class_uuid;
        let phase_warnings = &mut self.report.phase_warnings;
        let typed_losses = &mut self.report.typed_losses;
        ctx.fold(
            &warnings[..],
            (),
            |(), warning| match warning.code {
                Some(code @ RhinoLossCode::EnumerationValueDegraded) => {
                    push_report_loss(ctx, typed_losses, code, format_args!("{}", warning.message))
                }
                _ => append_class_diagnostic(ctx, phase_warnings, class, warning),
            },
            "Rhino decoded warning traversal",
        )?;
        let identity = &object.identity;
        let phase_losses = &mut self.report.phase_losses;
        ctx.fold(
            &raw.losses[..],
            (),
            |(), loss| {
                ctx.reserve_vec(phase_losses, 1, "Rhino phase decode losses")?;
                let mut copied = loss.try_clone_for_decode(ctx, "Rhino phase decode loss copy")?;
                copied.message = ctx.format_retained(
                    format_args!("{}: {}", object.class_uuid, loss.message),
                    "Rhino phase decode loss message",
                )?;
                phase_losses.push(copied);
                Ok(())
            },
            "Rhino decode brep traversal",
        )?;
        let Some(scale) = self.neutral_scale() else {
            self.scan_unbound_unit_warning(source_order, "Brep")?;
            return Ok(());
        };
        let (association_buffer, _association_storage) = self
            .expand
            .ctx()
            .with_scoped_storage("Rhino borrowed source association", || {
                self.source_association(identity)
            })?;
        let association = association_buffer;
        let Some((key_buffer, _key_storage)) = self.checked_object_key(identity, source_order)?
        else {
            return Ok(());
        };
        let key = key_buffer;
        let unknown = self.session.unknowns()[source_order]
            .id()
            .try_clone_for_decode(self.expand.ctx(), "Rhino unknown identity copy")?;
        let mut arena_storage = self
            .expand
            .ctx()
            .reserve_scoped(0, "Rhino Brep arena scratch")?;
        let mut metadata_storage = self
            .expand
            .ctx()
            .reserve_scoped(0, "Rhino Brep link scratch")?;
        let staged = ctx.with_scoped_storage("Rhino speculative Brep fields", || match &parsed {
            crate::brep::BrepParse::Valid(brep) => stage_brep(
                BrepTransferInput {
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
                },
                &mut arena_storage,
                &mut metadata_storage,
            ),
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
                &mut arena_storage,
                &mut metadata_storage,
            ),
        });
        match staged {
            Ok((staged, field_storage)) => {
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
                self.flush_seeded_source_links()?;
                budget.entities(self.expand.ctx(), entity_count)?;
                let committed = match cadmpeg_ir::validate::admit::validate_native_unknowns(
                    self.expand.ctx(),
                    self.session.unknowns(),
                )? {
                    Ok(()) => match self.session.commit(draft, &mut self.annotations)? {
                        Ok(()) => Ok(()),
                        Err(error) => Err(self.expand.ctx().format_retained(
                            format_args!("{error}"),
                            "Rhino draft admission message",
                        )?),
                    },
                    Err(error) => {
                        drop(draft);
                        Err(self.expand.ctx().format_retained(
                            format_args!("{error}"),
                            "Rhino native admission message",
                        )?)
                    }
                };
                drop(arena_storage);
                if let Err(error) = committed {
                    self.scan_warning(
                        source_order,
                        format_args!("Brep draft rejected before commit: {error}"),
                    )?;
                } else {
                    field_storage.commit()?;
                    self.expansion_budget = budget;
                    self.append_links(source_order, &links)?;
                    self.expand.ctx().extend_vec(
                        &mut self.report.typed_losses,
                        typed_losses,
                        "Rhino typed decode losses",
                    )?;
                    let class = self.scan.objects[source_order]
                        .class_uuid()
                        .unwrap_or_else(crate::wire::Uuid::nil);
                    let phase_warnings = &mut self.report.phase_warnings;
                    let typed_losses = &mut self.report.typed_losses;
                    ctx.fold(
                        &warnings[..],
                        (),
                        |(), warning| match warning.code {
                            Some(
                                code @ (RhinoLossCode::TopologyBrepFallback
                                | RhinoLossCode::PolycurveJoinGap
                                | RhinoLossCode::TrimPcurveDropped),
                            ) => push_report_loss(
                                ctx,
                                typed_losses,
                                code,
                                format_args!("{}", warning.message),
                            ),
                            _ => append_class_diagnostic(ctx, phase_warnings, class, warning),
                        },
                        "Rhino decoded warning traversal",
                    )?;
                    if cache_only {
                        self.scan_warning(
                            source_order,
                            format_args!(
                                "Brep emitted cache tessellations without decoded geometry"
                            ),
                        )?;
                    }
                    self.geometry_transferred |= full_topology || emitted_geometry;
                    if full_topology {
                        self.mark_decoded(source_order);
                    } else {
                        self.scan_warning(
                            source_order,
                            format_args!("Brep topology invalid; decoded child carriers retained"),
                        )?;
                    }
                }
            }
            Err(crate::curves::GeometryError::Codec(error)) => return Err(error),
            Err(error) => {
                self.scan_warning(
                    source_order,
                    format_args!("Brep geometry/topology degraded: {error}"),
                )?;
            }
        }
        Ok(())
    }

    fn transition(&mut self, source_order: usize, next: GeometryOutcome) -> bool {
        let Some(status @ None) = self.statuses.get_mut(source_order) else {
            return false;
        };
        *status = Some(next);
        true
    }

    fn class_outcomes(&self) -> Result<Vec<(String, ClassOutcome<'a>)>, cadmpeg_core::CodecError> {
        let ctx = self.expand.ctx();
        let mut storage = ctx.reserve_scoped(0, "Rhino class outcome scratch")?;
        let mut outcomes = BTreeMap::new();
        let mut sources = self.scan.objects[..self.scan.objects.len().min(self.statuses.len())]
            .iter()
            .zip(&self.statuses);
        let source_count = self.scan.objects.len().min(self.statuses.len());
        for _ in 0..source_count {
            let (object, status) = ctx
                .next_charged(&mut sources, "Rhino class outcomes traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino class outcome source ended early")
                })?;
            let class = object.class_uuid().unwrap_or_else(crate::wire::Uuid::nil);
            let outcome = storage
                .with_storage(|| {
                    ctx.entry_btree_map(&mut outcomes, class, "Rhino class outcome keys")
                })?
                .or_insert_with(|| ClassOutcome {
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
        let mut sorted = self
            .lookup_storage
            .borrow_mut()
            .with_storage(|| ctx.collection_vec(outcomes.len(), "Rhino class outcome rows"))?;
        let outcome_count = outcomes.len();
        let mut outcomes = outcomes.into_iter();
        for _ in 0..outcome_count {
            let (class, outcome) = ctx
                .next_charged(&mut outcomes, "Rhino class outcome rows traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino class outcome map ended early")
                })?;
            let label = self.lookup_storage.borrow_mut().with_storage(|| {
                ctx.format_retained(format_args!("{class}"), "Rhino class outcome label")
            })?;
            sorted.push((label, outcome));
        }
        Ok(sorted)
    }
}

fn duplicate_userdata_count(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    userdata: &[UserdataDescriptor],
    class: crate::wire::Uuid,
) -> Result<usize, cadmpeg_core::CodecError> {
    Ok(ctx
        .admit_iter(userdata, "Rhino duplicate userdata traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .filter_map(UserdataDescriptor::known)
        .filter(|value| value.class_uuid == class)
        .count())
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

fn validation_findings(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    report: &cadmpeg_ir::report::check::ValidationReport,
) -> Result<String, cadmpeg_core::CodecError> {
    let mut selected = [None; 3];
    let mut count = 0;
    ctx.any_by(
        &report.findings,
        |finding| {
            if finding.severity >= Severity::Error {
                selected[count] = Some(finding);
                count += 1;
            }
            Ok(count == selected.len())
        },
        "Rhino admission finding scan",
    )?;
    ctx.format_retained(
        format_args!("{}", ValidationFindings(selected)),
        "Rhino admission finding message",
    )
}

fn annotate_derived(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    annotations: &mut cadmpeg_ir::Annotations,
    id: &str,
) -> Result<(), cadmpeg_core::CodecError> {
    set_exactness(ctx, annotations, id, Exactness::Derived)
}

fn set_exactness(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    annotations: &mut cadmpeg_ir::Annotations,
    id: impl std::fmt::Display,
    exactness: Exactness,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    let result = builder.exactness(ctx, id, exactness).map(|_| ());
    *annotations = builder.build();
    result
}

struct CommittedExtrusionBoundary<'a> {
    boundary: &'a crate::extrusion::ExtrusionBoundary,
    directrix: cadmpeg_ir::ids::CurveId,
}

fn stage_extrusion_caps(
    scope: (
        &cadmpeg_core::decode::DecodeContext<'_>,
        &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ),
    ir: &mut CadIr,
    annotations: &mut cadmpeg_ir::Annotations,
    key: &str,
    association: &SourceObjectAssociation,
    extrusion: &crate::extrusion::DecodedExtrusion<'_>,
    boundaries: &[CommittedExtrusionBoundary<'_>],
) -> Result<cadmpeg_ir::ids::BodyId, CandidateError> {
    let (ctx, arena_storage) = scope;
    let key = IdentityKey::try_new(key.to_owned()).map_err(|error| {
        ctx.format_retained(format_args!("{error}"), "Rhino stage_extrusion_caps text")
            .map_or_else(Into::into, CandidateError::Admission)
    })?;
    let body_id = cadmpeg_ir::ids::BodyId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "object", "body"),
        key.clone().then(cadmpeg_ir::identity_key!(".caps")),
    );
    let mut region_ids = Vec::new();
    for cap in 0..2 {
        if !extrusion.caps[cap] {
            continue;
        }
        let cap_key = key
            .clone()
            .then(cadmpeg_ir::identity_key!(".cap-"))
            .then(cap);
        let region_id = cadmpeg_ir::ids::RegionId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "region"),
            cap_key.clone(),
        );
        let shell_id = cadmpeg_ir::ids::ShellId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "shell"),
            cap_key.clone(),
        );
        let surface_id = cadmpeg_ir::ids::SurfaceId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
            cap_key.clone(),
        );
        let face_id = cadmpeg_ir::ids::FaceId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "face"),
            cap_key.clone(),
        );
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
        ctx.push_scoped_vec(
            arena_storage,
            &mut ir.model.surfaces,
            Surface {
                id: surface_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::new(origin, frame),
                )),
                source_object: Some(
                    association.try_clone_for_decode(ctx, "Rhino source association copy")?,
                ),
            },
            "Rhino extrusion cap surfaces arena",
        )?;
        let mut loop_ids = ctx.collection_vec(boundaries.len(), "Rhino extrusion cap loop IDs")?;
        let mut cap_boundaries = boundaries.iter().enumerate();
        for _ in 0..boundaries.len() {
            let (profile, committed) = ctx
                .next_charged(&mut cap_boundaries, "Rhino stage extrusion caps traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Rhino extrusion cap boundary source ended early",
                    )
                })?;
            let boundary = committed.boundary;
            let suffix = key
                .clone()
                .then(cadmpeg_ir::identity_key!(".cap-"))
                .then(cap)
                .then(cadmpeg_ir::identity_key!(".profile-"))
                .then(profile);
            let curve_id = if cap == 0 {
                committed
                    .directrix
                    .try_clone_for_decode(ctx, "Rhino extrusion cap directrix identity copy")?
            } else {
                let id = cadmpeg_ir::ids::CurveId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "curve"),
                    suffix.clone(),
                );
                ctx.push_scoped_vec(
                    arena_storage,
                    &mut ir.model.curves,
                    Curve {
                        id: id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                            boundary
                                .end_nurbs
                                .try_clone_for_decode(ctx, "Rhino extrusion end curve copy")?,
                        )),
                        source_object: Some(
                            association
                                .try_clone_for_decode(ctx, "Rhino source association copy")?,
                        ),
                    },
                    "Rhino extrusion cap curves arena",
                )?;
                {
                    let (identity_text_buffer, _identity_storage) =
                        ctx.format_scoped(format_args!("{id}"), "Rhino stage_extrusion_caps text")?;
                    let identity_text = identity_text_buffer;
                    annotate_derived(ctx, annotations, &identity_text)?;
                }
                id
            };
            let endpoint_curve = if cap == 0 {
                &boundary.start_nurbs
            } else {
                &boundary.end_nurbs
            };
            let endpoint = match endpoint_curve.pole_rows() {
                cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
                    points.first().copied()
                }
                cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
                    points.first().map(|pole| pole.point)
                }
            };
            let Some(endpoint) = endpoint else {
                return Err(format!(
                    "extrusion cap staging: cap {cap} profile {profile} has no endpoint"
                )
                .into());
            };
            let point_id = cadmpeg_ir::ids::PointId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "point"),
                suffix.clone(),
            );
            let vertex_id = cadmpeg_ir::ids::VertexId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "vertex"),
                suffix.clone(),
            );
            let edge_id = cadmpeg_ir::ids::EdgeId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "edge"),
                suffix.clone(),
            );
            let loop_id = cadmpeg_ir::ids::LoopId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "loop"),
                suffix.clone(),
            );
            let coedge_id = cadmpeg_ir::ids::CoedgeId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "coedge"),
                suffix.clone(),
            );
            let pcurve_id = cadmpeg_ir::ids::PcurveId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "pcurve"),
                suffix.clone(),
            );
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
                ctx,
                pcurve.degree,
                ctx.copy_slice(&pcurve.knots, "Rhino extrusion cap pcurve knots")?,
                ctx.copy_slice(&pcurve.control_points, "Rhino extrusion cap pcurve poles")?,
                pcurve
                    .weights
                    .as_ref()
                    .map(|weights| ctx.copy_slice(weights, "Rhino extrusion cap pcurve weights"))
                    .transpose()?,
                pcurve.periodic,
            )?
            .map_err(|error| {
                ctx.format_retained(
                    format_args!("extrusion cap staging: {error}"),
                    "Rhino stage_extrusion_caps text",
                )
                .map_or_else(Into::into, CandidateError::Admission)
            })?;
            let carrier =
                cadmpeg_ir::topology::EdgeCarrier::new(Some(curve_id), Some(parameter_range))
                    .map_err(|error| {
                        ctx.format_retained(
                            format_args!("extrusion cap staging: {error}"),
                            "Rhino stage_extrusion_caps text",
                        )
                        .map_or_else(Into::into, CandidateError::Admission)
                    })?;
            ctx.push_scoped_vec(
                arena_storage,
                &mut ir.model.points,
                Point::new(
                    point_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    endpoint,
                    Some(association.try_clone_for_decode(ctx, "Rhino source association copy")?),
                ),
                "Rhino extrusion cap points arena",
            )?;
            ctx.push_scoped_vec(
                arena_storage,
                &mut ir.model.vertices,
                Vertex {
                    id: vertex_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    point: point_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    tolerance: None,
                },
                "Rhino extrusion cap vertices arena",
            )?;
            ctx.push_scoped_vec(
                arena_storage,
                &mut ir.model.edges,
                Edge {
                    id: edge_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    carrier,
                    start: vertex_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    end: vertex_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    tolerance: None,
                },
                "Rhino extrusion cap edges arena",
            )?;
            ctx.push_scoped_vec(arena_storage, &mut ir.model.pcurves, Pcurve {
                id: pcurve_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
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
            }, "Rhino extrusion cap pcurves arena")?;
            ctx.push_scoped_vec(
                arena_storage,
                &mut ir.model.coedges,
                Coedge {
                    id: coedge_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    owner_loop: loop_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    edge: edge_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    radial_next: coedge_id
                        .try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    sense: Sense::Forward,
                    pcurves: {
                        let mut uses =
                            ctx.collection_vec(1, "Rhino extrusion cap coedge pcurves")?;
                        uses.push(cadmpeg_ir::topology::PcurveUse {
                            pcurve: pcurve_id
                                .try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                            isoparametric: None,
                            parameter_range: None,
                        });
                        uses
                    },
                    use_curve: None,
                },
                "Rhino extrusion cap coedges arena",
            )?;
            ctx.push_scoped_vec(
                arena_storage,
                &mut ir.model.loops,
                Loop {
                    id: loop_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    face: face_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                        cadmpeg_ir::topology::LoopRing::new(
                            ctx,
                            {
                                let mut ids =
                                    ctx.collection_vec(1, "Rhino extrusion cap ring coedges")?;
                                ids.push(
                                    coedge_id
                                        .try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                                );
                                ids
                            },
                            Vec::new(),
                        )
                        .map_err(cadmpeg_core::CodecError::from)?
                        .map_err(|error| {
                            ctx.format_retained(
                                format_args!("extrusion cap staging: {error}"),
                                "Rhino stage_extrusion_caps text",
                            )
                            .map_or_else(Into::into, CandidateError::Admission)
                        })?,
                    ),
                },
                "Rhino extrusion cap loops arena",
            )?;
            loop_ids.push(loop_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?);
            for id in [
                point_id.as_str(),
                vertex_id.as_str(),
                edge_id.as_str(),
                pcurve_id.as_str(),
                coedge_id.as_str(),
                loop_id.as_str(),
            ] {
                annotate_derived(ctx, annotations, id)?;
            }
        }
        ctx.push_scoped_vec(
            arena_storage,
            &mut ir.model.faces,
            Face {
                id: face_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                shell: shell_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                surface: surface_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                sense: if cap == 0 {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                loops: cadmpeg_ir::topology::FaceLoops::unspecified(loop_ids),
                name: None,
                color: association.color,
                tolerance: None,
            },
            "Rhino extrusion cap faces arena",
        )?;
        {
            let (identity_text_buffer, _identity_storage) = ctx.format_scoped(
                format_args!("{surface_id}"),
                "Rhino stage_extrusion_caps text",
            )?;
            let identity_text = identity_text_buffer;
            annotate_derived(ctx, annotations, &identity_text)?;
        }
        {
            let (identity_text_buffer, _identity_storage) =
                ctx.format_scoped(format_args!("{face_id}"), "Rhino stage_extrusion_caps text")?;
            let identity_text = identity_text_buffer;
            annotate_derived(ctx, annotations, &identity_text)?;
        }
        ctx.push_scoped_vec(
            arena_storage,
            &mut ir.model.shells,
            {
                let mut members = ctx.collection_vec(1, "Rhino extrusion shell faces")?;
                members.push(face_id);
                Shell::new(
                    shell_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    region_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    members,
                    Vec::new(),
                    Vec::new(),
                )
                .map_err(cadmpeg_core::CodecError::from)?
            },
            "Rhino extrusion cap shells arena",
        )?;
        ctx.push_scoped_vec(
            arena_storage,
            &mut ir.model.regions,
            Region {
                id: region_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                body: body_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                shells: {
                    let mut ids = ctx.collection_vec(1, "Rhino extrusion cap region shells")?;
                    ids.push(shell_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?);
                    ids
                },
            },
            "Rhino extrusion cap regions arena",
        )?;
        {
            let (identity_text_buffer, _identity_storage) = ctx.format_scoped(
                format_args!("{shell_id}"),
                "Rhino stage_extrusion_caps text",
            )?;
            let identity_text = identity_text_buffer;
            annotate_derived(ctx, annotations, &identity_text)?;
        }
        {
            let (identity_text_buffer, _identity_storage) = ctx.format_scoped(
                format_args!("{region_id}"),
                "Rhino stage_extrusion_caps text",
            )?;
            let identity_text = identity_text_buffer;
            annotate_derived(ctx, annotations, &identity_text)?;
        }
        ctx.push_vec(
            &mut region_ids,
            region_id,
            "Rhino extrusion cap body regions",
        )?;
    }
    if region_ids.is_empty() {
        return Err("extrusion cap staging: no enabled caps".to_string().into());
    }
    ctx.push_scoped_vec(
        arena_storage,
        &mut ir.model.bodies,
        Body {
            id: body_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
            kind: BodyKind::Sheet,
            regions: region_ids,
            transform: None,
            name: association
                .name
                .as_deref()
                .map(|name| ctx.copy_retained_text(name, "Rhino extrusion cap body name"))
                .transpose()?,
            color: association.color,
            visible: association.visible,
        },
        "Rhino extrusion cap bodies arena",
    )?;
    {
        let (identity_text_buffer, _identity_storage) =
            ctx.format_scoped(format_args!("{body_id}"), "Rhino stage_extrusion_caps text")?;
        let identity_text = identity_text_buffer;
        annotate_derived(ctx, annotations, &identity_text)?;
    }
    Ok(body_id)
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

struct BrepCarrierDraft<'a> {
    staged: BrepDraft,
    c3: HashMap<usize, cadmpeg_ir::ids::CurveId>,
    surfaces: HashMap<usize, StagedBrepSurface>,
    child_cause: Option<(String, cadmpeg_core::decode::ScopedReservation<'a>)>,
    _storage: cadmpeg_core::decode::ScopedReservation<'a>,
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
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        kind: &str,
        index: usize,
        error: &impl std::fmt::Display,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.warnings.push_coded_admitted(
            ctx,
            RhinoLossCode::BrepMeshCacheDegraded,
            format_args!("invalid {kind} mesh cache slot {index}: {error}"),
        )
    }

    fn free_carrier_fallback(
        mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        cause: impl std::fmt::Display,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        self.kind = BrepTransferKind::FreeCarrierFallback;
        let mut storage = ctx.reserve_scoped(0, "Rhino Brep emitted fallback scratch")?;
        let mut emitted = BTreeSet::new();
        let model = self.draft.model();
        macro_rules! insert_emitted_ids {
            ($values:expr) => {{
                let values = $values;
                let mut values = values.iter();
                let count = values.len();
                for _ in 0..count {
                    let value = ctx
                        .next_charged(&mut values, "Rhino free carrier fallback traversal")?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Rhino free carrier source ended early",
                            )
                        })?;
                    let id = value.id.as_str();
                    if !ctx.contains_btree_set(
                        &emitted,
                        id,
                        "Rhino emitted fallback identity lookup",
                    )? {
                        storage.with_storage(|| {
                            ctx.insert_btree_set(
                                &mut emitted,
                                ctx.copy_retained_text(id, "Rhino Brep emitted fallback ID text")?,
                                "Rhino Brep emitted fallback IDs",
                            )
                        })?;
                    }
                }
            }};
        }
        insert_emitted_ids!(&model.curves);
        insert_emitted_ids!(&model.surfaces);
        insert_emitted_ids!(&model.tessellations);
        insert_emitted_ids!(&model.procedural_curves);
        ctx.retain_vec(
            &mut self.links,
            |id| ctx.contains_btree_set(&emitted, id, "Rhino emitted fallback identity lookup"),
            "Rhino fallback link retention",
        )?;
        self.draft.retain_exactness(ctx, |id| {
            ctx.contains_btree_set(&emitted, id, "Rhino fallback exactness lookup")
        })?;
        drop(emitted);
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
        self.warnings.push_coded_admitted(
            ctx,
            RhinoLossCode::TopologyBrepFallback,
            format_args!("Brep topology fallback: {cause}"),
        )?;
        Ok(self)
    }
}

fn stage_brep_carriers<'a>(
    input: BrepCarrierInput<'a>,
    retain_slot_lookup: bool,
    arena_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    metadata_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<BrepCarrierDraft<'a>, crate::curves::GeometryError> {
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
    let ctx = expand.ctx();
    let mut staged = BrepDraft::default();
    let mut carrier_storage = ctx.reserve_scoped(0, "Rhino Brep carrier lookup scratch")?;
    let mut c3 = HashMap::new();
    let mut surfaces = HashMap::new();
    let mut child_cause = None;
    for (kind, slots) in [
        ("render", &raw.render_meshes),
        ("analysis", &raw.analysis_meshes),
    ] {
        let mut slots = slots.iter();
        for index in 0..slots.len() {
            let slot = ctx
                .next_charged(&mut slots, "Rhino stage brep carriers traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino Brep mesh cache source ended early")
                })?;
            let Some(slot) = slot.as_ref() else {
                continue;
            };
            let id = ctx.format_retained(
                format_args!("rhino:object:tessellation#{key}.{kind}-{index}"),
                "Rhino stage_brep_carriers text",
            )?;
            match crate::mesh::decode(
                expand,
                data,
                slot.mesh.class_data_range.clone(),
                archive,
                crate::mesh::MeshDecodeOptions {
                    writer_version,
                    association: Some(
                        association.try_clone_for_decode(ctx, "Rhino source association copy")?,
                    ),
                    id: crate::mesh::MeshId::Ready(
                        cadmpeg_ir::tessellation::TessellationId::mint(id).map_err(|error| {
                            ctx.format_retained(
                                format_args!("{error}"),
                                "Rhino Brep mesh cache identity error",
                            )
                            .map_or_else(
                                std::convert::identity,
                                cadmpeg_core::CodecError::Malformed,
                            )
                        })?,
                    ),
                    scale,
                    userdata: &slot.userdata,
                },
                mesh_budget,
            ) {
                Ok(mut mesh) => {
                    metadata_storage.with_storage(|| {
                        staged
                            .warnings
                            .append_admitted(expand.ctx(), &mut mesh.warnings)
                    })?;
                    staged.draft.exactness(
                        ctx,
                        &mesh.tessellation.id,
                        if mesh.scaled {
                            Exactness::Derived
                        } else {
                            Exactness::ByteExact
                        },
                    )?;
                    {
                        let link = ctx.format_scoped_text(
                            metadata_storage,
                            format_args!("{}", mesh.tessellation.id),
                            "Rhino stage_brep_carriers text",
                        )?;
                        ctx.push_scoped_vec(
                            metadata_storage,
                            &mut staged.links,
                            link,
                            "Rhino Brep carrier links",
                        )?;
                    };
                    ctx.push_scoped_vec(
                        arena_storage,
                        &mut staged.draft.model_mut().tessellations,
                        mesh.tessellation,
                        "Rhino Brep carrier tessellations arena",
                    )?;
                }
                Err(error @ crate::curves::GeometryError::Codec(_)) => return Err(error),
                Err(error) => metadata_storage.with_storage(|| {
                    staged.mesh_cache_slot_dropped(expand.ctx(), kind, index, &error)
                })?,
            }
        }
    }
    let mut c3_slots = raw.c3.slots.iter();
    for index in 0..raw.c3.slots.len() {
        let child = ctx
            .next_charged(&mut c3_slots, "Rhino stage brep carriers traversal")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino Brep C3 source ended early")
            })?;
        let Some(child) = child.as_ref() else {
            continue;
        };
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
                metadata_storage.with_storage(|| {
                    append_curve_warnings(
                        expand.ctx(),
                        &mut staged.warnings,
                        &curve,
                        format_args!("C3 slot {index}"),
                    )
                })?;
                let id = match stage_curve_tree(
                    (expand.ctx(), &mut *arena_storage, &mut *metadata_storage),
                    &mut staged,
                    curve,
                    key,
                    &ctx.format_scoped(format_args!("c3-{index}"), "Rhino Brep carrier path")?
                        .0,
                    association,
                    unknown,
                ) {
                    Ok(id) => id,
                    Err(error @ crate::curves::GeometryError::Codec(_)) => return Err(error),
                    Err(error) => {
                        if retain_slot_lookup {
                            child_cause = Some(expand.ctx().format_scoped(
                                format_args!("C3 slot {index}: {error}"),
                                "Rhino Brep fallback cause",
                            )?);
                        }
                        continue;
                    }
                };
                if retain_slot_lookup {
                    carrier_storage.with_storage(|| {
                        ctx.insert_hash_map(&mut c3, index, id, "Rhino Brep C3 slots")
                    })?;
                }
            }
            Ok(_) => {
                if retain_slot_lookup {
                    child_cause = Some(expand.ctx().format_scoped(
                        format_args!("C3 slot {index} is not a curve"),
                        "Rhino Brep fallback cause",
                    )?);
                }
            }
            Err(error @ crate::curves::GeometryError::Codec(_)) => return Err(error),
            Err(error) => {
                if retain_slot_lookup {
                    child_cause = Some(expand.ctx().format_scoped(
                        format_args!("C3 slot {index}: {error}"),
                        "Rhino Brep fallback cause",
                    )?);
                }
            }
        }
    }
    let mut surface_slots = raw.surfaces.slots.iter();
    for index in 0..raw.surfaces.slots.len() {
        let child = ctx
            .next_charged(&mut surface_slots, "Rhino stage brep carriers traversal")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino Brep surface source ended early")
            })?;
        let Some(child) = child.as_ref() else {
            continue;
        };
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
                        if retain_slot_lookup {
                            child_cause = Some(expand.ctx().format_scoped(
                                format_args!("surface slot {index}: {error}"),
                                "Rhino Brep fallback cause",
                            )?);
                        }
                        continue;
                    }
                };
                let id = cadmpeg_ir::ids::SurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
                    surface_key
                        .then(cadmpeg_ir::identity_key!(".slot-"))
                        .then(index),
                );
                ctx.push_scoped_vec(
                    arena_storage,
                    &mut staged.draft.model_mut().surfaces,
                    Surface {
                        id: id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        geometry: geometry.into_geometry(),
                        source_object: Some(
                            association
                                .try_clone_for_decode(ctx, "Rhino source association copy")?,
                        ),
                    },
                    "Rhino Brep carrier surfaces arena",
                )?;
                staged.draft.exactness(
                    ctx,
                    &id,
                    if derived {
                        Exactness::Derived
                    } else {
                        Exactness::ByteExact
                    },
                )?;
                let link = ctx.copy_scoped_text(
                    id.as_str(),
                    metadata_storage,
                    "Rhino Brep surface link text",
                )?;
                ctx.push_scoped_vec(
                    metadata_storage,
                    &mut staged.links,
                    link,
                    "Rhino Brep carrier links",
                )?;
                if retain_slot_lookup {
                    carrier_storage.with_storage(|| {
                        ctx.insert_hash_map(
                            &mut surfaces,
                            index,
                            StagedBrepSurface {
                                id,
                                plane_parameterization,
                            },
                            "Rhino Brep surface slots",
                        )
                    })?;
                }
            }
            Ok(crate::curves::DecodedGeometry::Surface {
                surface:
                    crate::surfaces::DecodedSurface::Procedural {
                        geometry,
                        definition,
                    },
            }) => match stage_brep_procedural_surface(
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
                &mut *arena_storage,
                &mut *metadata_storage,
            ) {
                Ok(id) => {
                    if retain_slot_lookup {
                        carrier_storage.with_storage(|| {
                            ctx.insert_hash_map(
                                &mut surfaces,
                                index,
                                StagedBrepSurface {
                                    id,
                                    plane_parameterization: None,
                                },
                                "Rhino Brep surface slots",
                            )
                        })?;
                    }
                }
                Err(error @ crate::curves::GeometryError::Codec(_)) => return Err(error),
                Err(error) => {
                    if retain_slot_lookup {
                        child_cause = Some(expand.ctx().format_scoped(
                            format_args!("surface slot {index}: {error}"),
                            "Rhino Brep fallback cause",
                        )?);
                    }
                }
            },
            Ok(_) => {
                if retain_slot_lookup {
                    child_cause = Some(expand.ctx().format_scoped(
                        format_args!("surface slot {index} is not a surface"),
                        "Rhino Brep fallback cause",
                    )?);
                }
            }
            Err(error @ crate::curves::GeometryError::Codec(_)) => return Err(error),
            Err(error) => {
                if retain_slot_lookup {
                    child_cause = Some(expand.ctx().format_scoped(
                        format_args!("surface slot {index}: {error}"),
                        "Rhino Brep fallback cause",
                    )?);
                }
            }
        }
    }
    Ok(BrepCarrierDraft {
        staged,
        _storage: carrier_storage,
        c3,
        surfaces,
        child_cause,
    })
}

fn stage_invalid_brep(
    input: BrepCarrierInput<'_>,
    semantic_error: &crate::curves::GeometryError,
    arena_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    metadata_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<BrepDraft, crate::curves::GeometryError> {
    let ctx = input.expand.ctx();
    let carriers = stage_brep_carriers(input, false, &mut *arena_storage, &mut *metadata_storage)?;
    metadata_storage
        .with_storage(|| carriers.staged.free_carrier_fallback(ctx, semantic_error))
        .map_err(Into::into)
}

fn stage_brep(
    input: BrepTransferInput<'_>,
    arena_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    metadata_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<BrepDraft, crate::curves::GeometryError> {
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
    let key = IdentityKey::try_new(key.to_owned()).map_err(|error| {
        expand
            .ctx()
            .format_retained(format_args!("{error}"), "Rhino Brep identity error")
            .map_or_else(Into::into, crate::curves::GeometryError::unpositioned)
    })?;
    let raw = brep.raw();
    let resolved = brep.resolved();
    let BrepCarrierDraft {
        mut staged,
        _storage: _carrier_storage,
        c3,
        surfaces,
        child_cause,
    } = stage_brep_carriers(
        BrepCarrierInput {
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
        },
        true,
        &mut *arena_storage,
        &mut *metadata_storage,
    )?;
    let ctx = expand.ctx();
    if let Some((cause, _cause_storage)) = child_cause {
        return metadata_storage
            .with_storage(|| staged.free_carrier_fallback(ctx, cause))
            .map_err(Into::into);
    }
    let (staged, topology_storage) =
        ctx.with_scoped_storage("Rhino Brep topology fields", || {
            let DecodedPcurves {
                _storage: _pcurve_id_storage,
                ids: c2,
                values: pcurves,
                warnings: mut pcurve_warnings,
            } = decode_pcurves(
                (expand.ctx(), &mut *arena_storage),
                data,
                archive,
                raw,
                resolved,
                key.as_str(),
                &surfaces,
            )?;
            metadata_storage
                .with_storage(|| staged.warnings.append_admitted(ctx, &mut pcurve_warnings))?;
            staged.draft.model_mut().pcurves = pcurves;
            let body_id = cadmpeg_ir::ids::BodyId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "body"),
                key.clone(),
            );
            let (vertex_ids_buffer, _vertex_ids_storage) = ctx
                .temporary_vec(raw.vertices.len(), "Rhino staged Brep vertex IDs")
                .map_err(crate::curves::GeometryError::from)?;
            let mut vertex_ids = vertex_ids_buffer;
            ctx.reserve_scoped_vec(
                arena_storage,
                &mut staged.draft.model_mut().points,
                raw.vertices.len(),
                "Rhino staged Brep points",
            )
            .map_err(crate::curves::GeometryError::from)?;
            ctx.reserve_scoped_vec(
                arena_storage,
                &mut staged.draft.model_mut().vertices,
                raw.vertices.len(),
                "Rhino staged Brep vertices",
            )
            .map_err(crate::curves::GeometryError::from)?;
            let mut vertices = raw.vertices.iter().enumerate();
            for _ in 0..raw.vertices.len() {
                let (index, vertex) = ctx
                    .next_charged(&mut vertices, "Rhino stage brep traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("Rhino Brep vertex source ended early")
                    })?;
                let point_id = cadmpeg_ir::ids::PointId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "point"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".vertex-"))
                        .then(index),
                );
                let vertex_id = cadmpeg_ir::ids::VertexId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "vertex"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".slot-"))
                        .then(index),
                );
                let position =
                    crate::wire::scaled_point(vertex.point.get(), scale).ok_or_else(|| {
                        crate::curves::GeometryError::unpositioned(
                            "scaled Brep vertex coordinate is invalid",
                        )
                    })?;
                staged.draft.model_mut().points.push(Point::new(
                    point_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    position,
                    Some(association.try_clone_for_decode(ctx, "Rhino source association copy")?),
                ));
                staged.draft.model_mut().vertices.push(Vertex {
                    id: vertex_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    point: point_id,
                    tolerance: scaled_tolerance(resolved.vertices[index].tolerance, scale)?,
                });
                vertex_ids.push(vertex_id);
            }
            let (edge_ids_buffer, _edge_ids_storage) = ctx
                .temporary_vec(raw.edges.len(), "Rhino staged Brep edge IDs")
                .map_err(crate::curves::GeometryError::from)?;
            let mut edge_ids = edge_ids_buffer;
            ctx.reserve_scoped_vec(
                arena_storage,
                &mut staged.draft.model_mut().edges,
                raw.edges.len(),
                "Rhino staged Brep edges",
            )
            .map_err(crate::curves::GeometryError::from)?;
            let mut edges = raw.edges.iter().enumerate();
            for _ in 0..raw.edges.len() {
                let (index, edge) = ctx
                    .next_charged(&mut edges, "Rhino stage brep traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("Rhino Brep edge source ended early")
                    })?;
                let id = cadmpeg_ir::ids::EdgeId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "edge"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".slot-"))
                        .then(index),
                );
                let curve = ctx
                    .get_hash_map(
                        &c3,
                        &resolved.edges[index].curve,
                        "Rhino Brep C3 slot lookup",
                    )?
                    .map(|id| id.try_clone_for_decode(ctx, "Rhino carrier identity copy"))
                    .transpose()?;
                let vertices = edge_vertices(edge, &resolved.edges[index]);
                staged.draft.model_mut().edges.push(Edge {
                    id: id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                        curve,
                        Some(edge_param_range(edge)),
                    )
                    .map_err(crate::curves::GeometryError::unpositioned)?,
                    start: vertex_ids[vertices[0]]
                        .try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    end: vertex_ids[vertices[1]]
                        .try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    tolerance: scaled_tolerance(resolved.edges[index].tolerance, scale)?,
                });
                edge_ids.push(id);
            }
            let (components_buffer, _component_storage) = ctx
                .with_scoped_storage("Rhino Brep face component scratch", || {
                    face_components(ctx, resolved)
                })?;
            let components = components_buffer;
            let (grouping_buffer, _grouping_storage) = ctx
                .with_scoped_storage("Rhino Brep shell grouping scratch", || {
                    region_shell_groups(ctx, raw, resolved, &components)
                })?;
            let grouping = grouping_buffer;
            let (free_vertex_indices_buffer, _free_vertex_storage) = ctx
                .with_scoped_storage("Rhino Brep free vertex scratch", || {
                    brep_free_vertex_indices(ctx, resolved)
                })?;
            let free_vertex_indices = free_vertex_indices_buffer;
            if !free_vertex_indices.is_empty() && grouping.shells.len() != 1 {
                return metadata_storage
                    .with_storage(|| {
                        staged.free_carrier_fallback(
                            ctx,
                            "Brep free vertices have no unique shell membership",
                        )
                    })
                    .map_err(Into::into);
            }
            let mut free_vertex_ids = ctx
                .collection_vec(
                    free_vertex_indices.len(),
                    "Rhino staged Brep free vertex IDs",
                )
                .map_err(crate::curves::GeometryError::from)?;
            let mut free_vertices = free_vertex_indices.iter();
            for _ in 0..free_vertex_indices.len() {
                let index = ctx
                    .next_charged(&mut free_vertices, "Rhino stage brep traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Rhino free vertex index source ended early",
                        )
                    })?;
                free_vertex_ids.push(
                    vertex_ids[*index]
                        .try_clone_for_decode(ctx, "Rhino free vertex identity copy")?,
                );
            }
            if grouping.fallback {
                metadata_storage.with_storage(|| {
                    staged.warnings.push_admitted(
                        ctx,
                        format_args!(
                "Brep 3.3 region topology was not representable; incidence-derived shells used"
            ),
                    )
                })?;
            }
            let (face_ids_buffer, _face_ids_storage) = ctx
                .temporary_vec(raw.faces.len(), "Rhino staged Brep face IDs")
                .map_err(crate::curves::GeometryError::from)?;
            let mut face_ids = face_ids_buffer;
            let (pending_faces_buffer, _pending_faces_storage) = ctx
                .temporary_vec(raw.faces.len(), "Rhino staged Brep pending faces")
                .map_err(crate::curves::GeometryError::from)?;
            let mut pending_faces = pending_faces_buffer;
            ctx.reserve_scoped_vec(
                arena_storage,
                &mut staged.draft.model_mut().faces,
                raw.faces.len(),
                "Rhino staged Brep faces",
            )
            .map_err(crate::curves::GeometryError::from)?;
            let mut faces = raw.faces.iter().enumerate();
            for _ in 0..raw.faces.len() {
                let (index, face) = ctx
                    .next_charged(&mut faces, "Rhino stage brep traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("Rhino Brep face source ended early")
                    })?;
                let surface = ctx
                    .get_hash_map(
                        &surfaces,
                        &resolved.faces[index].surface,
                        "Rhino Brep surface slot lookup",
                    )?
                    .map(|surface| {
                        surface
                            .id
                            .try_clone_for_decode(ctx, "Rhino surface identity copy")
                    })
                    .transpose()?
                    .ok_or_else(|| {
                        crate::curves::error(face.source_range.start, "surface child missing")
                    })?;
                let component = grouping.face_groups[index];
                let id = cadmpeg_ir::ids::FaceId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "face"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".slot-"))
                        .then(index),
                );
                // The face's non-loop fields are held until its loops resolve, so the
                // face is constructed once with its complete boundary.
                pending_faces.push((
                    id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    {
                        cadmpeg_ir::ids::ShellId::compose(
                            &cadmpeg_ir::identity_namespace!("rhino", "object", "shell"),
                            key.clone()
                                .then(cadmpeg_ir::identity_key!(".component-"))
                                .then(component),
                        )
                    },
                    surface,
                    face_sense(face.reversed_surface),
                    face.color.map(color),
                ));
                face_ids.push(id);
            }
            let (face_loop_ids_buffer, _face_loop_storage) =
                ctx.with_scoped_storage("Rhino Brep face loop bridge scratch", || {
                    ctx.collect_indexed_vec(
                        raw.faces.len(),
                        "Rhino staged Brep face loop lists",
                        |_| Ok(Vec::<cadmpeg_ir::ids::LoopId>::new()),
                    )
                })?;
            let mut face_loop_ids = face_loop_ids_buffer;
            let (coedge_positions_buffer, _coedge_position_storage) =
                ctx.with_scoped_storage("Rhino Brep coedge position scratch", || {
                    ctx.alloc_filled(
                        raw.trims.len(),
                        None::<usize>,
                        "Rhino staged Brep coedge positions",
                    )
                })?;
            let mut coedge_positions = coedge_positions_buffer;
            ctx.reserve_scoped_vec(
                arena_storage,
                &mut staged.draft.model_mut().loops,
                resolved.loops.len(),
                "Rhino staged Brep loops",
            )
            .map_err(crate::curves::GeometryError::from)?;
            ctx.reserve_scoped_vec(
                arena_storage,
                &mut staged.draft.model_mut().coedges,
                raw.trims.len(),
                "Rhino staged Brep coedges",
            )
            .map_err(crate::curves::GeometryError::from)?;
            let mut loops = resolved.loops.iter().enumerate();
            for _ in 0..resolved.loops.len() {
                let (index, loop_record) = ctx
                    .next_charged(&mut loops, "Rhino stage brep traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("Rhino Brep loop source ended early")
                    })?;
                let id = cadmpeg_ir::ids::LoopId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "loop"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".slot-"))
                        .then(index),
                );
                let face_id = face_ids[loop_record.face]
                    .try_clone_for_decode(ctx, "Rhino typed identity copy")?;
                let mut coedges = ctx
                    .collection_vec(loop_record.trims.len(), "Rhino staged Brep loop coedges")
                    .map_err(crate::curves::GeometryError::from)?;
                let mut trims = loop_record.trims.iter();
                for _ in 0..loop_record.trims.len() {
                    let trim_index = ctx
                        .next_charged(&mut trims, "Rhino stage brep traversal")?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Rhino Brep trim source ended early",
                            )
                        })?;
                    let trim = &raw.trims[*trim_index];
                    let trim_refs = &resolved.trims[*trim_index];
                    let coedge_id = cadmpeg_ir::ids::CoedgeId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "coedge"),
                        key.clone()
                            .then(cadmpeg_ir::identity_key!(".slot-"))
                            .then(*trim_index),
                    );
                    let edge_id = if let Some(edge) = trim_refs.edge {
                        edge_ids
                            .get(edge)
                            .map(|id| id.try_clone_for_decode(ctx, "Rhino carrier identity copy"))
                            .transpose()?
                            .ok_or_else(|| {
                                crate::curves::error(trim.source_range.start, "trim edge missing")
                            })?
                    } else {
                        let synthetic_id = cadmpeg_ir::ids::EdgeId::compose(
                            &cadmpeg_ir::identity_namespace!("rhino", "object", "edge"),
                            key.clone()
                                .then(cadmpeg_ir::identity_key!(".singular-"))
                                .then(*trim_index),
                        );
                        if coedge_positions[*trim_index].is_none() {
                            ctx.reserve_scoped_vec(
                                arena_storage,
                                &mut staged.draft.model_mut().edges,
                                1,
                                "Rhino staged Brep singular edges",
                            )
                            .map_err(crate::curves::GeometryError::from)?;
                            staged.draft.model_mut().edges.push(Edge {
                                id: synthetic_id
                                    .try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                                carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(None),
                                start: vertex_ids[trim_refs.vertices[0]]
                                    .try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                                end: vertex_ids[trim_refs.vertices[0]]
                                    .try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                                tolerance: scaled_tolerance(trim_refs.tolerances[1], scale)?,
                            });
                        }
                        synthetic_id
                    };
                    let pcurve = if trim.trim_type == crate::brep::RawTrimKind::PointOnSurface {
                        None
                    } else {
                        ctx.get_hash_map(&c2, trim_index, "Rhino Brep pcurve identity lookup")?
                            .map(|id| id.try_clone_for_decode(ctx, "Rhino carrier identity copy"))
                            .transpose()?
                    };
                    coedge_positions[*trim_index] = Some(staged.draft.model().coedges.len());
                    let mut pcurves = ctx
                        .collection_vec(
                            usize::from(pcurve.is_some()),
                            "Rhino staged Brep coedge pcurves",
                        )
                        .map_err(crate::curves::GeometryError::from)?;
                    if let Some(pcurve) = pcurve {
                        pcurves.push(cadmpeg_ir::topology::PcurveUse {
                            pcurve,
                            isoparametric: None,
                            parameter_range: None,
                        });
                    }
                    staged.draft.model_mut().coedges.push(Coedge {
                        id: coedge_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        owner_loop: id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        edge: edge_id,
                        radial_next: coedge_id
                            .try_clone_for_decode(ctx, "Rhino typed identity copy")?,
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
                    id: id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    face: face_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                    boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                        cadmpeg_ir::topology::LoopRing::new(ctx, coedges, Vec::new())
                            .map_err(cadmpeg_core::CodecError::from)?
                            .map_err(|error| {
                                ctx.format_retained(
                                    format_args!("{error}"),
                                    "Rhino stage_brep text",
                                )
                                .map_or_else(Into::into, crate::curves::GeometryError::unpositioned)
                            })?,
                    ),
                });
                ctx.reserve_vec(
                    &mut face_loop_ids[loop_record.face],
                    1,
                    "Rhino staged Brep face loops",
                )
                .map_err(crate::curves::GeometryError::from)?;
                face_loop_ids[loop_record.face].push(id);
            }
            for (face_index, (id, shell, surface, sense, color)) in ctx
                .admit_iter(pending_faces, "Rhino pending Brep face traversal")
                .map_err(cadmpeg_core::CodecError::from)?
                .enumerate()
            {
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
            let mut edge_indices = 0..resolved.edges.len();
            for _ in 0..resolved.edges.len() {
                let edge_index = ctx
                    .next_charged(&mut edge_indices, "Rhino Brep radial edge traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("Rhino Brep edge source ended early")
                    })?;
                let uses = &resolved.edges[edge_index].trims;
                if uses.is_empty() {
                    continue;
                }
                let mut trims = uses.iter().enumerate();
                for _ in 0..uses.len() {
                    let (offset, trim_index) = ctx
                        .next_charged(&mut trims, "Rhino stage brep traversal")?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Rhino radial trim source ended early",
                            )
                        })?;
                    let next = cadmpeg_ir::ids::CoedgeId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "object", "coedge"),
                        key.clone()
                            .then(cadmpeg_ir::identity_key!(".slot-"))
                            .then(uses[(offset + 1) % uses.len()]),
                    );
                    let Some(position) = coedge_positions[*trim_index] else {
                        return Err(crate::curves::GeometryError::unpositioned(
                            "Brep coedge position is missing",
                        ));
                    };
                    staged.draft.model_mut().coedges[position].radial_next = next;
                }
            }
            let mut region_storage = ctx.reserve_scoped(0, "Rhino Brep region lookup scratch")?;
            let mut region_positions = BTreeMap::new();
            let mut regions: Vec<Region> = Vec::new();
            ctx.reserve_scoped_vec(
                arena_storage,
                &mut staged.draft.model_mut().shells,
                grouping.shells.len(),
                "Rhino staged Brep shells",
            )
            .map_err(crate::curves::GeometryError::from)?;
            let mut shells = grouping.shells.iter();
            for component in 0..grouping.shells.len() {
                let shell = ctx
                    .next_charged(&mut shells, "Rhino stage brep traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("Rhino Brep shell source ended early")
                    })?;
                let region_label = shell.region;
                let region_id = cadmpeg_ir::ids::RegionId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "region"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".slot-"))
                        .then(region_label),
                );
                let shell_id = cadmpeg_ir::ids::ShellId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "object", "shell"),
                    key.clone()
                        .then(cadmpeg_ir::identity_key!(".component-"))
                        .then(component),
                );
                let mut shell_faces = ctx
                    .collection_vec(shell.faces.len(), "Rhino staged Brep shell faces")
                    .map_err(crate::curves::GeometryError::from)?;
                let mut shell_face_indices = shell.faces.iter();
                for _ in 0..shell.faces.len() {
                    let index = ctx
                        .next_charged(&mut shell_face_indices, "Rhino stage brep traversal")?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Rhino Brep shell face source ended early",
                            )
                        })?;
                    shell_faces.push(
                        face_ids[*index].try_clone_for_decode(ctx, "Rhino face identity copy")?,
                    );
                }
                staged.draft.model_mut().shells.push(
                    Shell::new(
                        shell_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        region_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        shell_faces,
                        Vec::new(),
                        if component == 0 {
                            std::mem::take(&mut free_vertex_ids)
                        } else {
                            Vec::new()
                        },
                    )
                    .map_err(|message| {
                        ctx.format_retained(
                            format_args!("{message}"),
                            "Rhino Brep shell construction error",
                        )
                        .map_or_else(Into::into, crate::curves::GeometryError::unpositioned)
                    })?,
                );
                if let Some(&position) =
                    ctx.get_btree_map(&region_positions, &region_label, "Rhino Brep region lookup")?
                {
                    let region: &mut Region = &mut regions[position];
                    ctx.reserve_vec(&mut region.shells, 1, "Rhino staged Brep region shells")
                        .map_err(crate::curves::GeometryError::from)?;
                    region.shells.push(shell_id);
                } else {
                    ctx.reserve_scoped_vec(
                        arena_storage,
                        &mut regions,
                        1,
                        "Rhino staged Brep regions",
                    )
                    .map_err(crate::curves::GeometryError::from)?;
                    let mut shell_ids = ctx
                        .collection_vec(1, "Rhino staged Brep region shells")
                        .map_err(crate::curves::GeometryError::from)?;
                    shell_ids.push(shell_id);
                    region_storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut region_positions,
                            region_label,
                            regions.len(),
                            "Rhino Brep region lookup positions",
                        )
                    })?;
                    regions.push(Region {
                        id: region_id,
                        body: body_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                        shells: shell_ids,
                    });
                }
            }
            staged.draft.model_mut().regions = regions;
            let mut body_regions = ctx
                .collection_vec(
                    staged.draft.model().regions.len(),
                    "Rhino staged Brep body regions",
                )
                .map_err(crate::curves::GeometryError::from)?;
            let region_count = staged.draft.model().regions.len();
            let mut regions = staged.draft.model().regions.iter();
            for _ in 0..region_count {
                let region = ctx
                    .next_charged(&mut regions, "Rhino stage brep traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("Rhino Brep region source ended early")
                    })?;
                body_regions.push(
                    region
                        .id
                        .try_clone_for_decode(ctx, "Rhino region identity copy")?,
                );
            }
            let (body_kind, body_kind_substituted) = brep.body_kind(ctx, writer_version)?;
            if let Some(loss) = body_kind_substituted {
                ctx.reserve_scoped_vec(
                    metadata_storage,
                    &mut staged.typed_losses,
                    1,
                    "Rhino staged Brep typed losses",
                )?;
                staged.typed_losses.push(loss);
            }
            ctx.reserve_scoped_vec(
                arena_storage,
                &mut staged.draft.model_mut().bodies,
                1,
                "Rhino staged Brep bodies",
            )
            .map_err(crate::curves::GeometryError::from)?;
            staged.draft.model_mut().bodies.push(Body {
                id: body_id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                kind: match body_kind {
                    crate::brep::BrepBodyKind::Solid => BodyKind::Solid,
                    crate::brep::BrepBodyKind::Sheet => BodyKind::Sheet,
                },
                regions: body_regions,
                transform: None,
                name: association
                    .name
                    .as_deref()
                    .map(|name| ctx.copy_retained_text(name, "Rhino staged Brep body name"))
                    .transpose()?,
                color: association.color,
                visible: association.visible,
            });
            ctx.reserve_scoped_vec(
                metadata_storage,
                &mut staged.links,
                1,
                "Rhino staged Brep links",
            )?;
            staged.links.push(ctx.copy_scoped_text(
                body_id.as_str(),
                metadata_storage,
                "Rhino staged Brep link text",
            )?);
            let (derived_ids, _derived_id_storage) =
                ctx.with_scoped_storage("Rhino Brep derived identity scratch", || {
                    let model = staged.draft.model();
                    let count = model.bodies.len()
                        + model.regions.len()
                        + model.shells.len()
                        + model.faces.len()
                        + model.loops.len()
                        + model.coedges.len()
                        + model.edges.len()
                        + model.vertices.len()
                        + model.points.len()
                        + model.pcurves.len();
                    let mut ids = ctx.collection_vec(count, "Rhino staged Brep derived IDs")?;
                    macro_rules! append_ids {
                        ($field:ident) => {
                            let mut values = model.$field.iter();
                            for _ in 0..model.$field.len() {
                                let value = ctx
                                    .next_charged(
                                        &mut values,
                                        "Rhino Brep derived identity traversal",
                                    )?
                                    .ok_or_else(|| {
                                        cadmpeg_core::CodecError::malformed(
                                            "Rhino Brep derived identity source ended early",
                                        )
                                    })?;
                                ids.push(ctx.format_retained(
                                    format_args!("{}", value.id),
                                    "Rhino staged Brep derived ID text",
                                )?);
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
                    Ok::<_, cadmpeg_core::CodecError>(ids)
                })?;
            let mut derived_ids = derived_ids.into_iter();
            for _ in 0..derived_ids.len() {
                let id = ctx
                    .next_charged(&mut derived_ids, "Rhino derived identity traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Rhino derived identity source ended early",
                        )
                    })?;
                staged.draft.exactness(ctx, id, Exactness::Derived)?;
            }
            scale_plane_pcurves(ctx, &mut staged, scale)?;
            Ok::<_, crate::curves::GeometryError>(staged)
        })?;
    if staged.kind == BrepTransferKind::FullTopology {
        topology_storage.commit()?;
    }
    Ok(staged)
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
        format: cadmpeg_ir::CodecFormat::Rhino,
        object_id: cadmpeg_core::nonblank_literal!("embedded-history-brep"),
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
    let mut mesh_budget = crate::mesh::MeshBudget::from_session(expand.ctx());
    let mut arena_storage = match expand
        .ctx()
        .reserve_scoped(0, "Rhino embedded Brep arena scratch")
    {
        Ok(storage) => storage,
        Err(error) => {
            *refusal = Some(error);
            return None;
        }
    };
    let mut metadata_storage = match expand
        .ctx()
        .reserve_scoped(0, "Rhino embedded Brep link scratch")
    {
        Ok(storage) => storage,
        Err(error) => {
            *refusal = Some(error);
            return None;
        }
    };
    let staged = match stage_brep(
        BrepTransferInput {
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
        },
        &mut arena_storage,
        &mut metadata_storage,
    ) {
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
    let snapshot = match staged.draft.model().geometry_snapshot(expand.ctx(), "brep") {
        Ok(snapshot) => snapshot,
        Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => {
            *refusal = Some(error);
            return None;
        }
        Err(_) => return None,
    };
    match crate::wire::admitted_canonical_json(expand.ctx(), &snapshot, "Rhino embedded Brep JSON")
    {
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    staged: &mut BrepDraft,
    scale: MillimeterScale,
) -> Result<(), crate::curves::GeometryError> {
    fn insert_id(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        values: &mut BTreeSet<String>,
        id: &str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        if !ctx.contains_btree_set(values, id, "Rhino values membership")? {
            ctx.insert_btree_set(
                values,
                ctx.copy_retained_text(id, "Rhino plane pcurve lookup ID text")?,
                "Rhino plane pcurve lookup IDs",
            )?;
        }
        Ok(())
    }
    if scale == MillimeterScale::IDENTITY {
        return Ok(());
    }
    let mut lookup_storage = ctx.reserve_scoped(0, "Rhino plane pcurve lookup scratch")?;
    let mut plane_surfaces = BTreeSet::new();
    let surface_count = staged.draft.model().surfaces.len();
    let mut surfaces = staged.draft.model().surfaces.iter();
    for _ in 0..surface_count {
        let surface = ctx
            .next_charged(&mut surfaces, "Rhino scale plane pcurves traversal")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino surface source ended early")
            })?;
        if matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
        ) {
            lookup_storage
                .with_storage(|| insert_id(ctx, &mut plane_surfaces, surface.id.as_str()))?;
        }
    }
    let mut plane_faces = BTreeSet::new();
    let face_count = staged.draft.model().faces.len();
    let mut faces = staged.draft.model().faces.iter();
    for _ in 0..face_count {
        let face = ctx
            .next_charged(&mut faces, "Rhino scale plane pcurves traversal")?
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("Rhino face source ended early"))?;
        if ctx.contains_btree_set(
            &plane_surfaces,
            face.surface.as_str(),
            "Rhino plane surfaces membership",
        )? {
            lookup_storage.with_storage(|| insert_id(ctx, &mut plane_faces, face.id.as_str()))?;
        }
    }
    let mut plane_loops = BTreeSet::new();
    let loop_count = staged.draft.model().loops.len();
    let mut loops = staged.draft.model().loops.iter();
    for _ in 0..loop_count {
        let value = ctx
            .next_charged(&mut loops, "Rhino scale plane pcurves traversal")?
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("Rhino loop source ended early"))?;
        if ctx.contains_btree_set(
            &plane_faces,
            value.face.as_str(),
            "Rhino plane faces membership",
        )? {
            lookup_storage.with_storage(|| insert_id(ctx, &mut plane_loops, value.id.as_str()))?;
        }
    }
    let mut plane_pcurves = BTreeSet::new();
    let coedge_count = staged.draft.model().coedges.len();
    let mut coedges = staged.draft.model().coedges.iter();
    for _ in 0..coedge_count {
        let coedge = ctx
            .next_charged(&mut coedges, "Rhino scale plane pcurves traversal")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino coedge source ended early")
            })?;
        if ctx.contains_btree_set(
            &plane_loops,
            coedge.owner_loop.as_str(),
            "Rhino plane loops membership",
        )? {
            let mut curve_uses = coedge.pcurves.iter();
            for _ in 0..coedge.pcurves.len() {
                let curve_use = ctx
                    .next_charged(&mut curve_uses, "Rhino scale plane pcurves traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Rhino coedge pcurve source ended early",
                        )
                    })?;
                lookup_storage.with_storage(|| {
                    insert_id(ctx, &mut plane_pcurves, curve_use.pcurve.as_str())
                })?;
            }
        }
    }
    let pcurve_count = staged.draft.model().pcurves.len();
    let mut pcurves = staged.draft.model_mut().pcurves.iter_mut();
    for _ in 0..pcurve_count {
        let pcurve = ctx
            .next_charged(&mut pcurves, "Rhino plane pcurve traversal")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino plane pcurve source ended early")
            })?;
        if !ctx.contains_btree_set(
            &plane_pcurves,
            pcurve.id.as_str(),
            "Rhino plane pcurves membership",
        )? {
            continue;
        }
        if let PcurveGeometry::Nurbs { nurbs } = &mut pcurve.geometry {
            if let Err(message) = nurbs.try_map_control_points(
                |_, pole| {
                    let pole = pole.get();
                    cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(
                        pole.u * scale.value(),
                        pole.v * scale.value(),
                    ))
                    .ok_or("control_points contains a non-finite point")
                },
                ctx,
            )? {
                return Err(crate::curves::GeometryError::unpositioned(
                    ctx.copy_retained_text(message, "Rhino pole mapping refusal")?,
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
    staged: &mut BrepDraft,
    index: usize,
    geometry: cadmpeg_ir::geometry::nurbs::NurbsSurface,
    definition: crate::surfaces::DecodedProceduralSurface,
    context: &BrepStageContext<'_>,
    arena_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    metadata_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<cadmpeg_ir::ids::SurfaceId, crate::curves::GeometryError> {
    let key = IdentityKey::try_new(context.key.to_owned()).map_err(|error| {
        context
            .ctx
            .format_retained(
                format_args!("{error}"),
                "Rhino stage_brep_procedural_surface text",
            )
            .map_or_else(Into::into, crate::curves::GeometryError::unpositioned)
    })?;
    let definition = definition.into_definition(
        |child_index, _, child| {
            stage_curve_tree(
                (context.ctx, &mut *arena_storage, &mut *metadata_storage),
                staged,
                child,
                key.as_str(),
                &context
                    .ctx
                    .format_scoped(
                        format_args!("surface-{index}.child-{child_index}"),
                        "Rhino Brep procedural child path",
                    )?
                    .0,
                context.association,
                context.unknown,
            )
        },
        |error| crate::curves::GeometryError::unpositioned(error.to_string()),
    )?;
    let surface_id = cadmpeg_ir::ids::SurfaceId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
        key.clone()
            .then(cadmpeg_ir::identity_key!(".slot-"))
            .then(index),
    );
    context.ctx.push_scoped_vec(
        arena_storage,
        &mut staged.draft.model_mut().surfaces,
        Surface {
            id: surface_id.try_clone_for_decode(context.ctx, "Rhino typed identity copy")?,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(geometry)),
            source_object: Some(
                context
                    .association
                    .try_clone_for_decode(context.ctx, "Rhino source association copy")?,
            ),
        },
        "Rhino Brep procedural surface arena",
    )?;
    let procedural_id = cadmpeg_ir::ids::ProceduralSurfaceId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "object", "procedural-surface"),
        key.then(cadmpeg_ir::identity_key!(".slot-")).then(index),
    );
    staged
        .draft
        .model_mut()
        .add_procedural_surface(
            context.ctx,
            &surface_id,
            ProceduralSurface::new(
                procedural_id.try_clone_for_decode(context.ctx, "Rhino typed identity copy")?,
                definition,
                None,
            ),
        )?
        .map_err(|error| {
            context
                .ctx
                .format_retained(
                    format_args!("{error}"),
                    "Rhino stage_brep_procedural_surface text",
                )
                .map_or_else(Into::into, crate::curves::GeometryError::unpositioned)
        })?;
    staged
        .draft
        .exactness(context.ctx, &surface_id, Exactness::Derived)?;
    staged
        .draft
        .exactness(context.ctx, &procedural_id, Exactness::Derived)?;
    {
        let link = context.ctx.format_scoped_text(
            metadata_storage,
            format_args!("{surface_id}"),
            "Rhino stage_brep_procedural_surface text",
        )?;
        context.ctx.push_scoped_vec(
            metadata_storage,
            &mut staged.links,
            link,
            "Rhino Brep carrier links",
        )?;
    };
    {
        let link = context.ctx.format_scoped_text(
            metadata_storage,
            format_args!("{procedural_id}"),
            "Rhino stage_brep_procedural_surface text",
        )?;
        context.ctx.push_scoped_vec(
            metadata_storage,
            &mut staged.links,
            link,
            "Rhino Brep carrier links",
        )?;
    };
    Ok(surface_id)
}

fn stage_curve_tree(
    scope: (
        &cadmpeg_core::decode::DecodeContext<'_>,
        &mut cadmpeg_core::decode::ScopedReservation<'_>,
        &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ),
    staged: &mut BrepDraft,
    curve: crate::curves::DecodedCurve,
    key: &str,
    path: &str,
    association: &SourceObjectAssociation,
    unknown: &UnknownId,
) -> Result<cadmpeg_ir::ids::CurveId, crate::curves::GeometryError> {
    let (ctx, arena_storage, metadata_storage) = scope;
    let _nested = ctx.enter_nested("Rhino Brep curve tree")?;
    let (geometry, definition) = match curve {
        crate::curves::DecodedCurve::Leaf { geometry, .. } => (geometry, None),
        crate::curves::DecodedCurve::Compound {
            children,
            end_parameter,
            ..
        } => {
            let parameter_count = children.len().checked_add(1).ok_or_else(|| {
                crate::curves::GeometryError::unpositioned(
                    "compound curve parameter count overflow",
                )
            })?;
            let mut parameters =
                ctx.collection_vec(parameter_count, "Rhino Brep curve tree parameters")?;
            let mut components =
                ctx.collection_vec(children.len(), "Rhino Brep curve tree components")?;
            let mut children = children.into_iter().enumerate();
            for _ in 0..children.len() {
                let (index, (parameter, child)) = ctx
                    .next_charged(&mut children, "Rhino curve component traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Rhino curve component source ended early",
                        )
                    })?;
                let parameter = parameter.get();
                parameters.push(parameter);
                let (component_path_buffer, _path_storage) = ctx.format_scoped(
                    format_args!("{path}.component-{index}"),
                    "Rhino curve component path",
                )?;
                let component_path = component_path_buffer;
                components.push(cadmpeg_ir::geometry::CompoundComponent {
                    parameter,
                    component: stage_curve_tree(
                        (ctx, &mut *arena_storage, &mut *metadata_storage),
                        staged,
                        child,
                        key,
                        &component_path,
                        association,
                        unknown,
                    )?,
                });
            }
            parameters.push(end_parameter.get());
            (
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                    record: Some(unknown.try_clone_for_decode(ctx, "Rhino typed identity copy")?),
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
    let key = IdentityKey::try_new(key.to_owned()).map_err(|error| {
        ctx.format_retained(format_args!("{error}"), "Rhino stage_curve_tree text")
            .map_or_else(Into::into, crate::curves::GeometryError::unpositioned)
    })?;
    let id = cadmpeg_ir::ids::CurveId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "object", "curve"),
        if path == "root" {
            key.clone()
        } else {
            key.clone().then(cadmpeg_ir::identity_key!(".")).then(
                IdentityKey::try_new(path.to_owned()).map_err(|error| {
                    ctx.format_retained(format_args!("{error}"), "Rhino stage_curve_tree text")
                        .map_or_else(Into::into, crate::curves::GeometryError::unpositioned)
                })?,
            )
        },
    );
    ctx.push_scoped_vec(
        arena_storage,
        &mut staged.draft.model_mut().curves,
        Curve {
            id: id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
            geometry,
            source_object: Some(
                association.try_clone_for_decode(ctx, "Rhino source association copy")?,
            ),
        },
        "Rhino Brep curve arena",
    )?;
    staged.draft.exactness(ctx, &id, Exactness::Derived)?;
    {
        let link = ctx.format_scoped_text(
            metadata_storage,
            format_args!("{id}"),
            "Rhino Brep curve link text",
        )?;
        ctx.push_scoped_vec(
            metadata_storage,
            &mut staged.links,
            link,
            "Rhino Brep carrier links",
        )?;
    };
    if let Some(definition) = definition {
        let procedure_key = if path == "root" {
            key.clone()
        } else {
            key.clone().then(cadmpeg_ir::identity_key!(".")).then(
                IdentityKey::try_new(path.to_owned()).map_err(|error| {
                    ctx.format_retained(format_args!("{error}"), "Rhino stage_curve_tree text")
                        .map_or_else(Into::into, crate::curves::GeometryError::unpositioned)
                })?,
            )
        };
        let procedure_id = cadmpeg_ir::ids::ProceduralCurveId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "procedural-curve"),
            procedure_key,
        );
        staged
            .draft
            .exactness(ctx, &procedure_id, Exactness::Derived)?;
        {
            let link = ctx.format_scoped_text(
                metadata_storage,
                format_args!("{procedure_id}"),
                "Rhino stage_curve_tree text",
            )?;
            ctx.push_scoped_vec(
                metadata_storage,
                &mut staged.links,
                link,
                "Rhino Brep carrier links",
            )?;
        };
        staged
            .draft
            .model_mut()
            .add_procedural_curve(ctx, &id, ProceduralCurve::new(procedure_id, definition))?
            .map_err(|error| {
                ctx.format_retained(format_args!("{error}"), "Rhino stage_curve_tree text")
                    .map_or_else(Into::into, crate::curves::GeometryError::unpositioned)
            })?;
    }
    Ok(id)
}

struct DecodedPcurves<'a> {
    ids: HashMap<usize, cadmpeg_ir::ids::PcurveId>,
    values: Vec<Pcurve>,
    warnings: Diagnostics,
    _storage: cadmpeg_core::decode::ScopedReservation<'a>,
}

fn decode_pcurves<'a>(
    scope: (
        &'a cadmpeg_core::decode::DecodeContext<'_>,
        &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ),
    data: &[u8],
    archive: ArchiveVersion,
    raw: &crate::brep::RawBrep,
    resolved: &crate::brep::ResolvedBrep,
    key: &str,
    surfaces: &HashMap<usize, StagedBrepSurface>,
) -> Result<DecodedPcurves<'a>, crate::curves::GeometryError> {
    let (ctx, arena_storage) = scope;
    let mut id_storage = ctx.reserve_scoped(0, "Rhino Brep pcurve identity scratch")?;
    let mut ids = HashMap::new();
    let mut values = Vec::new();
    let mut warnings = Diagnostics::new();
    let mut scratch_warnings =
        ScratchDiagnostics::new(ctx, "Rhino temporary Brep pcurve diagnostics")?;
    let key = match IdentityKey::try_new(key.to_owned()) {
        Ok(key) => key,
        Err(error) => {
            scratch_warnings.push_coded_admitted(
                ctx,
                None,
                format_args!("Brep pcurve identity key is invalid: {error}"),
            )?;
            id_storage.with_storage(|| {
                warnings.append_scoped_admitted(
                    ctx,
                    scratch_warnings,
                    "Rhino Brep pcurve diagnostic promotion",
                )
            })?;
            return Ok(DecodedPcurves {
                _storage: id_storage,
                ids,
                values,
                warnings,
            });
        }
    };
    let mut cached_curve_storage = ctx.reserve_scoped(0, "Rhino temporary C2 cache")?;
    let mut decoded_slots =
        HashMap::<usize, Option<(NurbsCurve, cadmpeg_core::decode::ScopedReservation<'a>)>>::new();
    let mut trims = raw.trims.iter().enumerate();
    for _ in 0..raw.trims.len() {
        let (index, trim) = ctx
            .next_charged(&mut trims, "Rhino decode pcurves traversal")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino Brep pcurve trim source ended early")
            })?;
        if trim.trim_type == crate::brep::RawTrimKind::PointOnSurface {
            continue;
        }
        let trim_refs = &resolved.trims[index];
        let Some(trim_curve) = trim_refs.curve else {
            continue;
        };
        let slot = cached_curve_storage.with_storage(|| {
            ctx.entry_hash_map(
                &mut decoded_slots,
                trim_curve,
                "Rhino Brep decoded C2 slots",
            )
        })?;
        let nurbs = match slot {
            std::collections::hash_map::Entry::Occupied(slot) => slot.into_mut(),
            std::collections::hash_map::Entry::Vacant(slot) => {
                let mut source_storage = ctx.reserve_scoped(0, "Rhino temporary C2 source")?;
                let curve = match source_storage.with_storage(|| {
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
                    Ok(curve)
                }) {
                    Ok(curve) => curve,
                    Err(error @ crate::curves::GeometryError::Codec(_)) => return Err(error),
                    Err(error) => {
                        scratch_warnings.push_coded_admitted(
                            ctx,
                            crate::loss::RhinoLossCode::TrimPcurveDropped,
                            format_args!("trim {index} C2 omitted: {error}"),
                        )?;
                        slot.insert(None);
                        continue;
                    }
                };
                let source_is_output = matches!(
                    &curve,
                    crate::curves::DecodedCurve::Leaf {
                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_)),
                        ..
                    }
                );
                let mut curve_storage = ctx.reserve_scoped(0, "Rhino temporary C2 result")?;
                let mut c2_diagnostics =
                    ScratchDiagnostics::new(ctx, "Rhino temporary C2 diagnostics")?;
                let decoded = c2_curve_to_nurbs_join_scoped(
                    ctx,
                    curve,
                    trim.source_range.start,
                    &mut curve_storage,
                    &mut c2_diagnostics,
                );
                match decoded {
                    Ok(nurbs) => {
                        scratch_warnings.append_prefixed_admitted(
                            ctx,
                            c2_diagnostics,
                            format_args!("trim {index}"),
                        )?;
                        let storage = if source_is_output {
                            drop(curve_storage);
                            source_storage
                        } else {
                            drop(source_storage);
                            curve_storage
                        };
                        slot.insert(Some((nurbs, storage)))
                    }
                    Err(error @ crate::curves::GeometryError::Codec(_)) => return Err(error),
                    Err(error) => {
                        drop(curve_storage);
                        drop(source_storage);
                        drop(c2_diagnostics);
                        scratch_warnings.push_coded_admitted(
                            ctx,
                            crate::loss::RhinoLossCode::TrimPcurveDropped,
                            format_args!("trim {index} C2 omitted: {error}"),
                        )?;
                        slot.insert(None)
                    }
                }
            }
        };
        let Some((nurbs, _curve_storage)) = nurbs.as_ref() else {
            continue;
        };
        let plane_parameterization = match resolved
            .loops
            .get(trim_refs.loop_index)
            .and_then(|loop_record| resolved.faces.get(loop_record.face))
        {
            Some(face) => ctx
                .get_hash_map(surfaces, &face.surface, "Rhino Brep pcurve surface lookup")?
                .and_then(|surface| surface.plane_parameterization),
            None => None,
        };
        let map_point = |point: FinitePoint3| {
            let point = point.get();
            let point = Point2::new(point.x, point.y);
            FinitePoint2::new(plane_parameterization.map_or(point, |map| map.map_point(point)))
        };
        let mut pcurve_storage = ctx.reserve_scoped(0, "Rhino Brep pcurve output")?;
        let mut pole_storage = None;
        let invalid_point;
        let poles = match nurbs.pole_rows() {
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
                let mut mapped = pcurve_storage
                    .with_storage(|| ctx.collection_vec(points.len(), "Rhino Brep pcurve poles"))
                    .map_err(crate::curves::GeometryError::from)?;
                invalid_point = !ctx.all_by(
                    points,
                    |point| {
                        let Some(point) = map_point(*point) else {
                            return Ok(false);
                        };
                        mapped.push(point);
                        Ok(true)
                    },
                    "Rhino Brep pcurve pole mapping",
                )?;
                PcurveNurbsPoles::Polynomial { points: mapped }
            }
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
                let mut storage = ctx.reserve_scoped(0, "Rhino temporary Brep pcurve poles")?;
                let mut mapped = storage
                    .with_storage(|| ctx.collection_vec(points.len(), "Rhino Brep pcurve poles"))
                    .map_err(crate::curves::GeometryError::from)?;
                invalid_point = !ctx.all_by(
                    points,
                    |pole| {
                        let Some(point) = map_point(pole.point) else {
                            return Ok(false);
                        };
                        mapped.push(WeightedPole2 {
                            point,
                            weight: pole.weight,
                        });
                        Ok(true)
                    },
                    "Rhino Brep pcurve pole mapping",
                )?;
                pole_storage = Some(storage);
                PcurveNurbsPoles::Rational { points: mapped }
            }
        };
        if invalid_point {
            drop(poles);
            drop(pole_storage);
            drop(pcurve_storage);
            scratch_warnings.push_coded_admitted(
                ctx,
                None,
                format_args!("trim {index} C2 has an invalid NURBS shape: control_points contains a non-finite point"),
            )?;
            continue;
        }
        let id = cadmpeg_ir::ids::PcurveId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "pcurve"),
            key.clone()
                .then(cadmpeg_ir::identity_key!(".trim-"))
                .then(index),
        );
        // Knot and polynomial pole lanes move into the output. Rational mapped
        // poles are source scratch; the constructor creates the retained pair.
        let nurbs = match pcurve_storage.with_storage(|| {
            let knots = nurbs
                .knots()
                .try_clone_for_decode(ctx, "Rhino Brep pcurve knots")?;
            PcurveNurbs::new(ctx, nurbs.degree(), knots, poles, nurbs.periodic())
        })? {
            Ok(nurbs) => nurbs,
            Err(error) => {
                drop(pole_storage);
                drop(pcurve_storage);
                scratch_warnings.push_coded_admitted(
                    ctx,
                    None,
                    format_args!("trim {index} C2 has an invalid NURBS shape: {error}"),
                )?;
                continue;
            }
        };
        drop(pole_storage);
        let pcurve = pcurve_storage.with_storage(|| {
            Ok::<_, cadmpeg_core::CodecError>(Pcurve {
                id: id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
                geometry: PcurveGeometry::Nurbs { nurbs },
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                    Some(trim.proxy_reversed),
                    Some(trim.domain.0),
                    trim_refs.tolerances[0].fit(),
                ),
            })
        })?;
        ctx.reserve_scoped_vec(arena_storage, &mut values, 1, "Rhino Brep pcurves")
            .map_err(crate::curves::GeometryError::from)?;
        id_storage
            .with_storage(|| ctx.insert_hash_map(&mut ids, index, id, "Rhino Brep pcurve IDs"))?;
        values.push(pcurve_storage.commit_value(pcurve)?);
    }
    id_storage.with_storage(|| {
        warnings.append_scoped_admitted(
            ctx,
            scratch_warnings,
            "Rhino Brep pcurve diagnostic promotion",
        )
    })?;
    Ok(DecodedPcurves {
        _storage: id_storage,
        ids,
        values,
        warnings,
    })
}

fn c2_curve_to_nurbs_join_scoped(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    curve: crate::curves::DecodedCurve,
    offset: usize,
    result_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    warnings: &mut ScratchDiagnostics<'_>,
) -> Result<NurbsCurve, crate::curves::GeometryError> {
    let _nested = ctx.enter_nested("Rhino C2 join nesting")?;
    match curve {
        crate::curves::DecodedCurve::Leaf { geometry, .. } => match geometry {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => Ok(nurbs),
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
            let (segments_buffer, mut segment_storage) = ctx
                .temporary_vec(children.len(), "Rhino C2 joined segments")
                .map_err(crate::curves::GeometryError::from)?;
            let mut segments = segments_buffer;
            let mut children = children.into_iter();
            for _ in 0..children.len() {
                let (start, child) = ctx
                    .next_charged(&mut children, "Rhino C2 joined segment traversal")?
                    .ok_or_else(|| {
                        crate::curves::GeometryError::malformed(
                            offset,
                            "C2 child source ended early",
                        )
                    })?;
                // `as_slice` observes the next endpoint without advancing the
                // iterator. Every child remains behind its own admission,
                // including the suffix discarded after an earlier error.
                let end = children
                    .as_slice()
                    .first()
                    .map_or(end_parameter, |(start, _)| *start);
                let target = [start, end];
                if target[0] >= target[1] {
                    return Err(crate::curves::error(
                        offset,
                        "C2 polycurve segment domain is invalid",
                    ));
                }
                let joined = c2_curve_to_nurbs_join_scoped(
                    ctx,
                    child,
                    offset,
                    &mut segment_storage,
                    warnings,
                )?;
                segments.push(crate::curves::remap_nurbs_domain(
                    ctx, joined, target, offset,
                )?);
            }
            let joined = crate::curves::join_nurbs_curves(
                ctx,
                Some(segment_storage),
                segments,
                offset,
                Some(result_storage),
                Some(warnings),
            )?;
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
    let (parent_buffer, _parent_storage) = ctx
        .with_scoped_storage("Rhino Brep face parent scratch", || {
            ctx.alloc_filled(resolved.faces.len(), 0usize, "Rhino Brep face parents")
        })?;
    let mut parent = parent_buffer;
    for (index, value) in ctx
        .admit_iter(&mut parent, "Rhino Brep face parent initialization")
        .map_err(cadmpeg_core::CodecError::from)?
        .enumerate()
    {
        *value = index;
    }
    let mut edges = resolved.edges.iter();
    for _ in 0..resolved.edges.len() {
        let edge = ctx
            .next_charged(&mut edges, "Rhino face components traversal")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino Brep component edge source ended early")
            })?;
        let mut previous = None;
        let mut trims = edge.trims.iter();
        for _ in 0..edge.trims.len() {
            let trim = ctx
                .next_charged(&mut trims, "Rhino face components incidence traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Rhino Brep component trim source ended early",
                    )
                })?;
            let face = resolved.loops[resolved.trims[*trim].loop_index].face;
            if let Some(previous) = previous {
                let left = disjoint_root(ctx, &mut parent, previous)?;
                let right = disjoint_root(ctx, &mut parent, face)?;
                parent[left] = right;
            }
            previous = Some(face);
        }
    }
    let (labels_buffer, _label_storage) = ctx
        .with_scoped_storage("Rhino Brep face label scratch", || {
            ctx.alloc_filled(parent.len(), None, "Rhino Brep face labels")
        })?;
    let mut labels = labels_buffer;
    let mut components = ctx.alloc_filled(parent.len(), 0usize, "Rhino Brep face components")?;
    let mut next = 0;
    let component_count = components.len();
    let mut component_values = components.iter_mut().enumerate();
    for index in 0..component_count {
        let (_, component) = ctx
            .next_charged(&mut component_values, "Rhino Brep face component labeling")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino Brep face component source ended early")
            })?;
        let root = disjoint_root(ctx, &mut parent, index)?;
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
    let (attached_buffer, _attachment_storage) =
        ctx.with_scoped_storage("Rhino Brep free vertex attachment scratch", || {
            ctx.alloc_filled(
                resolved.vertices.len(),
                false,
                "Rhino Brep free-vertex attachment flags",
            )
        })?;
    let mut attached = attached_buffer;
    for (index, vertex) in ctx
        .admit_iter(
            &(resolved.vertices)[..],
            "Rhino brep free vertex indices traversal",
        )
        .map_err(cadmpeg_core::CodecError::from)?
        .enumerate()
    {
        if !vertex.edges.is_empty() {
            attached[index] = true;
        }
    }
    for trim in ctx
        .admit_iter(
            &(resolved.trims)[..],
            "Rhino brep free vertex indices traversal",
        )
        .map_err(cadmpeg_core::CodecError::from)?
    {
        if trim.edge.is_none() {
            attached[trim.vertices[0]] = true;
        }
    }
    let free_count = ctx
        .admit_iter(&attached[..], "Rhino brep free vertex indices traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .filter(|attached| !**attached)
        .count();
    let mut free = ctx.collection_vec(free_count, "Rhino Brep free vertices")?;
    for (index, attached) in ctx
        .admit_iter(attached, "Rhino free vertex output traversal")
        .map_err(cadmpeg_core::CodecError::from)?
        .enumerate()
    {
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
    let mut group_storage = ctx.reserve_scoped(0, "Rhino Brep shell group index scratch")?;
    if raw.minor < 3 || raw.regions.is_empty() {
        let mut face_groups =
            ctx.alloc_filled(components.len(), 0usize, "Rhino Brep fallback face groups")?;
        let mut groups = BTreeMap::new();
        let mut component_values = components.iter().copied().enumerate();
        for face in 0..components.len() {
            let (_, component) = ctx
                .next_charged(&mut component_values, "Rhino region shell groups traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino shell component source ended early")
                })?;
            push_group_face(ctx, &mut group_storage, &mut groups, component, face)?;
        }
        let mut shells = ctx
            .collection_vec(groups.len(), "Rhino Brep shell groups")
            .map_err(crate::curves::GeometryError::from)?;
        let group_count = groups.len();
        let mut groups = groups.into_iter();
        for group in 0..group_count {
            let (_component, faces) = ctx
                .next_charged(&mut groups, "Rhino Brep shell group traversal")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("Rhino shell group source ended early")
                })?;
            for face in ctx
                .admit_iter(&faces[..], "Rhino region shell groups traversal")
                .map_err(cadmpeg_core::CodecError::from)?
            {
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
    let mut grouped = BTreeMap::new();
    let (bounded_sides_buffer, _bounded_storage) =
        ctx.temporary_vec(raw.faces.len(), "Rhino Brep bounded face side scratch")?;
    let mut bounded_sides = bounded_sides_buffer;
    for _ in ctx
        .admit_iter(0..raw.faces.len(), "Rhino Brep bounded face initialization")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        bounded_sides.push((None, 0usize));
    }
    for side in ctx
        .admit_iter(&resolved.face_sides, "Rhino region shell groups traversal")
        .map_err(cadmpeg_core::CodecError::from)?
    {
        if let Some(region) = side.region.filter(|region| {
            raw.regions
                .get(*region)
                .is_some_and(|item| item.region_type == 1)
        }) {
            let Some((bounded_region, bounded_count)) = bounded_sides.get_mut(side.face) else {
                continue;
            };
            *bounded_region = Some(region);
            *bounded_count += 1;
        }
    }
    if !ctx.all_by(
        &bounded_sides,
        |(_, count)| Ok(*count == 1),
        "Rhino Brep bounded face validation",
    )? {
        return region_shell_groups_without_records(ctx, components);
    }
    let mut face_groups =
        ctx.alloc_filled(components.len(), 0usize, "Rhino Brep region face groups")?;
    let mut bounded_sides = bounded_sides.into_iter().enumerate();
    for face in 0..components.len() {
        let (_, (region, _count)) = ctx
            .next_charged(&mut bounded_sides, "Rhino Brep bounded face traversal")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino bounded face source ended early")
            })?;
        if let Some(region) = region {
            push_group_face(
                ctx,
                &mut group_storage,
                &mut grouped,
                (region, components[face]),
                face,
            )?;
        }
    }
    let mut shells = ctx
        .collection_vec(grouped.len(), "Rhino Brep shell groups")
        .map_err(crate::curves::GeometryError::from)?;
    let group_count = grouped.len();
    let mut grouped = grouped.into_iter();
    for group in 0..group_count {
        let ((region, _component), faces) = ctx
            .next_charged(&mut grouped, "Rhino Brep shell group traversal")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino shell group source ended early")
            })?;
        for face in ctx
            .admit_iter(&faces[..], "Rhino region shell groups traversal")
            .map_err(cadmpeg_core::CodecError::from)?
        {
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
    let mut group_storage = ctx.reserve_scoped(0, "Rhino Brep shell group index scratch")?;
    let mut face_groups =
        ctx.alloc_filled(components.len(), 0usize, "Rhino Brep incidence face groups")?;
    let mut groups = BTreeMap::new();
    let mut component_values = components.iter().copied().enumerate();
    for face in 0..components.len() {
        let (_, component) = ctx
            .next_charged(
                &mut component_values,
                "Rhino region shell groups without records traversal",
            )?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino shell component source ended early")
            })?;
        push_group_face(ctx, &mut group_storage, &mut groups, component, face)?;
    }
    let mut shells = ctx
        .collection_vec(groups.len(), "Rhino Brep shell groups")
        .map_err(crate::curves::GeometryError::from)?;
    let group_count = groups.len();
    let mut groups = groups.into_iter();
    for group in 0..group_count {
        let (_component, faces) = ctx
            .next_charged(&mut groups, "Rhino Brep shell group traversal")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino shell group source ended early")
            })?;
        for face in ctx
            .admit_iter(
                &(faces)[..],
                "Rhino region shell groups without records traversal",
            )
            .map_err(cadmpeg_core::CodecError::from)?
        {
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

fn push_group_face<K: Ord + cadmpeg_core::decode::cost::DecodeCost>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    groups: &mut BTreeMap<K, Vec<usize>>,
    key: K,
    face: usize,
) -> Result<(), crate::curves::GeometryError> {
    let faces = storage
        .with_storage(|| ctx.entry_btree_map(groups, key, "Rhino Brep shell group keys"))?
        .or_default();
    ctx.reserve_vec(faces, 1, "Rhino Brep shell group faces")?;
    faces.push(face);
    Ok(())
}

fn disjoint_root(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parent: &mut [usize],
    mut value: usize,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut steps = std::iter::repeat(());
    while parent[value] != value {
        ctx.next_charged(&mut steps, "Rhino Brep disjoint root traversal")?;
        parent[value] = parent[parent[value]];
        value = parent[value];
    }
    Ok(value)
}

fn append_curve_warnings<P: std::fmt::Display + Copy>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    destination: &mut Diagnostics,
    curve: &crate::curves::DecodedCurve,
    prefix: P,
) -> Result<(), cadmpeg_core::CodecError> {
    let _nested = ctx.enter_nested("Rhino curve warning tree")?;
    ctx.fold(
        &curve.warnings()[..],
        (),
        |(), warning| {
            destination.push_coded_admitted(
                ctx,
                warning.code,
                format_args!("{prefix}: {}", warning.message),
            )
        },
        "Rhino append curve warnings borrowed traversal",
    )?;
    if let crate::curves::DecodedCurve::Compound { children, .. } = curve {
        ctx.fold(
            children.as_slice(),
            (),
            |(), (_, child)| append_curve_warnings(ctx, destination, child, prefix),
            "Rhino append curve warnings view traversal",
        )?;
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
    annotations: &mut cadmpeg_ir::Annotations,
    curve: crate::curves::DecodedCurve,
    source: CurveCommitSource<'_>,
    arena_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<cadmpeg_ir::ids::CurveId, CandidateError> {
    let _nested = ctx.enter_nested("Rhino committed curve tree")?;
    let (geometry, definition) = match curve {
        crate::curves::DecodedCurve::Leaf { geometry, .. } => (geometry, None),
        crate::curves::DecodedCurve::Compound {
            children,
            end_parameter,
            ..
        } => {
            let parameter_count = children.len().checked_add(1).ok_or_else(|| {
                CandidateError::Admission("compound curve parameter count overflow".to_string())
            })?;
            let mut parameters =
                ctx.collection_vec(parameter_count, "Rhino committed curve tree parameters")?;
            let mut components =
                ctx.collection_vec(children.len(), "Rhino committed curve tree components")?;
            let mut children = children.into_iter().enumerate();
            for _ in 0..children.len() {
                let (index, (parameter, child)) = ctx
                    .next_charged(&mut children, "Rhino curve component traversal")
                    .map_err(CandidateError::from)?
                    .ok_or_else(|| {
                        CandidateError::Admission(
                            "Rhino curve component source ended early".to_string(),
                        )
                    })?;
                let parameter = parameter.get();
                parameters.push(parameter);
                let (child_path_buffer, _child_path_storage) = ctx.format_scoped(
                    format_args!("{}.component-{index}", source.path),
                    "Rhino committed curve component path",
                )?;
                let child_path = child_path_buffer;
                components.push(cadmpeg_ir::geometry::CompoundComponent {
                    parameter,
                    component: commit_curve_tree(
                        ctx,
                        ir,
                        annotations,
                        child,
                        CurveCommitSource {
                            key: source.key,
                            association: source.association,
                            record: None,
                            path: &child_path,
                        },
                        &mut *arena_storage,
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
    let key = IdentityKey::try_new(source.key.to_owned()).map_err(|error| {
        ctx.format_retained(format_args!("{error}"), "Rhino commit_curve_tree text")
            .map_or_else(Into::into, CandidateError::Admission)
    })?;
    let curve_key = if source.path == "root" {
        key.clone()
    } else {
        key.clone().then(cadmpeg_ir::identity_key!(".")).then(
            IdentityKey::try_new(source.path.to_owned()).map_err(|error| {
                ctx.format_retained(format_args!("{error}"), "Rhino commit_curve_tree text")
                    .map_or_else(Into::into, CandidateError::Admission)
            })?,
        )
    };
    let id = cadmpeg_ir::ids::CurveId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "object", "curve"),
        curve_key.clone(),
    );
    ctx.push_scoped_vec(
        arena_storage,
        &mut ir.model.curves,
        Curve {
            id: id.try_clone_for_decode(ctx, "Rhino typed identity copy")?,
            geometry,
            source_object: Some(
                source
                    .association
                    .try_clone_for_decode(ctx, "Rhino source association copy")?,
            ),
        },
        "Rhino committed curve arena",
    )?;
    set_exactness(ctx, annotations, &id, Exactness::Derived)?;
    if let Some(definition) = definition {
        let procedure_id = cadmpeg_ir::ids::ProceduralCurveId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "object", "procedural-curve"),
            curve_key,
        );
        ir.model
            .add_procedural_curve(ctx, &id, ProceduralCurve::new(procedure_id, definition))?
            .map_err(|error| {
                ctx.format_retained(format_args!("{error}"), "Rhino commit_curve_tree text")
                    .map_or_else(Into::into, CandidateError::Admission)
            })?;
    }
    Ok(id)
}

fn hatch_loop_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    key: &str,
    mut kinds: impl ExactSizeIterator<Item = crate::hatch::LoopKind>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<Vec<(crate::hatch::LoopKind, String)>, cadmpeg_core::CodecError> {
    let mut ids =
        storage.with_storage(|| ctx.collection_vec(kinds.len(), "Rhino hatch loop IDs"))?;
    let mut index = 0usize;
    for _ in 0..kinds.len() {
        let kind = ctx
            .next_charged(&mut kinds, "Rhino hatch loop kind traversal")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino hatch loop kind source ended early")
            })?;
        let id = ctx.format_scoped_text(
            storage,
            format_args!("rhino:object:curve#{key}.hatch-loop-{index}"),
            "Rhino hatch loop ID text",
        )?;
        ids.push((kind, id));
        index += 1;
    }
    Ok(ids)
}

fn hatch_source_links(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    loop_ids: Vec<(crate::hatch::LoopKind, String)>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<Vec<String>, cadmpeg_core::CodecError> {
    let count = loop_ids
        .len()
        .checked_add(1)
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("hatch source link count overflow"))?;
    let mut links =
        storage.with_storage(|| ctx.collection_vec(count, "Rhino hatch source links"))?;
    links.extend(
        ctx.admit_iter(loop_ids, "Rhino hatch source link traversal")
            .map_err(cadmpeg_core::CodecError::from)?
            .map(|(_, id)| id),
    );
    links.push(ctx.copy_scoped_text(
        feature_id.as_str(),
        storage,
        "Rhino hatch feature link text",
    )?);
    Ok(links)
}

/// The hatch plane's placement, scaled into millimetres.
///
/// Both the plane axes and `scale` come off the document, so a scale that
/// drives a coefficient non-finite is a source the transform carrier refuses,
/// not an impossible state. `record` names the hatch the plane came from.
fn hatch_plane_transform(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
    Transform::affine(rows).map_or_else(
        || {
            let offending = rows
                .iter()
                .flatten()
                .copied()
                .find(|value| !value.is_finite())
                .unwrap_or(f64::NAN);
            Err(cadmpeg_core::CodecError::malformed(ctx.format_retained(
                format_args!(
                    "{record}: the hatch plane scaled by {scale} states the non-finite \
             transform coefficient {offending}"
                ),
                "Rhino hatch_plane_transform text",
            )?))
        },
        Ok,
    )
}

fn transform_decoded_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    curve: &mut crate::curves::DecodedCurve,
    transform: Transform,
) -> Result<(), ReferenceFailure> {
    let _nested = ctx.enter_nested("Rhino curve placement nesting")?;
    match curve {
        crate::curves::DecodedCurve::Compound { children, .. } => {
            let child_count = children.len();
            let mut children = children.iter_mut();
            for _ in 0..child_count {
                let (_, child) = ctx
                    .next_charged(&mut children, "Rhino curve placement traversal")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Rhino curve placement child source ended early",
                        )
                    })?;
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
            if let Err(message) = nurbs.try_map_control_points(
                |_, pole| {
                    transform
                        .apply_point(pole.get())
                        .ok_or("instance control point transform produced a non-finite coordinate")
                },
                ctx,
            )? {
                return Err(ReferenceFailure::Semantic(
                    ctx.copy_retained_text(message, "Rhino pole mapping refusal")?,
                ));
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let decoded = crate::curves::DecodedCurve::leaf(
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)),
                Diagnostics::new(),
            );
            let mut nurbs = crate::curves::exact_nurbs(ctx, &decoded, 0).or_else(|error| {
                Err(match error {
                    crate::curves::GeometryError::Codec(error) => ReferenceFailure::Codec(error),
                    other => ReferenceFailure::Semantic(ctx.format_retained(
                        format_args!("analytic instance curve conversion failed: {other}"),
                        "Rhino transform_curve text",
                    )?),
                })
            })?;
            if let Err(message) = nurbs.try_map_control_points(
                |_, pole| {
                    transform
                        .apply_point(pole.get())
                        .ok_or("instance control point transform produced a non-finite coordinate")
                },
                ctx,
            )? {
                return Err(ReferenceFailure::Semantic(
                    ctx.copy_retained_text(message, "Rhino pole mapping refusal")?,
                ));
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

fn transform_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &mut Surface,
    transform: Transform,
) -> Result<(), ReferenceFailure> {
    let geometry = std::mem::replace(
        &mut surface.geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
    );
    surface.geometry = match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(mut nurbs)) => {
            if let Err(message) = nurbs.try_map_control_points(
                |_, pole| {
                    transform
                        .apply_point(pole.get())
                        .ok_or("instance control point transform produced a non-finite coordinate")
                },
                ctx,
            )? {
                return Err(ReferenceFailure::Semantic(
                    ctx.copy_retained_text(message, "Rhino pole mapping refusal")?,
                ));
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    identity: &crate::objects::SourceIdentity,
    instance_path: &[String],
    parent_color: Option<Color>,
    parent_visible: Option<bool>,
) -> Result<SourceObjectAssociation, cadmpeg_core::CodecError> {
    let object_id = ctx.format_retained(
        format_args!("{}", identity.object_id),
        "Rhino source association object ID",
    )?;
    let object_id =
        cadmpeg_core::text::NonBlankString::for_decode(ctx, object_id, "validate nonblank text")?
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("Rhino object UUID is blank"))?;
    let name = (!identity.name.is_empty())
        .then(|| ctx.copy_retained_text(&identity.name, "Rhino source association name"))
        .transpose()?;
    let layer = identity
        .layer
        .as_ref()
        .map(|layer| match layer.id {
            Some(id) => {
                ctx.format_retained(format_args!("{id}"), "Rhino source association layer ID")
            }
            None => ctx.copy_retained_text(&layer.name, "Rhino source association layer name"),
        })
        .transpose()?;
    let mut admitted_path = ctx.collection_vec(
        instance_path.len(),
        "Rhino source association instance path",
    )?;
    let mut path_segments = instance_path.iter();
    for _ in 0..instance_path.len() {
        let segment = ctx
            .next_charged(&mut path_segments, "Rhino source association traversal")?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("Rhino source association path ended early")
            })?;
        admitted_path
            .push(ctx.copy_retained_text(segment, "Rhino source association instance ID")?);
    }
    Ok(SourceObjectAssociation {
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    identity: &crate::objects::SourceIdentity,
    id: cadmpeg_ir::ids::BodyId,
    regions: Vec<cadmpeg_ir::ids::RegionId>,
    association: &SourceObjectAssociation,
) -> Result<Body, cadmpeg_core::CodecError> {
    Ok(Body {
        id,
        kind: BodyKind::General,
        regions,
        transform: None,
        name: (!identity.name.is_empty())
            .then(|| ctx.copy_retained_text(&identity.name, "Rhino body text copy"))
            .transpose()?,
        color: association.color,
        visible: association.visible,
    })
}

fn loss_provenance(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    class: &str,
    outcome: &ClassOutcome<'_>,
) -> Result<SourceProvenance, cadmpeg_core::CodecError> {
    let tag = ctx.format_retained(
        format_args!(
            "OBJECT_RECORD/class={class}/type=0x{:08x}",
            outcome
                .first_object
                .framed()
                .map_or(0, |object| object.object_type)
        ),
        "Rhino class loss tag",
    )?;
    Ok(SourceProvenance::root(
        "rhino",
        cadmpeg_core::decode::u64_from_index(outcome.first_object.range().start),
    )
    .with_tag(tag))
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
    let history_context = match context.neutral_scale() {
        Some(scale) => crate::history::ProjectionContext::Geometry {
            expand,
            archive: scan.archive,
            writer_version: scan.metadata.properties.writer_version,
            scale,
        },
        None => crate::history::ProjectionContext::Metadata(expand.ctx()),
    };
    let mut history_warnings = Diagnostics::new();
    let untyped = context.validate_candidate_fallible(|candidate, _annotations, _arena_storage| {
        crate::history::project(
            history_context,
            &scan.history,
            candidate,
            &mut history_warnings,
        )
    });
    context
        .report
        .phase_warnings
        .append_admitted(expand.ctx(), &mut history_warnings)?;
    match untyped {
        Ok((0, 0, 0, 0)) => {}
        Ok((untyped, failed, dropped_dependencies, redundant_repairs)) => {
            if untyped != 0 {
                push_report_loss(
                    expand.ctx(),
                    &mut context.report.typed_losses,
                    RhinoLossCode::HistoryGeometryNotTransferred,
                    format_args!("{untyped} history value(s) decoded without a neutral carrier"),
                )?;
            }
            if failed != 0 {
                push_report_loss(
                    expand.ctx(),
                    &mut context.report.typed_losses,
                    RhinoLossCode::HistoryEmbeddedGeometryDropped,
                    format_args!(
                        "{failed} embedded history geometry value(s) could not be decoded"
                    ),
                )?;
            }
            if dropped_dependencies != 0 {
                push_report_loss(
                    expand.ctx(),
                    &mut context.report.typed_losses,
                    RhinoLossCode::HistoryDependencyDropped,
                    format_args!("{dropped_dependencies} history dependency edge(s) point to later or ambiguous producers"),
                )?;
            }
            if redundant_repairs != 0 {
                push_report_loss(
                    expand.ctx(),
                    &mut context.report.typed_losses,
                    RhinoLossCode::RedundantFieldRepaired,
                    format_args!("{redundant_repairs} history geometry optional channel repair(s)"),
                )?;
            }
        }
        Err(CandidateError::Codec(error)) => return Err(error),
        Err(error) => context.scan_warnings_for_class(
            "history",
            format_args!("history projection rejected atomically by IR validation: {error}"),
        )?,
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    admitted: Option<T>,
    value: f64,
    default: T,
    field: &str,
    losses: &mut Vec<LossNote>,
) -> Result<T, cadmpeg_core::CodecError> {
    let Some(value_admitted) = admitted else {
        ctx.reserve_vec(losses, 1, "Rhino V1 tolerance losses")?;
        losses.push(crate::wire::admitted_loss(
            ctx,
            RhinoLossCode::RedundantFieldRepaired,
            format_args!(
                "{field} tolerance {value} replaced with default {}",
                default.into()
            ),
            "Rhino V1 tolerance loss text",
        )?);
        return Ok(default);
    };
    Ok(value_admitted)
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    attributes: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: std::fmt::Arguments<'_>,
    value: std::fmt::Arguments<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let key = ctx.format_retained(key, "Rhino full source attribute key")?;
    let key = cadmpeg_core::text::NonBlankString::for_decode(ctx, key, "validate nonblank text")?
        .ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("generated Rhino source attribute key is blank")
    })?;
    let value = ctx.format_retained(value, "Rhino full source attribute value")?;
    ctx.insert_btree_map(attributes, key, value, "Rhino full source attributes")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &Scan<'_>,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, cadmpeg_core::CodecError> {
    let mut attributes = BTreeMap::new();
    macro_rules! attribute {
        ($key:literal, $value:expr) => {
            insert_full_source_attribute(
                ctx,
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
    let mut layer_storage = ctx.reserve_scoped(0, "Rhino layer index scratch")?;
    let mut layer_index_counts = BTreeMap::<i32, usize>::new();
    let layer_count = scan.metadata.layers.len();
    let mut layers = scan.metadata.layers.iter();
    for _ in 0..layer_count {
        let layer = ctx
            .next_charged(&mut layers, "Rhino full source attributes traversal")?
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("Rhino layer source ended early"))?;
        *layer_storage
            .with_storage(|| {
                ctx.entry_btree_map(
                    &mut layer_index_counts,
                    layer.index,
                    "Rhino layer index counts",
                )
            })?
            .or_default() += 1;
    }
    let mut layer_index_occurrences = BTreeMap::<i32, usize>::new();
    let mut layers = scan.metadata.layers.iter();
    for _ in 0..layer_count {
        let layer = ctx
            .next_charged(&mut layers, "Rhino full source attributes traversal")?
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("Rhino layer source ended early"))?;
        let duplicate = if ctx.get_btree_map(
            &layer_index_counts,
            &layer.index,
            "Rhino layer index count lookup",
        )? == Some(&1)
        {
            None
        } else {
            let occurrence = layer_storage
                .with_storage(|| {
                    ctx.entry_btree_map(
                        &mut layer_index_occurrences,
                        layer.index,
                        "Rhino layer index occurrences",
                    )
                })?
                .or_default();
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
