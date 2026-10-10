// SPDX-License-Identifier: Apache-2.0
//! Sink for carrier records whose lanes the IR carrier refuses.

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
    resource_error: Option<cadmpeg_core::CodecError>,
}

/// The source record and refusal sink shared by one carrier conversion.
pub(crate) struct LaneRefusalContext<'record, 'sink> {
    pub(crate) record: &'record dyn std::fmt::Display,
    pub(crate) refusals: &'sink mut LaneRefusals,
}

/// Render retained lane refusals in their original order without a joined allocation.
pub(crate) struct JoinedLaneRecords<'a>(pub(crate) &'a [String]);

impl std::fmt::Display for JoinedLaneRecords<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, record) in self.0.iter().enumerate() {
            if index != 0 {
                formatter.write_str("; ")?;
            }
            formatter.write_str(record)?;
        }
        Ok(())
    }
}

impl<'record, 'sink> LaneRefusalContext<'record, 'sink> {
    /// Pair a source record with the sink that owns its refusal report.
    pub(crate) fn new(
        record: &'record dyn std::fmt::Display,
        refusals: &'sink mut LaneRefusals,
    ) -> Self {
        Self { record, refusals }
    }
}

impl LaneRefusals {
    /// An empty sink.
    pub(crate) fn new() -> Self {
        Self {
            records: Vec::new(),
            resource_error: None,
        }
    }

    /// Record one refusal against the record that stated it, with the reason
    /// the reader refused it.
    #[cfg(test)]
    pub(crate) fn note(&mut self, record: impl std::fmt::Display, reason: &dyn std::fmt::Display) {
        self.records.push(format!("{record}: {reason}"));
    }

    /// Admit one refusal through the caller's decode budget. A resource error
    /// stays pending until the owner drains this sink after its candidate route.
    pub(crate) fn note_checked(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        record: impl std::fmt::Display,
        reason: &dyn std::fmt::Display,
    ) {
        if self.resource_error.is_some() {
            return;
        }
        let admitted = ctx
            .format_retained(format_args!("{record}: {reason}"), "creo lane refusal text")
            .and_then(|record| {
                ctx.reserve_vec(&mut self.records, 1, "creo lane refusal records")?;
                Ok(record)
            });
        match admitted {
            Ok(record) => self.records.push(record),
            Err(error) => self.resource_error = Some(error),
        }
    }

    /// Take every refusal recorded so far, in reader order, and leave the sink
    /// empty.
    #[cfg(test)]
    pub(crate) fn take_records(&mut self) -> Vec<String> {
        std::mem::take(&mut self.records)
    }

    /// Drain admitted refusals, propagating any resource refusal from a
    /// candidate route before its ordinary loss report is constructed.
    pub(crate) fn take_records_checked(&mut self) -> Result<Vec<String>, cadmpeg_core::CodecError> {
        if let Some(error) = self.resource_error.take() {
            return Err(error);
        }
        Ok(std::mem::take(&mut self.records))
    }
}

#[cfg(test)]
mod tests {
    use super::LaneRefusals;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    #[test]
    fn refusal_records_obey_collection_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
        let mut refusals = LaneRefusals::new();
        refusals.note_checked(&ctx, "record 7", &"invalid lane");
        let error = refusals
            .take_records_checked()
            .expect_err("one record exceeds limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo lane refusal records"
        ));
    }
}
