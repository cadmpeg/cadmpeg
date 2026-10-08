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
pub(super) struct ArchiveSession<'a, 'ctx> {
    pub(super) _members_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    pub(super) members: BTreeMap<String, ClassifiedMember<'a>>,
    pub(super) layers: DialectLayers,
    pub(super) losses: Vec<LossNote>,
}

pub(super) enum ClassifiedMember<'a> {
    Scanned(Box<ContainerScan<'a>>),
    Unreadable(String),
}

impl ArchiveSession<'_, '_> {
    pub(super) fn member_scan(
        &self,
        ctx: &DecodeContext<'_>,
        path: &str,
    ) -> Result<&ContainerScan<'_>, CodecError> {
        match ctx.get_btree_map(&self.members, path, "look up F3Z member scan")? {
            Some(ClassifiedMember::Scanned(scan)) => Ok(scan),
            Some(ClassifiedMember::Unreadable(message)) => Err(ctx
                .format_retained(
                    format_args!("f3z document member {path} could not be scanned: {message}"),
                    "retain F3D malformed diagnostic",
                )
                .map_or_else(std::convert::identity, CodecError::Malformed)),
            None => Err(ctx
                .format_retained(
                    format_args!("f3z document member {path} is not present in the archive"),
                    "retain F3D malformed diagnostic",
                )
                .map_or_else(std::convert::identity, CodecError::Malformed)),
        }
    }
}

fn insert_member_charged<'a>(
    ctx: &DecodeContext<'_>,
    members: &mut BTreeMap<String, ClassifiedMember<'a>>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    path: &str,
    member: ClassifiedMember<'a>,
) -> Result<(), CodecError> {
    if let Some(existing) = ctx.get_mut_btree_map(members, path, "replace F3Z member scan")? {
        *existing = member;
        return Ok(());
    }
    let key =
        storage.with_storage(|| ctx.copy_retained_text(path, "retain F3Z member index path"))?;
    let previous = storage
        .with_storage(|| ctx.insert_btree_map(members, key, member, "index F3Z archive members"))?;
    drop(previous);
    Ok(())
}

/// Resolves the archive manifest to the F3D member that owns the model.
pub(super) fn model_root<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<
    (
        (String, Option<String>),
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let manifest_bytes = scan.entry_bytes(ctx, MANIFEST_ENTRY)?;
    let text = ctx
        .validate_utf8(manifest_bytes, "validate F3Z JSON UTF-8")?
        .map_err(|error| {
            ctx.format_retained(
                format_args!("{MANIFEST_ENTRY} is not valid JSON: {error}"),
                "retain F3D malformed diagnostic",
            )
            .map_or_else(std::convert::identity, CodecError::Malformed)
        })?;
    let manifest: ManifestJson =
        ctx.parse_json(text, "parse F3Z manifest JSON")
            .map_err(|error| {
                let CodecError::Malformed(error) = error else {
                    return error;
                };
                ctx.format_retained(
                    format_args!("{MANIFEST_ENTRY} is not valid JSON: {error}"),
                    "retain F3D malformed diagnostic",
                )
                .map_or_else(std::convert::identity, CodecError::Malformed)
            })?;
    let mut names_storage = ctx.reserve_scoped(0, "stage F3Z selected root names")?;
    let names = model_root_member(ctx, scan, &manifest.root, &mut names_storage)?;
    Ok((names, names_storage))
}

