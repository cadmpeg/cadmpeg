// SPDX-License-Identifier: Apache-2.0
//! Package decode.
//!
//! Decode opens the package, classifies it from its structure member, and
//! decodes the structure member into product definitions and the occurrence
//! tree. Each geometry member a definition names is then decoded once into
//! one body that every definition naming the member owns. The body holds the
//! neutral B-rep of the member's exact-geometry stream (§8) when that stream
//! decodes, and the member's tessellations and appearances.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{ArenaClass, DecodeContext, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, Annotations, StreamHandle};
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, EntityRewrite, SourceMeta, Topology};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::{BodyId, Identity};
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{DecodeParts, GivenUpPart, Part};
use cadmpeg_ir::SourceFidelity;

use crate::loss::PvzLossCode;
use crate::ol::PackageResidue;
use crate::package::Package;
use crate::pvs::Structure;

/// Source attribute naming the structure member.
const ATTRIBUTE_STRUCTURE_MEMBER: &str = "structure_member";
/// Most occurrences one package decodes to.
pub(crate) const MAX_OCCURRENCES: usize = 100_000;

pub(crate) fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Decoded, CodecError> {
    let package = Package::open(ctx, root)?;
    let structure_member = package.structure(ctx)?;
    let classification = crate::dialect::classify(structure_member);
    let mut losses: Vec<_> = package
        .recovery_loss()
        .into_iter()
        .chain(classification.loss())
        .collect();
    let attributes = BTreeMap::from([(
        cadmpeg_core::nonblank_const!(ATTRIBUTE_STRUCTURE_MEMBER),
        package.structure_name().to_owned(),
    )]);
    let mut ir = CadIr::decoded(SourceMeta::classified(
        DialectLayers::of(classification.into_matched()),
        attributes,
    ));
    let mut geometry_transferred = false;
    let mut annotations = Annotations::default();
    let mut given_up = Vec::new();
    if !ctx.container_only() {
        match ctx.run_part(|| crate::pvs::read(ctx, package.structure_name(), structure_member))? {
            Ok(mut structure) => {
                given_up.append(&mut structure.given_up);
                geometry_transferred = decode_model(
                    ctx,
                    &package,
                    &structure,
                    &mut ir,
                    &mut annotations,
                    &mut losses,
                    &mut given_up,
                )?;
            }
            Err(error) => {
                let part = Part::new(
                    "pvz",
                    "archive-entry",
                    package.stored(package.structure_name())?,
                    [ArenaClass::Model],
                )
                .named(package.structure_name())
                .costing(format!(
                    "the package carries no product structure, and none of its {} .ol \
                     geometry members was placed",
                    package.geometry_names().len()
                ));
                given_up.push(ctx.give_up_part(part, error)?);
            }
        }
    }
    let mut source_fidelity =
        SourceFidelity::with_annotations(annotations, Sha256Digest::digest(root.window()));
    for part in given_up {
        losses.push(part.commit(ctx, &mut ir, &mut source_fidelity)?);
    }
    Ok(Decoded {
        ir,
        body: DecodeBody {
            losses,
            ..DecodeBody::new(DecodeTransfer::at(ctx.depth(), geometry_transferred))
        },
        source_fidelity,
    })
}

/// Gives up the geometry member `name`, keeping its stored bytes, linked to
/// the first product definition that names it.
fn give_up_member(
    ctx: &DecodeContext<'_>,
    package: &Package<'_>,
    name: &str,
    owner: Option<Identity>,
    error: CodecError,
) -> Result<GivenUpPart, CodecError> {
    let mut part = Part::new(
        "pvz",
        "archive-entry",
        package.stored(name)?,
        [ArenaClass::Model],
    )
    .named(name);
    if let Some(owner) = owner {
        part = part.contained_in(owner);
    }
    ctx.give_up_part(part, error)
}

/// One expanded geometry member and its name in the package.
#[derive(Clone, Copy)]
struct MemberView<'n, 'm> {
    name: &'n str,
    view: View<'m>,
}

