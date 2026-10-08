// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.3dm` decoding and writing.
//!
//! Every fallback, approximation, and drop the codec reports carries a stable
//! machine-readable code from [`RhinoLossCode`]. Codes are the gating surface:
//! harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`RhinoLossCode::note`] is the single construction path for a
//! [`LossNote`] in this crate: it fixes the shared loss category and the
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `rhino` namespace.
//!
//! [`RhinoLossCode::shared_taxonomy`] is an exhaustive match with no fall-through
//! arm. A default arm would silently assign a category to a code added later,
//! and the categories this codec spans (geometry, annotation, attribute,
//! diagnostic) have no honest common default.

use cadmpeg_ir::report::{
    loss::{LossKind, LossNote, LossTaxonomy},
    Severity,
};

/// A vector whose backing storage is temporary while its values can be moved
/// into a retained report or document.
#[derive(Debug)]
pub(crate) struct ScratchVec<'ctx, T> {
    values: Vec<T>,
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'ctx>,
    value_storages: Vec<(usize, cadmpeg_core::decode::ScopedReservation<'ctx>)>,
    /// Shared vector-backing reservation; keep it last so it drops after both vectors.
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx, T> ScratchVec<'ctx, T> {
    pub(crate) fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            values: Vec::new(),
            ctx,
            value_storages: Vec::new(),
            storage: ctx.reserve_scoped(0, operation)?,
        })
    }

    #[cfg(test)]
    pub(crate) fn into_test_values(self) -> Vec<T> {
        self.values
    }

    pub(crate) fn push_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        value: T,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.storage.with_storage(|| {
            ctx.reserve_capacity(&mut self.values, 1, operation)?;
            Ok::<_, cadmpeg_core::CodecError>(())
        })?;
        ctx.charge_collection_items(1, operation)?;
        self.values.push(value);
        Ok(())
    }

    pub(crate) fn push_with_storage_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        make: impl FnOnce() -> Result<T, cadmpeg_core::CodecError>,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let mut value_storage = self.ctx.reserve_scoped(0, operation)?;
        let value = value_storage.with_storage(make)?;
        self.storage.with_storage(|| {
            ctx.reserve_capacity(&mut self.values, 1, operation)?;
            ctx.reserve_capacity(&mut self.value_storages, 1, operation)
        })?;
        ctx.charge_collection_items(1, operation)?;
        let index = self.values.len();
        self.values.push(value);
        self.value_storages.push((index, value_storage));
        Ok(())
    }
}

impl<'ctx> ScratchVec<'ctx, LossNote> {
    /// Moves retained notes and promotes scoped notes into retained output.
    /// It releases all source storage when this method returns.
    pub(crate) fn append_admitted(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        destination: &mut impl AdmittedVec<LossNote>,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let Self {
            values,
            value_storages,
            storage,
            ctx: _,
        } = self;
        let mut value_storages = value_storages.into_iter().peekable();
        let mut source = values.into_iter().enumerate();
        while let Some((index, note)) =
            ctx.find_map(&mut source, |value| Ok::<_, cadmpeg_core::CodecError>(Some(value)), operation)?
        {
            let scoped_storage = match value_storages.peek() {
                Some((stored_index, _)) if *stored_index == index => {
                    value_storages.next().map(|(_, storage)| storage)
                }
                _ => None,
            };
            if let Some(scoped_storage) = scoped_storage {
                let promoted = note
                    .try_clone_for_decode(ctx, operation)
                    .and_then(|promoted| destination.push_admitted(ctx, promoted, operation));
                drop(note);
                drop(scoped_storage);
                promoted?;
            } else {
                destination.push_admitted(ctx, note, operation)?;
            }
        }
        drop(source);
        drop(value_storages);
        drop(storage);
        Ok(())
    }
}

impl<T> std::ops::Deref for ScratchVec<'_, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

/// Admits a pushed value into either retained output or scoped staging.
pub(crate) trait AdmittedVec<T> {
    fn push_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        value: T,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::CodecError>;

    fn push_with_storage_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        make: impl FnOnce() -> Result<T, cadmpeg_core::CodecError>,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::CodecError>;

}

impl<T> AdmittedVec<T> for Vec<T> {
    fn push_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        value: T,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ctx.push_vec(self, value, operation)
    }

    fn push_with_storage_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        make: impl FnOnce() -> Result<T, cadmpeg_core::CodecError>,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.push_admitted(ctx, make()?, operation)
    }

}