/// Classifies all F3D members and attaches each nested layer to its archive path.
pub(super) fn classify_members<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<ArchiveSession<'a, 'ctx>, CodecError> {
    let mut members_storage = ctx.reserve_scoped(0, "stage F3Z classified members")?;
    let mut members = BTreeMap::new();
    let primary = scan
        .kind
        .dialect()
        .try_clone_for_decode(ctx, "copy dialect layers")?;
    let mut layers = DialectLayers::of(primary);
    let mut losses = Vec::new();
    for entry in ctx.admit_iter(&scan.entries, "classify F3Z document members")? {
        let member_path = entry.name.as_str();
        if !crate::container::is_f3d_name(ctx, member_path)? {
            continue;
        }
        let member_view = scan.entry_view(ctx, member_path)?.ok_or_else(|| {
            ctx.format_retained(
                format_args!("f3z document member {member_path} is not readable"),
                "retain F3D malformed diagnostic",
            )
            .map_or_else(std::convert::identity, CodecError::Malformed)
        })?;
        let member_scan = match crate::container::scan(ctx, member_view) {
            Ok(member_scan) => member_scan,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                let message = members_storage.with_storage(|| {
                    ctx.format_retained(
                        format_args!("{error}"),
                        "retain F3Z unreadable member error",
                    )
                })?;
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
                    &mut members_storage,
                    member_path,
                    ClassifiedMember::Unreadable(message),
                )?;
                continue;
            }
        };
        let (member_layers, mut member_losses) =
            crate::dialect::classify_layers(ctx, &member_scan)?;
        for loss in ctx.admit_iter(&mut member_losses, "scope F3Z member losses")? {
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
        members_storage.grow_limit(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            ContainerScan<'_>,
        >()))?;
        insert_member_charged(
            ctx,
            &mut members,
            &mut members_storage,
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
        _members_storage: members_storage,
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
    for matched in ctx.admit_iter(member, "scan F3Z dialect layers")? {
        let matched = matched.try_clone_for_decode(ctx, "copy dialect layers")?;
        let instance = match matched.instance() {
            Some(nested) => ctx.format_retained(
                format_args!("{member_path}/{nested}"),
                "retain F3Z dialect layer instance",
            )?,
            None => ctx.copy_retained_text(member_path, "retain F3Z dialect layer instance")?,
        };
        let matched = matched
            .with_declared_entry(
                ctx,
                cadmpeg_core::nonblank_const!(crate::dialect::DECLARED_ARCHIVE_MEMBER),
                member_path,
                "declare F3Z archive member",
            )?
            .with_instance(instance);
        if let Err(rejected) =
            match target.insert_for_decode(ctx, matched, "collect F3Z member dialect layers") {
                Ok(()) => Ok(()),
                Err(cadmpeg_core::dialect::DialectLayerError::Duplicate(layer)) => Err(layer),
                Err(cadmpeg_core::dialect::DialectLayerError::ResourceLimit(limit)) => {
                    return Err(CodecError::ResourceLimit(limit))
                }
            }
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
    names_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<(String, Option<String>), CodecError> {
    if crate::container::is_f3d_name(ctx, archive_root)? {
        return Ok((
            names_storage
                .with_storage(|| ctx.copy_retained_text(archive_root, "retain F3Z model root"))?,
            None,
        ));
    }

    let description_bytes = scan.entry_bytes(ctx, DESIGN_DESCRIPTION_ENTRY)?;
    let text = ctx
        .validate_utf8(description_bytes, "validate F3Z design description UTF-8")?
        .map_err(|error| {
            ctx.format_retained(
                format_args!("{DESIGN_DESCRIPTION_ENTRY} is not valid JSON: {error}"),
                "retain F3D malformed diagnostic",
            )
            .map_or_else(std::convert::identity, CodecError::Malformed)
        })?;
    let description: DesignDescriptionJson = ctx
        .parse_json(text, "match F3Z derived model reference")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            ctx.format_retained(
                format_args!("{DESIGN_DESCRIPTION_ENTRY} is not valid JSON: {error}"),
                "retain F3D malformed diagnostic",
            )
            .map_or_else(std::convert::identity, CodecError::Malformed)
        })?;
    let mut candidate_storage = ctx.reserve_scoped(0, "collect F3Z model candidates")?;
    let mut candidates = std::collections::BTreeSet::new();
    for graph in ctx.admit_iter(
        &description.design_description.design_graphs,
        "scan F3Z design graphs",
    )? {
        let mut root_storage = ctx.reserve_scoped(0, "index F3Z root object IDs")?;
        let root_ids = root_storage.with_storage(|| {
            ctx.collect_btree_set(
                ctx.admit_iter(&graph.root_ids, "scan F3Z root object IDs")?
                    .copied(),
                "index F3Z root object IDs",
            )
        })?;
        let root = ctx.find_by(
            &graph.design_objects,
            |object| {
                Ok(
                    ctx.contains_btree_set(&root_ids, &object.id, "match F3Z root object ID")?
                        && ctx.equal(
                            object.relative_path.as_str(),
                            archive_root,
                            "match F3Z root object path",
                        )?,
                )
            },
            "scan F3Z root objects",
        )?;
        drop(root_ids);
        drop(root_storage);
        let Some(root) = root else {
            continue;
        };
        // Object IDs the root's derived references name, built once.
        let mut derived_storage = ctx.reserve_scoped(0, "index F3Z derived model references")?;
        let mut derived_ids = std::collections::BTreeSet::new();
        for reference in ctx.admit_iter(&root.references, "scan F3Z root references")? {
            if reference.reference_type != "DERIVED" {
                continue;
            }
            for id in ctx.admit_iter(&reference.ids, "index F3Z derived model references")? {
                derived_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut derived_ids,
                        *id,
                        "index F3Z derived model references",
                    )
                })?;
            }
        }
        for object in ctx.admit_iter(&graph.design_objects, "scan F3Z derived model objects")? {
            if !ctx.eq_ignore_ascii_case(
                &object.content_type,
                "f3d",
                "classify F3Z object content type",
            )? || !crate::container::is_f3d_name(ctx, &object.relative_path)?
                || scan.entry_view(ctx, &object.relative_path)?.is_none()
            {
                continue;
            }
            let derived = ctx.contains_btree_set(
                &derived_ids,
                &object.id,
                "match F3Z derived model reference",
            )?;
            if derived {
                ctx.insert_scoped_btree_value(
                    &mut candidate_storage,
                    &mut candidates,
                    object.relative_path.as_str(),
                    "collect F3Z model candidates",
                )?;
            }
        }
    }
    if candidates.len() == 1 {
        let model_root = candidates
            .first()
            .copied()
            .ok_or_else(|| CodecError::malformed("F3Z candidate set is empty"))?;
        Ok((
            names_storage.with_storage(|| {
                ctx.copy_retained_text(model_root, "retain F3Z selected model root")
            })?,
            Some(names_storage.with_storage(|| {
                ctx.copy_retained_text(archive_root, "retain F3Z drawing root")
            })?),
        ))
    } else {
        Err(ctx.format_retained(format_args!(
            "f3z root member {archive_root} is not an f3d document and has {} unambiguous derived f3d model members",
            candidates.len()
        ), "retain F3D malformed diagnostic").map_or_else(std::convert::identity, CodecError::Malformed))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn drawing_root_search_preserves_work_refusal() {
        let description = br#"{"designDescription":{"designGraphs":[{"rootIds":[1,2],"designObjects":[{"id":2,"relativePath":"drawing.f2d","contentType":"f2d","references":[]}] }]}}"#;
        let bytes = crate::test_support::assembly_test::f3z_archive_with_design_description(
            "drawing.f2d",
            &[("drawing.f2d", b"drawing"), ("model.f3d", b"model")],
            description,
        );
        crate::test_support::with_decode_context(|scan_ctx| {
            let scan =
                crate::container::scan(scan_ctx, cadmpeg_core::decode::View::over_retained(&bytes))
                    .unwrap();
            let error = crate::test_support::resource_refusal_at(
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                "match F3Z root object ID",
                0,
                |ctx| {
                    let mut storage = ctx.reserve_scoped(0, "test root names")?;
                    super::model_root_member(ctx, &scan, "drawing.f2d", &mut storage)
                },
            );
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("root scan must refuse");
            };
            assert_eq!(limit.operation, "match F3Z root object ID");
        });
    }
    #[test]
    fn manifest_extensions_preserve_work_and_depth_refusals() {
        let bytes = crate::test_support::assembly_test::f3z_archive(
            r#"model.f3d","extension":[[[0]]],"spare":"unused"#,
            &[("model.f3d", b"model")],
        );
        crate::test_support::with_decode_context(|scan_ctx| {
            let root = cadmpeg_core::decode::View::over_retained(&bytes);
            let scan = crate::container::scan(scan_ctx, root).unwrap();
            for dimension in [
                cadmpeg_core::decode::ResourceDimension::WorkUnits,
                cadmpeg_core::decode::ResourceDimension::RecursionDepth,
            ] {
                if dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits {
                    let error = crate::test_support::resource_refusal_at(
                        dimension,
                        "parse F3Z manifest JSON",
                        0,
                        |ctx| super::model_root(ctx, &scan).map(|_| ()),
                    );
                    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                        panic!("manifest scan must refuse");
                    };
                    assert_eq!(limit.operation, "parse F3Z manifest JSON");
                    continue;
                }
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_recursion_depth = 1;
                crate::test_support::with_decode_policy(&policy, |ctx| {
                    let error = super::model_root(ctx, &scan).unwrap_err();
                    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                        panic!("manifest scan must refuse");
                    };
                    assert_eq!(limit.dimension, dimension);
                    assert_eq!(limit.operation, "parse F3Z manifest JSON");
                    assert_eq!(Some(limit), ctx.resource_refusal());
                });
            }
        });
    }

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
            &mut ctx.reserve_scoped(0, "test member index").unwrap(),
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
    fn f3z_member_index_path_refuses_materialized_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::insert_member_charged(
            &ctx,
            &mut std::collections::BTreeMap::new(),
            &mut ctx.reserve_scoped(0, "test member index").unwrap(),
            "part.f3d",
            super::ClassifiedMember::Unreadable("bad member".to_owned()),
        )
        .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3Z member index path")
        );
    }

    #[test]
    fn direct_model_root_copy_uses_scoped_storage() {
        let bytes = crate::test_support::assembly_test::f3z_archive(
            "model.f3d",
            &[("model.f3d", b"model")],
        );
        crate::test_support::with_decode_context(|scan_ctx| {
            let scan =
                crate::container::scan(scan_ctx, cadmpeg_core::decode::View::over_retained(&bytes))
                    .unwrap();
            let error = crate::test_support::resource_refusal_at(
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
                "retain F3Z model root",
                0,
                |ctx| {
                    let mut storage = ctx.reserve_scoped(0, "test root names")?;
                    super::model_root_member(ctx, &scan, "model.f3d", &mut storage)
                },
            );
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "retain F3Z model root")
            );
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut storage = ctx.reserve_scoped(0, "test root names").unwrap();
            let names = super::model_root_member(&ctx, &scan, "model.f3d", &mut storage).unwrap();
            assert_eq!(names, ("model.f3d".to_owned(), None));
            drop(storage);
            ctx.finish_session().unwrap();
        });
    }

    #[test]
    fn missing_derived_root_diagnostic_refuses_retained_storage() {
        let bytes = crate::test_support::assembly_test::f3z_archive_with_design_description(
            "drawing.f2d",
            &[("drawing.f2d", b"drawing"), ("model.f3d", b"model")],
            br#"{"designDescription":{"designGraphs":[]}}"#,
        );
        crate::test_support::with_decode_context(|normal| {
            let scan =
                crate::container::scan(normal, cadmpeg_core::decode::View::over_retained(&bytes))
                    .unwrap();
            let error = super::model_root(normal, &scan).unwrap_err();
            assert!(
                matches!(error, cadmpeg_core::CodecError::Malformed(message) if message == "f3z root member drawing.f2d is not an f3d document and has 0 unambiguous derived f3d model members")
            );
            let error = crate::test_support::resource_refusal_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                "retain F3D malformed diagnostic",
                0,
                |ctx| super::model_root(ctx, &scan).map(|_| ()),
            );
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "retain F3D malformed diagnostic")
            );
        });
    }
}
