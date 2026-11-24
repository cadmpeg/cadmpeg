// SPDX-License-Identifier: Apache-2.0
//! Decode of an Alibre assembly: its inclusions as occurrences of component
//! files, each component decoded from the file the assembly names.
//!
//! An inclusion names a component by a path without extension. The decode
//! reads the file name of that path from the directory of the assembly, as
//! the companion source resolves it, and decodes a part or a subassembly
//! recursively. A component that is not available stays an external
//! definition, with the loss that says so.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{CompanionLoad, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::products::{
    AssemblyJoint, ExternalDocument, JointConnector, JointId, JointOperand, Occurrence,
    ProductDefinition, ProductDefinitionKind, Storage,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;

use crate::assembly::{self, Assembly, Inclusion, Placement};
use crate::container::{AlibreContainer, DocumentKind, STEP_STREAM};
use crate::dialect::classify_assembly;
use crate::loss::AlibreLossCode;
use crate::merge::merge_component;
use crate::part21;

/// Deepest nesting of subassemblies that one expansion reads. A subassembly
/// below it stays an external definition.
const MAX_NESTING: usize = 8;
/// Most occurrences that one expansion places, subassemblies included. Past it
/// the expansion stops.
const MAX_OCCURRENCES: usize = 10_000;
/// Extensions of the component files, in the order the decode tries them.
const EXTENSIONS: [&str; 4] = ["AD_PRT", "AD_ASM", "ad_prt", "ad_asm"];

/// The state that one expansion shares across its nesting levels.
#[derive(Debug, Default)]
pub(crate) struct Expansion {
    /// The identities of the files being decoded, outermost first.
    stack: Vec<String>,
    /// The occurrences placed so far.
    occurrences: usize,
}

/// The decode of one assembly while its components are resolved.
struct Build {
    ir: CadIr,
    fidelity: cadmpeg_ir::SourceFidelity,
    losses: Vec<LossNote>,
    geometry_transferred: bool,
    /// The definitions of the resolved components, by lower-case file stem.
    definitions: BTreeMap<String, ProductDefinitionId>,
    /// The next scope number.
    scopes: u32,
    /// The next external definition number.
    externals: u32,
    /// The occurrences of each file stem so far.
    ordinals: BTreeMap<String, u32>,
}

fn next(counter: &mut u32) -> Result<u32, CodecError> {
    let current = *counter;
    *counter = current
        .checked_add(1)
        .ok_or_else(|| CodecError::malformed("Alibre assembly counter overflows"))?;
    Ok(current)
}

