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

use crate::assembly::{self, Assembly, Inclusion, Placement, DEFAULT_CONFIGURATION};
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
/// Most entity names that the loss of unknown entities lists.
const MAX_UNREAD_NAMES: usize = 16;
/// Most examples that one aggregated loss lists by inclusion.
const MAX_LOSS_EXAMPLES: usize = 16;
/// Extensions of the component files, in the order the decode tries them.
const EXTENSIONS: [&str; 4] = ["AD_PRT", "AD_ASM", "ad_prt", "ad_asm"];
/// Tolerance of the checks that a placement is a rigid motion: the length of
/// each axis, the angle between two axes, and the stored third axis.
const RIGID_TOLERANCE: f64 = 1e-6;

/// The state that one expansion shares across its nesting levels.
#[derive(Debug)]
pub(crate) struct Expansion {
    /// The identities of the files being decoded, outermost first.
    stack: Vec<String>,
    /// The occurrences placed so far.
    occurrences: usize,
    /// The digest of the file the expansion started from, which no component
    /// may repeat: the root has no companion identity of its own.
    root: Option<Sha256Digest>,
    /// The most occurrences the expansion places.
    limit: usize,
}

impl Default for Expansion {
    fn default() -> Self {
        Self {
            stack: Vec::new(),
            occurrences: 0,
            root: None,
            limit: MAX_OCCURRENCES,
        }
    }
}