impl<T> AdmittedVec<T> for ScratchVec<'_, T> {
    fn push_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        value: T,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ScratchVec::push_admitted(self, ctx, value, operation)
    }

    fn push_with_storage_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        make: impl FnOnce() -> Result<T, cadmpeg_core::CodecError>,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ScratchVec::push_with_storage_admitted(self, ctx, make, operation)
    }

}

/// One decode diagnostic: its message and, when the producer knows it, its code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RhinoDiagnostic {
    /// The loss code the producer assigned, or `None` when the channel decides.
    pub(crate) code: Option<RhinoLossCode>,
    /// The human-readable message.
    pub(crate) message: String,
}

/// The decode diagnostic channel: messages carrying the code their producer knew.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Diagnostics(Vec<RhinoDiagnostic>);

impl Diagnostics {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    /// Admits one decoded diagnostic and its retained message before insertion.
    pub(crate) fn push_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        message: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.push_coded_admitted(ctx, None, message)
    }

    /// Admits one classified diagnostic and its retained message before insertion.
    pub(crate) fn push_coded_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        code: impl Into<Option<RhinoLossCode>>,
        message: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ctx.reserve_vec(&mut self.0, 1, "Rhino diagnostics")?;
        let message = ctx.format_retained(message, "Rhino diagnostic message")?;
        self.0.push(RhinoDiagnostic {
            code: code.into(),
            message,
        });
        Ok(())
    }

    /// Records a diagnostic whose category the consuming channel decides.
    pub(crate) fn push(&mut self, message: impl Into<String>) {
        self.0.push(RhinoDiagnostic {
            code: None,
            message: message.into(),
        });
    }

    #[cfg(test)]
    pub(crate) fn messages<'a>(
        &'a self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<impl Iterator<Item = &'a str> + 'a, cadmpeg_core::CodecError> {
        Ok(ctx
            .admit_iter(self.0.as_slice(), "Rhino diagnostic message traversal")?
            .map(|entry| entry.message.as_str()))
    }

    /// Places `earlier` ahead of the diagnostics already recorded.
    pub(crate) fn prepend_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut earlier: Self,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ctx.append_vec(
            &mut earlier.0,
            &mut self.0,
            "Rhino prepended diagnostic moves",
        )?;
        std::mem::swap(&mut self.0, &mut earlier.0);
        Ok(())
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        self.0.truncate(len);
    }

    /// Moves admitted diagnostics into a second report collection.
    pub(crate) fn append_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        other: &mut Self,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ctx.append_vec(&mut self.0, &mut other.0, "Rhino diagnostic copies")
    }

    /// Adds a source label while admitting each destination diagnostic and message.
    pub(crate) fn append_prefixed_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        other: Self,
        prefix: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ctx.fold(
            &other.0,
            (),
            |(), diagnostic| {
            self.push_coded_admitted(
                ctx,
                diagnostic.code,
                format_args!("{prefix}: {}", diagnostic.message),
            )?;
            Ok(())
            },
            "Rhino prefixed diagnostic traversal",
        )
    }

    /// Copies scratch diagnostics into retained report storage after their
    /// producing storage scopes are no longer active.
    pub(crate) fn append_prefixed_scoped_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        source: ScratchDiagnostics<'_>,
        prefix: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let ScratchDiagnostics { values, storage, ctx: _ } = source;
        let mut values = values.into_iter();
        while let Some((diagnostic, value_storage)) =
            ctx.find_map(&mut values, |value| Ok::<_, cadmpeg_core::CodecError>(Some(value)), "Rhino scoped diagnostic traversal")?
        {
            let copied = self.push_coded_admitted(
                ctx,
                diagnostic.code,
                format_args!("{prefix}: {}", diagnostic.message),
            );
            drop(diagnostic);
            drop(value_storage);
            copied?;
        }
        drop(values);
        drop(storage);
        Ok(())
    }

    /// Promotes scratch diagnostics without changing their message text.
    pub(crate) fn append_scoped_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        source: ScratchDiagnostics<'_>,
        operation: &'static str,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let ScratchDiagnostics { values, storage, ctx: _ } = source;
        let mut values = values.into_iter();
        while let Some((diagnostic, value_storage)) =
            ctx.find_map(&mut values, |value| Ok::<_, cadmpeg_core::CodecError>(Some(value)), operation)?
        {
            let copied = self.push_coded_admitted(
                ctx,
                diagnostic.code,
                format_args!("{}", diagnostic.message),
            );
            drop(diagnostic);
            drop(value_storage);
            copied?;
        }
        drop(values);
        drop(storage);
        Ok(())
    }

    /// Copies diagnostics into another report after admitting the slots and text.
    pub(crate) fn extend_cloned_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        other: &Self,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ctx.fold(
            &other.0,
            (),
            |(), diagnostic| {
                ctx.reserve_vec(&mut self.0, 1, "Rhino diagnostic copies")?;
                self.0.push(RhinoDiagnostic {
                    code: diagnostic.code,
                    message: ctx
                        .copy_retained_text(&diagnostic.message, "Rhino diagnostic copy text")?,
                });
                Ok(())
            },
            "Rhino extend cloned admitted traversal",
        )
    }
}

