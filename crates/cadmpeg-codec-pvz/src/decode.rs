// SPDX-License-Identifier: Apache-2.0
//! Package decode.
//!
//! Decode opens the package, classifies it from its structure member, and
//! decodes the structure member into product definitions and the occurrence
//! tree. Each geometry member a definition names is then decoded once: its
//! display geometry becomes one body with its tessellations and appearances,
//! and every definition that names the member owns that body.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::SourceFidelity;

use crate::loss::PvzLossCode;
use crate::ol::PackageResidue;
use crate::package::Package;
use crate::pvs::Structure;

/// Source attribute naming the structure member.
const ATTRIBUTE_STRUCTURE_MEMBER: &str = "structure_member";
/// Source attribute counting the geometry members.
const ATTRIBUTE_GEOMETRY_MEMBERS: &str = "geometry_member_count";

pub(crate) fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Decoded, CodecError> {
    let package = Package::open(ctx, root)?;
    let structure_member = package.structure(ctx)?;
    let classification = crate::dialect::classify(structure_member);
    let mut losses: Vec<_> = package
        .recovery_loss()
        .into_iter()
        .chain(classification.loss())
        .collect();
    let geometry_members = package.geometry_names().len();
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
    let mut ir = CadIr::decoded(SourceMeta::classified(
        DialectLayers::of(classification.into_matched()),
        attributes,
    ));
    let mut geometry_transferred = false;
    if !ctx.container_only() {
        match crate::pvs::read(ctx, structure_member) {
            Ok(structure) => {
                geometry_transferred =
                    decode_model(ctx, &package, &structure, &mut ir, &mut losses)?;
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => losses.push(PvzLossCode::StructureMemberMalformed.note(format!(
                "the .pvs structure member {} did not decode ({error}); the package carries no \
                 product structure, and none of its {geometry_members} .ol geometry members \
                 was placed",
                package.structure_name()
            ))),
        }
    }
    Ok(Decoded {
        ir,
        body: DecodeBody {
            losses,
            ..DecodeBody::new(DecodeTransfer::at(ctx.depth(), geometry_transferred))
        },
        source_fidelity: SourceFidelity::default(),
    })
}

/// Decodes the product structure and the geometry members it names.
/// Returns whether any geometry was transferred.
fn decode_model(
    ctx: &DecodeContext<'_>,
    package: &Package<'_>,
    structure: &Structure,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<bool, CodecError> {
    losses.extend(structure.residue.losses());
    crate::model::definitions(structure, ir, losses);
    crate::model::occurrences(ctx, structure, ir, losses)?;

    let mut owners: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut order = Vec::new();
    for (index, definition) in structure.definitions.iter().enumerate() {
        if let Some(member) = definition.geometry.as_deref() {
            let owners = owners.entry(member).or_default();
            if owners.is_empty() {
                order.push(member);
            }
            owners.push(index);
        }
    }
    let mut residue = PackageResidue::default();
    let mut missing = Vec::new();
    let mut transferred = false;
    for name in order {
        if !package.contains(name) {
            missing.push(name);
            continue;
        }
        let member = package.member(ctx, name)?;
        let geometry = match crate::ol::decode(ctx, member) {
            Ok(geometry) => geometry,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                losses.push(
                    PvzLossCode::GeometryMemberMalformed
                        .note(format!("geometry member {name:?} did not decode: {error}")),
                );
                continue;
            }
        };
        residue.absorb(name, &geometry.residue);
        let meshes = ir.model.tessellations.len();
        let body = crate::model::display_geometry(ctx, name, &geometry, ir, losses)?;
        transferred |= ir.model.tessellations.len() > meshes;
        if let Some(body) = body {
            attach_exact_geometry(ctx, name, member, losses)?;
            for index in owners.get(name).into_iter().flatten() {
                if let Some(definition) = ir.model.product_definitions.get_mut(*index) {
                    definition.bodies.push(body.clone());
                }
            }
        }
    }
    losses.extend(residue.losses());
    if !missing.is_empty() {
        losses.push(PvzLossCode::GeometryMemberMissing.note(format!(
            "geometry elements name {} members that the package does not hold: {}",
            missing.len(),
            quoted(&missing)
        )));
    }
    let unreferenced: Vec<&str> = package
        .geometry_names()
        .iter()
        .map(String::as_str)
        .filter(|name| !owners.contains_key(name))
        .collect();
    if !unreferenced.is_empty() {
        losses.push(PvzLossCode::GeometryMemberUnreferenced.note(format!(
            "{} .ol members are named by no definition and were not decoded: {}",
            unreferenced.len(),
            quoted(&unreferenced)
        )));
    }
    Ok(transferred)
}

/// Attaches the exact B-rep geometry (§8) of geometry member `name` to the
/// member's body.
fn attach_exact_geometry(
    ctx: &DecodeContext<'_>,
    name: &str,
    member: View<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let exact = match crate::ol::exact_stream(ctx, member) {
        Ok(exact) => exact,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => {
            losses.push(PvzLossCode::GeometryStreamUndecoded.note(format!(
                "geometry member {name:?}: the exact-geometry stream was not expanded: {error}"
            )));
            return Ok(());
        }
    };
    if let Some(exact) = exact {
        losses.push(PvzLossCode::GeometryExactUndecoded.note(format!(
            "geometry member {name:?}: exact-geometry stream {:#010x} ({} expanded bytes) was \
             not decoded into B-rep topology",
            exact.stream_id,
            exact.payload.window().len()
        )));
    }
    Ok(())
}

/// The names, quoted and comma-separated.
fn quoted(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}
