// SPDX-License-Identifier: Apache-2.0
//! IGES B-rep loop ordering and orientations.

use cadmpeg_ir::topology::{PcurveUse, Sense};

pub(super) fn push_vertex_list(
    ir: &cadmpeg_ir::CadIr,
    entities: &mut Vec<super::Entity>,
    mut entity: super::Entity,
    source: Option<String>,
    counts: &std::collections::BTreeMap<String, usize>,
) {
    if let Some(source) = source.filter(|source| counts.get(source) == Some(&1)) {
        super::native_records::numeric::retain_directory(ir, &source, &mut entity);
        entity.source_targets.push(source);
    }
    entities.push(entity);
}

fn directory_source(
    ir: &cadmpeg_ir::CadIr,
    sequence: u32,
    type_code: i64,
) -> Option<(String, String)> {
    let id = format!("iges:entity:directory#{sequence}");
    let record = ir
        .native
        .namespace("iges")?
        .arenas()
        .get("entities")?
        .iter()
        .find(|record| record.id() == id)?;
    if record.field("entity_type")?.as_i64()? != type_code {
        return None;
    }
    let bytes: cadmpeg_ir::native::bytes::NativeBytes =
        serde_json::from_value(record.field("label")?).ok()?;
    let label = std::str::from_utf8(bytes.as_ref()).ok()?;
    if label.len() > 8
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' ')
    {
        return None;
    }
    Some((id, label.trim().to_owned()))
}

fn apply_topology_source(
    ir: &cadmpeg_ir::CadIr,
    id: &str,
    type_code: i64,
    entity: &mut super::Entity,
) {
    let prefix = match type_code {
        508 => "iges:model:loop#",
        510 => "iges:model:face#",
        514 => "iges:model:shell#",
        _ => return,
    };
    let sequence = id
        .strip_prefix(prefix)
        .and_then(|stem| stem.rsplit(':').next())
        .and_then(|stem| stem.strip_prefix('D'))
        .and_then(|value| value.parse().ok());
    if let Some((_, label)) =
        sequence.and_then(|sequence| directory_source(ir, sequence, type_code))
    {
        entity.label = label;
    }
}

pub(super) fn apply_topology_sources(ir: &cadmpeg_ir::CadIr, entities: &mut [super::Entity]) {
    for entity in entities {
        let prefix = match entity.type_code {
            508 => "iges:model:loop#",
            510 => "iges:model:face#",
            514 => "iges:model:shell#",
            _ => continue,
        };
        if let Some(id) = entity
            .source_targets
            .iter()
            .find(|id| id.starts_with(prefix))
            .cloned()
        {
            apply_topology_source(ir, &id, i64::from(entity.type_code), entity);
        }
    }
}

pub(super) fn edge_list_source(
    ir: &cadmpeg_ir::CadIr,
    edges: &[String],
) -> Option<(String, String)> {
    let mut source = None;
    for (index, edge) in edges.iter().enumerate() {
        let mut parts = edge.strip_prefix("iges:model:edge#")?.rsplit(':');
        if parts.next()?.parse::<usize>().ok()? != index + 1 {
            return None;
        }
        let sequence = parts.next()?.strip_prefix('D')?.parse::<u32>().ok()?;
        if source.is_some_and(|previous| previous != sequence) {
            return None;
        }
        source = Some(sequence);
    }
    let (id, label) = directory_source(ir, source?, 504)?;
    let record = ir
        .native
        .namespace("iges")?
        .arenas()
        .get("entities")?
        .iter()
        .find(|record| record.id() == id)?;
    if record.field("entity_type")?.as_i64()? != 504 || record.field("form")?.as_i64()? != 1 {
        return None;
    }
    let parameters = record.field("parameters")?;
    let parameters = parameters.as_array()?;
    if parameters.get(1)?.get("value")?.get("value")?.as_u64()?
        != u64::try_from(edges.len()).ok()?
    {
        return None;
    }
    Some((id, label))
}

/// The Type 508 edge-use orientation of a coedge. A Type 508 loop keeps its
/// face on the left about the support normal, and a reversed face keeps it on
/// the left about the reversed normal, so a reversed face writes its loops
/// backward.
pub(super) fn edge_use_sense(coedge: Sense, face: Sense) -> Sense {
    if coedge == face {
        Sense::Forward
    } else {
        Sense::Reversed
    }
}

/// The pcurve uses of a pole-vertex use in Type 508 order. A reversed face
/// writes its loops backward, so it writes the uses in reverse order.
pub(super) fn pole_use_order(uses: &[PcurveUse], face: Sense) -> Vec<&PcurveUse> {
    match face {
        Sense::Forward => uses.iter().collect(),
        Sense::Reversed => uses.iter().rev().collect(),
    }
}

/// The ring positions of a loop in Type 508 edge-use order.
pub(super) fn edge_use_positions(count: usize, face: Sense) -> Vec<usize> {
    match face {
        Sense::Forward => (0..count).collect(),
        Sense::Reversed => (0..count).rev().collect(),
    }
}

pub(super) fn brep_sense(sense: Sense) -> i32 {
    match sense {
        Sense::Forward => 1,
        Sense::Reversed => 0,
    }
}