/// Decodes the product structure and the geometry members it names.
/// Returns whether any geometry was transferred.
fn decode_model(
    ctx: &DecodeContext<'_>,
    package: &Package<'_>,
    structure: &Structure,
    ir: &mut CadIr,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
    given_up: &mut Vec<GivenUpPart>,
) -> Result<bool, CodecError> {
    losses.extend(structure.residue.losses());
    crate::model::definitions(ctx, structure, ir, losses)?;
    crate::model::occurrences(ctx, structure, MAX_OCCURRENCES, ir, losses)?;

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
        let decoded = ctx.run_part(|| {
            let member = package.member(ctx, name)?;
            crate::ol::decode(ctx, name, member).map(|geometry| (member, geometry))
        })?;
        let owner = owners
            .get(name)
            .and_then(|indices| indices.first())
            .and_then(|index| ir.model.product_definitions.get(*index))
            .map(|definition| Identity::from(definition.id.clone()));
        let (member, mut geometry) = match decoded {
            Ok(decoded) => decoded,
            Err(error) => {
                given_up.push(give_up_member(ctx, package, name, owner, error)?);
                continue;
            }
        };
        for part in geometry.given_up.drain(..) {
            given_up.push(match &owner {
                Some(owner) => part.contained_in(owner.clone()),
                None => part,
            });
        }
        residue.absorb(name, &geometry.residue);
        let exact = if geometry.has_exact {
            let member = MemberView { name, view: member };
            exact_geometry(
                ctx,
                member,
                owner.as_ref(),
                ir,
                annotations,
                losses,
                given_up,
            )?
        } else {
            Vec::new()
        };
        let meshes = ir.model.tessellations.len();
        transferred |= !exact.is_empty();
        let definition = match owners.get(name).map(Vec::as_slice) {
            Some([index]) => ir
                .model
                .product_definitions
                .get(*index)
                .map(|definition| definition.id.clone()),
            _ => None,
        };
        crate::model::display_geometry(
            ctx,
            name,
            &geometry,
            exact.first().cloned(),
            definition,
            ir,
            losses,
        )?;
        transferred |= ir.model.tessellations.len() > meshes;
        for index in owners.get(name).into_iter().flatten() {
            if let Some(definition) = ir.model.product_definitions.get_mut(*index) {
                definition.bodies.extend(exact.iter().cloned());
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
    ir.finalize();
    Ok(transferred)
}

/// Writes the neutral B-rep of the exact-geometry stream (§8) of geometry
/// member `member` into `ir` and returns its bodies, the solid body first; a
/// stream without a bounded face has none. A stored stream that does not
/// decode is given up as a part, linked to `owner`, and adds nothing to
/// `ir`; the member keeps its tessellation.
fn exact_geometry(
    ctx: &DecodeContext<'_>,
    member: MemberView<'_, '_>,
    owner: Option<&Identity>,
    ir: &mut CadIr,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
    given_up: &mut Vec<GivenUpPart>,
) -> Result<Vec<BodyId>, CodecError> {
    let name = member.name;
    let mut streams = crate::ol::stored_exact_streams(ctx, member.view)?;
    if streams.len() > 1 {
        losses.push(PvzLossCode::GeometryExactUndecoded.note(format!(
            "geometry member {name:?}: the member carries {} exact-geometry streams, and the \
             decode takes none of them as its B-rep; the member keeps its tessellation",
            streams.len()
        )));
        for stream in streams {
            let mut part = crate::ol::stream_part(name, member.view, stream.stored);
            if let Some(owner) = owner {
                part = part.contained_in(owner.clone());
            }
            given_up.push(ctx.give_up_part(
                part,
                CodecError::malformed(
                    "the geometry member carries more than one exact-geometry stream",
                ),
            )?);
        }
        return Ok(Vec::new());
    }
    let Some(stored) = streams.pop() else {
        return Ok(Vec::new());
    };
    let stream_id = stored.stream_id;
    let mut geometry = Topology::default();
    let mut builder = AnnotationBuilder::new();
    let mut notes = Vec::new();
    let stream = StreamHandle::new(
        cadmpeg_ir::stream_name!("pvz:").with_suffix(format_args!("{name}#{stream_id:#010x}")),
    );
    let emitted = ctx.run_part(|| {
        let payload = crate::gzip::expand(ctx, stored.stored)?;
        let stream_model = crate::brep::parse(ctx, stream_id, payload)?;
        crate::brep::emit(
            ctx,
            &stream_model,
            &crate::ids::member_scope(name),
            &mut crate::brep::emit::Sink {
                geometry: &mut geometry,
                annotations: &mut builder,
                losses: &mut notes,
                stream: &stream,
                member: name,
            },
        )
    })?;
    match emitted {
        Ok(bodies) => {
            ir.geometry.extend_rewritten(geometry, &mut Unchanged)?;
            annotations.append(builder.build())?;
            losses.extend(notes);
            Ok(bodies)
        }
        Err(error) => {
            losses.push(PvzLossCode::GeometryExactUndecoded.note(format!(
                "geometry member {name:?}: exact-geometry stream {stream_id:#010x} did not \
                 decode into B-rep ({error}); the member keeps its tessellation"
            )));
            let mut part = crate::ol::stream_part(name, member.view, stored.stored);
            if let Some(owner) = owner {
                part = part.contained_in(owner.clone());
            }
            given_up.push(ctx.give_up_part(part, error)?);
            Ok(Vec::new())
        }
    }
}

/// Moves the records of one stream's model into the package model as they
/// are: their identities are already scoped by geometry member.
struct Unchanged;

impl EntityRewrite for Unchanged {
    type Error = CodecError;

    fn rewrite<T: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities>(
        &mut self,
        entity: T,
    ) -> Result<T, CodecError> {
        Ok(entity)
    }
}

/// The names, quoted and comma-separated.
fn quoted(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}