impl std::ops::Deref for Diagnostics {
    type Target = [RhinoDiagnostic];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Temporary diagnostics whose message and vector storage stay scoped until
/// the parent result succeeds or is discarded.
#[derive(Debug)]
pub(crate) struct ScratchDiagnostics<'ctx> {
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'ctx>,
    values: Vec<(
        RhinoDiagnostic,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    )>,
    /// Vector-backing reservation; keep it last so it drops after `values`.
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx> ScratchDiagnostics<'ctx> {
    pub(crate) fn new(
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            values: Vec::new(),
            ctx,
            storage: ctx.reserve_scoped(0, operation)?,
        })
    }

    pub(crate) fn push_coded_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        code: impl Into<Option<RhinoLossCode>>,
        message: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let code = code.into();
        let mut value_storage = self.ctx.reserve_scoped(0, "Rhino scoped diagnostic message")?;
        let message = value_storage.with_storage(|| {
            ctx.format_retained(message, "Rhino diagnostic message")
        })?;
        self.storage.with_storage(|| {
            ctx.reserve_capacity(&mut self.values, 1, "Rhino scoped diagnostics")
        })?;
        ctx.charge_collection_items(1, "Rhino scoped diagnostics")?;
        self.values.push((
            RhinoDiagnostic { code, message },
            value_storage,
        ));
        Ok(())
    }

    /// Copies child diagnostics into this parent scratch collection. A failed
    /// parent can then drop all merged messages and their storage together.
    pub(crate) fn append_prefixed_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        source: Self,
        prefix: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let Self { values, storage, ctx: _ } = source;
        let mut values = values.into_iter();
        while let Some((diagnostic, source_storage)) =
            ctx.find_map(&mut values, |value| Ok::<_, cadmpeg_core::CodecError>(Some(value)), "Rhino scoped diagnostic traversal")?
        {
            let copied = self.push_coded_admitted(
                ctx,
                diagnostic.code,
                format_args!("{prefix}: {}", diagnostic.message),
            );
            drop(diagnostic);
            drop(source_storage);
            copied?;
        }
        drop(values);
        drop(storage);
        Ok(())
    }
}

impl std::ops::Deref for RhinoDiagnostic {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.message
    }
}

/// Diagnostic destination shared by retained and scratch parser paths.
pub(crate) trait DiagnosticSink {
    fn push_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        message: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError>;

    fn push_coded_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        code: impl Into<Option<RhinoLossCode>>,
        message: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError>;
}

impl DiagnosticSink for Diagnostics {
    fn push_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        message: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        Diagnostics::push_admitted(self, ctx, message)
    }

    fn push_coded_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        code: impl Into<Option<RhinoLossCode>>,
        message: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        Diagnostics::push_coded_admitted(self, ctx, code, message)
    }
}

impl DiagnosticSink for ScratchDiagnostics<'_> {
    fn push_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        message: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ScratchDiagnostics::push_coded_admitted(self, ctx, None, message)
    }

    fn push_coded_admitted(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        code: impl Into<Option<RhinoLossCode>>,
        message: std::fmt::Arguments<'_>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ScratchDiagnostics::push_coded_admitted(self, ctx, code, message)
    }
}

impl Extend<RhinoDiagnostic> for Diagnostics {
    fn extend<T: IntoIterator<Item = RhinoDiagnostic>>(&mut self, iter: T) {
        self.0.extend(iter);
    }
}

