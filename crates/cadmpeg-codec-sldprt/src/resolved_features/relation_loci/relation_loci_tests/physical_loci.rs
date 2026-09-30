use super::super::canonicalize_physical_loci;
use super::line_entity;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchEntityId, SketchId, SketchLocus};

const EPS_PHYSICAL_LOCUS_QUANTIZATION: f64 = 1.0e-8;

#[test]
fn coincident_physical_loci_keep_the_smallest_identity_and_role() {
    let sketch = SketchId::mint("synthetic:test:id#physical").unwrap();
    let first = line_entity(
        "synthetic:test:id#first",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
    );
    let second = line_entity(
        "synthetic:test:id#second",
        &sketch,
        Point2::new(1.0, 0.0),
        Point2::new(2.0, 0.0),
    );
    let mut loci = vec![
        SketchLocus::Start(second.id().clone()),
        SketchLocus::End(first.id().clone()),
    ];
    let expected = vec![SketchLocus::End(first.id().clone())];
    canonicalize_physical_loci(
        &cadmpeg_test_support::service_decode_context(),
        &mut loci,
        &[first, second],
        EPS_PHYSICAL_LOCUS_QUANTIZATION,
    )
    .unwrap();
    assert_eq!(loci, expected);
}

#[test]
fn distinct_physical_loci_preserve_input_order() {
    let sketch = SketchId::mint("synthetic:test:id#physical").unwrap();
    let line = line_entity(
        "synthetic:test:id#line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
    );
    let mut loci = vec![
        SketchLocus::End(line.id().clone()),
        SketchLocus::Start(line.id().clone()),
    ];
    let expected = loci.clone();
    canonicalize_physical_loci(
        &cadmpeg_test_support::service_decode_context(),
        &mut loci,
        &[line],
        EPS_PHYSICAL_LOCUS_QUANTIZATION,
    )
    .unwrap();
    assert_eq!(loci, expected);
}

#[test]
fn unresolved_physical_loci_preserve_input_order() {
    let sketch = SketchId::mint("synthetic:test:id#physical").unwrap();
    let line = line_entity(
        "synthetic:test:id#line",
        &sketch,
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
    );
    let mut loci = vec![
        SketchLocus::End(line.id().clone()),
        SketchLocus::Start(SketchEntityId::mint("synthetic:test:id#absent").unwrap()),
    ];
    let expected = loci.clone();
    canonicalize_physical_loci(
        &cadmpeg_test_support::service_decode_context(),
        &mut loci,
        &[line],
        EPS_PHYSICAL_LOCUS_QUANTIZATION,
    )
    .unwrap();
    assert_eq!(loci, expected);
}
