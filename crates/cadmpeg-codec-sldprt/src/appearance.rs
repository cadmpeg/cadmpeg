// SPDX-License-Identifier: Apache-2.0
//! `SolidWorks` visual-property records.

use cadmpeg_core::decode::View;
use cadmpeg_ir::topology::Color;

use crate::container::ContainerScan;

const TOKEN: &[u8] = b"moVisualProperties_c";

pub struct Material {
    pub name: String,
    pub color: Color,
    pub source_name: String,
    pub record_offset: usize,
}

pub fn materials(scan: &ContainerScan) -> Vec<Material> {
    let mut out = Vec::new();
    for section in scan.sections() {
        let bytes = section.payload();
        for token_at in bytes
            .windows(TOKEN.len())
            .enumerate()
            .filter_map(|(i, w)| (w == TOKEN).then_some(i))
        {
            let p = token_at + TOKEN.len();
            let Some(raw) = bytes.get(p..p + 4) else {
                continue;
            };
            let Some(packed) = View::u32_le_at(raw, 0) else {
                continue;
            };
            let name_header = p + 16;
            if bytes.get(name_header..name_header + 3) != Some(&[0xff, 0xfe, 0xff]) {
                continue;
            }
            let Some(length) = bytes.get(name_header + 3).copied().map(usize::from) else {
                continue;
            };
            let start = name_header + 4;
            let Some(raw_name) = bytes.get(start..start + length * 2) else {
                continue;
            };
            let mut view = View::over_retained(raw_name);
            let mut units = Vec::new();
            while let Some(unit) = view.u16_le() {
                units.push(unit);
            }
            let name = String::from_utf16_lossy(&units).trim().to_string();
            if name.is_empty() {
                continue;
            }
            out.push(Material {
                name,
                color: Color {
                    r: (packed & 0xff) as f32 / 255.0,
                    g: ((packed >> 8) & 0xff) as f32 / 255.0,
                    b: ((packed >> 16) & 0xff) as f32 / 255.0,
                    a: 1.0,
                },
                source_name: section.display_name(),
                record_offset: token_at,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests;