impl FromIterator<RhinoDiagnostic> for Diagnostics {
    fn from_iter<T: IntoIterator<Item = RhinoDiagnostic>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl IntoIterator for Diagnostics {
    type Item = RhinoDiagnostic;
    type IntoIter = std::vec::IntoIter<RhinoDiagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a RhinoDiagnostic;
    type IntoIter = std::slice::Iter<'a, RhinoDiagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// A stable, machine-readable identifier for one `.3dm` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`RhinoLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum RhinoLossCode {
    /// Container or table scan surfaced a structural diagnostic.
    ContainerScanDiagnostic,
    /// A stored checksum does not match the protected bytes.
    IntegrityFailure,
    /// A framed presentation record could not be transferred.
    PresentationRecordDropped,
    /// A framed annotation record could not be transferred.
    AnnotationRecordDropped,
    /// A framed product occurrence could not be transferred to the native graph.
    ProductOccurrenceDropped,
    /// A recognized annotation userdata payload could not be typed.
    AnnotationUserdataDropped,
    /// Viewport userdata has no typed CADIR owner.
    ViewportUserdataDropped,
    /// Mesh n-gon grouping is not represented in neutral tessellation.
    MeshNgonGroupingDropped,
    /// A stored enumeration value was retained but could not select a neutral value.
    EnumerationValueDegraded,
    /// A redundant count or size was inconsistent; dependent data was dropped.
    RedundantFieldRepaired,
    /// A Brep display-mesh cache slot was wrong-class or unreadable.
    BrepMeshCacheDegraded,
    /// A duplicate source record was resolved by the format's ownership rule.
    DuplicateRecordResolved,
    /// A stored quad was converted to neutral triangles.
    MeshQuadTopologyTriangulated,
    /// Embedded history geometry could not be decoded.
    HistoryEmbeddedGeometryDropped,
    /// A dimension-style override object was not applied.
    DimensionOverrideDropped,
    /// A history dependency points to a later producer and cannot enter ordered IR.
    HistoryDependencyDropped,
    /// Instance-definition records were malformed, ambiguous, or checksum-degraded.
    ContainerInstanceDefinitionDegraded,
    /// Census of object records framed against object records transferred.
    ObjectRecordCensus,
    /// An object class is not decoded; its records carry only retained bytes.
    ObjectFamilyNotTransferred,
    /// Object attributes (name, layer, color, visibility) did not transfer whole.
    ObjectAttributesDegraded,
    /// A framed object record could not be decoded from its payload.
    ObjectFramingUndecodable,
    /// A decode phase surfaced a per-record diagnostic.
    ObjectDecodeDiagnostic,
    /// A discontinuous polycurve join moved both source endpoints to their midpoint.
    PolycurveJoinGap,
    /// One B-rep trim lost its parameter-space curve while its topology remained.
    TrimPcurveDropped,
    /// B-rep topology fell back to a carrier-only transfer.
    TopologyBrepFallback,
    /// Hatch fill pattern is retained as a native pattern index, not a filled region.
    HatchFillNotTransferred,
    /// Polyedge segment references are retained without resolved edge identities.
    PolyedgeReferencesNotResolved,
    /// Detail-view projection state is retained as a digest, not a decoded view.
    DetailViewNotTransferred,
    /// Trivariate cage control lattice is retained as text, not a typed deformation.
    CageLatticeNotTransferred,
    /// Space-morph deformation is retained as native parameters, not applied.
    MorphDeformationNotApplied,
    /// Curve-on-surface trim binding is retained as native parameters.
    CurveOnSurfaceBindingNotTransferred,
    /// A dimension style reference does not resolve to a decoded style record.
    DimensionStyleUnresolved,
    /// A dimension detail-view reference does not resolve to a decoded view.
    DimensionDetailReferenceUnresolved,
    /// A definition member or captive object UUID does not resolve to one record.
    ReferenceMemberUnresolved,
    /// A definition member or captive object UUID resolves to several records.
    ReferenceMemberAmbiguous,
    /// History-record geometry is retained without a neutral carrier.
    HistoryGeometryNotTransferred,
    /// Standalone mesh vertices are written at reduced (f32) precision.
    MeshVertexPrecisionReduced,
    /// Mesh normals are written at reduced (f32) precision.
    MeshNormalPrecisionReduced,
    /// A field was read under the legacy reading because the archive carries no
    /// openNURBS writer-version stamp to verify that record against.
    ///
    /// Per record, inside a dialect the archive word already identified and
    /// whose grammar this codec verified. It is deliberately *not*
    /// `source.dialect-unverified`: the other codecs pin that string for the
    /// document-level statement "the dialect itself was not verified", which
    /// holds exactly when `Admission::Residual` is reported. Rhino
    /// charges this one inside `Admission::Admitted` documents, so a consumer
    /// joining loss code to admission state must be able to tell them apart.
    /// The taxonomy family below is still the right one.
    SourceWriterStampUnverified,
    /// The archive-version word is outside the declared set, so the document
    /// was read by the residual chunk-width route without a declared identity.
    ///
    /// Document level, and the counterpart of the per-record code above:
    /// charged exactly when the primary-layer `crate::dialect` match is
    /// `Admission::Residual`, from the same predicate that decides
    /// the admission. A word no row claims still selects its own chunk width
    /// and checksum mechanics. No declared archive row is substituted, and
    /// nothing verified that the observed word means those mechanics.
    SourceDialectUnverified,
    /// The selected write target differs from the same-format source dialect.
    SourceDialectDisplaced,
    /// Body kind came from the closed-shell gauge or from an unverified stored
    /// solid flag rather than from a flag the writer stamp vouches for.
    TopologyBodyKindGaugeSubstituted,
}

impl RhinoLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [RhinoLossCode] = &[
        Self::ContainerScanDiagnostic,
        Self::IntegrityFailure,
        Self::PresentationRecordDropped,
        Self::AnnotationRecordDropped,
        Self::ProductOccurrenceDropped,
        Self::AnnotationUserdataDropped,
        Self::ViewportUserdataDropped,
        Self::MeshNgonGroupingDropped,
        Self::EnumerationValueDegraded,
        Self::RedundantFieldRepaired,
        Self::BrepMeshCacheDegraded,
        Self::DuplicateRecordResolved,
        Self::MeshQuadTopologyTriangulated,
        Self::HistoryEmbeddedGeometryDropped,
        Self::DimensionOverrideDropped,
        Self::HistoryDependencyDropped,
        Self::ContainerInstanceDefinitionDegraded,
        Self::ObjectRecordCensus,
        Self::ObjectFamilyNotTransferred,
        Self::ObjectAttributesDegraded,
        Self::ObjectFramingUndecodable,
        Self::ObjectDecodeDiagnostic,
        Self::PolycurveJoinGap,
        Self::TrimPcurveDropped,
        Self::TopologyBrepFallback,
        Self::HatchFillNotTransferred,
        Self::PolyedgeReferencesNotResolved,
        Self::DetailViewNotTransferred,
        Self::CageLatticeNotTransferred,
        Self::MorphDeformationNotApplied,
        Self::CurveOnSurfaceBindingNotTransferred,
        Self::DimensionStyleUnresolved,
        Self::DimensionDetailReferenceUnresolved,
        Self::ReferenceMemberUnresolved,
        Self::ReferenceMemberAmbiguous,
        Self::HistoryGeometryNotTransferred,
        Self::MeshVertexPrecisionReduced,
        Self::MeshNormalPrecisionReduced,
        Self::SourceWriterStampUnverified,
        Self::SourceDialectUnverified,
        Self::SourceDialectDisplaced,
        Self::TopologyBodyKindGaugeSubstituted,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::ContainerScanDiagnostic => "container.scan-diagnostic",
            Self::IntegrityFailure => "container.integrity-failure",
            Self::PresentationRecordDropped => "presentation.record-dropped",
            Self::AnnotationRecordDropped => "annotation.record-dropped",
            Self::ProductOccurrenceDropped => "product.occurrence-dropped",
            Self::AnnotationUserdataDropped => "annotation.userdata-dropped",
            Self::ViewportUserdataDropped => "viewport.userdata-dropped",
            Self::MeshNgonGroupingDropped => "mesh.ngon-grouping-dropped",
            Self::EnumerationValueDegraded => "container.enumeration-value-degraded",
            Self::RedundantFieldRepaired => "container.redundant-field-repaired",
            Self::BrepMeshCacheDegraded => "brep.mesh-cache-degraded",
            Self::DuplicateRecordResolved => "container.duplicate-record-resolved",
            Self::MeshQuadTopologyTriangulated => "mesh.quad-topology-triangulated",
            Self::HistoryEmbeddedGeometryDropped => "history.embedded-geometry-dropped",
            Self::DimensionOverrideDropped => "dimension.override-dropped",
            Self::HistoryDependencyDropped => "history.dependency-dropped",
            Self::ContainerInstanceDefinitionDegraded => "container.instance-definition-degraded",
            Self::ObjectRecordCensus => "object.record-census",
            Self::ObjectFamilyNotTransferred => "object.family-not-transferred",
            Self::ObjectAttributesDegraded => "object.attributes-degraded",
            Self::ObjectFramingUndecodable => "object.framing-undecodable",
            Self::ObjectDecodeDiagnostic => "object.decode-diagnostic",
            Self::PolycurveJoinGap => "curve.polycurve-join-gap",
            Self::TrimPcurveDropped => "brep.trim-pcurve-dropped",
            Self::TopologyBrepFallback => "topology.brep-fallback",
            Self::HatchFillNotTransferred => "hatch.fill-not-transferred",
            Self::PolyedgeReferencesNotResolved => "polyedge.references-not-resolved",
            Self::DetailViewNotTransferred => "detail.view-not-transferred",
            Self::CageLatticeNotTransferred => "cage.lattice-not-transferred",
            Self::MorphDeformationNotApplied => "morph.deformation-not-applied",
            Self::CurveOnSurfaceBindingNotTransferred => "curve-on-surface.binding-not-transferred",
            Self::DimensionStyleUnresolved => "dimension.style-unresolved",
            Self::DimensionDetailReferenceUnresolved => "dimension.detail-reference-unresolved",
            Self::ReferenceMemberUnresolved => "reference.member-unresolved",
            Self::ReferenceMemberAmbiguous => "reference.member-ambiguous",
            Self::HistoryGeometryNotTransferred => "history.geometry-not-transferred",
            Self::MeshVertexPrecisionReduced => "mesh.vertex-precision-reduced",
            Self::MeshNormalPrecisionReduced => "mesh.normal-precision-reduced",
            Self::SourceWriterStampUnverified => "source.writer-stamp-unverified",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::SourceDialectDisplaced => "target.source-dialect-displaced",
            Self::TopologyBodyKindGaugeSubstituted => "topology.body-kind-gauge-substituted",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::ObjectRecordCensus => Severity::Info,
            Self::ObjectFramingUndecodable | Self::IntegrityFailure => Severity::Error,
            _ => Severity::Warning,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::ContainerScanDiagnostic
            | Self::ContainerInstanceDefinitionDegraded
            | Self::ObjectFramingUndecodable
            | Self::ObjectDecodeDiagnostic
            | Self::AnnotationRecordDropped
            | Self::AnnotationUserdataDropped
            | Self::PolycurveJoinGap
            | Self::ReferenceMemberUnresolved
            | Self::ReferenceMemberAmbiguous => LossTaxonomy::DecodeDiagnostic,
            Self::TrimPcurveDropped => LossTaxonomy::PcurveOmitted,
            Self::IntegrityFailure => LossTaxonomy::IntegrityFailure,
            Self::PresentationRecordDropped => LossTaxonomy::AssetNotTransferred,
            Self::ViewportUserdataDropped | Self::ProductOccurrenceDropped => {
                LossTaxonomy::RecordNotTyped
            }
            Self::MeshNgonGroupingDropped | Self::MeshQuadTopologyTriangulated => {
                LossTaxonomy::RecordNotTyped
            }
            Self::EnumerationValueDegraded | Self::RedundantFieldRepaired => {
                LossTaxonomy::DecodeDiagnostic
            }
            Self::BrepMeshCacheDegraded => LossTaxonomy::RecordNotTyped,
            Self::DuplicateRecordResolved => LossTaxonomy::DecodeDiagnostic,
            Self::HistoryEmbeddedGeometryDropped => LossTaxonomy::GeometryNotTransferred,
            Self::DimensionOverrideDropped => LossTaxonomy::PmiOmitted,
            Self::HistoryDependencyDropped => LossTaxonomy::ReferenceGraphNotClosed,
            Self::ObjectRecordCensus => LossTaxonomy::ObjectRecordsUntransferred,
            Self::ObjectFamilyNotTransferred => LossTaxonomy::UnsupportedObjectFamily,
            Self::ObjectAttributesDegraded => LossTaxonomy::AttributesNotTransferred,
            Self::TopologyBrepFallback => LossTaxonomy::TopologyNotTransferred,
            Self::HatchFillNotTransferred
            | Self::PolyedgeReferencesNotResolved
            | Self::DetailViewNotTransferred
            | Self::CageLatticeNotTransferred
            | Self::MorphDeformationNotApplied
            | Self::CurveOnSurfaceBindingNotTransferred
            | Self::HistoryGeometryNotTransferred => LossTaxonomy::RecordNotTyped,
            Self::DimensionStyleUnresolved | Self::DimensionDetailReferenceUnresolved => {
                LossTaxonomy::PmiOmitted
            }
            Self::MeshVertexPrecisionReduced | Self::MeshNormalPrecisionReduced => {
                LossTaxonomy::MeshVertexPrecision
            }
            Self::SourceWriterStampUnverified | Self::SourceDialectUnverified => {
                LossTaxonomy::SourceDialectUnverified
            }
            Self::SourceDialectDisplaced => LossTaxonomy::SourceDialectDisplaced,
            Self::TopologyBodyKindGaugeSubstituted => LossTaxonomy::TopologyGaugeSubstituted,
        }
    }

    /// Strict floor pinned from this local code (independent of taxonomy remap).
    ///
    /// Defaults to the taxonomy floor so a later local→taxonomy remap cannot
    /// silently change rejection. `ObjectFramingUndecodable` pins Warning above
    /// its `DecodeDiagnostic` taxonomy (which is otherwise tolerable).
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::IntegrityFailure | Self::ObjectFramingUndecodable => Some(Severity::Warning),
            Self::SourceWriterStampUnverified => None,
            other => other.shared_taxonomy().strict_floor(),
        }
    }

    /// Namespaced [`LossKind`] for this local code (taxonomy + pinned floor).
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        cadmpeg_ir::report::loss::NamespacedLossKind::new(
            const {
                match cadmpeg_ir::report::loss::LossNamespace::new("rhino") {
                    Ok(namespace) => namespace,
                    Err(_) => panic!("reserved codec namespace"),
                }
            },
            self.code(),
            self.shared_taxonomy(),
        )
        .with_strict_floor(self.strict_floor())
        .into()
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `rhino/<local>`; the message is the per-instance
    /// text only. Severity and strict floor come from the local code.
    #[must_use]
    pub(crate) fn note(self, message: String) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::RhinoLossCode;
    use std::collections::BTreeSet;

    #[test]
    fn failed_scoped_scratch_value_releases_its_storage() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_materialized_bytes = 4;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        let mut values = super::ScratchVec::<String>::new(&ctx, "scratch values")
            .expect("scratch vector reservation");
        let error = values
            .push_with_storage_admitted(
                &ctx,
                || {
                    ctx.copy_retained_text("four", "scratch field")?;
                    Err(cadmpeg_core::CodecError::malformed("injected failure"))
                },
                "scratch values",
            )
            .expect_err("failed value is not inserted");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
        assert!(ctx.reserve_scoped(4, "released scratch field").is_ok());
    }

    #[test]
    fn scratch_loss_transfer_stops_at_the_first_unadmitted_note() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        let mut source = super::ScratchVec::new(&ctx, "source notes")
            .expect("scratch vector reservation");
        for message in ["first", "second"] {
            source
                .push_admitted(
                    &ctx,
                    RhinoLossCode::PresentationRecordDropped.note(message.to_owned()),
                    "source notes",
                )
                .expect("source note slot");
        }
        let mut destination = Vec::new();
        let error = source
            .append_admitted(&ctx, &mut destination, "loss note traversal")
            .expect_err("second source step exceeds the work limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.operation == "loss note traversal"
        ));
        assert_eq!(destination.len(), 1);
        assert_eq!(destination[0].message, "first");
    }

    #[test]
    fn diagnostic_copy_stops_at_the_first_unadmitted_source() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        let mut source = super::Diagnostics::new();
        source.push("");
        source.push("");
        let mut destination = super::Diagnostics::new();
        let error = destination
            .extend_cloned_admitted(&ctx, &source)
            .expect_err("second source step exceeds the work limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.operation == "Rhino extend cloned admitted traversal"
        ));
        assert_eq!(destination.len(), 1);
    }

    #[test]
    fn prefixed_diagnostic_copy_stops_at_the_first_unadmitted_source() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 5;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        let mut source = super::Diagnostics::new();
        source.push("");
        source.push("");
        let mut destination = super::Diagnostics::new();
        let error = destination
            .append_prefixed_admitted(&ctx, source, format_args!(""))
            .expect_err("second source step exceeds the work limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.operation == "Rhino prefixed diagnostic traversal"
        ));
        assert_eq!(destination.len(), 1);
        assert_eq!(destination[0].message, ": ");
    }

    #[test]
    fn diagnostic_copy_refuses_collection_limit() {
        let mut source = super::Diagnostics::new();
        source.push("mesh warning");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        let refusal = super::Diagnostics::new()
            .extend_cloned_admitted(&ctx, &source)
            .expect_err("one diagnostic copy exceeds zero collection items");
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(item)
                if item.operation == "Rhino diagnostic copies"
        ));
        let mut copy = super::Diagnostics::new();
        copy.extend_cloned_admitted(&cadmpeg_test_support::service_decode_context(), &source)
            .expect("service profile admits diagnostic copy");
        assert_eq!(copy, source);
    }

    #[test]
    fn diagnostics_refuse_collection_and_retained_limits() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        let refusal = super::Diagnostics::new()
            .push_admitted(&ctx, format_args!("fixture warning"))
            .expect_err("one diagnostic exceeds zero collection items");
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino diagnostics"
        ));
        let mut retained_policy = cadmpeg_core::decode::DecodePolicy::service();
        retained_policy.limits.max_retained_bytes =
            crate::test_support::retained_limit_at("Rhino diagnostic message", 0, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .expect("empty root");
                match super::Diagnostics::new()
                    .push_coded_admitted(
                        &ctx,
                        RhinoLossCode::IntegrityFailure,
                        format_args!("fixture warning"),
                    )
                    .expect_err("diagnostic refusal")
                {
                    cadmpeg_core::CodecError::ResourceLimit(limit) => limit,
                    error => panic!("diagnostic refusal: {error:?}"),
                }
            });
        let (retained_ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &retained_policy)
                .expect("empty root admitted");
        let refusal = super::Diagnostics::new()
            .push_coded_admitted(
                &retained_ctx,
                RhinoLossCode::IntegrityFailure,
                format_args!("fixture warning"),
            )
            .expect_err("diagnostic text exceeds zero retained bytes");
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino diagnostic message"
        ));
    }

    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = RhinoLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "container.scan-diagnostic",
                "container.integrity-failure",
                "presentation.record-dropped",
                "annotation.record-dropped",
                "product.occurrence-dropped",
                "annotation.userdata-dropped",
                "viewport.userdata-dropped",
                "mesh.ngon-grouping-dropped",
                "container.enumeration-value-degraded",
                "container.redundant-field-repaired",
                "brep.mesh-cache-degraded",
                "container.duplicate-record-resolved",
                "mesh.quad-topology-triangulated",
                "history.embedded-geometry-dropped",
                "dimension.override-dropped",
                "history.dependency-dropped",
                "container.instance-definition-degraded",
                "object.record-census",
                "object.family-not-transferred",
                "object.attributes-degraded",
                "object.framing-undecodable",
                "object.decode-diagnostic",
                "curve.polycurve-join-gap",
                "brep.trim-pcurve-dropped",
                "topology.brep-fallback",
                "hatch.fill-not-transferred",
                "polyedge.references-not-resolved",
                "detail.view-not-transferred",
                "cage.lattice-not-transferred",
                "morph.deformation-not-applied",
                "curve-on-surface.binding-not-transferred",
                "dimension.style-unresolved",
                "dimension.detail-reference-unresolved",
                "reference.member-unresolved",
                "reference.member-ambiguous",
                "history.geometry-not-transferred",
                "mesh.vertex-precision-reduced",
                "mesh.normal-precision-reduced",
                "source.writer-stamp-unverified",
                "source.dialect-unverified",
                "target.source-dialect-displaced",
                "topology.body-kind-gauge-substituted",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in RhinoLossCode::ALL {
            let text = code.code();
            assert!(seen.insert(text), "duplicate code {text}");
            let (family, detail) = text.split_once('.').expect("family.detail shape");
            assert!(!family.is_empty() && !detail.is_empty());
            assert!(
                text.bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'-'),
                "code {text} is not lowercase kebab"
            );
        }
    }

    /// The note builder fixes severity from the code and emits the code string.
    #[test]
    fn note_takes_severity_from_the_code_and_renders_it() {
        for code in RhinoLossCode::ALL {
            let note = code.note("x".into());
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert_eq!(note.code.namespace(), "rhino");
            assert_eq!(note.code.local_code(), code.code());
            assert!(note.provenance.is_none());
        }
    }

    #[test]
    fn missing_writer_stamp_does_not_trigger_document_dialect_strictness() {
        assert_eq!(
            RhinoLossCode::SourceWriterStampUnverified
                .kind()
                .strict_floor(),
            None
        );
        assert_eq!(
            RhinoLossCode::SourceDialectUnverified.kind().strict_floor(),
            Some(cadmpeg_ir::report::Severity::Warning)
        );
    }
}
