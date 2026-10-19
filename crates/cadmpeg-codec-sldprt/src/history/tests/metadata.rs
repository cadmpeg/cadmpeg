use super::feature;
use crate::history::classify::HistoryMetadata;
use crate::records::{Feature, FeatureSource};

#[test]
fn history_metadata_matches_the_attribute_scan() {
    fn text(next: &mut impl FnMut(u64) -> usize, length: u64) -> String {
        (0..next(length)).map(|_| ['a', 'b'][next(2)]).collect()
    }

    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from((state >> 33) % bound).unwrap()
    };
    let without_attributes = HistoryMetadata::new(&[]);
    let mut attribute_named = 0;
    for _ in 0..256 {
        let features = (0..next(16))
            .map(|ordinal| {
                let source = [None, Some("-1"), Some("7")][next(3)];
                let mut record = feature(&format!("record-{ordinal}"), source, 0);
                record.name = text(&mut next, 4);
                record.input_class =
                    [None, Some("moAttribute_c"), Some("moSketch_c")][next(3)].map(str::to_owned);
                if next(4) == 0 {
                    record.kind = "Attribute-Definition".into();
                }
                record
            })
            .collect::<Vec<Feature>>();
        let metadata = HistoryMetadata::new(&features);
        for record in &features {
            let named_by_attribute = record.input_class.is_none()
                && record.source_id == Some(FeatureSource::Reserved)
                && !record.name.is_empty()
                && features.iter().any(|candidate| {
                    candidate.input_class.as_deref() == Some("moAttribute_c")
                        && candidate.name.starts_with(&record.name)
                });
            attribute_named += usize::from(named_by_attribute);
            assert_eq!(
                metadata.contains(record),
                without_attributes.contains(record) || named_by_attribute
            );
        }
    }
    assert!(attribute_named > 0);
}
