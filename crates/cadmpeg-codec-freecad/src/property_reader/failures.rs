// SPDX-License-Identifier: Apache-2.0
//! Projection availability comes only from witnessed source failure authorities.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{FinalizedFailureRegistry, GivenUpHome, PartState};
use cadmpeg_ir::{document::CadIr, SourceFidelity};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn failure_homes(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    fidelity: &SourceFidelity,
    registry: &FinalizedFailureRegistry,
) -> Result<BTreeMap<String, GivenUpHome>, CodecError> {
    let mut result = BTreeMap::new();
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(result);
    };
    if !fidelity
        .parts()
        .values()
        .any(|part| part.state() == &PartState::GivenUp {})
    {
        return Ok(result);
    }
    let property_records = namespace.arenas().get("properties");
    let properties: BTreeSet<_> = property_records
        .into_iter()
        .flatten()
        .map(|record| record.id())
        .collect();
    let object_records = namespace.arenas().get("objects");
    let objects: BTreeSet<_> = object_records
        .into_iter()
        .flatten()
        .map(|record| record.id())
        .collect();
    for unknown in namespace
        .arena_iter_as_for_decode::<cadmpeg_ir::unknown::NativeUnknownRecord>(ctx, "unknowns")
    {
        let unknown = unknown.map_err(CodecError::malformed)?;
        let Some(part) = fidelity
            .parts()
            .get(&unknown.id)
            .filter(|part| part.state() == &PartState::GivenUp {})
        else {
            continue;
        };
        let whole_body = matches!(
            part.level().as_str(),
            "freecad.object_data" | "freecad.properties"
        );
        let Some(home) = registry.home(unknown.id.as_str()) else {
            continue;
        };
        for link in &unknown.links {
            let id = link.as_str();
            if properties.contains(id) || id.starts_with("fcstd:native:property#") {
                insert(ctx, &mut result, id, home.clone())?;
            } else if whole_body && objects.contains(id) {
                insert(ctx, &mut result, id, home.clone())?;
            }
        }
    }
    // These identities were actually attempted by source helpers, including
    // refused headers; they are never reconstructed from source names.
    for (record, home) in registry.dropped_record_homes() {
        if properties.contains(record) || record.starts_with("fcstd:native:property#") {
            insert(ctx, &mut result, record, home)?;
        }
    }
    Ok(result)
}

fn insert(
    ctx: &DecodeContext<'_>,
    result: &mut BTreeMap<String, GivenUpHome>,
    id: &str,
    home: GivenUpHome,
) -> Result<(), CodecError> {
    if !result.contains_key(id) {
        let mut key = String::new();
        key.try_reserve_exact(id.len()).map_err(|_| {
            cadmpeg_core::decode::ResourceLimit::allocation_failed(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                u64_from_index(id.len()),
                "FreeCAD property failure home",
            )
        })?;
        key.push_str(id);
        result.insert(key, home);
    }
    Ok(())
}
