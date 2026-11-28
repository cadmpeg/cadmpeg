// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{u64_from_index, ByteRange, DecodeArena, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::value_unwritable;
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition, ParameterValue};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::parameters::{DesignParameter as Legacy, ParameterValue as LegacyValue};
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::report::loss::LossNote;

use crate::container::ContainerScan;
use crate::design::decode::parameters::decode_parameter_frame;
use crate::design::decode::record::is_fatal;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design_v11::configurations::rebase::{rebased, Rebased};
use crate::design_v11::configurations::{agree, legacy_value};
use crate::ids::{native_scope_matches, native_stream, neutral_parameter_id};
use crate::native::F3dNative;
use crate::records::parameters::DesignParameter as Native;
use crate::writer::generate::design::parameters::{database_value, printed_expression};

pub(super) struct Parameters {
    changes: BTreeMap<ParameterId, Change>,
    bulk: BTreeMap<String, Vec<Replacement>>,
    meta: BTreeMap<String, Vec<u8>>,
    pub(super) own: Vec<Legacy>,
    pub(super) written: BTreeSet<ParameterId>,
}

#[derive(Default)]
struct Change {
    definition: Option<ParameterDefinition>,
    expression: Option<String>,
    value: Option<ParameterValue>,
    cache: Option<LegacyValue>,
    dependencies: Option<BTreeSet<ParameterId>>,
}

struct Field<T> {
    range: Range<usize>,
    bytes: Vec<u8>,
    value: T,
}

struct Replacement {
    id: ParameterId,
    frame: Range<usize>,
    expression: Option<Field<String>>,
    cache: Option<Field<LegacyValue>>,
}

impl Parameters {
    pub(super) fn plan(
        ctx: &DecodeContext<'_>,
        archive: &[u8],
        baseline: &DecodeResult,
        target: &CadIr,
        native: Option<&F3dNative>,
        active: BTreeMap<ParameterId, (String, Option<LegacyValue>)>,
    ) -> Result<Self, CodecError> {
        let mut changes = changes(baseline.ir(), target);
        for (id, (expression, cache)) in active {
            let change = changes.entry(id).or_default();
            if change.definition.is_none() && change.value.is_none() {
                change.expression = Some(expression);
                change.cache = cache;
                change.dependencies = None;
            }
        }
        if !changes.is_empty()
            && baseline.report().dialects().is_some_and(|layers| {
                crate::dialect::REPLAY_ROWS.contains(layers.primary().dialect())
            })
        {
            return Err(CodecError::NotImplemented(
                "a Fusion archive with nested Design documents requires synthesis to write parameter edits"
                    .into(),
            ));
        }
        let mut plan = Self {
            changes,
            bulk: BTreeMap::new(),
            meta: BTreeMap::new(),
            own: baseline.ir().model.parameters.clone(),
            written: BTreeSet::new(),
        };
        let Some(native) = native.filter(|_| !plan.changes.is_empty()) else {
            return Ok(plan);
        };

        let arena = DecodeArena::new();
        let result = ctx.with_arena(&arena, |local| {
            let root = local.register_slice(
                View::over_retained(archive),
                ByteRange {
                    start: 0,
                    end: u64_from_index(archive.len()),
                },
            )?;
            let scan = crate::container::scan_configurations(local, root)?;
            plan.locate(&scan, native)
        });
        match result {
            Ok(()) => {}
            Err(error) if is_fatal(&error) => return Err(error),
            Err(_) => {}
        }
        let mut own = plan
            .own
            .iter_mut()
            .map(|parameter| (parameter.id.clone(), parameter))
            .collect::<BTreeMap<_, _>>();
        for edit in plan.bulk.values().flatten() {
            plan.written.insert(edit.id.clone());
            if let Some(parameter) = own.get_mut(&edit.id) {
                if let Some(expression) = &edit.expression {
                    parameter.expression.clone_from(&expression.value);
                }
                if let Some(value) = &edit.cache {
                    parameter.value = Some(value.value.clone());
                }
            }
        }
        Ok(plan)
    }

