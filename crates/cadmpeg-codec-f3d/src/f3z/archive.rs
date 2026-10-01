// SPDX-License-Identifier: Apache-2.0
//! F3Z manifest resolution and archive-member dialect classification.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use serde::Deserialize;

use crate::container::ContainerScan;
use crate::loss::F3dLossCode;

const MANIFEST_ENTRY: &str = "Manifest.json";
const DESIGN_DESCRIPTION_ENTRY: &str = "DesignDescription.json";

#[derive(Deserialize)]
struct ManifestJson {
    root: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesignDescriptionJson {
    design_description: DesignDescription,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesignDescription {
    design_graphs: Vec<DesignGraph>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesignGraph {
    root_ids: Vec<u64>,
    design_objects: Vec<DesignObject>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesignObject {
    id: u64,
    relative_path: String,
    content_type: String,
    references: Vec<DesignObjectReference>,
}

#[derive(Deserialize)]
struct DesignObjectReference {
    #[serde(rename = "type")]
    reference_type: String,
    ids: Vec<u64>,
}

/// Classifies every member once and retains the lightweight scans needed by
/// inspect, root decode, and recursive XREF merge.
pub(super) struct ArchiveSession<'a> {
    pub(super) members: BTreeMap<String, ClassifiedMember<'a>>,
    pub(super) layers: DialectLayers,
    pub(super) losses: Vec<LossNote>,
}

pub(super) enum ClassifiedMember<'a> {
    Scanned(Box<ContainerScan<'a>>),
    Unreadable(String),
}

impl ArchiveSession<'_> {
    pub(super) fn member_scan(&self, path: &str) -> Result<&ContainerScan<'_>, CodecError> {
        match self.members.get(path) {
            Some(ClassifiedMember::Scanned(scan)) => Ok(scan),
            Some(ClassifiedMember::Unreadable(message)) => Err(CodecError::malformed(
                format_args!("f3z document member {path} could not be scanned: {message}"),
            )),
            None => Err(CodecError::malformed(format_args!(
                "f3z document member {path} is not present in the archive"
            ))),
        }
    }
}

fn insert_member_charged<'a>(
    ctx: &DecodeContext<'_>,
    members: &mut BTreeMap<String, ClassifiedMember<'a>>,
    path: &str,
    member: ClassifiedMember<'a>,
) -> Result<(), CodecError> {
    if let Some(existing) = members.get_mut(path) {
        *existing = member;
        return Ok(());
    }
    let key = ctx.copy_retained_text(path, "retain F3Z member index path")?;
    let previous = ctx.insert_btree_map(members, key, member, "index F3Z archive members")?;
    drop(previous);
    Ok(())
}

/// Resolves the archive manifest to the F3D member that owns the model.
pub(super) fn model_root(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<(String, Option<String>), CodecError> {
    let manifest_bytes = scan.entry_bytes(MANIFEST_ENTRY)?;
    let bytes = u64::try_from(manifest_bytes.len())
        .map_err(|_| ctx.refuse_codec_limit("parse F3Z manifest JSON", 0, u64::MAX))?;
    let _reservation = ctx.reserve_scoped(bytes, "parse F3Z manifest JSON")?;
    let manifest = serde_json::from_slice::<ManifestJson>(manifest_bytes).map_err(|error| {
        CodecError::malformed(format_args!("{MANIFEST_ENTRY} is not valid JSON: {error}"))
    })?;
    model_root_member(ctx, scan, &manifest.root)
}

/// Classifies all F3D members and attaches each nested layer to its archive path.
pub(super) fn classify_members<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<ArchiveSession<'a>, CodecError> {
    let mut members = BTreeMap::new();
    let primary = scan.kind.dialect().try_clone_for_decode(ctx)?;
    let mut layers = DialectLayers::of(primary);
    let mut losses = Vec::new();
    for member_path in scan
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .filter(|name| crate::container::is_f3d_name(name))
    {
        let member_view = scan.entry_view(member_path).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "f3z document member {member_path} is not readable"
            ))
        })?;
        let member_scan = match crate::container::scan(ctx, member_view) {
            Ok(member_scan) => member_scan,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                let message = ctx.format_retained(
                    format_args!("{error}"),
                    "retain F3Z unreadable member error",
                )?;
                super::push_loss(
                    ctx,
                    &mut losses,
                    F3dLossCode::XrefMemberUndecoded,
                    format_args!(
                        "xref {member_path}: member could not be scanned as an F3D document ({message}); its source bytes remain retained"
                    ),
                )?;
                insert_member_charged(
                    ctx,
                    &mut members,
                    member_path,
                    ClassifiedMember::Unreadable(message),
                )?;
                continue;
            }
        };
        let (member_layers, mut member_losses) =
            crate::dialect::classify_layers(ctx, &member_scan)?;
        for loss in &mut member_losses {
            loss.message = ctx.format_retained(
                format_args!("archive member {member_path}: {}", loss.message),
                "prefix F3Z member classification loss",
            )?;
        }
        ctx.append_vec(
            &mut losses,
            &mut { member_losses },
            "append F3Z report losses",
        )?;
        ctx.append_vec(
            &mut losses,
            &mut { merge_member_layers(ctx, &mut layers, &member_layers, member_path)? },
            "append F3Z report losses",
        )?;
        ctx.charge_collection_items(1, "retain F3Z member scan")?;
        insert_member_charged(
            ctx,
            &mut members,
            member_path,
            ClassifiedMember::Scanned(Box::new(member_scan)),
        )?;
    }
    ctx.append_vec(
        &mut losses,
        &mut { crate::dialect::dialect_losses(ctx, &layers)? },
        "append F3Z report losses",
    )?;
    Ok(ArchiveSession {
        members,
        layers,
        losses,
    })
}

