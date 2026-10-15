// SPDX-License-Identifier: Apache-2.0
//! Sink for carrier records whose lanes the IR carrier refuses.

use cadmpeg_core::CodecError;

/// Every carrier record a reader refused, each named by the record that stated
/// it.
///
/// The IR carrier states which lanes disagree; the codec states which source
/// record stated them. The sink is owned by the reader that reports, so no
/// `return None`, `?` or `.ok()?` between a refusal and the report can drop it,
/// and N refusals in one document stay N records.
#[derive(Debug, Default)]
pub(crate) struct LaneRefusals {
    records: Vec<String>,
}

impl LaneRefusals {
    /// An empty sink.
    pub(crate) fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// Record one refusal against the record that stated it.
    pub(crate) fn note(
        &mut self,
        record: impl std::fmt::Display,
        error: &cadmpeg_ir::geometry::nurbs::NurbsError,
    ) -> Result<(), CodecError> {
        if let cadmpeg_ir::geometry::nurbs::NurbsError::ResourceLimit(limit) = error {
            return Err((*limit).into());
        }
        self.records.push(format!("{record}: {error}"));
        Ok(())
    }

    /// Take every refusal recorded so far, in reader order, and leave the sink
    /// empty.
    pub(crate) fn take_records(&mut self) -> Vec<String> {
        std::mem::take(&mut self.records)
    }
}

#[cfg(test)]
mod tests {
    use super::LaneRefusals;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::nurbs::NurbsError;

    #[test]
    fn lane_refusal_records_named_message() {
        let mut refusals = LaneRefusals::new();
        refusals
            .note("record", &NurbsError::Structure("invalid".into()))
            .expect("structural refusal is recorded");
        assert_eq!(refusals.take_records(), ["record: invalid"]);
    }

    #[test]
    fn lane_refusal_preserves_constructor_resource_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let CodecError::ResourceLimit(limit) = ctx
            .retain_bytes(1, "test constructor admission")
            .unwrap_err()
        else {
            panic!("constructor admission must refuse the retained limit");
        };
        let error = NurbsError::ResourceLimit(limit);
        let mut refusals = LaneRefusals::new();
        assert!(
            matches!(refusals.note("record", &error), Err(CodecError::ResourceLimit(actual)) if actual == limit)
        );
        assert!(refusals.take_records().is_empty());
        assert!(
            matches!(CodecError::from(error), CodecError::ResourceLimit(actual) if actual == limit)
        );
    }
}