    fn locate(&mut self, scan: &ContainerScan<'_>, native: &F3dNative) -> Result<(), CodecError> {
        let records = native
            .design_parameters
            .iter()
            .map(|parameter| (neutral_parameter_id(parameter), parameter))
            .collect::<BTreeMap<_, _>>();
        for entry in scan
            .entries
            .iter()
            .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
        {
            let bytes = scan.entry_bytes(&entry.name)?;
            let mut edits = self
                .changes
                .iter()
                .filter_map(|(id, change)| {
                    let record = records.get(id)?;
                    let stream = native_stream(&record.id)?;
                    if !native_scope_matches(stream, &entry.name) {
                        return None;
                    }
                    replacement(
                        bytes,
                        &entry.name,
                        record,
                        id,
                        change,
                        &records,
                        self.own.iter().find(|parameter| parameter.id == *id),
                    )
                })
                .collect::<Vec<_>>();
            if edits.is_empty() {
                continue;
            }
            edits.sort_unstable_by_key(|edit| edit.frame.start);
            if edits
                .windows(2)
                .any(|pair| pair[0].frame.end > pair[1].frame.start)
            {
                continue;
            }
            if edits.iter().any(Replacement::resizes) {
                if let Some(name) = entry
                    .name
                    .strip_suffix("BulkStream.dat")
                    .map(|prefix| format!("{prefix}MetaStream.dat"))
                    .filter(|name| scan.entries.iter().any(|entry| entry.name == *name))
                {
                    match scan.entry_bytes(&name).and_then(|meta| {
                        crate::metastream::relocate_records(meta, &name, bytes.len(), |offset| {
                            relocate(offset, &edits)
                        })
                    }) {
                        Ok(meta) => {
                            self.meta.insert(name, meta);
                        }
                        Err(error) if is_fatal(&error) => return Err(error),
                        Err(_) => edits.retain(|edit| !edit.resizes()),
                    }
                }
            }
            self.bulk.insert(entry.name.clone(), edits);
        }
        Ok(())
    }

    pub(super) fn rewrite_index(&self, name: &str, bytes: &mut Vec<u8>) {
        if let Some(meta) = self.meta.get(name) {
            bytes.clone_from(meta);
        }
    }

    pub(super) fn rewrite_bulk(&self, name: &str, bytes: &mut Vec<u8>) -> Result<(), CodecError> {
        if let Some(edits) = self.bulk.get(name) {
            for edit in edits.iter().rev() {
                edit.write(bytes, 0).ok_or_else(|| {
                    CodecError::malformed("F3D parameter patch leaves its Design stream")
                })?;
            }
        }
        Ok(())
    }

    pub(super) fn verify(
        &self,
        ctx: &DecodeContext<'_>,
        archive: &[u8],
        target: &CadIr,
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        if self.changes.is_empty() {
            return Ok(());
        }
        let decoded = match decode_retained_source(ctx, archive) {
            Ok(decoded) => Some(decoded),
            Err(error) if is_fatal(&error) => return Err(error),
            Err(_) => None,
        };
        let mut legacy = BTreeMap::new();
        let mut design = BTreeMap::new();
        if let Some(decoded) = &decoded {
            legacy.extend(decoded.ir().model.parameters.iter().map(|p| (&p.id, p)));
            design.extend(decoded.ir().design.parameters.iter().map(|p| (&p.id, p)));
        }
        let names = legacy
            .iter()
            .map(|(id, parameter)| (*id, parameter.name.as_str()))
            .collect::<BTreeMap<_, _>>();
        losses.extend(value_unwritable(
            "F3D",
            "reads these edited Design parameters with a missing or different expression or value",
            self.changes.iter().filter_map(|(id, change)| {
                let read = legacy.get(id).copied();
                let projected = design.get(id).copied();
                (!change.reads_back(read, projected, &names, &target.tolerances))
                    .then(|| id.to_string())
            }),
        ));
        Ok(())
    }
}

impl Replacement {
    fn write(&self, bytes: &mut Vec<u8>, origin: usize) -> Option<()> {
        let relative = |range: &Range<usize>| {
            Some(range.start.checked_sub(origin)?..range.end.checked_sub(origin)?)
        };
        if let Some(cache) = &self.cache {
            bytes
                .get_mut(relative(&cache.range)?)?
                .copy_from_slice(&cache.bytes);
        }
        if let Some(expression) = &self.expression {
            let range = relative(&expression.range)?;
            bytes.get(range.clone())?;
            bytes.splice(range, expression.bytes.iter().copied());
        }
        Some(())
    }

