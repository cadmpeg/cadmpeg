// SPDX-License-Identifier: Apache-2.0

use super::{base, own_definition, positive, read_definition, BaseVersion, DataType, SchemaPlan};
use crate::framing::xmt_reference::NonNullXmt;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;
use std::ops::Range;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PrimaryEnd {
    Terminated {
        terminator: Range<usize>,
        suffix: Range<usize>,
    },
    Stopped {
        offset: usize,
        reason: StopReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopReason {
    MissingTerminator,
    InvalidKind,
    InvalidDefinition,
    InvalidIdentity,
    IncompleteField,
    UnsupportedScalar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrimaryRecord {
    definition: Rc<SchemaPlan>,
    xmt: NonNullXmt,
    range: Range<usize>,
    identity_at: usize,
    variable_length: Option<u32>,
    fields: Vec<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrimaryRecords<'a> {
    source: &'a [u8],
    records: Vec<PrimaryRecord>,
    end: PrimaryEnd,
}

struct Header {
    version: BaseVersion,
    maximum: u16,
    embedded: bool,
}

fn header(view: &mut View<'_>) -> Option<Header> {
    (view.take(2)? == b"PS").then_some(())?;
    let length = usize::try_from(view.u32_be()?).ok()?;
    let description = std::str::from_utf8(view.take(length)?).ok()?;
    (super::is_partition_description(description.as_bytes())
        || description.starts_with(": TRANSMIT FILE created "))
    .then_some(())?;
    let length = usize::try_from(view.u32_be()?).ok()?;
    let schema_bytes = view.take(length)?;
    let token = cadmpeg_parasolid::find_schema_token(schema_bytes)?;
    (token.value().as_bytes() == schema_bytes).then_some(())?;
    let embedded = match token.value().split('_').count() {
        3 => false,
        4 => true,
        _ => return None,
    };
    let version = match token.value().rsplit('_').next()? {
        "11004" => BaseVersion::V11004,
        "13006" => BaseVersion::V13006,
        _ => return None,
    };
    let maximum = if embedded {
        view.u16_be()?
    } else {
        base::maximum(version)
    };
    (view.u32_be()? == 0).then_some(())?;
    Some(Header {
        version,
        maximum,
        embedded,
    })
}

impl<'a> PrimaryRecords<'a> {
    pub(crate) fn records(&self) -> &[PrimaryRecord] {
        &self.records
    }

    pub(crate) fn read(
        ctx: &DecodeContext<'_>,
        stream: &'a [u8],
    ) -> Result<Option<Self>, CodecError> {
        let mut view = View::over_retained(stream);
        let Some(header) = header(&mut view) else {
            return Ok(None);
        };
        ctx.charge_work(u64_from_index(view.position()), "read NX primary header")?;
        let mut schemas = BTreeMap::<u16, Rc<SchemaPlan>>::new();
        let mut records = Vec::new();
        let end = loop {
            let start = view.position();
            let Some(kind) = view.u16_be() else {
                break PrimaryEnd::Stopped {
                    offset: start,
                    reason: StopReason::MissingTerminator,
                };
            };
            ctx.charge_work(1, "read NX primary record")?;
            if kind == 1 {
                if positive(&mut view) != Some(0) {
                    break PrimaryEnd::Stopped {
                        offset: start,
                        reason: StopReason::InvalidIdentity,
                    };
                }
                break PrimaryEnd::Terminated {
                    terminator: start..view.position(),
                    suffix: view.position()..stream.len(),
                };
            }
            if kind <= 1 || kind > header.maximum {
                break PrimaryEnd::Stopped {
                    offset: start,
                    reason: StopReason::InvalidKind,
                };
            }
            let definition = if let Some(definition) = schemas.get(&kind) {
                Rc::clone(definition)
            } else {
                let definition = if header.embedded {
                    let Some(count) = view.u8() else {
                        break PrimaryEnd::Stopped {
                            offset: start,
                            reason: StopReason::InvalidDefinition,
                        };
                    };
                    read_definition(ctx, &mut view, header.version, kind, count)?
                } else {
                    base::fields(header.version, kind)
                        .map(|fields| own_definition(ctx, kind, fields))
                        .transpose()?
                };
                let Some(definition) = definition else {
                    break PrimaryEnd::Stopped {
                        offset: start,
                        reason: StopReason::InvalidDefinition,
                    };
                };
                ctx.charge_retained(
                    u64_from_index(std::mem::size_of::<SchemaPlan>()),
                    "NX primary schema plan",
                )?;
                let definition = Rc::new(definition);
                ctx.insert_btree_map(
                    &mut schemas,
                    kind,
                    Rc::clone(&definition),
                    "NX primary schema plans",
                )?;
                definition
            };
            let variable_length = if definition.fields.iter().any(|field| field.n_elements == 1) {
                let Some(count) = view.u32_be() else {
                    break PrimaryEnd::Stopped {
                        offset: start,
                        reason: StopReason::IncompleteField,
                    };
                };
                Some(count)
            } else {
                None
            };
            let identity_at = view.position();
            let Some(xmt) = positive(&mut view)
                .and_then(|value| value.checked_add(1))
                .and_then(|value| NonNullXmt::try_from(value).ok())
            else {
                break PrimaryEnd::Stopped {
                    offset: start,
                    reason: StopReason::InvalidIdentity,
                };
            };
            let mut fields = Vec::new();
            let mut failure = None;
            for field in &definition.fields {
                let field_start = view.position();
                if field.transmit {
                    let count = if field.n_elements == 1 {
                        variable_length.unwrap_or(0)
                    } else {
                        field.n_elements.max(1)
                    };
                    match consume_field(ctx, &mut view, field.data_type, count)? {
                        Ok(()) => (),
                        Err(reason) => {
                            failure = Some(reason);
                            break;
                        }
                    }
                }
                ctx.push_vec(
                    &mut fields,
                    field_start..view.position(),
                    "NX primary field spans",
                )?;
            }
            if let Some(reason) = failure {
                break PrimaryEnd::Stopped {
                    offset: start,
                    reason,
                };
            }
            ctx.push_vec(
                &mut records,
                PrimaryRecord {
                    definition,
                    xmt,
                    range: start..view.position(),
                    identity_at,
                    variable_length,
                    fields,
                },
                "NX primary records",
            )?;
        };
        Ok(Some(Self {
            source: stream,
            records,
            end,
        }))
    }
}

fn consume_field(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    data_type: DataType,
    count: u32,
) -> Result<Result<(), StopReason>, CodecError> {
    let Some(count) = usize::try_from(count)
        .ok()
        .filter(|count| *count <= view.remaining())
    else {
        return Ok(Err(StopReason::IncompleteField));
    };
    ctx.charge_work(u64_from_index(count), "read NX primary field")?;
    if count == 0 {
        return Ok(Ok(()));
    }
    let width = match data_type {
        DataType::Pointer => {
            for _ in 0..count {
                if positive(view).is_none() {
                    return Ok(Err(StopReason::IncompleteField));
                }
            }
            return Ok(Ok(()));
        }
        DataType::Byte | DataType::Char | DataType::Logical => 1,
        DataType::Short | DataType::Word => 2,
        DataType::Integer => 4,
        DataType::Real => 8,
        DataType::Interval => 16,
        DataType::Vector | DataType::Hvec => 24,
        DataType::Box => 48,
        DataType::Unestablished(_) => return Ok(Err(StopReason::UnsupportedScalar)),
    };
    Ok(
        if count
            .checked_mul(width)
            .and_then(|length| view.take(length))
            .is_some()
        {
            Ok(())
        } else {
            Err(StopReason::IncompleteField)
        },
    )
}

#[cfg(test)]
mod tests;

mod group;
