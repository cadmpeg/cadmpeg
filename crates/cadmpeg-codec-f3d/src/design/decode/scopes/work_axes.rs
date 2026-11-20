use super::point_data::POINT_DATA_TYPE_GUID;
use crate::bytes::reference_fields::{read_reference_fields, ReadField};
use crate::design::decode::sketch::{next_indexed_record_offset, IndexedRecordOffsets};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_axes::{
    AxisCarrierPrefix, AxisCarrierProperty, AxisCarrierPropertyValue, AxisEndpoint,
    AxisEndpointReading, AxisReference, AxisReferenceRun, DesignWorkAxisCandidate,
    DesignWorkAxisConstruction, DesignWorkAxisSource,
};
use crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID;
use cadmpeg_core::decode::{u64_from_index, ArenaClass, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::unknown::part::{DecodeParts, GivenUpPart, Part};
use std::collections::{HashMap, HashSet};

mod legacy;
#[cfg(test)]
mod tests;

struct Failure {
    start: usize,
    end: usize,
    error: CodecError,
}

struct Reader<'a> {
    source: View<'a>,
    view: View<'a>,
    failures: Vec<Failure>,
}

impl<'a> Reader<'a> {
    fn new(source: View<'a>, start: usize, end: usize) -> Result<Self, CodecError> {
        let view = source
            .child(
                source
                    .start()
                    .checked_add(start)
                    .ok_or_else(|| CodecError::malformed("axis frame offset overflow"))?,
                source
                    .start()
                    .checked_add(end)
                    .ok_or_else(|| CodecError::malformed("axis frame end overflow"))?,
            )
            .ok_or_else(|| CodecError::malformed("axis frame outside its Design stream"))?;
        Ok(Self {
            source,
            view,
            failures: Vec::new(),
        })
    }

    fn fail(&mut self, start: usize, end: usize, error: CodecError) {
        if let Some(previous) = self
            .failures
            .last_mut()
            .filter(|previous| previous.end >= start)
        {
            previous.end = previous.end.max(end);
        } else {
            self.failures.push(Failure { start, end, error });
        }
    }

    fn tail(&mut self, start: usize, message: &'static str) {
        self.fail(start, self.view.end(), CodecError::malformed(message));
        self.view.seek_to_end();
    }

