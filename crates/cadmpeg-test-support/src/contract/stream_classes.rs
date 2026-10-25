// SPDX-License-Identifier: Apache-2.0
//! All-golden evidence for Design-only source stream defaults.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::source_fidelity::b1::classification::StreamClasses;
use cadmpeg_ir::source_fidelity::b1::provenance::NativeOwnerResolver;
use cadmpeg_ir::{ArenaClass, ArenaClasses, CadIr, SourceFidelity};

/// Check a table against every supplied golden's actual typed records.
/// The codec harness must enumerate its complete golden set. Every listed
/// stream needs records; missing layouts or unresolved classes do not prove
/// Design. Backed neutral records use their actual native record's layout;
/// unbacked records need their own. A record belongs to each stream its extent
/// or semantic reads name.
pub fn check_design_only_streams(
    ctx: &DecodeContext<'_>,
    table: StreamClasses,
    goldens: &[(&CadIr, &SourceFidelity, &dyn NativeOwnerResolver)],
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    let mut declared = BTreeSet::new();
    for stream in table.design_only() {
        if stream.is_empty() || !declared.insert(*stream) {
            return Err("stream class table has an empty or duplicate stream".into());
        }
    }
    for (ir, fidelity, resolver) in goldens {
        if ir.source.as_ref().map(|source| source.format()) != Some(table.format().as_str()) {
            return Err("golden format differs from the stream class table".into());
        }
        let mut classes = BTreeMap::<String, ArenaClasses>::new();
        let mut records = BTreeMap::<String, ArenaClasses>::new();
        let mut native_targets = BTreeSet::<String>::new();
        let mut native_records = BTreeSet::<String>::new();
        let mut neutral_records = BTreeSet::<String>::new();
        let mut unknown_targets = BTreeSet::<String>::new();
        ir.visit_neutral_provenance(&mut |record| {
            neutral_records.insert(record.id().into());
            classes
                .entry(record.id().into())
                .or_default()
                .insert(record.class());
            if let Some(native) = record.native_ref() {
                classes
                    .entry(native.into())
                    .or_default()
                    .insert(record.class());
                native_targets.insert(native.into());
            } else {
                records.insert(record.id().into(), ArenaClasses::from([record.class()]));
            }
            Ok(())
        })
        .map_err(|error| error.to_string())?;
        ir.visit_native_provenance_with_owner_resolver(ctx, *resolver, &mut |record| {
            native_records.insert(record.id().into());
            unknown_targets.extend(
                record
                    .unknown_links()
                    .iter()
                    .map(|id| id.as_str().to_owned()),
            );
            let mut own = classes.get(record.id()).cloned().unwrap_or_default();
            for id in record
                .owner()
                .into_iter()
                .chain(record.unknown_links().iter().map(|id| id.as_str()))
            {
                if let Some(source) = classes.get(id) {
                    own.extend(source.iter().copied());
                }
            }
            records.insert(record.id().into(), own);
            Ok(())
        })
        .map_err(|error| error.to_string())?;
        for target in native_targets {
            if !native_records.contains(&target) {
                return Err(format!(
                    "neutral source reference {target} has no native record"
                ));
            }
        }
        for target in unknown_targets {
            if !neutral_records.contains(&target) && !native_records.contains(&target) {
                return Err(format!("unknown source reference {target} has no record"));
            }
        }
        for (id, classes) in records {
            let layout = fidelity
                .record_layout()
                .get(&id)
                .ok_or_else(|| format!("golden record {id} has no source layout"))?;
            for stream in &declared {
                if layout.stream() == *stream || layout.reads().contains_key(*stream) {
                    if classes != ArenaClasses::from([ArenaClass::Design]) {
                        return Err(format!(
                            "stream {stream} record {id} is not exclusively Design"
                        ));
                    }
                    seen.insert(*stream);
                }
            }
        }
    }
    for stream in declared {
        if !seen.contains(stream) {
            return Err(format!("stream {stream} has no decoded golden record"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
