// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::topology::BodyKind;

use super::{region_chain, FaceNode, Facts, RegionKind, ShellNode};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::brep) enum ShellRole {
    Exterior,
    Cavity,
}

pub(in crate::brep) struct ShellEvidence {
    pub(in crate::brep) shells: Vec<(u32, Option<ShellRole>)>,
    pub(in crate::brep) complete: bool,
}

impl Facts {
    pub(in crate::brep) fn shell_evidence(&self) -> HashMap<u32, ShellEvidence> {
        let Some((bodies, regions, shells, faces)) = self.ownership_maps() else {
            return HashMap::new();
        };
        let mut shell_counts = HashMap::<u32, usize>::new();
        for shell in &self.shells {
            *shell_counts.entry(shell.refs[6]).or_default() += 1;
        }
        let mut evidence = HashMap::new();
        for region in regions.values() {
            let mut members = Vec::new();
            let mut visited = HashSet::new();
            let mut next = region.refs[4];
            while next > 1 && visited.insert(next) {
                let Some(shell) = shells
                    .get(&next)
                    .filter(|shell| shell.refs[6] == region.attr)
                else {
                    break;
                };
                members.push((next, None));
                next = shell.refs[2];
            }
            evidence.insert(
                region.attr,
                ShellEvidence {
                    complete: next <= 1
                        && members.len()
                            == shell_counts.get(&region.attr).copied().unwrap_or_default(),
                    shells: members,
                },
            );
        }

        let mut fronts = HashMap::new();
        let mut front_claims = HashMap::<u32, Option<u32>>::new();
        let mut back_counts = HashMap::<u32, usize>::new();
        for face in faces.values() {
            *back_counts.entry(face.refs[3]).or_default() += 1;
        }
        for shell in shells
            .values()
            .filter(|shell| regions[&shell.refs[6]].kind == RegionKind::Void)
        {
            if let Some(members) = face_chain(shell, &faces, false) {
                for &face in &members {
                    front_claims
                        .entry(face)
                        .and_modify(|claim| *claim = None)
                        .or_insert(Some(shell.attr));
                }
                fronts.insert(shell.attr, members);
            }
        }

        let mut partners = HashMap::new();
        for shell in shells
            .values()
            .filter(|shell| regions[&shell.refs[6]].kind == RegionKind::Solid)
        {
            let Some(back) = face_chain(shell, &faces, true) else {
                continue;
            };
            if back_counts.get(&shell.attr) != Some(&back.len()) {
                continue;
            }
            let Some(opposite) = back
                .iter()
                .next()
                .and_then(|face| front_claims.get(face))
                .copied()
                .flatten()
            else {
                continue;
            };
            if fronts.get(&opposite) == Some(&back)
                && back
                    .iter()
                    .all(|face| front_claims.get(face) == Some(&Some(opposite)))
                && regions[&shell.refs[6]].refs[1] == regions[&shells[&opposite].refs[6]].refs[1]
            {
                partners.insert(shell.attr, opposite);
                partners.insert(opposite, shell.attr);
            }
        }

        let mut region_counts = HashMap::<u32, usize>::new();
        for region in regions.values() {
            *region_counts.entry(region.refs[1]).or_default() += 1;
        }
        let mut roles = HashMap::new();
        for body in bodies
            .values()
            .filter(|body| matches!(body.kind, BodyKind::Solid | BodyKind::General))
        {
            let Some(chain) = region_chain(body, &regions) else {
                continue;
            };
            let Some(root) = chain
                .first()
                .filter(|region| region.kind == RegionKind::Void)
            else {
                continue;
            };
            let members: HashSet<_> = chain.iter().map(|region| region.attr).collect();
            let mut direct = HashMap::<u32, Option<u32>>::new();
            for (shell, _) in &evidence[&root.attr].shells {
                if let Some(&opposite) = partners.get(shell) {
                    let region = shells[&opposite].refs[6];
                    if members.contains(&region) {
                        direct
                            .entry(region)
                            .and_modify(|slot| *slot = None)
                            .or_insert(Some(opposite));
                    }
                }
            }
            if body.kind == BodyKind::Solid {
                roles.extend(
                    direct
                        .into_values()
                        .flatten()
                        .map(|shell| (shell, ShellRole::Exterior)),
                );
            }

            let mut visited = HashSet::from([root.attr]);
            let mut pending = vec![(root.attr, None)];
            let mut indirect = HashMap::new();
            let mut tree = true;
            while let Some((region, incoming)) = pending.pop() {
                for &(shell, _) in &evidence[&region].shells {
                    if incoming == Some(shell) {
                        continue;
                    }
                    let Some(&opposite) = partners.get(&shell) else {
                        continue;
                    };
                    let target = shells[&opposite].refs[6];
                    if !members.contains(&target) || !visited.insert(target) {
                        tree = false;
                        continue;
                    }
                    match regions[&region].kind {
                        RegionKind::Solid => {
                            indirect.insert(shell, ShellRole::Cavity);
                        }
                        RegionKind::Void => {
                            indirect.insert(opposite, ShellRole::Exterior);
                        }
                    }
                    pending.push((target, Some(opposite)));
                }
            }
            let complete = region_counts.get(&body.attr) == Some(&visited.len())
                && chain.iter().all(|region| {
                    let source = &evidence[&region.attr];
                    source.complete
                        && source
                            .shells
                            .iter()
                            .all(|(shell, _)| partners.contains_key(shell))
                });
            if tree && (body.kind == BodyKind::Solid || complete) {
                roles.extend(indirect);
            }
        }
        for source in evidence.values_mut() {
            for (attr, role) in &mut source.shells {
                *role = roles.get(attr).copied();
            }
        }
        evidence
    }
}

fn face_chain(
    shell: &ShellNode,
    faces: &HashMap<u32, FaceNode>,
    back: bool,
) -> Option<HashSet<u32>> {
    let (head, other_head) = if back {
        (shell.refs[3], shell.refs[7])
    } else {
        (shell.refs[7], shell.refs[3])
    };
    if head <= 1 || other_head > 1 || shell.refs[4] > 1 || shell.refs[5] > 1 {
        return None;
    }
    let mut visited = HashSet::new();
    let mut next = head;
    let mut previous = None;
    while next > 1 {
        let face = faces.get(&next)?;
        let [following, preceding, owner] = if back {
            [face.refs[0], face.refs[1], face.refs[3]]
        } else {
            face.front?
        };
        // A complete front chain can supply the owner of a null front-shell cell.
        if (owner != shell.attr && (back || owner > 1))
            || (preceding > 1).then_some(preceding) != previous
            || !visited.insert(next)
        {
            return None;
        }
        previous = Some(next);
        next = following;
    }
    Some(visited)
}
