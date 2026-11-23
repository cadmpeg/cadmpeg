// SPDX-License-Identifier: Apache-2.0
//! Contain errors at a bounded Design record.

use crate::loss::F3dLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

pub(in crate::design) fn recover<T>(
    result: Result<T, CodecError>,
    losses: &mut Vec<LossNote>,
    code: F3dLossCode,
    identity: impl std::fmt::Display,
) -> Result<Option<T>, CodecError> {
    match result {
        Ok(record) => Ok(Some(record)),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => {
            losses.push(code.note(format!(
                "{identity} remains in the source archive because its decoding failed: {error}"
            )));
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::recover;
    use crate::loss::F3dLossCode;
    use cadmpeg_core::decode::{LimitScope, ResourceDimension, ResourceFailure, ResourceLimit};
    use cadmpeg_core::CodecError;

    #[test]
    fn record_recovery_preserves_resource_refusals() {
        let limit = ResourceLimit {
            scope: LimitScope::for_dimension(ResourceDimension::RecursionDepth),
            dimension: ResourceDimension::RecursionDepth,
            reason: ResourceFailure::BudgetExceeded,
            limit: 1,
            used: 1,
            additional: 1,
            operation: "synthetic record recursion",
        };
        let mut losses = Vec::new();
        let result = recover::<()>(
            Err(CodecError::ResourceLimit(limit)),
            &mut losses,
            F3dLossCode::SketchPointUndecoded,
            "Sketch point record 42",
        );
        assert!(matches!(result, Err(CodecError::ResourceLimit(_))));
        assert!(losses.is_empty());
    }
}
