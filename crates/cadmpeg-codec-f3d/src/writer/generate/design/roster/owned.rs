// SPDX-License-Identifier: Apache-2.0
//! The records of a parameter a timeline item owns (f3d.md §3.1, "Owned
//! parameters", "Sketch dimensions" and §3.1's parameter record forms): the
//! 104-byte scalar owner with its paired part, the parameter in the
//! discriminated or the wide discriminated form, and the owner's 58-byte
//! parameter companion.

use cadmpeg_core::CodecError;

use super::super::base_feature::timestamp_table;
use super::super::frames::DesignRecords;
use super::super::parameters::OwnedParameter;
use super::super::types;
use super::sketches::OWNER_COLLECTION;
use super::{Context, PARAMETER_COLLECTION};
use crate::layout::design_parameter_wide_expression_trailer as wide_trailer;

/// The family discriminator of an owned parameter (f3d-open-items.md
/// DR-911, DR-917).
pub(super) const DISCRIMINATOR: u32 = 6;

/// The form of an owned parameter's record (f3d.md §3.1, the parameter
/// record forms).
#[derive(Clone, Copy)]
pub(super) enum Form {
    /// The discriminated form, whose expression trailer is four zero bytes
    /// and five flag bytes, each `0`.
    Flagged,
    /// The discriminated form, whose expression trailer is ten zero bytes.
    Owned,
    /// The wide discriminated form with no comment, whose wide expression
    /// trailer stores the driven flag `1`: the parameter of a driven
    /// dimension.
    Driven,
}

/// The wide expression trailer of the parameter of a driven dimension.
const DRIVEN_TRAILER: [u8; wide_trailer::LEN] = {
    let mut trailer = [0; wide_trailer::LEN];
    trailer[wide_trailer::DRIVEN] = 1;
    trailer
};

/// One owned parameter's records and where its owner sits.
pub(super) struct Owned<'a> {
    pub(super) owner: u32,
    pub(super) parameter: u32,
    pub(super) companion: u32,
    /// The owning item's scope.
    pub(super) scope: u32,
    /// The owner's position in the scope's owners, from `0`.
    pub(super) local: u32,
    /// The parameter's source kind.
    pub(super) kind: &'a str,
    pub(super) form: Form,
    pub(super) value: &'a OwnedParameter,
}

/// The owner, the parameter and the companion of `owned`.
pub(super) fn write(
    ctx: &Context<'_>,
    out: &mut DesignRecords,
    owned: &Owned<'_>,
) -> Result<(), CodecError> {
    let &Owned {
        owner,
        parameter,
        companion,
        scope,
        local,
        kind,
        form,
        value,
    } = owned;
    out.record(&ctx.class(types::PARAMETER_OWNER.guid)?, owner)
        .zeros(8)
        .byte(1)
        .u32(1)
        .reference(scope)
        .u32(local)
        .byte(0)
        .f64(value.value)
        .reference(parameter)
        .u64(u64::from(value.ordinal))
        .reference(scope)
        .raw(&[1, 0, 0])
        .reference(companion)
        .byte(0)
        .reference(scope)
        .header(&ctx.class(types::DATA_MODEL_OBJECT.guid)?, owner)
        .zeros(8)
        .reference(OWNER_COLLECTION);
    out.record(&ctx.class(types::PARAMETER.guid)?, parameter);
    match form {
        Form::Flagged | Form::Owned => {
            out.zeros(11).u64(u64::from(DISCRIMINATOR)).byte(0);
        }
        Form::Driven => {
            out.zeros(15).u32(DISCRIMINATOR).byte(0).u32(0);
        }
    }
    out.u32(value.ordinal)
        .reference(owner)
        .lp_utf16(&value.expression)?;
    match form {
        Form::Flagged => out.zeros(9),
        Form::Owned => out.zeros(10),
        Form::Driven => out.raw(&DRIVEN_TRAILER),
    };
    out.lp_utf16(kind)?
        .u32(0)
        .lp_utf16(value.unit.name())?
        .lp_utf16(&value.name)?
        .f64(value.value)
        .byte(0)
        .reference(PARAMETER_COLLECTION);
    timestamp_table(
        out,
        &ctx.class(types::RECORD_TIMESTAMPS.guid)?,
        companion,
        false,
        &[],
        (owner, ctx.values.stamps.scope),
    )
}
