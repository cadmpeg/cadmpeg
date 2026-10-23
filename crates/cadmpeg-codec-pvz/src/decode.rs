// SPDX-License-Identifier: Apache-2.0
//! Package decode.
//!
//! Decode opens the package, classifies it from its structure member, and
//! records the member partition as source attributes. The structure and
//! geometry members are not decoded: a decode deeper than the container
//! charges [`PvzLossCode::PackageContentUndecoded`].

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::SourceFidelity;

use crate::loss::PvzLossCode;
use crate::package::Package;

/// Source attribute naming the structure member.
const ATTRIBUTE_STRUCTURE_MEMBER: &str = "structure_member";
/// Source attribute counting the geometry members.
const ATTRIBUTE_GEOMETRY_MEMBERS: &str = "geometry_member_count";

pub(crate) fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Decoded, CodecError> {
    let package = Package::open(ctx, root)?;
    let classification = crate::dialect::classify(package.structure(ctx)?);
    let mut losses: Vec<_> = package
        .recovery_loss()
        .into_iter()
        .chain(classification.loss())
        .collect();
    let geometry_members = package.geometry_names().len();
    if !ctx.container_only() {
        losses.push(PvzLossCode::PackageContentUndecoded.note(format!(
            "the .pvs structure member {} and {geometry_members} .ol geometry members were \
             located but not decoded; the model carries no product structure, tessellation, \
             or exact geometry",
            package.structure_name()
        )));
    }
    let attributes = BTreeMap::from([
        (
            cadmpeg_core::nonblank_const!(ATTRIBUTE_STRUCTURE_MEMBER),
            package.structure_name().to_owned(),
        ),
        (
            cadmpeg_core::nonblank_const!(ATTRIBUTE_GEOMETRY_MEMBERS),
            geometry_members.to_string(),
        ),
    ]);
    let ir = CadIr::decoded(SourceMeta::classified(
        DialectLayers::of(classification.into_matched()),
        attributes,
    ));
    Ok(Decoded {
        ir,
        body: DecodeBody {
            losses,
            ..DecodeBody::new(DecodeTransfer::at(ctx.depth(), false))
        },
        source_fidelity: SourceFidelity::default(),
    })
}