    fn resizes(&self) -> bool {
        self.expression
            .as_ref()
            .is_some_and(|field| field.bytes.len() != field.range.len())
    }
}

impl Change {
    fn reads_back(
        &self,
        read: Option<&Legacy>,
        projected: Option<&DesignParameter>,
        names: &BTreeMap<&ParameterId, &str>,
        tolerances: &cadmpeg_ir::units::Tolerances,
    ) -> bool {
        let Some(read) = read else {
            return false;
        };
        let definition = self.definition.as_ref().is_none_or(|expected| {
            projected.is_some_and(|parameter| &parameter.definition == expected)
                || (printed_expression(expected, &|id| names.get(id).copied()).as_deref()
                    == Some(read.expression.as_str())
                    && crate::writer::generate::design::parameters::reference_ids(expected)
                        == read.dependencies.iter().collect())
        });
        let expression = self
            .expression
            .as_ref()
            .is_none_or(|expected| &read.expression == expected);
        let value = self.value.as_ref().is_none_or(|expected| {
            projected.is_some_and(|parameter| {
                parameter.value.as_ref().is_some_and(|actual| {
                    let (actual_cache, expected_cache) = (
                        legacy_value(actual, &parameter.kind),
                        legacy_value(expected, &parameter.kind),
                    );
                    match (actual_cache.as_ref(), expected_cache.as_ref()) {
                        (Some(actual), Some(expected)) => agree(expected, actual, tolerances),
                        _ => actual == expected,
                    }
                })
            })
        });
        let cache = self.cache.as_ref().is_none_or(|expected| {
            read.value
                .as_ref()
                .is_some_and(|actual| agree(expected, actual, tolerances))
        });
        let dependencies = self.dependencies.as_ref().is_none_or(|expected| {
            read.dependencies.iter().eq(expected.iter())
                || (read.dependencies.iter().count() == expected.len()
                    && read.dependencies.iter().all(|id| expected.contains(id)))
        });
        definition && expression && value && cache && dependencies
    }
}

fn changes(baseline: &CadIr, target: &CadIr) -> BTreeMap<ParameterId, Change> {
    let mut changes = BTreeMap::<_, Change>::new();
    let old = baseline
        .model
        .parameters
        .iter()
        .map(|p| (&p.id, p))
        .collect::<BTreeMap<_, _>>();
    for parameter in &target.model.parameters {
        let before = old.get(&parameter.id).copied();
        let expression = before.is_none_or(|before| before.expression != parameter.expression);
        let cache = parameter
            .value
            .as_ref()
            .filter(|value| before.and_then(|before| before.value.as_ref()) != Some(*value));
        let dependencies =
            before.is_none_or(|before| before.dependencies != parameter.dependencies);
        if expression || cache.is_some() || dependencies {
            changes.insert(
                parameter.id.clone(),
                Change {
                    expression: (expression || dependencies).then(|| parameter.expression.clone()),
                    cache: cache.cloned(),
                    dependencies: dependencies
                        .then(|| parameter.dependencies.iter().cloned().collect()),
                    ..Default::default()
                },
            );
        }
    }
    let old = baseline
        .design
        .parameters
        .iter()
        .map(|p| (&p.id, p))
        .collect::<BTreeMap<_, _>>();
    for parameter in &target.design.parameters {
        let before = old.get(&parameter.id).copied();
        let definition = before.is_none_or(|before| before.definition != parameter.definition);
        let value = parameter
            .value
            .as_ref()
            .filter(|value| before.and_then(|before| before.value.as_ref()) != Some(*value));
        if definition || value.is_some() {
            let change = changes.entry(parameter.id.clone()).or_default();
            if definition {
                change.expression = None;
                change.dependencies = None;
                change.definition = Some(parameter.definition.clone());
            }
            if let Some(value) = value {
                change.value = Some(value.clone());
                change.cache = legacy_value(value, &parameter.kind);
            }
        }
    }
    changes
}

