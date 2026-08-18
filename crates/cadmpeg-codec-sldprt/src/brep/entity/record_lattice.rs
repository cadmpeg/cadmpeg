use cadmpeg_ir::topology::BodyKind;
use std::collections::{HashMap, HashSet};

use super::{BodyRecord, EntityRecord, RegionRecord, ShellRecord};

#[derive(Clone, Copy)]
enum ChainTerminal {
    Sentinel,
    Missing,
    Record { disc: u16, flo: u8 },
}

#[derive(Clone, Copy)]
enum FaceLink {
    Reciprocal,
    ReciprocalOrBridge { disc: u16, flo: u8 },
    SharedUse,
}

#[derive(Clone, Copy)]
struct Options<'a> {
    chain_shape: &'a [(u16, u8)],
    terminal: ChainTerminal,
    canonical_disc: u16,
    companion_disc: u16,
    use_disc: u16,
    shell_index: usize,
    link: FaceLink,
}

pub(super) fn disc1e_disc1c_disc1a_disc18_disc16_shared_use_root_body(
    entities: &[EntityRecord],
) -> Vec<BodyRecord> {
    root_body_from_lattice(
        entities,
        Options {
            chain_shape: &[
                (0x001e, 2),
                (0x001c, 2),
                (0x001a, 2),
                (0x0018, 2),
                (0x0016, 2),
            ],
            terminal: ChainTerminal::Record {
                disc: 0x0014,
                flo: 2,
            },
            canonical_disc: 0x000e,
            companion_disc: 0x0010,
            use_disc: 0x0020,
            shell_index: 4,
            link: FaceLink::SharedUse,
        },
    )
}

pub(super) fn disc1e_disc1c_disc18_disc10_reciprocal_root_body(
    entities: &[EntityRecord],
) -> Vec<BodyRecord> {
    root_body_from_lattice(
        entities,
        Options {
            chain_shape: &[(0x001e, 2), (0x001c, 2), (0x0018, 2), (0x0010, 2)],
            terminal: ChainTerminal::Sentinel,
            canonical_disc: 0x0012,
            companion_disc: 0x001a,
            use_disc: 0x0020,
            shell_index: 2,
            link: FaceLink::Reciprocal,
        },
    )
}

pub(super) fn disc20_disc1e_disc1c_disc18_disc16_disc12_root_body(
    entities: &[EntityRecord],
) -> Vec<BodyRecord> {
    root_body_from_lattice(
        entities,
        Options {
            chain_shape: &[
                (0x0020, 2),
                (0x001e, 2),
                (0x001c, 2),
                (0x0018, 2),
                (0x0016, 1),
                (0x0012, 2),
            ],
            terminal: ChainTerminal::Missing,
            canonical_disc: 0x0010,
            companion_disc: 0x001a,
            use_disc: 0x0022,
            shell_index: 4,
            link: FaceLink::ReciprocalOrBridge {
                disc: 0x0014,
                flo: 2,
            },
        },
    )
}

fn chain_candidates<'a>(
    entities: &'a [EntityRecord],
    chain_shape: &[(u16, u8)],
    terminal: ChainTerminal,
) -> Vec<Vec<&'a EntityRecord>> {
    let Some(&(root_disc, root_flo)) = chain_shape.first() else {
        return Vec::new();
    };
    let mut by_attr = HashMap::<u16, Vec<&EntityRecord>>::new();
    for record in entities {
        by_attr.entry(record.attr).or_default().push(record);
    }
    let mut candidates = Vec::new();
    let mut seen_sequences = HashSet::new();
    for root in entities.iter().filter(|record| {
        record.disc == root_disc
            && record.flo() == root_flo
            && record.refs.first().is_some_and(|key| *key > 1)
            && record.refs.get(1).is_none_or(|attr| *attr <= 1)
            && record.refs.get(2).is_some_and(|attr| *attr > 1)
    }) {
        let key = root.refs[0];
        let mut pending = vec![(vec![root], root.refs[2])];
        while let Some((chain, next)) = pending.pop() {
            if chain.len() == chain_shape.len() {
                let Some(last) = chain.last().copied() else {
                    continue;
                };
                let terminal_matches = match terminal {
                    ChainTerminal::Sentinel => next <= 1,
                    ChainTerminal::Missing => {
                        next > 1
                            && !by_attr.get(&next).is_some_and(|records| {
                                records.iter().any(|record| {
                                    record.refs.first() == Some(&key)
                                        && record.refs.get(1) == Some(&last.attr)
                                })
                            })
                    }
                    ChainTerminal::Record { disc, flo } => {
                        next > 1
                            && by_attr.get(&next).is_some_and(|records| {
                                records.iter().any(|record| {
                                    record.disc == disc
                                        && record.flo() == flo
                                        && record.refs.first() == Some(&key)
                                        && record.refs.get(1) == Some(&last.attr)
                                })
                            })
                    }
                };
                if terminal_matches {
                    let sequence = chain.iter().map(|record| record.attr).collect::<Vec<_>>();
                    if seen_sequences.insert(sequence) {
                        candidates.push(chain);
                    }
                }
                continue;
            }
            let Some(successors) = by_attr.get(&next) else {
                continue;
            };
            let (disc, flo) = chain_shape[chain.len()];
            let previous = chain
                .last()
                .expect("a nonempty chain has a predecessor")
                .attr;
            for successor in successors.iter().copied().filter(|record| {
                record.disc == disc
                    && record.flo() == flo
                    && record.refs.first() == Some(&key)
                    && record.refs.get(1) == Some(&previous)
                    && !chain.iter().any(|current| current.attr == record.attr)
            }) {
                let mut next_chain = chain.clone();
                next_chain.push(successor);
                let next = successor.refs.get(2).copied().unwrap_or(0);
                pending.push((next_chain, next));
            }
        }
    }
    candidates
}

