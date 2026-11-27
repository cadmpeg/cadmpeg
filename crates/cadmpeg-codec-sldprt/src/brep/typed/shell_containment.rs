// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::topology::BodyKind;

use super::{region_chain, FaceNode, Facts, RegionKind, ShellNode};

impl Facts {
    /// Maps a finite solid region to its containing shell and its shell count.
    /// The path to the infinite void crosses that shell before any other.
    pub(in crate::brep) fn containing_shells(
        &self,
        body_attr: u32,
        face_stated: impl Fn(u32) -> bool,
    ) -> Option<HashMap<u32, (u32, usize)>> {
        let (bodies, regions, shells, faces) = self.ownership_maps()?;
        let body = bodies.get(&body_attr)?;
        if body.kind != BodyKind::Solid {
            return None;
        }
        let chain = region_chain(body, &regions)?;
        let root = chain.first()?;
        if root.kind != RegionKind::Void {
            return None;
        }
        let mut by_shell = HashMap::new();
        let mut by_region = HashMap::new();
        for region in &chain {
            let mut own_shells = Vec::new();
            let mut next = region.refs[4];
            while next > 1 {
                let shell = shells.get(&next)?;
                if shell.refs[6] != region.attr
                    || by_shell.insert(next, (shell, region.kind)).is_some()
                {
                    return None;
                }
                own_shells.push(next);
                next = shell.refs[2];
            }
            by_region.insert(region.attr, own_shells);
        }
        if regions
            .values()
            .any(|region| region.refs[1] == body_attr && !by_region.contains_key(&region.attr))
            || shells.values().any(|shell| {
                by_region.contains_key(&shell.refs[6]) && !by_shell.contains_key(&shell.attr)
            })
        {
            return None;
        }

        let mut back_faces = HashSet::new();
        let mut front_faces = HashSet::new();
        let mut partners = HashMap::new();
        for (&attr, &(shell, kind)) in &by_shell {
            let opposite = match kind {
                RegionKind::Solid => opposite_shell(shell, &faces, &mut back_faces, true)?,
                RegionKind::Void => opposite_shell(shell, &faces, &mut front_faces, false)?,
            };
            let &(_, opposite_kind) = by_shell.get(&opposite)?;
            if opposite_kind == kind {
                return None;
            }
            partners.insert(attr, opposite);
        }
        if back_faces != front_faces
            || faces.values().any(|face| {
                by_shell.contains_key(&face.refs[3]) && !back_faces.contains(&face.attr)
            })
            || partners
                .iter()
                .any(|(shell, other)| partners.get(other) != Some(shell))
        {
            return None;
        }
        let incomplete_regions: HashSet<_> = back_faces
            .into_iter()
            .filter(|attr| !face_stated(*attr))
            .map(|attr| by_shell[&faces[&attr].refs[3]].0.refs[6])
            .collect();

        let mut visited = HashSet::from([root.attr]);
        let mut pending = vec![(root.attr, None)];
        let mut exteriors = HashMap::new();
        while let Some((region, incoming)) = pending.pop() {
            let members = by_region.get(&region)?;
            for &shell in members {
                if incoming == Some(shell) {
                    continue;
                }
                let opposite = *partners.get(&shell)?;
                let &(target, kind) = by_shell.get(&opposite)?;
                let region = target.refs[6];
                if !visited.insert(region) {
                    return None;
                }
                if kind == RegionKind::Solid && !incomplete_regions.contains(&region) {
                    exteriors.insert(region, (opposite, by_region.get(&region)?.len()));
                }
                pending.push((region, Some(opposite)));
            }
        }
        (visited.len() == chain.len()).then_some(exteriors)
    }
}

fn opposite_shell(
    shell: &ShellNode,
    faces: &HashMap<u32, FaceNode>,
    visited: &mut HashSet<u32>,
    back: bool,
) -> Option<u32> {
    let (head, other_head) = if back {
        (shell.refs[3], shell.refs[7])
    } else {
        (shell.refs[7], shell.refs[3])
    };
    if other_head > 1 || shell.refs[4] > 1 || shell.refs[5] > 1 {
        return None;
    }
    let mut next = head;
    let mut previous = None;
    let mut opposite = None;
    while next > 1 {
        let face = faces.get(&next)?;
        let [front_next, front_prev, front_shell] = face.front?;
        let [following, preceding, owner, other] = if back {
            [face.refs[0], face.refs[1], face.refs[3], front_shell]
        } else {
            [front_next, front_prev, front_shell, face.refs[3]]
        };
        if owner != shell.attr
            || (preceding > 1).then_some(preceding) != previous
            || opposite.is_some_and(|expected| other != expected)
            || !visited.insert(next)
        {
            return None;
        }
        opposite = Some(other);
        previous = Some(next);
        next = following;
    }
    opposite
}