fn replacement(
    bulk: &[u8],
    stream: &str,
    record: &Native,
    id: &ParameterId,
    change: &Change,
    records: &BTreeMap<ParameterId, &Native>,
    before: Option<&Legacy>,
) -> Option<Replacement> {
    let start = usize::try_from(record.byte_offset()).ok()?;
    let end = next_indexed_record_offset(bulk, start.checked_add(11)?).unwrap_or(bulk.len());
    let frame = bulk.get(start..end)?;
    let parsed = decode_parameter_frame(frame, stream, start).ok()??;
    if parsed.id != record.id {
        return None;
    }
    let expression = match &change.definition {
        Some(definition) => {
            let scope = native_stream(&record.id)?;
            let name = |id: &ParameterId| {
                let parameter = records.get(id)?;
                (native_stream(&parameter.id) == Some(scope)).then(|| parameter.name())
            };
            printed_expression(definition, &name)
        }
        None => change.expression.clone(),
    };
    let cache = change.cache.as_ref().and_then(|value| {
        let current = before?.value.as_ref()?;
        if std::mem::discriminant(value) != std::mem::discriminant(current) {
            return None;
        }
        let stored = database_value(value)?;
        let at = usize::try_from(parsed.evaluated_value_offset()).ok()?;
        let range = at..at.checked_add(size_of::<f64>())?;
        Some(Field {
            range,
            bytes: stored.to_le_bytes().to_vec(),
            value: value.clone(),
        })
    });
    let expression = if let Some(text) = expression {
        let at = usize::try_from(parsed.expression_offset()).ok()?;
        let end = at.checked_add(parsed.expression().encode_utf16().count().checked_mul(2)?)?;
        let range = at.checked_sub(size_of::<u32>())?..end;
        let payload = crate::bytes::lp_utf16_bytes(&text).ok()?;
        Some(Field {
            range,
            bytes: payload,
            value: text,
        })
    } else {
        None
    };
    if expression.is_none() && cache.is_none() {
        return None;
    }
    let edit = Replacement {
        id: id.clone(),
        frame: start..end,
        expression,
        cache,
    };
    let mut written = frame.to_vec();
    edit.write(&mut written, start)?;
    let written = decode_parameter_frame(&written, stream, start).ok()??;
    if written.id != record.id
        || edit
            .expression
            .as_ref()
            .is_some_and(|field| written.expression() != field.value)
        || edit.cache.as_ref().is_some_and(|field| {
            database_value(&field.value) != Some(written.evaluated_value().get())
        })
    {
        return None;
    }
    Some(edit)
}

fn relocate(offset: u64, edits: &[Replacement]) -> Option<u64> {
    let original = usize::try_from(offset).ok()?;
    let mut adjusted = original;
    for field in edits
        .iter()
        .filter(|edit| edit.resizes())
        .filter_map(|edit| edit.expression.as_ref())
    {
        if original < field.range.start {
            break;
        }
        if original < field.range.end {
            return None;
        }
        adjusted = adjusted.checked_add_signed(
            isize::try_from(field.bytes.len()).ok()? - isize::try_from(field.range.len()).ok()?,
        )?;
    }
    u64::try_from(adjusted).ok()
}

pub(super) fn active_edits(target: &CadIr, baseline: &CadIr) -> Option<Rebased> {
    let selection = baseline
        .design
        .scopes
        .iter()
        .map(|scope| (&scope.definition, scope.active_configuration.as_ref()))
        .collect::<BTreeMap<_, _>>();
    if !target.design.configurations.iter().any(|row| {
        selection.get(&row.scope).copied().flatten() == Some(&row.id)
            && (row.parent.is_some() || !row.overrides.parameters.is_empty())
    }) {
        return None;
    }
    let mut retained = target.clone();
    for scope in &mut retained.design.scopes {
        scope.active_configuration = selection.get(&scope.definition).copied().flatten().cloned();
    }
    let mut rebased = rebased(&retained)?;
    for (scope, requested) in rebased
        .ir
        .design
        .scopes
        .iter_mut()
        .zip(&target.design.scopes)
    {
        scope
            .active_configuration
            .clone_from(&requested.active_configuration);
    }
    Some(rebased)
}

