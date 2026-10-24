// SPDX-License-Identifier: Apache-2.0
//! Failure of one measurement step: an unavailable value, or a session
//! refusal that ends the measurement.

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::EvaluationFailure;

use crate::reason::UnavailableReason;
use crate::value::{Quantity, Unavailable};

/// The result of one measurement step.
pub(crate) type Measure<T> = Result<T, MeasureFailure>;

/// Why a measurement step has no value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MeasureFailure {
    /// The value is unavailable for a pinned reason.
    Unavailable {
        reason: UnavailableReason,
        entity: Option<String>,
        message: String,
    },
    /// The decode session refused. The measurement ends.
    Refusal(ResourceLimit),
}

impl MeasureFailure {
    pub(crate) fn unavailable(
        reason: UnavailableReason,
        entity: Option<&str>,
        message: impl Into<String>,
    ) -> Self {
        Self::Unavailable {
            reason,
            entity: entity.map(str::to_owned),
            message: message.into(),
        }
    }

    /// The same failure naming `entity` when it names none.
    #[must_use]
    pub(crate) fn at(self, entity: &str) -> Self {
        match self {
            Self::Unavailable {
                reason,
                entity: None,
                message,
            } => Self::Unavailable {
                reason,
                entity: Some(entity.to_owned()),
                message,
            },
            other => other,
        }
    }

    /// Map a codec error from an entry point that takes the session context.
    /// A resource refusal propagates; every other error has no value.
    pub(crate) fn from_codec(error: CodecError) -> Self {
        match error {
            CodecError::ResourceLimit(limit) => Self::Refusal(limit),
            other => Self::unavailable(
                UnavailableReason::EvaluationNoValue,
                None,
                format!("evaluation failed: {other}"),
            ),
        }
    }
}

impl<R> From<EvaluationFailure<R>> for MeasureFailure {
    fn from(failure: EvaluationFailure<R>) -> Self {
        match failure {
            EvaluationFailure::NoValue => Self::unavailable(
                UnavailableReason::EvaluationNoValue,
                None,
                "an evaluator has no value at a needed parameter",
            ),
            EvaluationFailure::NonFinite(_) => Self::unavailable(
                UnavailableReason::EvaluationNonFinite,
                None,
                "an evaluation left the finite range",
            ),
            EvaluationFailure::WorkExhausted => Self::unavailable(
                UnavailableReason::EvaluationWorkExhausted,
                None,
                "the work slice ran out",
            ),
            EvaluationFailure::ResourceLimit(limit) => Self::Refusal(limit),
        }
    }
}

impl From<ResourceLimit> for MeasureFailure {
    fn from(limit: ResourceLimit) -> Self {
        Self::Refusal(limit)
    }
}

/// Why a value is unavailable, without the session refusal that
/// [`MeasureFailure`] can also carry.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Missing {
    pub(crate) reason: UnavailableReason,
    pub(crate) entity: Option<String>,
    pub(crate) message: String,
}

/// A step's value, or why it is unavailable.
pub(crate) type Outcome<T> = Result<T, Missing>;

impl Missing {
    /// The unavailable quantity this states.
    pub(crate) fn quantity<T>(&self) -> Quantity<T> {
        Quantity::Unavailable(self.unavailable())
    }

    /// The unavailable value this states.
    pub(crate) fn unavailable<T>(&self) -> Unavailable<T> {
        Unavailable::new(self.reason, self.entity.as_deref(), self.message.clone())
    }

    /// The same reason as a measurement failure.
    pub(crate) fn failure(&self) -> MeasureFailure {
        MeasureFailure::Unavailable {
            reason: self.reason,
            entity: self.entity.clone(),
            message: self.message.clone(),
        }
    }
}

impl From<Missing> for MeasureFailure {
    fn from(missing: Missing) -> Self {
        let Missing {
            reason,
            entity,
            message,
        } = missing;
        Self::Unavailable {
            reason,
            entity,
            message,
        }
    }
}

/// Separate a step's unavailability from a session refusal.
pub(crate) fn settle<T>(result: Measure<T>) -> Result<Outcome<T>, ResourceLimit> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(MeasureFailure::Unavailable {
            reason,
            entity,
            message,
        }) => Ok(Err(Missing {
            reason,
            entity,
            message,
        })),
        Err(MeasureFailure::Refusal(limit)) => Err(limit),
    }
}

#[cfg(test)]
mod tests {
    use super::MeasureFailure;
    use crate::reason::UnavailableReason;
    use cadmpeg_ir::eval::EvaluationFailure;

    #[test]
    fn evaluation_failures_map_to_pinned_reasons_and_keep_refusals() {
        let cases = [
            (
                EvaluationFailure::<()>::NoValue,
                UnavailableReason::EvaluationNoValue,
            ),
            (
                EvaluationFailure::NonFinite(()),
                UnavailableReason::EvaluationNonFinite,
            ),
            (
                EvaluationFailure::WorkExhausted,
                UnavailableReason::EvaluationWorkExhausted,
            ),
        ];
        for (failure, expected) in cases {
            match MeasureFailure::from(failure) {
                MeasureFailure::Unavailable { reason, .. } => assert_eq!(reason, expected),
                MeasureFailure::Refusal(limit) => panic!("unexpected refusal {limit:?}"),
            }
        }
        let limit = cadmpeg_core::decode::ResourceLimit {
            scope: cadmpeg_core::decode::LimitScope::Part,
            dimension: cadmpeg_core::decode::ResourceDimension::RecursionDepth,
            reason: cadmpeg_core::decode::ResourceFailure::BudgetExceeded,
            limit: 1,
            used: 1,
            additional: 1,
            operation: "test",
        };
        assert_eq!(
            MeasureFailure::from(EvaluationFailure::<()>::ResourceLimit(limit)),
            MeasureFailure::Refusal(limit)
        );
        assert_eq!(
            MeasureFailure::from_codec(cadmpeg_core::CodecError::ResourceLimit(limit)),
            MeasureFailure::Refusal(limit)
        );
    }
}