    fn read<T>(
        &mut self,
        read: impl FnOnce(&mut View<'a>) -> Result<T, cadmpeg_core::decode::ParseError>,
    ) -> ReadField<T> {
        let start = self.view.position();
        match read(&mut self.view) {
            Ok(value) => ReadField::Value(value),
            Err(error) => {
                self.fail(start, self.view.end(), error.into());
                self.view.seek_to_end();
                ReadField::Unread
            }
        }
    }

    fn real(&mut self) -> ReadField<FiniteReal> {
        let start = self.view.position();
        match self.read(View::req_f64_le) {
            ReadField::Value(value) => match FiniteReal::try_from(value) {
                Ok(value) => ReadField::Value(value),
                Err(_) => {
                    self.fail(
                        start,
                        self.view.position(),
                        CodecError::malformed("nonfinite WorkAxis scalar"),
                    );
                    ReadField::Unread
                }
            },
            _ => ReadField::Unread,
        }
    }

    fn reals<const N: usize>(&mut self) -> [ReadField<FiniteReal>; N] {
        std::array::from_fn(|_| self.real())
    }

    fn ascii(&mut self, maximum: usize) -> ReadField<String> {
        let start = self.view.position();
        let ReadField::Value(length) = self.read(View::req_u32_le) else {
            return ReadField::Unread;
        };
        let Some(length) = self
            .view
            .counted(u64::from(length), 1)
            .map(cadmpeg_core::decode::BoundedCount::get)
            .filter(|length| *length <= maximum)
        else {
            self.tail(start, "WorkAxis text length exceeds its frame");
            return ReadField::Unread;
        };
        match self.view.req_take(length) {
            Ok(raw) if raw.iter().all(u8::is_ascii_graphic) => {
                ReadField::Value(String::from_utf8_lossy(raw).into_owned())
            }
            Ok(_) => {
                self.tail(start, "WorkAxis text contains invalid ASCII");
                ReadField::Unread
            }
            Err(error) => {
                self.fail(start, self.view.end(), error.into());
                self.view.seek_to_end();
                ReadField::Unread
            }
        }
    }

    fn prefix(
        &mut self,
        ctx: &DecodeContext<'_>,
        prefix: &mut AxisCarrierPrefix,
        record_index: u32,
    ) -> Result<bool, CodecError> {
        self.read(|view| view.req_take(7));
        if self.read(View::req_u32_le) != ReadField::Value(record_index) {
            self.tail(
                self.view.position(),
                "WorkAxis carrier header identity disagrees",
            );
            return Ok(false);
        }
        prefix.header_word = self.read(View::req_u32_le);
        prefix.name = self.ascii(256);
        if prefix.name.value().is_none() {
            return Ok(false);
        }
        self.prologue(ctx, prefix)
    }

    fn prologue(
        &mut self,
        ctx: &DecodeContext<'_>,
        prefix: &mut AxisCarrierPrefix,
    ) -> Result<bool, CodecError> {
        let leading_at = self.view.position();
        prefix.leading_block = self.read(View::req_u8);
        match prefix.leading_block {
            ReadField::Value(0) => {}
            ReadField::Value(_) => self.fail(
                leading_at,
                self.view.end(),
                CodecError::malformed("WorkAxis carrier leading block layout is assumed"),
            ),
            ReadField::Absent | ReadField::Unread => return Ok(false),
        }
        let presence_at = self.view.position();
        prefix.property_presence = self.read(View::req_u8);
        match prefix.property_presence {
            ReadField::Value(0) => {
                prefix.property_count = ReadField::Absent;
                return Ok(true);
            }
            ReadField::Value(1) => {}
            _ => {
                self.tail(presence_at, "invalid WorkAxis property presence");
                return Ok(false);
            }
        }
        let count_at = self.view.position();
        prefix.property_count = self.read(View::req_u32_le);
        let Some(declared) = prefix.property_count.value().copied() else {
            return Ok(false);
        };
        let Some(count) = self
            .view
            .counted(
                u64::from(declared).min(u64_from_index(self.view.remaining().div_ceil(8))),
                1,
            )
            .map(cadmpeg_core::decode::BoundedCount::get)
        else {
            self.tail(count_at, "WorkAxis property count exceeds its frame");
            return Ok(false);
        };
        ctx.retain_bytes(
            u64_from_index(
                count
                    .checked_mul(std::mem::size_of::<AxisCarrierProperty>())
                    .ok_or_else(|| {
                        CodecError::malformed("WorkAxis property allocation overflow")
                    })?,
            ),
            "WorkAxis property fields",
        )?;
        prefix.properties = ctx.alloc_filled(
            count,
            AxisCarrierProperty::default(),
            "WorkAxis property fields",
        )?;
        for index in 0..count {
            if self.view.remaining() == 0 {
                prefix.properties.truncate(index);
                self.tail(
                    count_at,
                    "WorkAxis property count exceeds its readable prefix",
                );
                return Ok(false);
            }
            prefix.properties[index].key = self.ascii(self.view.remaining());
            if prefix.properties[index].key.value().is_none() {
                prefix.properties.truncate(index + 1);
                return Ok(false);
            }
            prefix.properties[index].intrinsic_type = self.ascii(64);
            let value_at = self.view.position();
            let value = match prefix.properties[index]
                .intrinsic_type
                .value()
                .map(String::as_str)
            {
                Some("IntrinsicMetaTypeuint64") => match self.read(View::req_u64_le) {
                    ReadField::Value(value) => {
                        ReadField::Value(AxisCarrierPropertyValue::Unsigned(value))
                    }
                    _ => ReadField::Unread,
                },
                Some("IntrinsicMetaTypedouble") => match self.real() {
                    ReadField::Value(value) => {
                        ReadField::Value(AxisCarrierPropertyValue::Real(value))
                    }
                    _ => ReadField::Unread,
                },
                _ => {
                    prefix.properties.truncate(index + 1);
                    self.tail(value_at, "WorkAxis property type has no admitted layout");
                    return Ok(false);
                }
            };
            prefix.properties[index].value = value;
            if self.view.remaining() == 0 && !self.failures.is_empty() {
                prefix.properties.truncate(index + 1);
                return Ok(false);
            }
        }
        if u64_from_index(count) != u64::from(declared) {
            self.tail(
                count_at,
                "WorkAxis property count exceeds its readable prefix",
            );
            return Ok(false);
        }
        Ok(true)
    }

    fn references(&mut self, ctx: &DecodeContext<'_>) -> Result<AxisReferenceRun, CodecError> {
        let at = self.view.position();
        let mut run = AxisReferenceRun {
            declared: self.read(View::req_u32_le),
            ..Default::default()
        };
        let Some(count) = run
            .declared
            .value()
            .filter(|count| **count != 0)
            .and_then(|count| {
                self.view.counted(
                    u64::from(*count).min(u64_from_index(self.view.remaining())),
                    1,
                )
            })
            .map(cadmpeg_core::decode::BoundedCount::get)
        else {
            self.tail(
                at,
                "WorkAxis input count is zero, truncated or exceeds its frame",
            );
            return Ok(run);
        };
        ctx.retain_bytes(
            u64_from_index(
                count
                    .checked_mul(std::mem::size_of::<AxisReference>())
                    .ok_or_else(|| {
                        CodecError::malformed("WorkAxis reference allocation overflow")
                    })?,
            ),
            "WorkAxis reference fields",
        )?;
        run.references =
            ctx.alloc_filled(count, AxisReference::default(), "WorkAxis reference fields")?;
        for index in 0..count {
            if self.view.remaining() == 0 {
                run.references.truncate(index);
                self.tail(at, "WorkAxis input count exceeds its readable prefix");
                return Ok(run);
            }
            let start = self.view.position();
            let relative = start - self.source.start();
            let end = self.view.end() - self.source.start();
            let read = read_reference_fields(&self.source.window()[..end], relative);
            let complete = read.fields.complete;
            run.references[index] = read.fields.into_owned();
            if !complete {
                run.references.truncate(index + 1);
                self.tail(start, "unreadable WorkAxis input reference");
                return Ok(run);
            }
            self.view
                .seek(self.source.start() + read.end)
                .ok_or_else(|| {
                    CodecError::malformed("WorkAxis reference cursor escaped its frame")
                })?;
        }
        if run.declared
            != ReadField::Value(u32::try_from(run.references.len()).map_err(|_| {
                CodecError::malformed("WorkAxis retained reference count exceeds u32")
            })?)
        {
            self.tail(at, "WorkAxis input count exceeds its readable prefix");
            return Ok(run);
        }
        Ok(run)
    }

    fn close(&mut self) -> bool {
        let position = self.view.position();
        if position != self.view.end() {
            let at = position - self.source.start();
            if next_indexed_record_offset(self.source.window(), at)
                != Some(self.view.end() - self.source.start())
            {
                self.tail(position, "WorkAxis carrier has an unreadable class tail");
            }
        }
        self.failures.is_empty()
    }

    fn commit(
        self,
        ctx: &DecodeContext<'_>,
        scope: &DesignParameterScope,
        stream: &str,
        given_up: &mut Vec<GivenUpPart>,
    ) -> Result<(), CodecError> {
        for failure in self.failures {
            let extent = self
                .source
                .child(failure.start, failure.end)
                .ok_or_else(|| {
                    CodecError::malformed("WorkAxis unreadable part escaped its source space")
                })?;
            let part = Part::at(
                "f3d",
                "work-axis-member",
                u64_from_index(extent.start() - self.source.start()),
                extent.window(),
                [ArenaClass::Design],
            )
            .in_stream(
                stream
                    .to_owned()
                    .try_into()
                    .map_err(CodecError::malformed)?,
            )
            .contained_in(scope.id.clone().try_into().map_err(CodecError::malformed)?);
            given_up.push(ctx.give_up_part(part, failure.error)?);
        }
        Ok(())
    }
}

fn point_direction(
    ctx: &DecodeContext<'_>,
    reader: &mut Reader<'_>,
    candidate: &mut DesignWorkAxisCandidate,
    version: u32,
) -> Result<(), CodecError> {
    if !reader.prefix(
        ctx,
        &mut candidate.prefix,
        candidate
            .record_index
            .ok_or_else(|| CodecError::malformed("carrier record identity missing"))?,
    )? {
        return Ok(());
    }
    let bounding_box_index = reader.read(View::req_u32_le);
    candidate.origin = reader.reals();
    candidate.displacement = reader.reals();
    let point_parameters = reader.reals();
    let reference_type = reader.read(View::req_u32_le);
    let (tangent_prefix, tangent) = if version == 4 {
        (reader.read(View::req_u8), reader.reals())
    } else {
        (ReadField::Absent, [ReadField::Absent; 3])
    };
    candidate.source = Some(DesignWorkAxisSource::PointDirection {
        bounding_box_index,
        point_parameters,
        reference_type,
        tangent_prefix,
        tangent,
        inputs: AxisReferenceRun::default(),
    });
    let inputs = reader.references(ctx)?;
    if let Some(DesignWorkAxisSource::PointDirection { inputs: slot, .. }) = &mut candidate.source {
        *slot = inputs;
    }
    candidate.complete = reader.close();
    Ok(())
}

fn point_data(
    ctx: &DecodeContext<'_>,
    reader: &mut Reader<'_>,
    reading: &mut AxisEndpointReading,
    record_index: u32,
    version: u32,
) -> Result<(), CodecError> {
    if !reader.prefix(ctx, &mut reading.prefix, record_index)? {
        return Ok(());
    }
    reading.bounding_box_index = if version >= 2 {
        reader.read(View::req_u32_le)
    } else {
        ReadField::Absent
    };
    reading.parameters = reader.reals();
    reading.pick_point_parameter = if version >= 1 {
        let start = reader.view.position();
        let end = reader.view.end() - reader.source.start();
        let read = read_reference_fields(
            &reader.source.window()[..end],
            start - reader.source.start(),
        );
        let complete = read.fields.complete;
        let field = ReadField::Value(read.fields.into_owned());
        if !complete {
            reading.pick_point_parameter = field;
            reader.tail(start, "unreadable WorkAxis endpoint pick parameter");
            return Ok(());
        }
        reader
            .view
            .seek(reader.source.start() + read.end)
            .ok_or_else(|| CodecError::malformed("endpoint reference escaped its frame"))?;
        field
    } else {
        ReadField::Absent
    };
    reading.position = reader.reals();
    reading.reference_type = reader.read(View::req_u32_le);
    reading.tangent = if version >= 3 {
        reader.reals()
    } else {
        [ReadField::Absent; 3]
    };
    reading.inputs = reader.references(ctx)?;
    reading.complete = reading.inputs.complete() && reader.failures.is_empty();
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) struct WorkAxisRegistrations<'a> {
    pub(super) entities: &'a HashMap<u64, (&'a str, u32)>,
    pub(super) classes: &'a HashMap<u32, (&'a str, u32)>,
}