/// Attaches one archive member's identity and nested layers to its archive path.
pub(super) fn merge_member_layers(
    ctx: &DecodeContext<'_>,
    target: &mut DialectLayers,
    member: &DialectLayers,
    member_path: &str,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    for matched in member.iter() {
        let matched = matched.try_clone_for_decode(ctx)?;
        let instance = match matched.instance() {
            Some(nested) => ctx.format_retained(
                format_args!("{member_path}/{nested}"),
                "retain F3Z dialect layer instance",
            )?,
            None => ctx.copy_retained_text(member_path, "retain F3Z dialect layer instance")?,
        };
        let matched = matched
            .with_declared_entry_charged(
                ctx,
                cadmpeg_core::nonblank_const!(crate::dialect::DECLARED_ARCHIVE_MEMBER),
                member_path,
                "declare F3Z archive member",
            )?
            .with_instance(instance);
        if let Err(rejected) =
            target.insert_charged(ctx, matched, "collect F3Z member dialect layers")?
        {
            let format = rejected.format();
            let collision_instance = rejected.instance().unwrap_or("unidentified");
            super::push_loss(
                ctx,
                &mut losses,
                F3dLossCode::DialectLayerCollision,
                format_args!(
                    "archive member {member_path} produced a duplicate {format} dialect layer at instance {collision_instance}; the later layer was omitted"
                ),
            )?;
        }
    }
    Ok(losses)
}

fn model_root_member(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    archive_root: &str,
) -> Result<(String, Option<String>), CodecError> {
    if crate::container::is_f3d_name(archive_root) {
        return Ok((
            ctx.copy_retained_text(archive_root, "retain F3Z model root")?,
            None,
        ));
    }

    let description_bytes = scan.entry_bytes(DESIGN_DESCRIPTION_ENTRY)?;
    let description_len = u64::try_from(description_bytes.len()).map_err(|_| {
        ctx.refuse_codec_limit("preflight F3Z design description JSON", 0, u64::MAX)
    })?;
    let _reservation =
        ctx.reserve_scoped(description_len, "preflight F3Z design description JSON")?;
    if !crate::json_budget::preflight(
        ctx,
        description_bytes,
        "preflight F3Z design description JSON",
        "match F3Z derived model reference",
        "collect F3Z model candidates",
    )? {
        return Err(CodecError::malformed(format_args!(
            "{DESIGN_DESCRIPTION_ENTRY} is not valid JSON"
        )));
    }
    let description: DesignDescriptionJson =
        serde_json::from_slice(description_bytes).map_err(|error| {
            CodecError::malformed(format_args!(
                "{DESIGN_DESCRIPTION_ENTRY} is not valid JSON: {error}"
            ))
        })?;
    let mut candidates = Vec::new();
    for graph in description.design_description.design_graphs {
        let Some(root) = graph.design_objects.iter().find(|object| {
            graph.root_ids.contains(&object.id) && object.relative_path == archive_root
        }) else {
            continue;
        };
        for object in &graph.design_objects {
            if !object.content_type.eq_ignore_ascii_case("f3d")
                || !crate::container::is_f3d_name(&object.relative_path)
                || scan.entry_view(&object.relative_path).is_none()
            {
                continue;
            }
            let mut derived = false;
            for reference in root
                .references
                .iter()
                .filter(|reference| reference.reference_type == "DERIVED")
            {
                for id in &reference.ids {
                    ctx.charge_work(1, "match F3Z derived model reference")?;
                    if *id == object.id {
                        derived = true;
                        break;
                    }
                }
                if derived {
                    break;
                }
            }
            if derived {
                ctx.reserve_vec(&mut candidates, 1, "collect F3Z model candidates")?;
                candidates.push(ctx.copy_retained_text(
                    &object.relative_path,
                    "retain F3Z model candidate name",
                )?);
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    match candidates.as_slice() {
        [model_root] => Ok((
            ctx.copy_retained_text(model_root, "retain F3Z selected model root")?,
            Some(ctx.copy_retained_text(archive_root, "retain F3Z drawing root")?),
        )),
        _ => Err(CodecError::malformed(format_args!(
            "f3z root member {archive_root} is not an f3d document and has {} unambiguous derived f3d model members",
            candidates.len()
        ))),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn f3z_member_index_refuses_collection_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::insert_member_charged(
            &ctx,
            &mut std::collections::BTreeMap::new(),
            "part.f3d",
            super::ClassifiedMember::Unreadable("bad member".to_owned()),
        )
        .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3Z archive members")
        );
    }

    #[test]
    fn f3z_member_index_path_refuses_retained_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::insert_member_charged(
            &ctx,
            &mut std::collections::BTreeMap::new(),
            "part.f3d",
            super::ClassifiedMember::Unreadable("bad member".to_owned()),
        )
        .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3Z member index path")
        );
    }
}