/// The file name of a stored path: the text after its last separator.
fn stem(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Decodes an assembly. `anchor` identifies the file the assembly was read
/// from, so that its components resolve beside it.
pub(crate) fn decode_assembly<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
    container: &AlibreContainer<'a>,
    anchor: Option<&str>,
    expansion: &mut Expansion,
) -> Result<Decoded, CodecError> {
    let digest = Sha256Digest::digest(root.window());
    let mut losses = container.deviation_losses();
    let classification = classify_assembly();
    if ctx.container_only() {
        let ir = CadIr::decoded(SourceMeta::classified(
            classification.layers()?,
            cadmpeg_core::text::named_entries("the alibre assembly", BTreeMap::new())?,
        ));
        return Ok(Decoded {
            ir,
            body: DecodeBody {
                losses,
                ..DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {})
            },
            source_fidelity: cadmpeg_ir::SourceFidelity::new(digest),
        });
    }
    let step = container.stream(ctx, STEP_STREAM)?;
    let part = part21::parse(ctx, step.window())?;
    if !part.is_alibre() {
        return Err(CodecError::malformed(
            "the STEP_DATA stream of the assembly does not name the ALIBRE_SCHEMA",
        ));
    }
    let model = assembly::read(&part)?;
    let mut attributes = BTreeMap::new();
    if let Some(version) = &model.product_version {
        attributes.insert("product_version".to_owned(), version.clone());
    }
    let ir = CadIr::decoded(SourceMeta::classified(
        classification.layers()?,
        cadmpeg_core::text::named_entries("the alibre assembly", attributes)?,
    ));
    losses.extend(container.undecoded_assembly_streams_loss());
    losses.extend(model_losses(&model));
    let mut build = Build {
        ir,
        fidelity: cadmpeg_ir::SourceFidelity::new(digest),
        losses,
        geometry_transferred: false,
        definitions: BTreeMap::new(),
        scopes: 0,
        externals: 0,
        ordinals: BTreeMap::new(),
    };
    let mut owned = Vec::new();
    let mut stopped = 0_usize;
    for inclusion in &model.inclusions {
        if expansion.occurrences >= MAX_OCCURRENCES {
            stopped += 1;
            continue;
        }
        expansion.occurrences += 1;
        ctx.charge_entities(1, "place Alibre occurrence")?;
        let prototype = component(ctx, anchor, inclusion, &mut build, expansion)?;
        owned.push(place(&mut build, &model, inclusion, prototype)?);
    }
    if stopped > 0 {
        build.losses.push(AlibreLossCode::AssemblyExpansionTruncated.note(format!(
            "{stopped} inclusion(s) lie past the limit of {MAX_OCCURRENCES} occurrences and were left out"
        )));
    }
    let name = model.name.clone();
    if let Some(definition) = build.ir.state_root_definition(
        ctx,
        ProductDefinitionId::compose(
            &cadmpeg_ir::identity_namespace!("alibre", "model", "product-definition"),
            cadmpeg_ir::identity_key!("root"),
        ),
        ExternalDocument::default(),
    )? {
        definition.kind = Some(ProductDefinitionKind::Assembly);
        definition.source_name = name;
        definition.occurrences = owned;
    }
    let body = DecodeBody {
        losses: build.losses,
        ..DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::full(
            build.geometry_transferred,
        ))
    };
    Ok(Decoded {
        ir: build.ir,
        body,
        source_fidelity: build.fidelity,
    })
}

/// The losses of the assembly records that the decode does not map.
fn model_losses(model: &Assembly) -> Vec<LossNote> {
    let mut losses = Vec::new();
    if !model.unread.is_empty() {
        let mut names = model
            .unread
            .iter()
            .map(|(name, count)| format!("{name} x{count}"))
            .collect::<Vec<_>>();
        names.truncate(16);
        losses.push(AlibreLossCode::AssemblyEntitiesUnread.note(format!(
            "the feature model holds entities that the assembly reader does not know: {}",
            names.join(", ")
        )));
    }
    if model.relation_references > 0 {
        losses.push(AlibreLossCode::AssemblyEntitiesUnread.note(format!(
            "the assembly entity lists {} relation reference(s) that the reader does not map; \
             assembly constraints are not read",
            model.relation_references
        )));
    }
    let named = model
        .inclusions
        .iter()
        .filter(|inclusion| inclusion.configuration.is_some())
        .count();
    let others = model.configurations.iter().skip(1).count();
    if named > 0 || others > 0 {
        losses.push(AlibreLossCode::AssemblyConfigurationUnmapped.note(format!(
            "{named} occurrence(s) name a configuration of their component, and {others} assembly \
             configuration(s) beyond the active one are not read; the occurrences carry the state \
             of the active configuration only"
        )));
    }
    for inclusion in &model.inclusions {
        if !inclusion.unmapped.is_empty() {
            losses.push(AlibreLossCode::AssemblyInclusionUnmapped.note(format!(
                "inclusion #{} states attributes {:?} that the occurrence does not carry",
                inclusion.id, inclusion.unmapped
            )));
        }
        let hidden = inclusion
            .members
            .iter()
            .filter(|member| member.nested)
            .filter(|member| {
                member.hidden
                    || model
                        .active_configuration()
                        .is_some_and(|configuration| configuration.hidden.contains(&member.id))
            })
            .count();
        if hidden > 0 {
            losses.push(AlibreLossCode::AssemblyNestedStateUnread.note(format!(
                "inclusion #{} hides {hidden} component(s) of its subassembly; the occurrence carries no per-instance state of nested occurrences",
                inclusion.id
            )));
        }
    }
    losses
}

/// A component file the companion source opened.
struct Opened<'a> {
    id: String,
    name: String,
    view: View<'a>,
}

