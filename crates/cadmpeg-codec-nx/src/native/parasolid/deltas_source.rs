// SPDX-License-Identifier: Apache-2.0

use super::deltas_instance::{ParasolidDeltasInstance, ParasolidDeltasSchema};
use super::deltas_metadata::ParasolidDeltasMetadata;
use super::{
    deltas_events_for_stream, ParasolidDeltasEvents, ParasolidDeltasPartitionPrefix,
    ParasolidDeltasRecord, ParasolidDeltasResidualSpan, ParasolidDeltasTransmitHeader,
};
use crate::parasolid::{classify, ParasolidSubtype, StreamBody};
use crate::records::deltas::{DeltasEnd, DeltasRecords};
use crate::RETAINED_PARASOLID_STREAM_ID_PREFIX;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::native::{NativeConvertError, NativeNamespace, NativeRecord};
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::SourceFidelity;
use serde::Serialize;
use std::collections::BTreeMap;

type Records<'a> = BTreeMap<&'a str, &'a NativeRecord>;

#[derive(Default)]
struct Claimed<'a> {
    transmit_headers: Records<'a>,
    partition_prefixes: Records<'a>,
    schemas: Records<'a>,
    instances: Records<'a>,
    records: Records<'a>,
    residual_spans: Records<'a>,
    metadata: Records<'a>,
}