fn root_body_from_lattice(entities: &[EntityRecord], options: Options<'_>) -> Vec<BodyRecord> {
    let matching_chains = chain_candidates(entities, options.chain_shape, options.terminal);
    let [chain] = matching_chains.as_slice() else {
        return Vec::new();
    };
    let Some(root) = chain.first().copied() else {
        return Vec::new();
    };
    let Some(shell) = chain.get(options.shell_index).copied() else {
        return Vec::new();
    };
    let records_by_key = |disc: u16, flo: u8| {
        let mut records = HashMap::<u16, Vec<&EntityRecord>>::new();
        for record in entities.iter().filter(|record| {
            record.disc == disc
                && record.flo() == flo
                && record.refs.first().is_some_and(|key| *key > 1)
        }) {
            records.entry(record.refs[0]).or_default().push(record);
        }
        records
    };
    let canonical = records_by_key(options.canonical_disc, 1);
    let companions = records_by_key(options.companion_disc, 1);
    let uses = records_by_key(options.use_disc, 4);
    let selected_keys = canonical
        .keys()
        .copied()
        .filter(|key| companions.contains_key(key) && uses.contains_key(key))
        .collect::<HashSet<_>>();
    if selected_keys.is_empty() {
        return Vec::new();
    }
    for key in &selected_keys {
        let mut links = HashSet::<(u16, u16)>::new();
        for face in &canonical[key] {
            for companion in &companions[key] {
                for use_node in &uses[key] {
                    let valid = match options.link {
                        FaceLink::Reciprocal => {
                            face.refs.get(1) == Some(&companion.attr)
                                && companion.refs.get(2) == Some(&face.attr)
                                && companion.refs.get(1) == Some(&use_node.attr)
                                && use_node.refs.get(2) == Some(&companion.attr)
                        }
                        FaceLink::ReciprocalOrBridge { disc, flo } => {
                            let direct = face.refs.get(1) == Some(&companion.attr)
                                && companion.refs.get(2) == Some(&face.attr);
                            let bridge_attrs = entities
                                .iter()
                                .filter(|bridge| {
                                    bridge.disc == disc
                                        && bridge.flo() == flo
                                        && bridge.refs.first() == Some(key)
                                        && face.refs.get(1) == Some(&bridge.attr)
                                        && bridge.refs.get(2) == Some(&face.attr)
                                        && companion.refs.get(2) == Some(&bridge.attr)
                                })
                                .map(|bridge| bridge.attr)
                                .collect::<HashSet<_>>();
                            (usize::from(direct) + bridge_attrs.len() == 1)
                                && companion.refs.get(1) == Some(&use_node.attr)
                                && use_node.refs.get(2) == Some(&companion.attr)
                        }
                        FaceLink::SharedUse => {
                            face.refs.get(1) == Some(&use_node.attr)
                                && companion.refs.get(1) == Some(&use_node.attr)
                                && companion.refs.get(2).is_none_or(|attr| *attr <= 1)
                                && use_node.refs.get(2) == Some(&companion.attr)
                        }
                    };
                    if valid {
                        links.insert((companion.attr, use_node.attr));
                    }
                }
            }
        }
        if links.len() != 1 {
            return Vec::new();
        }
    }
    let mut refs = entities
        .iter()
        .map(|record| record.attr)
        .chain(selected_keys.iter().copied())
        .collect::<Vec<_>>();
    refs.sort_unstable();
    refs.dedup();
    vec![BodyRecord {
        attr: root.attr,
        kind: BodyKind::Solid,
        refs: refs.clone(),
        offset: root.offset,
        regions: vec![RegionRecord {
            attr: root.attr,
            offset: root.offset,
            shells: vec![ShellRecord {
                attr: shell.attr,
                offset: shell.offset,
                refs,
            }],
        }],
    }]
}
