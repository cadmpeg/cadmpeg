// SPDX-License-Identifier: Apache-2.0
//! Typed Siemens NX object-model records retained in the native namespace.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;

use crate::decode::Scan;

pub(crate) mod assembly_placement;
mod attach;
pub(crate) mod catalogue;
pub(crate) mod display_jt;
pub(crate) mod document_validation;
mod features;
pub(crate) mod hex;
pub(crate) mod history;
pub(crate) mod model;
pub(crate) mod om;
mod parasolid;
pub(crate) mod rmfastload_storage;
mod segments;
pub(crate) mod structure;
pub(crate) mod substrate;
pub(crate) mod toggle;

/// Availability of typed native records during container retention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypedNative {
    /// Typed native extraction is available.
    Available,
    /// Only container records are retained.
    ContainerOnly,
}

/// Attach a pre-extracted [`crate::native::model::NativeModel`] to `ir`: annotations, namespace arenas,
/// and semantic islands. Build the model with [`crate::native::model::NativeModel::extract`].
pub(crate) fn attach_annotations(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    attach::attach(ctx, ir, model, scan, annotations, unknowns, losses)
}

/// Preserve container-layer records without extracting typed native entities.
pub(crate) fn attach_container_layer(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    scan: &Scan,
    annotations: &mut AnnotationBuilder,
    unknowns: &mut Vec<UnknownRecord>,
    typed_native: TypedNative,
) -> Result<(), CodecError> {
    attach::attach_container_layer(ctx, ir, scan, annotations, unknowns, typed_native)
}

/// Admit scanner contracts while the actual source positions and encodings exist.
pub(crate) fn source_validation_findings(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
) -> Result<Vec<cadmpeg_ir::report::check::Finding>, CodecError> {
    match admit_source_records(ctx, namespace) {
        Ok(()) => Ok(Vec::new()),
        Err(error) => {
            let message = error.to_string();
            let error = CodecError::from(error);
            if matches!(error, CodecError::ResourceLimit(_)) {
                return Err(error);
            }
            Ok(vec![cadmpeg_ir::report::check::Finding {
                related: Vec::new(),
                check: cadmpeg_ir::report::check::Check::NativeLinks,
                severity: cadmpeg_ir::report::Severity::Error,
                message,
                entity: None,
            }])
        }
    }
}

/// Admit scanner contracts while the actual source positions and encodings exist.
pub(crate) fn admit_source_records(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
) -> Result<(), cadmpeg_ir::native::NativeConvertError> {
    rmfastload_storage::admit(ctx, namespace)?;
    admit_source_construction_records(ctx, namespace)?;
    for record in namespace.arena_iter_as_for_decode::<assembly_placement::AssemblyPlacement>(
        ctx,
        "assembly_placements",
    ) {
        record?;
    }
    display_jt::admission::DisplayJtGraph::from_namespace(ctx, namespace)?;
    structure::occurrences::FastLoadOccurrences::from_namespace(ctx, namespace)?;
    Ok(())
}

/// Admit construction target byte contracts and complete surface allocations.
fn admit_source_construction_records(
    ctx: &DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
) -> Result<(), cadmpeg_ir::native::NativeConvertError> {
    for record in namespace
        .arena_iter_as_for_decode::<features::operation_record::FeatureOperationRecord>(
            ctx,
            "feature_operation_records",
        )
    {
        record?;
    }
    for record in namespace
        .arena_iter_as_for_decode::<features::named_target::FeatureConstructionNamedTarget>(
            ctx,
            "feature_construction_named_targets",
        )
    {
        record?.validate_links(namespace)?;
    }
    for record in namespace
        .arena_iter_as_for_decode::<features::swp104_branch::FeatureSwp104LeadingBranch>(
            ctx,
            "feature_swp104_leading_branches",
        )
    {
        record?;
    }
    for record in namespace
        .arena_iter_as_for_decode::<features::enlarge::FeatureEnlargeConstructionEnvelope>(
            ctx,
            "feature_enlarge_construction_envelopes",
        )
    {
        record?;
    }
    for record in namespace
        .arena_iter_as_for_decode::<features::extract_face::FeatureExtractFaceConstructionEnvelope>(
            ctx,
            "feature_extract_face_construction_envelopes",
        )
    {
        record?;
    }
    for record in namespace.arena_iter_as_for_decode::<features::holes::FeatureSymbolicThread>(
        ctx,
        "feature_symbolic_threads",
    ) {
        record?;
    }
    for arena in [
        "feature_operation_body_writes",
        "feature_unlabeled_operation_body_writes",
    ] {
        for record in
            namespace.arena_iter_as_for_decode::<features::FeatureOperationBodyWrite>(ctx, arena)
        {
            record?;
        }
    }
    features::surface_allocation::admit_surface_allocations(ctx, namespace)
}

/// Failure injection into [`attach_annotations`] for decode tests.
#[cfg(test)]
pub(crate) mod attach_failure {
    use cadmpeg_core::CodecError;
    use std::cell::Cell;

    /// Where the injected failure occurs.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Point {
        /// Before attachment writes anything.
        Entry,
        /// After attachment has written everything.
        Exit,
    }

    /// An injected failure: where it occurs and the error it returns.
    type Injection = (Point, fn() -> CodecError);

    std::thread_local! {
        static INJECTED: Cell<Option<Injection>> = const { Cell::new(None) };
    }

    /// Clears the injected failure when dropped.
    pub(crate) struct Injected(());

    impl Drop for Injected {
        fn drop(&mut self) {
            INJECTED.set(None);
        }
    }

    /// Fail attachment on this thread at `point` while the guard lives.
    pub(crate) fn inject(point: Point, error: fn() -> CodecError) -> Injected {
        INJECTED.set(Some((point, error)));
        Injected(())
    }

    pub(super) fn check(point: Point) -> Result<(), CodecError> {
        match INJECTED.get() {
            Some((injected, error)) if injected == point => Err(error()),
            _ => Ok(()),
        }
    }
}
