use super::*;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SketchFeatureBinding};

pub(super) fn planar_position_source(geometry: bool) -> Vec<u8> {
    let mut source = hole_ownership_source(geometry);
    // A native profile declaration and name marker identify an empty sketch.
    // Marker projection retains it with a planar binding and unresolved frame.
    let resolved = crate::test_support::history::resolved_feature_classes_with_ids(&[
        ("moHoleWzd_c", "Hole", 7),
        ("moProfileFeature_c", "Profile", 8),
        ("moProfileFeature_c", "Position", 9),
    ]);
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &resolved,
    ));
    source
}

#[test]
fn resolved_position_name_retains_a_planar_sketch_in_both_decode_routes() {
    for geometry in [false, true] {
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(planar_position_source(geometry)),
                &DecodeOptions {
                    container_only: !geometry,
                    ..DecodeOptions::default()
                },
            )
            .expect("native profile declarations are supported in both routes");
        let ir = decoded.ir();
        let feature = ir.model.features.iter()
            .find(|feature| feature.name.as_deref() == Some("Position"))
            .expect("the keyword Position retains its model feature");
        let FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(id)),
        }) = feature.evaluation.definition() else {
            panic!("the native Position object must bind to its planar sketch");
        };
        assert!(feature.native_ref.is_some());
        let sketch = ir.model.sketches.iter().find(|sketch| &sketch.id == id)
            .expect("the planar binding names a retained sketch");
        assert_eq!(sketch.name.as_deref(), Some("Position"));
        assert!(sketch.native_ref.is_some());
        assert!(sketch.resolved_placement().is_none());
        assert!(!ir.model.sketch_entities.iter().any(|entity| &entity.sketch == id));
    }
}

#[test]
fn keyword_only_position_preserves_the_missing_sketch_route() {
    for geometry in [false, true] {
        let decoded = SldprtCodec
            .decode(
                &mut Cursor::new(hole_ownership_source(geometry)),
                &DecodeOptions {
                    container_only: !geometry,
                    ..DecodeOptions::default()
                },
            )
            .expect("a keyword-only Position remains recoverable");
        let ir = decoded.ir();
        assert!(ir.model.sketches.is_empty());
        let feature = ir.model.features.iter()
            .find(|feature| feature.name.as_deref() == Some("Position"))
            .expect("the keyword Position retains its model feature");
        assert!(matches!(feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(None),
            })
        ));
    }
}