fn open_component<'a>(
    ctx: &DecodeContext<'a>,
    anchor: Option<&str>,
    stem: &str,
) -> Result<Result<Opened<'a>, String>, CodecError> {
    let mut reason = String::new();
    for extension in EXTENSIONS {
        let name = format!("{stem}.{extension}");
        match ctx.open_companion(anchor, &name)? {
            CompanionLoad::Loaded { id, view } => return Ok(Ok(Opened { id, name, view })),
            CompanionLoad::Unavailable(why) => reason = why,
        }
    }
    Ok(Err(reason))
}

/// The component an inclusion places: its definition in the build, decoded and
/// merged when the file is available and external when it is not.
fn component(
    ctx: &DecodeContext<'_>,
    anchor: Option<&str>,
    inclusion: &Inclusion,
    build: &mut Build,
    expansion: &mut Expansion,
) -> Result<ProductDefinitionId, CodecError> {
    let file = stem(&inclusion.path);
    let key = file.to_ascii_lowercase();
    if let Some(definition) = build.definitions.get(&key) {
        return Ok(definition.clone());
    }
    let mut reference = ExternalDocument::path(&inclusion.path);
    reference.document_id =
        cadmpeg_core::text::NonBlankString::new(inclusion.document_id.clone().unwrap_or_default());
    let definition = match load(ctx, anchor, file, expansion, &mut build.losses)? {
        Loaded::Decoded { opened, decoded } => {
            let scope = format!("m{}", next(&mut build.scopes)?);
            let mut document = reference.clone();
            document.path = cadmpeg_core::text::NonBlankString::new(opened.name.clone());
            let merged = merge_component(
                ctx,
                &mut build.ir,
                &mut build.fidelity,
                *decoded,
                &scope,
                document,
            )?;
            build.geometry_transferred |= merged.geometry_transferred;
            build
                .losses
                .extend(merged.losses.into_iter().map(|mut loss| {
                    loss.message = format!("component {file}: {}", loss.message);
                    // The kernel layer belongs to the component, not to the
                    // assembly, whose own layers hold no kernel.
                    if loss.code == AlibreLossCode::SourceDialectUnverified.kind() {
                        loss.code = AlibreLossCode::AssemblyMemberDialectUnverified.kind();
                    }
                    loss
                }));
            if merged.design_left_out {
                build
                    .losses
                    .push(AlibreLossCode::AssemblyMemberDesignUnmerged.note(format!(
                    "component {file}: design records of the file are not merged into the assembly"
                )));
            }
            merged.definition
        }
        Loaded::Absent => {
            ctx.charge_entities(1, "place Alibre external component")?;
            let id = ProductDefinitionId::compose(
                &cadmpeg_ir::identity_namespace!("alibre", "model", "product-definition"),
                cadmpeg_ir::identity_key!("external").dash(next(&mut build.externals)?),
            );
            build
                .ir
                .model
                .product_definitions
                .push(ProductDefinition::new(
                    id.clone(),
                    Storage::External(reference),
                ));
            id
        }
    };
    build.definitions.insert(key, definition.clone());
    Ok(definition)
}

/// The result of reading the component file of one inclusion.
enum Loaded<'a> {
    Decoded {
        opened: Opened<'a>,
        decoded: Box<Decoded>,
    },
    Absent,
}