fn registered_frame(
    source: View<'_>,
    start: usize,
    guid: &str,
    version: Option<u32>,
    registrations: WorkAxisRegistrations<'_>,
) -> bool {
    registered_version(source, start, guid, registrations)
        .is_some_and(|actual| version.is_none_or(|expected| expected == actual))
}

fn registered_version(
    source: View<'_>,
    start: usize,
    guid: &str,
    registrations: WorkAxisRegistrations<'_>,
) -> Option<u32> {
    super::super::sketch::indexed_record_header_at(source.window(), start)
        .and_then(|header| registrations.classes.get(&header.class_tag.code()))
        .filter(|(actual, _)| actual.eq_ignore_ascii_case(guid))
        .map(|(_, revision)| *revision)
}

fn scope_member_matches(
    source: View<'_>,
    scope: &DesignParameterScope,
    ordinal: usize,
    target: u32,
    start: usize,
    registrations: WorkAxisRegistrations<'_>,
) -> bool {
    let scope_header = usize::try_from(scope.byte_offset())
        .ok()
        .and_then(|at| super::super::sketch::indexed_record_header_at(source.window(), at));
    if scope_header.is_none_or(|header| header.record_index != scope.record_index) {
        return true;
    }
    let Some(row) = scope
        .reference_members()
        .located_rows()
        .and_then(|rows| rows.get(ordinal))
    else {
        return false;
    };
    let Some(at) = usize::try_from(row.offset)
        .ok()
        .and_then(|at| at.checked_sub(1))
    else {
        return false;
    };
    let fields = read_reference_fields(source.window(), at).fields;
    let Some((actual, inline)) = fields.local() else {
        return false;
    };
    actual == u64::from(target)
        && inline.is_none_or(|guid| {
            super::super::sketch::indexed_record_header_at(source.window(), start)
                .and_then(|header| registrations.classes.get(&header.class_tag.code()))
                .is_some_and(|(actual, _)| actual.eq_ignore_ascii_case(guid))
        })
}

