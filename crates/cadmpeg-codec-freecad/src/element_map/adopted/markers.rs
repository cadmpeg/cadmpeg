// SPDX-License-Identifier: Apache-2.0
//! A marker and its successor contribute to one canonical naming record.

use super::{compatibility_elements, known_attributes, validate_carrier, Context};
use crate::native::PropertyRecord;
use crate::xml_framing::XmlSource;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartOutput;

pub(super) enum Owner<'source> {
    Admitted(cadmpeg_core::decode::RecordOwner<'source>),
    UnknownPolicy,
    InvalidDispatch,
}

pub(super) fn prepare<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    marker: usize,
    id: &str,
    kind: &str,
    property: Option<&PropertyRecord>,
    context: &Context<'_, 'source>,
    stage: &mut PartOutput,
) -> Result<(), CodecError> {
    // The primary is exactly the reached marker part. The genuine successor
    // contributes through record_fragment, whose reads may be outside it.
    let extent = source.extent(marker)?;
    let valid_carrier = kind == "StringHasher"
        && source.validate(marker).is_ok()
        && validate_carrier(source, marker, kind, property).is_ok();
    let independent_successor = if valid_carrier {
        match source.peek_attribute(ctx, marker, "new") {
            Ok(value) => value.is_some_and(|value| {
                crate::element_map::marker_enabled(&value).is_ok_and(|flag| flag)
            }),
            Err(CodecError::Malformed(_)) => false,
            Err(error) => return Err(error),
        }
    } else {
        false
    };
    let invalid_policy = independent_successor
        && ["saveall", "threshold"].into_iter().try_fold(
            false,
            |invalid, field| -> Result<_, CodecError> {
                let invalid_field = match source.peek_attribute(ctx, marker, field) {
                    Ok(value) => value.is_some_and(|value| {
                        if field == "saveall" {
                            crate::element_map::require_bool(&value).is_err()
                        } else {
                            crate::element_map::parse_decimal(&value, "threshold").is_err()
                        }
                    }),
                    Err(CodecError::Malformed(_)) => true,
                    Err(error) => return Err(error),
                };
                Ok(invalid || invalid_field)
            },
        )?;
    if invalid_policy {
        // The successor supplies an independent table, so this failed policy
        // callback never attempts to declare that table's canonical identity.
        context
            .marker_owners
            .borrow_mut()
            .insert(id.to_owned(), Owner::UnknownPolicy);
        let metadata = known_attributes(ctx, source, marker, &["new", "saveall", "threshold"])?;
        validate_policy(&metadata)?;
        return Err(CodecError::malformed("invalid naming policy"));
    }
    let prepared = ctx.record_owner(id, extent, || {
        source.validate(marker)?;
        validate_carrier(source, marker, kind, property)?;
        let metadata = known_attributes(
            ctx,
            source,
            marker,
            if kind == "StringHasher" {
                &["new", "saveall", "threshold"]
            } else {
                &["new"]
            },
        )?;
        if metadata
            .get("new")
            .map(|value| crate::element_map::marker_enabled(value))
            .transpose()?
            != Some(true)
        {
            return Err(CodecError::malformed(
                "naming marker does not select its direct successor",
            ));
        }
        if kind == "StringHasher" {
            validate_policy(&metadata)?;
        }
        compatibility_elements(ctx, source, marker, context, stage)?;
        Ok(metadata)
    });
    match prepared {
        Ok((metadata, owner)) => {
            context
                .marker_metadata
                .borrow_mut()
                .insert(id.to_owned(), metadata);
            context
                .marker_owners
                .borrow_mut()
                .insert(id.to_owned(), Owner::Admitted(owner));
            Ok(())
        }
        Err(error) => {
            context.marker_owners.borrow_mut().insert(
                id.to_owned(),
                if independent_successor {
                    Owner::UnknownPolicy
                } else {
                    Owner::InvalidDispatch
                },
            );
            Err(error)
        }
    }
}

fn validate_policy(
    metadata: &std::collections::BTreeMap<String, String>,
) -> Result<(), CodecError> {
    metadata
        .get("saveall")
        .map(|value| crate::element_map::require_bool(value))
        .transpose()?;
    metadata
        .get("threshold")
        .map(|value| crate::element_map::parse_decimal(value, "threshold"))
        .transpose()?;
    Ok(())
}

pub(super) fn record<'source>(
    ctx: &DecodeContext<'source>,
    source: &XmlSource<'source>,
    index: usize,
    marker: usize,
    id: &str,
    context: &Context<'_, 'source>,
    decode: impl FnOnce() -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    if marker == index {
        return ctx.record(id, source.extent(index)?, decode);
    }
    let owner = context
        .marker_owners
        .borrow_mut()
        .remove(id)
        .ok_or_else(|| CodecError::malformed("naming successor has no reached marker callback"))?;
    match owner {
        Owner::Admitted(owner) => ctx.record_fragment(&owner, source.extent(index)?, decode),
        Owner::UnknownPolicy => ctx.record(id, source.extent(index)?, decode),
        Owner::InvalidDispatch => Ok(()),
    }
}
