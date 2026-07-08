// SPDX-License-Identifier: Apache-2.0
//! Typed views over SolidWorks ResolvedFeatures sketch records.

use cadmpeg_ir::history::{FeatureInputLane, SketchInputEntity, SketchInputKind};
use cadmpeg_ir::provenance::{EntityMeta, Exactness, Provenance};

use crate::container::ContainerScan;

const SKETCH_MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x03];

pub fn lanes(scan: &ContainerScan) -> Vec<FeatureInputLane> {
    scan.blocks
        .iter()
        .filter_map(|block| {
            let section = block.section.as_deref()?;
            if !section.to_ascii_lowercase().contains("resolvedfeatures") {
                return None;
            }
            let meta = |offset, tag: &str| EntityMeta {
                provenance: Provenance {
                    format: "sldprt".into(),
                    stream: section.into(),
                    offset,
                    tag: Some(tag.into()),
                },
                exactness: Exactness::ByteExact,
            };
            let sketch_entities = block
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
                .map(|(ordinal, (offset, code))| SketchInputEntity {
                    ordinal: ordinal as u32,
                    offset: offset as u64,
                    kind: SketchInputKind::from_native_code(code),
                    meta: meta(offset as u64, "ff_ff_1f_00_03"),
                })
                .collect();
            Some(FeatureInputLane {
                id: format!("sldprt:resolved-features:{}", block.offset),
                configuration: configuration(section),
                native_payload: block.payload.clone(),
                sketch_entities,
                meta: meta(0, "ResolvedFeatures"),
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