/// The decode of one assembly while its components are resolved.
struct Build {
    ir: CadIr,
    fidelity: cadmpeg_ir::SourceFidelity,
    losses: Vec<LossNote>,
    geometry_transferred: bool,
    /// The definitions of the components, by lower-case file stem and upper-case
    /// document GUID, so that two documents of one file name stay apart.
    definitions: BTreeMap<(String, String), ProductDefinitionId>,
    /// The next scope number.
    scopes: u32,
    /// The next external definition number.
    externals: u32,
    /// The occurrences of each file stem so far.
    ordinals: BTreeMap<String, u32>,
    /// The inclusions without a readable placement, with the reason.
    unplaced: Vec<(u32, String)>,
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

/// The file name that the decode may ask the companion source for: a bare
/// name that no loader reads as a path, an escape, or a drive. The companion
/// source of the command line decodes `%XX` escapes and follows absolute
/// paths, so the name must hold none of those characters.
fn bare_stem(path: &str) -> Result<&str, &'static str> {
    let name = stem(path);
    if name.is_empty() || name == "." || name == ".." {
        return Err("names no file");
    }
    if name.len() > 255 {
        return Err("names a file longer than 255 octets");
    }
    if name
        .chars()
        .any(|c| c.is_control() || matches!(c, '%' | ':' | '/' | '\\'))
    {
        return Err("holds an escape, a drive, a separator or a control character");
    }
    Ok(name)
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
    if anchor.is_none() {
        expansion.root = Some(digest.clone());
    }
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
        unplaced: Vec::new(),
    };
    let mut owned = Vec::new();
    let mut stopped = 0_usize;
    for inclusion in &model.inclusions {
        if expansion.occurrences >= expansion.limit {
            stopped += 1;
            continue;
        }
        expansion.occurrences += 1;
        ctx.charge_entities(1, "place Alibre occurrence")?;
        let prototype = component(ctx, anchor, inclusion, &mut build, expansion)?;
        owned.push(place(&mut build, &model, inclusion, prototype)?);
    }
    if !build.unplaced.is_empty() {
        let examples = build
            .unplaced
            .iter()
            .take(MAX_LOSS_EXAMPLES)
            .map(|(id, reason)| format!("#{id} ({reason})"))
            .collect::<Vec<_>>()
            .join(", ");
        build
            .losses
            .push(AlibreLossCode::AssemblyPlacementUnreadable.note(format!(
                "{} inclusion(s) have no readable rigid placement and their occurrences have none: {examples}{}",
                build.unplaced.len(),
                more(build.unplaced.len())
            )));
    }
    if stopped > 0 {
        build
            .losses
            .push(AlibreLossCode::AssemblyExpansionTruncated.note(format!(
                "{stopped} inclusion(s) lie past the limit of {} occurrences and were left out",
                expansion.limit
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

/// The suffix of an aggregated loss that lists `total` examples up to the cap.
fn more(total: usize) -> String {
    if total > MAX_LOSS_EXAMPLES {
        format!(" and {} more", total - MAX_LOSS_EXAMPLES)
    } else {
        String::new()
    }
}

/// The inclusion numbers of an aggregated loss, up to the cap.
fn numbers(ids: &[u32]) -> String {
    let shown = ids
        .iter()
        .take(MAX_LOSS_EXAMPLES)
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{shown}{}", more(ids.len()))
}

/// The losses of the assembly records that the decode does not map. Each loss
/// that could repeat for every inclusion is one loss with a count.
fn model_losses(model: &Assembly) -> Vec<LossNote> {
    let mut losses = Vec::new();
    if !model.unread.is_empty() {
        let total = model.unread.len();
        let names = model
            .unread
            .iter()
            .take(MAX_UNREAD_NAMES)
            .map(|(name, count)| format!("{name} x{count}"))
            .collect::<Vec<_>>();
        let rest = if total > MAX_UNREAD_NAMES {
            format!(" and {} more names", total - MAX_UNREAD_NAMES)
        } else {
            String::new()
        };
        losses.push(AlibreLossCode::AssemblyEntitiesUnread.note(format!(
            "the feature model holds entities that the assembly reader does not know: {}{rest}",
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
    if !model.unreadable.is_empty() {
        let shown = model
            .unreadable
            .iter()
            .take(MAX_LOSS_EXAMPLES)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        losses.push(AlibreLossCode::AssemblyRecordUnreadable.note(format!(
            "{} assembly record(s) do not read: {shown}{}",
            model.unreadable.len(),
            more(model.unreadable.len())
        )));
    }
    let named = model
        .inclusions
        .iter()
        .filter(|inclusion| {
            inclusion
                .configuration
                .as_deref()
                .is_some_and(|name| name != DEFAULT_CONFIGURATION)
        })
        .count();
    let others = model
        .configurations
        .len()
        .saturating_sub(usize::from(model.active.is_some()));
    if named > 0 || others > 0 {
        losses.push(AlibreLossCode::AssemblyConfigurationUnmapped.note(format!(
            "{named} occurrence(s) name a configuration of their component other than {DEFAULT_CONFIGURATION}, \
             and {others} assembly configuration(s) beyond the active one are not read; the occurrences carry the \
             state of the active configuration only"
        )));
    }
    let unmapped = model
        .inclusions
        .iter()
        .filter(|inclusion| !inclusion.unmapped.is_empty())
        .collect::<Vec<_>>();
    if !unmapped.is_empty() {
        let ids = unmapped
            .iter()
            .map(|inclusion| inclusion.id)
            .collect::<Vec<_>>();
        let first = unmapped[0];
        losses.push(AlibreLossCode::AssemblyInclusionUnmapped.note(format!(
            "{} inclusion(s) state attributes that the occurrence does not carry: {}; \
             inclusion #{} states attributes {:?}",
            unmapped.len(),
            numbers(&ids),
            first.id,
            first.unmapped
        )));
    }
    let mut nested = Vec::new();
    let mut total = 0_usize;
    for inclusion in &model.inclusions {
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
            nested.push(inclusion.id);
            total += hidden;
        }
    }
    if !nested.is_empty() {
        losses.push(AlibreLossCode::AssemblyNestedStateUnread.note(format!(
            "{} inclusion(s) hide {total} component(s) of their subassembly: {}; the occurrence \
             carries no per-instance state of nested occurrences",
            nested.len(),
            numbers(&nested)
        )));
    }
    losses
}

/// A component file the companion source opened.
struct Opened<'a> {
    id: String,
    name: String,
    view: View<'a>,
}

/// Whether a reason says that the file does not exist.
fn is_missing(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    reason.contains("not found") || reason.contains("no such file")
}

/// The reason to report for a component that no extension opened: the first
/// reason that says more than that the file is missing, else the first.
fn informative(reasons: &[String]) -> String {
    let tried = EXTENSIONS.join(", ");
    match reasons.iter().find(|reason| !is_missing(reason)) {
        Some(reason) => reason.clone(),
        None => format!(
            "{} (tried the extensions {tried})",
            reasons.first().map_or("not found", String::as_str)
        ),
    }
}

fn open_component<'a>(
    ctx: &DecodeContext<'a>,
    anchor: Option<&str>,
    stem: &str,
) -> Result<Result<Opened<'a>, String>, CodecError> {
    let mut reasons = Vec::new();
    for extension in EXTENSIONS {
        let name = format!("{stem}.{extension}");
        match ctx.open_companion(anchor, &name)? {
            CompanionLoad::Loaded { id, view } => return Ok(Ok(Opened { id, name, view })),
            CompanionLoad::Unavailable(why) => reasons.push(why),
        }
    }
    Ok(Err(informative(&reasons)))
}

/// The key of a component in the build: its file stem and document GUID.
fn definition_key(file: &str, inclusion: &Inclusion) -> (String, String) {
    (
        file.to_ascii_lowercase(),
        inclusion
            .document_id
            .as_deref()
            .map(str::to_ascii_uppercase)
            .unwrap_or_default(),
    )
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
    let key = definition_key(file, inclusion);
    if let Some(definition) = build.definitions.get(&key) {
        return Ok(definition.clone());
    }
    let mut reference = ExternalDocument::path(&inclusion.path);
    reference.document_id =
        cadmpeg_core::text::NonBlankString::new(inclusion.document_id.clone().unwrap_or_default());
    let definition = match load(ctx, anchor, inclusion, expansion, &mut build.losses)? {
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
    inclusion: &Inclusion,
    expansion: &mut Expansion,
    losses: &mut Vec<LossNote>,
) -> Result<Loaded<'a>, CodecError> {
    let file = stem(&inclusion.path);
    if expansion.stack.len() >= MAX_NESTING {
        losses.push(AlibreLossCode::AssemblyExpansionTruncated.note(format!(
            "component {file} lies past the nesting limit of {MAX_NESTING} files and stays external"
        )));
        return Ok(Loaded::Absent);
    }
    let name = match bare_stem(&inclusion.path) {
        Ok(name) => name,
        Err(why) => {
            losses.push(AlibreLossCode::AssemblyMemberUnavailable.note(format!(
                "component {:?} is not looked up: its stored path {:?} {why}",
                file, inclusion.path
            )));
            return Ok(Loaded::Absent);
        }
    };
    let opened = match open_component(ctx, anchor, name)? {
        Ok(opened) => opened,
        Err(reason) => {
            losses.push(
                AlibreLossCode::AssemblyMemberUnavailable
                    .note(format!("component {name} is not available: {reason}")),
            );
            return Ok(Loaded::Absent);
        }
    };
    let is_root = expansion
        .root
        .as_ref()
        .is_some_and(|root| *root == Sha256Digest::digest(opened.view.window()));
    if is_root || expansion.stack.contains(&opened.id) {
        losses.push(AlibreLossCode::AssemblyMemberCycle.note(format!(
            "component {name} is a file that is already being decoded; the occurrence stays external"
        )));
        return Ok(Loaded::Absent);
    }
    let _depth = ctx.enter_nested("decode Alibre component")?;
    expansion.stack.push(opened.id.clone());
    let result = decode_file(
        ctx,
        opened.view,
        &opened.id,
        inclusion.document_id.as_deref(),
        expansion,
    );
    expansion.stack.pop();
    match result {
        Ok(FileDecode::Decoded(decoded)) => Ok(Loaded::Decoded { opened, decoded }),
        Ok(FileDecode::OtherDocument(actual)) => {
            losses.push(AlibreLossCode::AssemblyMemberDocumentMismatch.note(format!(
                "the file {} beside the assembly is the document {actual}, and the inclusion references the document {}; \
                 the occurrence stays external",
                opened.name,
                inclusion.document_id.as_deref().unwrap_or_default()
            )));
            Ok(Loaded::Absent)
        }
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => {
            losses.push(
                AlibreLossCode::AssemblyMemberUndecoded
                    .note(format!("component {name} does not decode: {error}")),
            );
            Ok(Loaded::Absent)
        }
    }
}

/// What reading one component file gave.
enum FileDecode {
    Decoded(Box<Decoded>),
    /// The file states another document GUID than the inclusion references.
    OtherDocument(String),
}

/// Decodes one component file: a part or a subassembly, when its document is
/// the one that `expected` names.
fn decode_file<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
    id: &str,
    expected: Option<&str>,
    expansion: &mut Expansion,
) -> Result<FileDecode, CodecError> {
    let container = AlibreContainer::open(ctx, view)?;
    if let (Some(expected), Some(actual)) = (expected, container.document_guid(ctx)) {
        if !actual.eq_ignore_ascii_case(expected) {
            return Ok(FileDecode::OtherDocument(actual));
        }
    }
    let decoded = match container.kind() {
        DocumentKind::Part => crate::decode::decode_part(ctx, view, &container)?,
        DocumentKind::Assembly => decode_assembly(ctx, view, &container, Some(id), expansion)?,
    };
    Ok(FileDecode::Decoded(Box::new(decoded)))
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
    let transform = match inclusion.placement.as_ref() {
        Some(placement) => transform(placement),
        None => Err("no CARTESIAN_TRANSFORMATION_OPERATOR_3D".to_owned()),
    };
    if let Err(reason) = &transform {
        build.unplaced.push((inclusion.id, reason.clone()));
    }
    let transform = transform.ok();
    let configuration = model.active_configuration();
    let own = inclusion.members.iter().filter(|member| !member.nested);
    let evidence = own.clone().next().is_some();
    let hidden = own.into_iter().any(|member| {
        member.hidden
            || configuration.is_some_and(|configuration| configuration.hidden.contains(&member.id))
    });
    let file = stem(&inclusion.path);
    let counter = build.ordinals.entry(file.to_ascii_lowercase()).or_insert(1);
    let ordinal = next(counter)?;
    let mut occurrence = Occurrence::new(id.clone(), prototype, transform);
    occurrence.name = Some(format!("{file}<{ordinal}>"));
    // An inclusion without a member of its own states no hidden flag.
    occurrence.visible = evidence.then_some(!hidden);
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

/// Whether two values agree within the rigid-motion tolerance; a value that is
/// not a number agrees with nothing.
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= RIGID_TOLERANCE
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The placement as a rigid transform, in millimetres. The operator maps a
/// point `p` of the component to `origin + p.x * x + p.y * y + p.z * z`, so
/// the images of the local axes are the columns of the rotation. The error
/// names why the placement is not a rigid motion: the Alibre schema allows a
/// scale and three free axes, and the IR transform carries rotation and
/// translation only.
fn transform(placement: &Placement) -> Result<Transform, String> {
    let [x, y, z] = placement.axes;
    if !near(placement.scale, 1.0) {
        return Err(format!("scale {}", placement.scale));
    }
    for (axis, name) in [(x, "x"), (y, "y"), (z, "z")] {
        if !near(dot(axis, axis).sqrt(), 1.0) {
            return Err(format!("the {name} axis is not a unit vector"));
        }
    }
    for (a, b, name) in [(x, y, "x and y"), (x, z, "x and z"), (y, z, "y and z")] {
        if !near(dot(a, b), 0.0) {
            return Err(format!("the {name} axes are not orthogonal"));
        }
    }
    let third = cross(x, y);
    if (0..3).any(|i| !near(third[i], z[i])) {
        let handed = if dot(third, z) < 0.0 {
            "a mirror"
        } else {
            "not the cross product of the x and y axes"
        };
        return Err(format!("the z axis is {handed}"));
    }
    let [ox, oy, oz] = placement.origin;
    Transform::affine([
        [x[0], y[0], z[0], ox],
        [x[1], y[1], z[1], oy],
        [x[2], y[2], z[2], oz],
    ])
    .ok_or_else(|| "a value is not finite".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{bare_stem, decode_assembly, informative, stem, transform, Expansion};
    use crate::assembly::Placement;
    use crate::container::AlibreContainer;
    use crate::integration_tests::assembly::{Files, ASM_REPEAT};
    use crate::loss::AlibreLossCode;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    #[test]
    fn the_expansion_stops_at_the_occurrence_limit_and_says_so() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(ASM_REPEAT, &arena, &policy).unwrap();
        let files = Files::corpus();
        let ctx = ctx.with_companions(&files);
        let container = AlibreContainer::open(&ctx, root).unwrap();
        let mut expansion = Expansion {
            limit: 3,
            ..Expansion::default()
        };
        let decoded = decode_assembly(&ctx, root, &container, None, &mut expansion).unwrap();
        assert_eq!(decoded.ir.model.occurrences.len(), 3);
        let cut: Vec<_> = decoded
            .body
            .losses
            .iter()
            .filter(|loss| loss.code == AlibreLossCode::AssemblyExpansionTruncated.kind())
            .collect();
        assert_eq!(cut.len(), 1);
        assert!(
            cut[0]
                .message
                .starts_with("1 inclusion(s) lie past the limit of 3 occurrences"),
            "{}",
            cut[0].message
        );
    }

    #[test]
    fn a_stored_path_names_its_file_after_the_last_separator() {
        assert_eq!(stem("D:/work/parts/asm_box"), "asm_box");
        assert_eq!(stem("C:\\parts\\bracket"), "bracket");
        assert_eq!(stem("plain"), "plain");
    }

    #[test]
    fn only_a_bare_name_is_looked_up() {
        assert_eq!(bare_stem("D:/work/parts/asm_box"), Ok("asm_box"));
        assert_eq!(bare_stem("C:\\parts\\my part (2)"), Ok("my part (2)"));
        for hostile in [
            "",
            "a/",
            "a/.",
            "a/..",
            "..\\..",
            "x/%2e%2e%2fsecret%2fpart",
            "%2fhome%2fu%2fother",
            "a/b%",
            "C:evil",
            "a/b\0c",
            "a/b\tc",
            "a/b\u{85}c",
        ] {
            assert!(bare_stem(hostile).is_err(), "{hostile:?}");
        }
        assert!(bare_stem(&"x".repeat(256)).is_err());
        assert!(bare_stem(&"x".repeat(255)).is_ok());
    }

    #[test]
    fn the_most_informative_reason_is_reported() {
        let missing = vec!["a.AD_PRT: not found".to_owned(); 4];
        let text = informative(&missing);
        assert!(text.starts_with("a.AD_PRT: not found"), "{text}");
        assert!(text.contains("AD_PRT, AD_ASM, ad_prt, ad_asm"), "{text}");
        let mixed = vec![
            "x: No such file or directory (os error 2)".to_owned(),
            "x: exceeds the remaining input budget of 5 bytes".to_owned(),
        ];
        assert_eq!(
            informative(&mixed),
            "x: exceeds the remaining input budget of 5 bytes"
        );
    }

    fn placement(axes: [[f64; 3]; 3], scale: f64) -> Placement {
        Placement {
            origin: [10.0, 20.0, 30.0],
            axes,
            scale,
        }
    }

    #[test]
    fn a_rotated_operator_is_a_rigid_transform_with_axis_images_as_columns() {
        // +90 deg about +Z: local x -> +y, local y -> -x.
        let turned = transform(&placement(
            [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            1.0,
        ))
        .unwrap();
        assert_eq!(
            turned.affine_rows(),
            [
                [0.0, -1.0, 0.0, 10.0],
                [1.0, 0.0, 0.0, 20.0],
                [0.0, 0.0, 1.0, 30.0]
            ]
        );
        // A rounding error of the export is within the tolerance.
        assert!(transform(&placement(
            [
                [6.123_031_769_111_89e-17, 1.0, 0.0],
                [-1.0, 6.123_031_769_111_89e-17, 0.0],
                [0.0, 0.0, 1.0]
            ],
            1.0
        ))
        .is_ok());
    }

    #[test]
    fn a_sheared_mirrored_or_scaled_operator_is_not_a_placement() {
        let sheared = transform(&placement(
            [[1.0, 0.0, 0.0], [0.5, 1.0, 0.0], [0.0, 0.0, 1.0]],
            1.0,
        ));
        assert!(sheared.is_err());
        let mirrored = transform(&placement(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
            1.0,
        ))
        .unwrap_err();
        assert!(mirrored.contains("mirror"), "{mirrored}");
        let scaled = transform(&placement(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            2.0,
        ));
        assert!(scaled.is_err());
        let short = transform(&placement(
            [[0.5, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            1.0,
        ));
        assert!(short.is_err());
        // A third axis that is orthonormal but not x cross y.
        let third = transform(&placement(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, f64::NAN]],
            1.0,
        ));
        assert!(third.is_err());
    }
}
