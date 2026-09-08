use super::{DesignDimensionLocusPairs, DesignDimensionNullLocusPairs};
use crate::native::F3dNative;
use crate::records::DesignDimensionLocusPair;

fn pair(null_first: bool) -> DesignDimensionLocusPair {
    let shared = r#""id":"pair","companion_record_index":1,"governing_companion_record_index":2,"byte_offset":10,"class_tag":"274","record_index":3,"frame_length":80"#;
    let (opaque, first) = if null_first {
        ("", 0)
    } else {
        (r#","opaque_index":4,"opaque_index_offset":45"#, 40)
    };
    let wire = format!(
        r#"{{{shared}{opaque},"first_geometry_record_index":{first},"first_geometry_reference_offset":50,"first_role":0,"first_role_offset":60,"second_geometry_record_index":41,"second_geometry_reference_offset":65,"second_role":1,"second_role_offset":75,"paired_class_tag":"273","paired_byte_offset":90}}"#
    );
    serde_json::from_str(&wire).unwrap()
}

#[test]
fn nonnull_arena_rejects_null_form_at_construction_and_deserialization() {
    let null_pair = pair(true);
    assert!(DesignDimensionLocusPairs::try_from(vec![null_pair.clone()]).is_err());
    let entry = serde_json::to_string(&null_pair).unwrap();
    let wire = format!(r#"{{"design_dimension_locus_pairs":[{entry}]}}"#);
    let error = serde_json::from_str::<F3dNative>(&wire).unwrap_err();
    assert!(error.to_string().contains("design_dimension_locus_pairs"));
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    namespace
        .set_arena("design_dimension_locus_pairs", &[null_pair])
        .unwrap();
    assert!(F3dNative::load(&namespace).is_err());

    let mut missing_opaque = pair(false);
    missing_opaque.opaque_index = None;
    assert!(DesignDimensionLocusPairs::try_from(vec![missing_opaque]).is_err());
    let mut missing_second = pair(false);
    missing_second.loci[1].geometry_record_index = None;
    assert!(DesignDimensionLocusPairs::try_from(vec![missing_second]).is_err());
}

#[test]
fn null_arena_rejects_nonnull_form_at_construction_and_deserialization() {
    let nonnull_pair = pair(false);
    assert!(DesignDimensionNullLocusPairs::try_from(vec![nonnull_pair.clone()]).is_err());
    let entry = serde_json::to_string(&nonnull_pair).unwrap();
    let wire = format!(r#"{{"design_dimension_null_locus_pairs":[{entry}]}}"#);
    assert!(serde_json::from_str::<F3dNative>(&wire).is_err());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    namespace
        .set_arena("design_dimension_null_locus_pairs", &[nonnull_pair])
        .unwrap();
    assert!(F3dNative::load(&namespace).is_err());

    let mut stray_opaque = pair(true);
    stray_opaque.opaque_index = pair(false).opaque_index;
    assert!(DesignDimensionNullLocusPairs::try_from(vec![stray_opaque]).is_err());
    let mut missing_second = pair(true);
    missing_second.loci[1].geometry_record_index = None;
    assert!(DesignDimensionNullLocusPairs::try_from(vec![missing_second]).is_err());
}

#[test]
fn nonnull_arena_preserves_pair_wire() {
    let pair = pair(false);
    let entry = serde_json::to_string(&pair).unwrap();
    let native = F3dNative {
        design_dimension_locus_pairs: vec![pair].try_into().unwrap(),
        ..F3dNative::default()
    };
    let encoded = serde_json::to_string(&native).unwrap();
    assert!(encoded.contains(&format!(r#""design_dimension_locus_pairs":[{entry}]"#)));
    assert_eq!(serde_json::from_str::<F3dNative>(&encoded).unwrap(), native);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&mut namespace).unwrap();
    let arena: Vec<DesignDimensionLocusPair> =
        namespace.arena_as("design_dimension_locus_pairs").unwrap();
    assert_eq!(serde_json::to_string(&arena).unwrap(), format!("[{entry}]"));
    assert_eq!(F3dNative::load(&namespace).unwrap(), native);
}
