// SPDX-License-Identifier: Apache-2.0
//! Sink for carrier records whose lanes the IR carrier refuses.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

struct ChargedMessage<'a, 'ctx> {
    ctx: &'a DecodeContext<'ctx>,
    operation: &'static str,
    text: String,
    failure: Option<CodecError>,
}

impl std::fmt::Write for ChargedMessage<'_, '_> {
    fn write_str(&mut self, fragment: &str) -> std::fmt::Result {
        if let Err(error) = self.ctx.reserve_retained_string(
            &mut self.text,
            fragment.len(),
            self.operation,
        ) {
            self.failure = Some(error);
            return Err(std::fmt::Error);
        }
        self.text.push_str(fragment);
        Ok(())
    }
}

pub(crate) fn format_retained(
    ctx: &DecodeContext<'_>,
    message: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut charged = ChargedMessage {
        ctx,
        operation,
        text: String::new(),
        failure: None,
    };
    if std::fmt::write(&mut charged, message).is_err() {
        return Err(charged.failure.unwrap_or_else(|| {
            CodecError::malformed("cannot format SLDPRT lane refusal")
        }));
    }
    Ok(charged.text)
}

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
        ctx: &DecodeContext<'_>,
        record: impl std::fmt::Display,
        error: &cadmpeg_ir::geometry::nurbs::NurbsError,
    ) -> Result<(), CodecError> {
        let message = format_retained(
            ctx,
            format_args!("{record}: {error}"),
            "record SLDPRT lane refusal",
        )?;
        ctx.reserve_collection_vec(&mut self.records, 1, "collect SLDPRT lane refusals")?;
        self.records.push(message);
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
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::nurbs::NurbsError;

    #[test]
    fn lane_refusal_message_refuses_retained_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 5;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits policy");
        let mut refusals = LaneRefusals::new();
        let Err(CodecError::ResourceLimit(limit)) = refusals.note(
            &ctx, "record", &NurbsError::Structure("invalid".into()),
        ) else { panic!("refusal message must use retained budget") };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert!(refusals.take_records().is_empty());
    }

    #[test]
    fn lane_refusal_record_refuses_collection_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits policy");
        let mut refusals = LaneRefusals::new();
        let Err(CodecError::ResourceLimit(limit)) = refusals.note(
            &ctx, "record", &NurbsError::Structure("invalid".into()),
        ) else { panic!("refusal record must use collection budget") };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        assert!(refusals.take_records().is_empty());
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty root fits service policy");
        refusals.note(&ctx, "record", &NurbsError::Structure("invalid".into()))
            .expect("service budget");
        assert_eq!(refusals.take_records(), ["record: invalid"]);
    }
}