pub(super) fn keep_unwritten_overrides(
    rebased: &mut Rebased,
    target: &CadIr,
    written: &BTreeSet<ParameterId>,
) {
    let rows = target
        .design
        .configurations
        .iter()
        .map(|row| (&row.id, row))
        .collect::<BTreeMap<_, _>>();
    for id in rebased
        .parameters
        .keys()
        .filter(|id| !written.contains(*id))
    {
        for row in &mut rebased.ir.design.configurations {
            match rows[&row.id].overrides.parameters.get(id) {
                Some(stated) => {
                    row.overrides.parameters.insert(id.clone(), stated.clone());
                }
                None => {
                    row.overrides.parameters.remove(id);
                }
            }
        }
    }
}

pub(super) fn adopt(supported: &mut CadIr, target: &CadIr) {
    let model = target
        .model
        .parameters
        .iter()
        .map(|p| (&p.id, p))
        .collect::<BTreeMap<_, _>>();
    let known = supported
        .model
        .parameters
        .iter()
        .map(|p| p.id.clone())
        .collect::<BTreeSet<_>>();
    for parameter in &mut supported.model.parameters {
        if let Some(stated) = model.get(&parameter.id) {
            parameter.expression.clone_from(&stated.expression);
            parameter.value.clone_from(&stated.value);
            parameter.dependencies.clone_from(&stated.dependencies);
        }
    }
    supported.model.parameters.extend(
        target
            .model
            .parameters
            .iter()
            .filter(|parameter| !known.contains(&parameter.id))
            .cloned(),
    );
    let design = target
        .design
        .parameters
        .iter()
        .map(|p| (&p.id, p))
        .collect::<BTreeMap<_, _>>();
    let stale = !supported.design.scopes.eq(&target.design.scopes)
        || !supported.design.evaluations.eq(&target.design.evaluations)
        || !supported.design.sketches.eq(&target.design.sketches)
        || !supported
            .design
            .spatial_sketches
            .eq(&target.design.spatial_sketches);
    if stale {
        let mut replay = supported.clone();
        for parameter in &supported.design.parameters {
            let Some(stated) = design
                .get(&parameter.id)
                .filter(|stated| stated.definition != parameter.definition)
            else {
                continue;
            };
            if let ParameterDefinition::Expression(expression) = &stated.definition {
                if let Ok(mut editor) = replay.edit() {
                    // Only matching derived state can be adopted below.
                    drop(editor.set_parameter_expression(&parameter.id, expression.clone()));
                }
            }
        }
        if replay.design.scopes == target.design.scopes {
            supported.design.scopes = replay.design.scopes;
        }
        if replay.design.evaluations == target.design.evaluations {
            supported.design.evaluations = replay.design.evaluations;
        }
        if replay.design.sketches == target.design.sketches {
            supported.design.sketches = replay.design.sketches;
        }
        if replay.design.spatial_sketches == target.design.spatial_sketches {
            supported.design.spatial_sketches = replay.design.spatial_sketches;
        }
    }
    let known = supported
        .design
        .parameters
        .iter()
        .map(|p| p.id.clone())
        .collect::<BTreeSet<_>>();
    for parameter in &mut supported.design.parameters {
        if let Some(stated) = design.get(&parameter.id) {
            parameter.definition.clone_from(&stated.definition);
            parameter.value.clone_from(&stated.value);
        }
    }
    supported.design.parameters.extend(
        target
            .design
            .parameters
            .iter()
            .filter(|parameter| !known.contains(&parameter.id))
            .cloned(),
    );
}

#[cfg(test)]
mod tests;

fn decode_retained_source(
    ctx: &DecodeContext<'_>,
    image: &[u8],
) -> Result<DecodeResult, CodecError> {
    use cadmpeg_ir::codec::CodecBackend;
    let arena = DecodeArena::new();
    ctx.with_arena(&arena, |ctx| {
        let root = ctx.open_input(image, "F3D staged archive")?;
        let scan = crate::container::scan_for_write(ctx, root)?;
        let decoded = ctx.run_part(|| crate::decode::decode_scanned(ctx, &scan))??;
        DecodeResult::from_decoded_without_digests(
            ctx,
            decoded,
            <crate::F3dCodec as CodecBackend>::FORMAT,
            &cadmpeg_ir::report::strictness::Strictness::default(),
        )
        .map_err(|error| match error {
            cadmpeg_ir::codec::DecodeFailure::Codec(error) => error,
            other => CodecError::malformed(format!("staged archive decode refused: {other}")),
        })
    })
}