pub(in crate::native) fn verify(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
    source: Option<&SourceFidelity>,
) -> Result<Vec<Finding>, NativeConvertError> {
    let mut claims = BTreeMap::<u32, Claimed<'_>>::new();
    macro_rules! claim {
        ($field:ident, $record:ty) => {
            let arena = concat!("parasolid_deltas_", stringify!($field));
            let records = namespace.arenas().get(arena).into_iter().flatten();
            for (raw, parsed) in
                records.zip(namespace.arena_iter_as_for_decode::<$record>(ctx, arena))
            {
                let ordinal = parsed?.stream_ordinal;
                let claimed = claims.entry(ordinal).or_default();
                if claimed.$field.insert(raw.id(), raw).is_some() {
                    return Err(NativeConvertError::InvalidCollection(format!(
                        "duplicate NX Deltas record {} in {arena}",
                        raw.id()
                    )));
                }
            }
        };
    }
    claim!(transmit_headers, ParasolidDeltasTransmitHeader);
    claim!(partition_prefixes, ParasolidDeltasPartitionPrefix);
    claim!(schemas, ParasolidDeltasSchema);
    claim!(instances, ParasolidDeltasInstance);
    claim!(records, ParasolidDeltasRecord);
    claim!(residual_spans, ParasolidDeltasResidualSpan);
    claim!(metadata, ParasolidDeltasMetadata);
    if claims.is_empty() {
        return Ok(Vec::new());
    }

    if let Some(source) = source {
        for (id, retained) in source.retained_records() {
            let Some(suffix) = id
                .as_str()
                .strip_prefix(RETAINED_PARASOLID_STREAM_ID_PREFIX)
            else {
                continue;
            };
            if suffix.is_empty()
                || !suffix.bytes().all(|byte| byte.is_ascii_digit())
                || (suffix.len() > 1 && suffix.starts_with('0'))
            {
                continue;
            }
            let Ok(ordinal) = suffix.parse::<u32>() else {
                continue;
            };
            let Some(bytes) = retained.data() else {
                continue;
            };
            if matches!(
                classify(bytes),
                StreamBody::Parasolid {
                    subtype: ParasolidSubtype::Deltas,
                    ..
                }
            ) {
                claims.entry(ordinal).or_default();
            }
        }
    }

    let mut findings = Vec::new();
    for (ordinal, claimed) in claims {
        let id = format!("{RETAINED_PARASOLID_STREAM_ID_PREFIX}{ordinal}");
        let bytes = match source {
            None => {
                push_info(
                    &mut findings,
                    id,
                    format_args!(
                        "NX Deltas stream has no source sidecar; structural admission only"
                    ),
                );
                continue;
            }
            Some(source) => {
                let Some(record) = source.retained_record(&id) else {
                    return Err(NativeConvertError::InvalidCollection(format!(
                        "NX Deltas source sidecar is missing retained stream {id}"
                    )));
                };
                let Some(bytes) = record.data() else {
                    push_info(&mut findings, id, format_args!("NX Deltas retained stream contains only a digest; structural admission only"));
                    continue;
                };
                bytes
            }
        };
        // Retained data is inflated. Its container offset is a different coordinate space.
        let owner = DeltasRecords::read(bytes)?;
        let mut derived = ParasolidDeltasEvents::default();
        deltas_events_for_stream(&owner, ordinal, &mut derived)?;
        let ParasolidDeltasEvents {
            transmit_headers,
            partition_prefixes,
            schemas,
            instances,
            records,
            residual_spans,
            metadata,
        } = derived;
        let Claimed {
            transmit_headers: claimed_transmit_headers,
            partition_prefixes: claimed_partition_prefixes,
            schemas: claimed_schemas,
            instances: claimed_instances,
            records: claimed_records,
            residual_spans: claimed_residual_spans,
            metadata: claimed_metadata,
        } = claimed;
        compare_arena(
            ctx,
            &id,
            "parasolid_deltas_transmit_headers",
            &claimed_transmit_headers,
            transmit_headers
                .iter()
                .map(|record| (record.id.as_str(), record)),
        )?;
        compare_arena(
            ctx,
            &id,
            "parasolid_deltas_partition_prefixes",
            &claimed_partition_prefixes,
            partition_prefixes
                .iter()
                .map(|record| (record.id.as_str(), record)),
        )?;
        compare_arena(
            ctx,
            &id,
            "parasolid_deltas_schemas",
            &claimed_schemas,
            schemas.iter().map(|record| (record.id.as_str(), record)),
        )?;
        compare_arena(
            ctx,
            &id,
            "parasolid_deltas_instances",
            &claimed_instances,
            instances.iter().map(|record| (record.id.as_str(), record)),
        )?;
        compare_arena(
            ctx,
            &id,
            "parasolid_deltas_records",
            &claimed_records,
            records.iter().map(|record| (record.id.as_str(), record)),
        )?;
        compare_arena(
            ctx,
            &id,
            "parasolid_deltas_residual_spans",
            &claimed_residual_spans,
            residual_spans
                .iter()
                .map(|record| (record.id.as_str(), record)),
        )?;
        compare_arena(
            ctx,
            &id,
            "parasolid_deltas_metadata",
            &claimed_metadata,
            metadata.iter().map(|record| (record.id.as_str(), record)),
        )?;
        if let DeltasEnd::Stopped { offset, reason } = owner.end() {
            push_info(&mut findings, id, format_args!("NX Deltas source agrees with native records; only the bounded prefix was parsed, stopping at inflated offset {offset} ({reason:?}); the residual suffix was compared by range and digest"));
        }
    }
    Ok(findings)
}

fn compare_arena<'a, T: Serialize + 'a>(
    ctx: &DecodeContext<'_>,
    stream: &str,
    arena: &str,
    claimed: &Records<'_>,
    derived: impl ExactSizeIterator<Item = (&'a str, &'a T)>,
) -> Result<(), NativeConvertError> {
    if claimed.len() != derived.len() {
        return Err(NativeConvertError::InvalidCollection(format!(
            "NX Deltas stream {stream} {arena} record set differs from retained source: {} claimed, {} derived",
            claimed.len(),
            derived.len()
        )));
    }
    for (id, expected) in derived {
        let matches = match claimed.get(id) {
            Some(actual) => actual.matches_typed_for_decode(ctx, expected)?,
            None => false,
        };
        if !matches {
            return Err(NativeConvertError::InvalidCollection(format!(
                "NX Deltas stream {stream} record {id} in {arena} differs from retained source or is missing from CADIR"
            )));
        }
    }
    Ok(())
}

fn push_info(findings: &mut Vec<Finding>, id: String, message: std::fmt::Arguments<'_>) {
    findings.push(Finding {
        related: Vec::new(),
        check: Check::NativeLinks,
        severity: Severity::Info,
        message: format!("{id}: {message}"),
        entity: Some(id),
    });
}
