// SPDX-License-Identifier: Apache-2.0
//! F3Z manifest resolution and archive-member dialect classification.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::LossNote;
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

/// Resolves the archive manifest to the F3D member that owns the model.
pub(super) fn model_root(scan: &ContainerScan<'_>) -> Result<(String, Option<String>), CodecError> {
    let manifest: ManifestJson = serde_json::from_slice(scan.entry_bytes(MANIFEST_ENTRY)?)
        .map_err(|error| {
            CodecError::malformed(format_args!("{MANIFEST_ENTRY} is not valid JSON: {error}"))
        })?;
    model_root_member(scan, &manifest.root)
}

/// Classifies all F3D members and attaches each nested layer to its archive path.
pub(super) fn classify_members<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<ArchiveSession<'a>, CodecError> {
    let mut members = BTreeMap::new();
    let mut layers = DialectLayers::of(scan.kind.dialect().clone());
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
            Err(error) => {
                let message = error.to_string();
                losses.push(F3dLossCode::XrefMemberUndecoded.note(format!(
                    "xref {member_path}: member could not be scanned as an F3D document ({message}); its source bytes remain retained"
                )));
                members.insert(
                    member_path.to_owned(),
                    ClassifiedMember::Unreadable(message),
                );
                continue;
            }
        };
        let (member_layers, member_losses) = crate::dialect::classify_layers(&member_scan);
        losses.extend(member_losses.into_iter().map(|mut loss| {
            loss.message = format!("archive member {member_path}: {}", loss.message);
            loss
        }));
        losses.extend(merge_member_layers(
            &mut layers,
            &member_layers,
            member_path,
        ));
        members.insert(
            member_path.to_owned(),
            ClassifiedMember::Scanned(Box::new(member_scan)),
        );
    }
    losses.extend(crate::dialect::dialect_losses(&layers));
    Ok(ArchiveSession {
        members,
        layers,
        losses,
    })
}

/// Attaches one archive member's identity and nested layers to its archive path.
pub(super) fn merge_member_layers(
    target: &mut DialectLayers,
    member: &DialectLayers,
    member_path: &str,
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    for matched in member.iter().cloned() {
        let instance = matched.instance().map_or_else(
            || member_path.to_owned(),
            |nested| format!("{member_path}/{nested}"),
        );
        let collision_instance = instance.clone();
        let mut declared = matched.declared().clone();
        declared.insert(
            crate::dialect::DECLARED_ARCHIVE_MEMBER.to_owned(),
            member_path.to_owned(),
        );
        let matched = matched.with_declared(declared).with_instance(instance);
        let format = matched.format().to_owned();
        if target.insert(matched).is_err() {
            losses.push(F3dLossCode::DialectLayerCollision.note(format!(
                "archive member {member_path} produced a duplicate {format} dialect layer at instance {collision_instance}; the later layer was omitted",
            )));
        }
    }
    losses
}

fn model_root_member(
    scan: &ContainerScan<'_>,
    archive_root: &str,
) -> Result<(String, Option<String>), CodecError> {
    if crate::container::is_f3d_name(archive_root) {
        return Ok((archive_root.to_owned(), None));
    }

    let description: DesignDescriptionJson =
        serde_json::from_slice(scan.entry_bytes(DESIGN_DESCRIPTION_ENTRY)?).map_err(|error| {
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
        let derived_ids = root
            .references
            .iter()
            .filter(|reference| reference.reference_type == "DERIVED")
            .flat_map(|reference| reference.ids.iter().copied())
            .collect::<Vec<_>>();
        for object in &graph.design_objects {
            if derived_ids.contains(&object.id)
                && object.content_type.eq_ignore_ascii_case("f3d")
                && crate::container::is_f3d_name(&object.relative_path)
                && scan.entry_view(&object.relative_path).is_some()
            {
                candidates.push(object.relative_path.clone());
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    match candidates.as_slice() {
        [model_root] => Ok((model_root.clone(), Some(archive_root.to_owned()))),
        _ => Err(CodecError::malformed(format_args!(
            "f3z root member {archive_root} is not an f3d document and has {} unambiguous derived f3d model members",
            candidates.len()
        ))),
    }
}
