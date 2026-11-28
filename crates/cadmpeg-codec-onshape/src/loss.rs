// SPDX-License-Identifier: Apache-2.0
//! Onshape transfer loss vocabulary.
use cadmpeg_ir::report::loss::{LossKind, LossNote, LossTaxonomy};

cadmpeg_ir::loss_codes! {
#[derive(Clone, Copy)]
pub(super) enum OnshapeLossCode {
    HistoryNotSupported,
    PlaneNotSupported,
    SketchPlaneAssumed,
    VariableEvaluated,
    PartNotMatched,
    ConstraintNotSupported,
    ThinNotSupported,
    SweepNotSupported,
    SweepScaleAssumed,
    ShellNotSupported,
    ThinModelNotSupported,
    DesignOperationNative,
    DesignOperationUnresolved,
    DesignParameterUnprojected,
    ChamferSideAssumed,
    ParameterExpressionText,
    EmptyMember,
    SketchGeometryNotSupported,
    RecordMalformed,
    MemberUnreadable,
    EdgeGeometryUnsampled,
    StepUnreadable,
    RollbackUnread,
    AssemblyReferenceUnresolved,
    AssemblyRecordUnsupported,
    AssemblyRecordRepeated,
    AssemblyInstancesMissing,
    MateUnsupported,
    MateLimitsUnresolved,
    AssemblyFeatureUnsupported,
    MateFrameAssumed,
    MateFrameUnresolved,
    MateWorkLimited,
    MateDiagnosticsAggregated,
}
}
impl OnshapeLossCode {
    const fn code(self) -> &'static str {
        match self {
            Self::SketchPlaneAssumed => "sketch.plane-assumed",
            Self::PlaneNotSupported => "plane.not-supported",
            Self::HistoryNotSupported => "history.not-supported",
            Self::VariableEvaluated => "history.variable-evaluated",
            Self::PartNotMatched => "history.part-not-matched",
            Self::ConstraintNotSupported => "sketch.constraint-not-supported",
            Self::SweepNotSupported => "sweep.not-supported",
            Self::SweepScaleAssumed => "sweep.scale-interpolation-assumed",
            Self::ShellNotSupported => "shell.not-supported",
            Self::ThinNotSupported => "extrude.thin-not-supported",
            Self::ThinModelNotSupported => "extrude.thin-model-not-supported",
            Self::DesignOperationNative => "history.design-operation-native",
            Self::DesignOperationUnresolved => "history.design-operation-unresolved",
            Self::DesignParameterUnprojected => "history.parameter-not-projected",
            Self::ChamferSideAssumed => "history.chamfer-side-assumed",
            Self::ParameterExpressionText => "parameter.expression-text",
            Self::EmptyMember => "container.empty-member",
            Self::SketchGeometryNotSupported => "sketch.geometry-not-supported",
            Self::RecordMalformed => "record.malformed",
            Self::MemberUnreadable => "member.unreadable",
            Self::EdgeGeometryUnsampled => "topology.edge-geometry-unsampled",
            Self::StepUnreadable => "step.unreadable",
            Self::RollbackUnread => "history.rollback-unread",
            Self::AssemblyReferenceUnresolved => "assembly.reference-unresolved",
            Self::AssemblyRecordUnsupported => "assembly.record-unsupported",
            Self::AssemblyRecordRepeated => "assembly.record-repeated",
            Self::AssemblyInstancesMissing => "assembly.instances-missing",
            Self::MateUnsupported => "assembly.mate-unsupported",
            Self::MateLimitsUnresolved => "assembly.mate-limits-unresolved",
            Self::AssemblyFeatureUnsupported => "assembly.feature-unsupported",
            Self::MateFrameAssumed => "assembly.mate-frame-assumed",
            Self::MateFrameUnresolved => "assembly.mate-frame-unresolved",
            Self::MateWorkLimited => "assembly.mate-work-limited",
            Self::MateDiagnosticsAggregated => "assembly.mate-diagnostics-aggregated",
        }
    }
    pub(super) fn matches(self, note: &LossNote) -> bool {
        note.code.namespace() == "onshape" && note.code.local_code() == self.code()
    }
    pub(super) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(
            LossKind::namespaced(
                cadmpeg_ir::loss_namespace!("onshape"),
                self.code(),
                match self {
                    Self::StepUnreadable => LossTaxonomy::GeometryNotTransferred,
                    Self::AssemblyReferenceUnresolved | Self::AssemblyInstancesMissing => {
                        LossTaxonomy::AssemblyComponentsExternal
                    }
                    Self::AssemblyRecordRepeated => LossTaxonomy::ObjectRecordsUntransferred,
                    Self::AssemblyRecordUnsupported => {
                        LossTaxonomy::AssemblyPlacementsNotTransferred
                    }
                    Self::EmptyMember | Self::MemberUnreadable => {
                        LossTaxonomy::MetadataNotTransferred
                    }
                    _ => LossTaxonomy::FeatureHistoryRetained,
                },
            ),
            message,
        )
    }
}
