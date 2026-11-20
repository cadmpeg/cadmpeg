// SPDX-License-Identifier: Apache-2.0
//! Onshape transfer loss vocabulary.
use cadmpeg_ir::report::loss::{LossKind, LossNote, LossTaxonomy};

#[derive(Clone, Copy)]
pub(super) enum OnshapeLossCode {
    HistoryNotSupported,
    VariableEvaluated,
    PartNotMatched,
    ConstraintNotSupported,
    ThinNotSupported,
    ThinModelNotSupported,
    DesignOperationNative,
    EmptyMember,
    SketchGeometryNotSupported,
    RecordMalformed,
    MemberTooLarge,
    MemberUnreadable,
    TopologyTooLarge,
    EdgeGeometryUnsampled,
    StepUnreadable,
}
impl OnshapeLossCode {
    const fn code(self) -> &'static str {
        match self {
            Self::HistoryNotSupported => "history.not-supported",
            Self::VariableEvaluated => "history.variable-evaluated",
            Self::PartNotMatched => "history.part-not-matched",
            Self::ConstraintNotSupported => "sketch.constraint-not-supported",
            Self::ThinNotSupported => "extrude.thin-not-supported",
            Self::ThinModelNotSupported => "extrude.thin-model-not-supported",
            Self::DesignOperationNative => "history.design-operation-native",
            Self::EmptyMember => "container.empty-member",
            Self::SketchGeometryNotSupported => "sketch.geometry-not-supported",
            Self::RecordMalformed => "record.malformed",
            Self::MemberUnreadable => "member.unreadable",
            Self::MemberTooLarge => "member.too-large",
            Self::TopologyTooLarge => "topology.too-large",
            Self::EdgeGeometryUnsampled => "topology.edge-geometry-unsampled",
            Self::StepUnreadable => "step.unreadable",
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
                    Self::EmptyMember | Self::MemberTooLarge | Self::MemberUnreadable => {
                        LossTaxonomy::MetadataNotTransferred
                    }
                    _ => LossTaxonomy::FeatureHistoryRetained,
                },
            ),
            message,
        )
    }
}
