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
    EmptyMember,
}
impl OnshapeLossCode {
    const fn code(self) -> &'static str {
        match self {
            Self::HistoryNotSupported => "history.not-supported",
            Self::VariableEvaluated => "history.variable-evaluated",
            Self::PartNotMatched => "history.part-not-matched",
            Self::ConstraintNotSupported => "sketch.constraint-not-supported",
            Self::ThinNotSupported => "extrude.thin-not-supported",
            Self::EmptyMember => "container.empty-member",
        }
    }
    pub(super) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(
            LossKind::namespaced(
                cadmpeg_ir::loss_namespace!("onshape"),
                self.code(),
                match self {
                    Self::EmptyMember => LossTaxonomy::MetadataNotTransferred,
                    _ => LossTaxonomy::FeatureHistoryRetained,
                },
            ),
            message,
        )
    }
}
