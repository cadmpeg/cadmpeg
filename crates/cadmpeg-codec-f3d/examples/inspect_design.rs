use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::{codec::DecodeOptions, validate::validate, Codec};
use std::{env, fs::File};
fn main() {
    let mut file = File::open(env::args().nth(1).unwrap()).unwrap();
    let ir = F3dCodec
        .decode(&mut file, &DecodeOptions::default())
        .unwrap()
        .ir;
    let wanted: usize = ir
        .design_entity_headers
        .iter()
        .map(|e| e.reference_indices.len())
        .sum();
    println!(
        "wanted={wanted} indexed={} relations={} points={} curves={} exact_curves={} valid={}",
        ir.design_record_headers.len(),
        ir.sketch_relations.len(),
        ir.sketch_points.len(),
        ir.sketch_curve_identities.len(),
        ir.sketch_curve_identities
            .iter()
            .filter(|curve| curve.geometry.is_some())
            .count(),
        validate(&ir, Vec::new()).is_ok()
    );
    println!(
        "classes={:?}",
        ir.design_record_headers
            .iter()
            .fold(std::collections::BTreeMap::new(), |mut m, r| {
                *m.entry(r.class_tag.clone()).or_insert(0usize) += 1;
                m
            })
    );
    let mut offsets = ir
        .design_record_headers
        .iter()
        .map(|r| r.meta.provenance.offset)
        .collect::<Vec<_>>();
    offsets.sort_unstable();
    println!(
        "deltas={:?}",
        offsets
            .windows(2)
            .fold(std::collections::BTreeMap::new(), |mut m, pair| {
                *m.entry(pair[1] - pair[0]).or_insert(0usize) += 1;
                m
            })
    );
    for relation in &ir.sketch_relations {
        let at = relation.meta.provenance.offset;
        println!(
            "relation {} delta={:?}",
            relation.record_index,
            offsets
                .iter()
                .copied()
                .find(|offset| *offset > at)
                .map(|offset| offset - at)
        );
    }
}
