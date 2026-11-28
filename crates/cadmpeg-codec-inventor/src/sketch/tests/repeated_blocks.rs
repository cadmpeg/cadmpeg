// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::unique_index;

use super::super::place_block_curves;
use super::{block_definition, block_instance, half_disc_inventory, real, PmDcSketchEntityKind};

pub(crate) fn repeated_block_lines(
    sketch: &cadmpeg_ir::sketches::SketchId,
) -> Vec<cadmpeg_ir::sketches::SketchEntity> {
    let mut inventory = half_disc_inventory();
    inventory.entities = block_definition();
    for (index, u) in [(2, 0.0), (3, 0.5)] {
        inventory.entities[index].edit(|record| {
            let PmDcSketchEntityKind::Point { position, .. } = &mut record.kind else {
                panic!("definition point");
            };
            *position = [real(u), real(0.0)];
        });
    }
    inventory.entities[4].edit(|record| {
        let PmDcSketchEntityKind::Line {
            origin, direction, ..
        } = &mut record.kind
        else {
            panic!("definition line");
        };
        *origin = [real(0.0), real(0.0)];
        *direction = [real(0.5), real(0.0)];
    });
    let entities = unique_index(inventory.entities.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    [(9, 0.0), (10, 1.0)]
        .into_iter()
        .flat_map(|(ordinal, translation)| {
            let instance = block_instance(ordinal, [0x0111, 0x00ca], &[translation, 0.0]);
            place_block_curves(&instance, sketch, &inventory, &entities).expect("placed definition")
        })
        .filter(|entity| {
            matches!(
                entity.geometry.definition(),
                cadmpeg_ir::sketches::SketchGeometryDefinition::Line { .. }
            )
        })
        .collect()
}