fn load<'a>(
    ctx: &DecodeContext<'a>,
    anchor: Option<&str>,
    file: &str,
    expansion: &mut Expansion,
    losses: &mut Vec<LossNote>,
) -> Result<Loaded<'a>, CodecError> {
    if expansion.stack.len() >= MAX_NESTING {
        losses.push(AlibreLossCode::AssemblyExpansionTruncated.note(format!(
            "component {file} lies past the nesting limit of {MAX_NESTING} files and stays external"
        )));
        return Ok(Loaded::Absent);
    }
    let opened = match open_component(ctx, anchor, file)? {
        Ok(opened) => opened,
        Err(reason) => {
            losses.push(
                AlibreLossCode::AssemblyMemberUnavailable
                    .note(format!("component {file} is not available: {reason}")),
            );
            return Ok(Loaded::Absent);
        }
    };
    if expansion.stack.contains(&opened.id) {
        losses.push(AlibreLossCode::AssemblyMemberCycle.note(format!(
            "component {file} is a file that is already being decoded; the occurrence stays external"
        )));
        return Ok(Loaded::Absent);
    }
    let _depth = ctx.enter_nested("decode Alibre component")?;
    expansion.stack.push(opened.id.clone());
    let result = decode_file(ctx, opened.view, &opened.id, expansion);
    expansion.stack.pop();
    match result {
        Ok(decoded) => Ok(Loaded::Decoded {
            opened,
            decoded: Box::new(decoded),
        }),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => {
            losses.push(
                AlibreLossCode::AssemblyMemberUndecoded
                    .note(format!("component {file} does not decode: {error}")),
            );
            Ok(Loaded::Absent)
        }
    }
}

/// Decodes one component file: a part or a subassembly.
fn decode_file<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
    id: &str,
    expansion: &mut Expansion,
) -> Result<Decoded, CodecError> {
    let container = AlibreContainer::open(ctx, view)?;
    match container.kind() {
        DocumentKind::Part => crate::decode::decode_part(ctx, view, &container),
        DocumentKind::Assembly => decode_assembly(ctx, view, &container, Some(id), expansion),
    }
}

/// The occurrence an inclusion places, stated in the build; its identity.
fn place(
    build: &mut Build,
    model: &Assembly,
    inclusion: &Inclusion,
    prototype: ProductDefinitionId,
) -> Result<OccurrenceId, CodecError> {
    let id = OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("alibre", "model", "occurrence"),
        inclusion.id,
    );
    let transform = inclusion.placement.as_ref().and_then(transform);
    if transform.is_none() {
        build
            .losses
            .push(AlibreLossCode::AssemblyPlacementUnreadable.note(format!(
                "inclusion #{} has no readable placement; the occurrence has none",
                inclusion.id
            )));
    }
    let configuration = model.active_configuration();
    let hidden = inclusion
        .members
        .iter()
        .filter(|member| !member.nested)
        .any(|member| {
            member.hidden
                || configuration
                    .is_some_and(|configuration| configuration.hidden.contains(&member.id))
        });
    let file = stem(&inclusion.path);
    let counter = build.ordinals.entry(file.to_ascii_lowercase()).or_insert(1);
    let ordinal = next(counter)?;
    let mut occurrence = Occurrence::new(id.clone(), prototype, transform);
    occurrence.name = Some(format!("{file}<{ordinal}>"));
    occurrence.visible = Some(!hidden);
    occurrence.suppressed =
        configuration.map(|configuration| configuration.suppressed.contains(&inclusion.id));
    build.ir.model.occurrences.push(occurrence);
    if inclusion.anchored {
        let frame = transform.unwrap_or_else(Transform::identity);
        let joint = AssemblyJoint::grounded(
            JointId::compose(
                &cadmpeg_ir::identity_namespace!("alibre", "model", "joint"),
                inclusion.id,
            ),
            JointConnector {
                operand: JointOperand::occurrence(id.clone(), file, Vec::new()),
                frame,
                detached: false,
            },
            None,
        );
        build.ir.model.assembly_joints.push(joint);
    }
    Ok(id)
}

/// The placement as an affine transform, in millimetres.
fn transform(placement: &Placement) -> Option<Transform> {
    let [x, y, z] = placement.axes;
    let s = placement.scale;
    let [ox, oy, oz] = placement.origin;
    Transform::affine([
        [x[0] * s, y[0] * s, z[0] * s, ox],
        [x[1] * s, y[1] * s, z[1] * s, oy],
        [x[2] * s, y[2] * s, z[2] * s, oz],
    ])
}

#[cfg(test)]
mod tests {
    use super::stem;

    #[test]
    fn a_stored_path_names_its_file_after_the_last_separator() {
        assert_eq!(stem("D:/work/parts/asm_box"), "asm_box");
        assert_eq!(stem("C:\\parts\\bracket"), "bracket");
        assert_eq!(stem("plain"), "plain");
    }
}