pub(super) fn decode_work_axis(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream: &str,
    registrations: WorkAxisRegistrations<'_>,
    given_up: &mut Vec<GivenUpPart>,
) -> Result<DesignWorkAxisConstruction, CodecError> {
    let mut evidence = DesignWorkAxisConstruction::default();
    let scope_present = usize::try_from(scope.byte_offset())
        .ok()
        .and_then(|at| super::super::sketch::indexed_record_header_at(source.window(), at))
        .is_some_and(|header| header.record_index == scope.record_index);
    if let Some(references) = scope
        .reference_members()
        .located_rows()
        .filter(|_| scope_present)
    {
        for reference in references {
            let fields = usize::try_from(reference.offset)
                .ok()
                .and_then(|at| at.checked_sub(1))
                .map(|at| {
                    read_reference_fields(source.window(), at)
                        .fields
                        .into_owned()
                })
                .unwrap_or_default();
            evidence.scope_references.push(fields);
        }
    }
    if let Some(candidate) =
        legacy::decode(ctx, source, records, scope, stream, registrations, given_up)?
    {
        evidence.candidates.push(candidate);
    }
    let mut seen = evidence
        .candidates
        .iter()
        .filter_map(|candidate| candidate.record_index)
        .collect::<HashSet<_>>();
    for (ordinal, record_index) in scope.reference_members().values().enumerate() {
        let registered = registrations
            .entities
            .get(&u64::from(*record_index))
            .copied();
        if registered.is_some_and(|(guid, _)| !guid.eq_ignore_ascii_case(POINT_DIRECTION_TYPE_GUID))
        {
            continue;
        }
        let reference = evidence
            .scope_references
            .get(ordinal)
            .and_then(AxisReference::local);
        if scope_present
            && reference.is_none_or(|(target, inline)| {
                target != u64::from(*record_index)
                    || inline
                        .is_some_and(|guid| !guid.eq_ignore_ascii_case(POINT_DIRECTION_TYPE_GUID))
            })
        {
            continue;
        }
        if !seen.insert(*record_index) {
            continue;
        }
        let first_candidate = evidence.candidates.len();
        for (start, end) in records.frames(*record_index) {
            let Some(version) =
                registered_version(source, start, POINT_DIRECTION_TYPE_GUID, registrations)
            else {
                continue;
            };
            if registered.is_some_and(|(_, expected)| expected != version) {
                continue;
            }
            let mut candidate = DesignWorkAxisCandidate {
                record_index: Some(*record_index),
                version: ReadField::Value(version),
                ..Default::default()
            };
            if registered.is_none() || !matches!(version, 1 | 4) {
                evidence.candidates.push(candidate);
                continue;
            }
            let mut reader = Reader::new(source, start, end)?;
            if let Err(error) =
                ctx.run_part(|| point_direction(ctx, &mut reader, &mut candidate, version))?
            {
                reader.fail(reader.view.position(), reader.view.end(), error);
            }
            reader.commit(ctx, scope, stream, given_up)?;
            if let Some(DesignWorkAxisSource::PointDirection { inputs, .. }) = &candidate.source {
                let mut endpoint_targets = HashSet::new();
                for input in &inputs.references {
                    let Some((target, inline)) = input.local() else {
                        continue;
                    };
                    let Ok(target) = u32::try_from(target) else {
                        continue;
                    };
                    let registered = registrations.entities.get(&u64::from(target)).copied();
                    if registered
                        .is_some_and(|(guid, _)| !guid.eq_ignore_ascii_case(POINT_DATA_TYPE_GUID))
                        || inline
                            .is_some_and(|guid| !guid.eq_ignore_ascii_case(POINT_DATA_TYPE_GUID))
                    {
                        continue;
                    }
                    if !endpoint_targets.insert(target) {
                        continue;
                    }
                    let mut endpoint = AxisEndpoint {
                        record_index: target,
                        unique_frame: records
                            .frames(target)
                            .filter(|(start, _)| {
                                registered_frame(
                                    source,
                                    *start,
                                    POINT_DATA_TYPE_GUID,
                                    registered.map(|(_, version)| version),
                                    registrations,
                                )
                            })
                            .count()
                            == 1,
                        ..Default::default()
                    };
                    for (point_start, point_end) in records.frames(target) {
                        let Some(version) = registered_version(
                            source,
                            point_start,
                            POINT_DATA_TYPE_GUID,
                            registrations,
                        ) else {
                            continue;
                        };
                        if registered.is_some_and(|(_, expected)| expected != version) {
                            continue;
                        }
                        if version > 3 {
                            endpoint.readings.push(AxisEndpointReading {
                                version: ReadField::Value(version),
                                ..Default::default()
                            });
                            continue;
                        }
                        let mut reader = Reader::new(source, point_start, point_end)?;
                        let mut reading = AxisEndpointReading {
                            version: ReadField::Value(version),
                            ..Default::default()
                        };
                        if let Err(error) = ctx.run_part(|| {
                            point_data(ctx, &mut reader, &mut reading, target, version)
                        })? {
                            reader.fail(reader.view.position(), reader.view.end(), error);
                        }
                        reader.commit(ctx, scope, stream, given_up)?;
                        endpoint.readings.push(reading);
                    }
                    candidate.endpoints.push(endpoint);
                }
            }
            evidence.candidates.push(candidate);
        }
        if evidence.candidates.len() == first_candidate
            && (registered.is_some() || reference.is_some_and(|(_, inline)| inline.is_some()))
        {
            evidence.candidates.push(DesignWorkAxisCandidate {
                record_index: Some(*record_index),
                version: registered
                    .map_or(ReadField::Unread, |(_, version)| ReadField::Value(version)),
                ..Default::default()
            });
        }
    }
    Ok(evidence)
}
