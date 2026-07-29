// SPDX-License-Identifier: Apache-2.0
//! Zero-entity decode route for independently complete geometry carriers.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;

use crate::assemble::{annotate, link_payload_carriers, preserve_raw_payload, source_meta};
use crate::container::{self, ContainerScan};
use crate::families::FamilyOutput;

pub(crate) fn try_decode_zero_entity(scan: &ContainerScan) -> Option<FamilyOutput> {
    let surfaces = crate::families::zero_entity::records::zero_entity_surfaces(&scan.data);
    if surfaces.is_empty() {
        return None;
    }
    let ownership_root =
        crate::families::zero_entity::records::zero_entity_ownership_root(&scan.data);

    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(
        &mut unknowns,
        &mut annotations,
        scan,
        "catia:payload:unknown#zero-entity",
    );

    for (index, surface) in surfaces.into_iter().enumerate() {
        let id = SurfaceId(format!("catia:zero-entity:surf#{index}"));
        annotate(
            &mut annotations,
            &id,
            "zero_entity_a9_03",
            surface.pos as u64,
            "analytic_surface",
            Exactness::ByteExact,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: surface.geometry,
            source_object: None,
        });
    }

    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    Some(FamilyOutput {
        ir,
        report: DecodeReport {
            format: "catia".to_string(),
            container_only: false,
            geometry_transferred: true,
            coverage: std::collections::BTreeMap::new(),
            losses: vec![LossNote {
                code: cadmpeg_ir::report::LossCode::TopologyNotTransferred,
                category: LossCategory::Topology,
                severity: Severity::Blocking,
                message: if ownership_root.is_some() {
                    "Zero-entity loop members bind their face-local support occurrences and the terminal ownership root binds the complete face roster through one shell and body, but support-to-oriented-use, oriented-use-to-incidence, and physical endpoint bindings remain unresolved; no neutral topology was transferred."
                } else {
                    "Zero-entity loop members bind their face-local support occurrences, but support-to-oriented-use, oriented-use-to-incidence, physical endpoint, and body/shell bindings remain unresolved; no neutral topology was transferred."
                }
                .to_string(),
                provenance: None,
            }],
            notes: container::summarize(scan).notes,
        },
        annotations: annotations.build(),
        unknowns,
    })
}
