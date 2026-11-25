// SPDX-License-Identifier: Apache-2.0
//! Open shells restated as unjoined trimmed surfaces for an IGES version that
//! defines no open shell.

use super::{has_brep_topology, is_decoder_free_geometry_body};
use crate::loss::IgesLossCode;
use cadmpeg_ir::ids::{BodyId, EdgeId, RegionId, ShellId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{BodyKind, Region, Shell};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

/// The document with each sheet body of more than one face restated as one
/// body per face, with a copy of each edge for each further use, and a
/// `writer.open-shell-unjoined` note per restated body. `None` when `version`
/// defines the open shell (Type 514 Form 2), or when the document needs no
/// restatement or has no trimmed-surface form.
pub(super) fn unjoined_open_shells(
    ir: &CadIr,
    version: crate::IgesVersion,
) -> Option<(CadIr, Vec<LossNote>)> {
    let geometry = &ir.geometry;
    if version.defines_open_shells()
        || !has_brep_topology(ir)
        || geometry
            .bodies
            .iter()
            .any(|body| !is_decoder_free_geometry_body(body) && body.kind == BodyKind::Solid)
        || geometry
            .loops
            .iter()
            .any(|loop_| loop_.vertices().next().is_some())
        || geometry
            .regions
            .iter()
            .any(|region| region.unstated.is_some())
        || geometry
            .shells
            .iter()
            .any(|shell| shell.unstated.is_some() || !shell.members.is_empty())
    {
        return None;
    }
    let mut unjoined = ir.clone();
    let mut split_bodies = BTreeMap::new();
    for body in geometry
        .bodies
        .iter()
        .filter(|body| !is_decoder_free_geometry_body(body) && body.kind == BodyKind::Sheet)
    {
        let regions = geometry.stated_body_regions(&body.id).collect::<Vec<_>>();
        let shells = regions
            .iter()
            .flat_map(|region| region.stated_shells())
            .collect::<Vec<_>>();
        let faces = shells
            .iter()
            .flat_map(|shell| geometry.stated_shell_faces(shell))
            .collect::<Vec<_>>();
        let Some(first) = faces.first() else {
            continue;
        };
        if faces.len() == 1 && regions.len() == 1 && shells.len() == 1 {
            continue;
        }
        let kept_shell = first.shell.clone();
        let kept_region = geometry
            .shells
            .iter()
            .find(|shell| shell.id == kept_shell)?
            .region
            .clone();
        let removed_regions = regions
            .iter()
            .map(|region| region.id.clone())
            .filter(|region| *region != kept_region)
            .collect::<BTreeSet<_>>();
        let removed_shells = shells
            .iter()
            .map(|shell| (*shell).clone())
            .filter(|shell| *shell != kept_shell)
            .collect::<BTreeSet<_>>();
        unjoined
            .geometry
            .regions
            .retain(|region| !removed_regions.contains(&region.id));
        unjoined
            .geometry
            .shells
            .retain(|shell| !removed_shells.contains(&shell.id));
        let region = unjoined
            .geometry
            .regions
            .iter_mut()
            .find(|region| region.id == kept_region)?;
        let native_ref = region.native_ref.take();
        *region = Region::new(
            kept_region.clone(),
            body.id.clone(),
            vec![kept_shell.clone()],
        )
        .with_native_ref(native_ref);
        let mut bodies = Vec::new();
        for (ordinal, face) in faces.iter().enumerate().skip(1) {
            let body_id = BodyId::try_from(format!("{}:unjoined.{ordinal}", body.id)).ok()?;
            let region_id = RegionId::try_from(format!("{kept_region}:unjoined.{ordinal}")).ok()?;
            let shell_id = ShellId::try_from(format!("{kept_shell}:unjoined.{ordinal}")).ok()?;
            let mut split = body.clone();
            split.id = body_id.clone();
            unjoined.geometry.bodies.push(split);
            unjoined.geometry.regions.push(Region::new(
                region_id.clone(),
                body_id.clone(),
                vec![shell_id.clone()],
            ));
            unjoined
                .geometry
                .shells
                .push(Shell::new(shell_id.clone(), region_id));
            unjoined
                .geometry
                .faces
                .iter_mut()
                .find(|record| record.id == face.id)?
                .shell = shell_id;
            bodies.push(body_id.to_string());
        }
        split_bodies.insert(body.id.as_str(), (faces.len(), bodies));
    }
    let body_of_loop = owning_bodies(ir);
    let mut uses = BTreeMap::<EdgeId, usize>::new();
    let mut copies = Vec::new();
    let mut shared = BTreeSet::new();
    for coedge in &mut unjoined.geometry.coedges {
        coedge.radial_next = coedge.id.clone();
        let count = uses.entry(coedge.edge.clone()).or_default();
        *count += 1;
        if *count == 1 {
            continue;
        }
        let edge = geometry.edges.iter().find(|edge| edge.id == coedge.edge)?;
        let mut copy = edge.clone();
        copy.id = EdgeId::try_from(format!("{}:unjoined.{count}", edge.id)).ok()?;
        coedge.edge = copy.id.clone();
        copies.push(copy);
        shared.extend(body_of_loop.get(coedge.owner_loop.as_str()).copied());
    }
    unjoined.geometry.edges.extend(copies);
    if has_brep_topology(&unjoined) {
        return None;
    }
    let mut losses = Vec::new();
    for body in &geometry.bodies {
        let split = split_bodies.get(body.id.as_str());
        if split.is_none() && !shared.contains(body.id.as_str()) {
            continue;
        }
        let surfaces = split.map_or_else(
            || "its faces are written as unjoined trimmed surfaces".to_owned(),
            |(faces, bodies)| {
                format!(
                    "it is written as {faces} unjoined trimmed surfaces, in it and in bodies {}",
                    bodies.join(", ")
                )
            },
        );
        losses.push(IgesLossCode::WriterOpenShellUnjoined.note(format!(
            "IGES {} defines no open shell (Type 514 Form 2) for body {}: {surfaces}, and the edges its faces share are not joined",
            version.name(),
            body.id
        )));
    }
    Some((unjoined, losses))
}

/// The body that owns each face loop.
fn owning_bodies(ir: &CadIr) -> BTreeMap<&str, &str> {
    let geometry = &ir.geometry;
    let region_body = geometry
        .regions
        .iter()
        .map(|region| (region.id.as_str(), region.body.as_str()))
        .collect::<BTreeMap<_, _>>();
    let shell_body = geometry
        .shells
        .iter()
        .filter_map(|shell| Some((shell.id.as_str(), *region_body.get(shell.region.as_str())?)))
        .collect::<BTreeMap<_, _>>();
    geometry
        .faces
        .iter()
        .filter_map(|face| Some((face, *shell_body.get(face.shell.as_str())?)))
        .flat_map(|(face, body)| face.loops.stated().map(move |loop_| (loop_.as_str(), body)))
        .collect()
}
