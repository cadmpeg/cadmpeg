// SPDX-License-Identifier: Apache-2.0
//! Typed views over `SolidWorks` `ResolvedFeatures` sketch records.

use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::Exactness;

use crate::container::ContainerScan;

const SKETCH_MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x03];

pub fn lanes(scan: &ContainerScan, annotations: &mut Annotations) -> Vec<FeatureInputLane> {
    scan.blocks
        .iter()
        .filter_map(|block| {
            let section = block.section.as_deref()?;
            if !section.to_ascii_lowercase().contains("resolvedfeatures") {
                return None;
            }
            let parent = format!("sldprt:feature-input:resolved-features#{}", block.offset);
            let mut sketch_entities = block
                .payload
                .windows(SKETCH_MARKER.len())
                .enumerate()
                .filter_map(|(offset, bytes)| (bytes == SKETCH_MARKER).then_some(offset))
                .filter_map(|offset| {
                    let code = u32::from_le_bytes(
                        block
                            .payload
                            .get(offset + 17..offset + 21)?
                            .try_into()
                            .ok()?,
                    );
                    Some((offset, code))
                })
                .enumerate()
                .map(|(ordinal, (offset, code))| {
                    let id = format!(
                        "sldprt:feature-input:sketch-entity#{}:{offset}",
                        block.offset
                    );
                    crate::annotations::note(
                        annotations,
                        id.clone(),
                        section,
                        offset as u64,
                        "ff_ff_1f_00_03",
                        Exactness::ByteExact,
                    );
                    SketchInputEntity {
                        id,
                        parent: parent.clone(),
                        ordinal: ordinal as u32,
                        offset: offset as u64,
                        kind: SketchInputKind::from_native_code(code),
                    }
                })
                .collect::<Vec<_>>();
            sketch_entities.sort_by(|a, b| a.id.cmp(&b.id));
            let id = parent;
            crate::annotations::note(
                annotations,
                id.clone(),
                section,
                0,
                "ResolvedFeatures",
                Exactness::ByteExact,
            );
            Some(FeatureInputLane {
                id,
                configuration: configuration(section),
                native_payload: block.payload.clone(),
                sketch_entities,
            })
        })
        .collect()
}

fn configuration(section: &str) -> Option<String> {
    let start = section.find("Config-")? + "Config-".len();
    let tail = &section[start..];
    let end = tail
        .find("-ResolvedFeatures")
        .or_else(|| tail.find('/'))
        .unwrap_or(tail.len());
    (!tail[..end].is_empty()).then(|| tail[..end].to_string())
}
