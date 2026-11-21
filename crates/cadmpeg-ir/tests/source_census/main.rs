// SPDX-License-Identifier: Apache-2.0
//! Census coverage for hand-written and derived wire readers.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use declarations::{initializer_items, module_scope, Declaration, Declarations};
use shim_forwards::forwarded_shim;
use writers::{container_conversion, wire_index, WireIndex, HAND_WRITTEN_KEYS};

mod declarations;
mod reader_routes;
mod roots;
mod shim_forwards;
mod writers;

/// Every hand-written `Deserialize` in the three wire crates, with the reader
/// route that admits its wire value.
///
/// A hand impl never reaches `scripts/check-deny-census.py`, which reads
/// `derive(Deserialize)` items only, so its coverage is stated here. The test
/// below parses the source at run time and fails when this table and the source
/// disagree in either direction, so a new hand impl cannot land uncovered and a
/// deleted one cannot leave a stale entry.
///
/// The route classes are checked against parsed implementation bodies by the
/// `reader_routes` module. They cannot drift independently of the reader.
///
/// * `wire` - the impl reads a named or local wire object whose declaration
///   states `deny_unknown_fields`;
/// * `keyless` - the impl reads a scalar, byte string, fixed array, or list;
/// * `free-form` - the impl admits an open map or canonical JSON value;
/// * `validated-value` - the impl reads a general JSON value and applies an
///   explicit version gate before constructing its scalar wrapper.
const HAND_IMPLS: &[(&str, &str, &str)] = &[
    ("crates/cadmpeg-asm/src/brep/mod.rs", "AsmBrep", "wire"),
    ("crates/cadmpeg-asm/src/brep/records.rs", "$name", "wire"),
    (
        "crates/cadmpeg-asm/src/brep/records.rs",
        "FaceSidedness",
        "wire",
    ),
    ("crates/cadmpeg-asm/src/brep/stats.rs", "Stats", "wire"),
    ("crates/cadmpeg-core/src/dialect.rs", "DialectId", "keyless"),
    (
        "crates/cadmpeg-core/src/distinct_keys.rs",
        "JsonValue",
        "free-form",
    ),
    (
        "crates/cadmpeg-core/src/dialect.rs",
        "DialectLayers",
        "wire",
    ),
    (
        "crates/cadmpeg-core/src/printable.rs",
        "PrintableString",
        "keyless",
    ),
    ("crates/cadmpeg-ir/src/assets.rs", "AssetData", "keyless"),
    (
        "crates/cadmpeg-ir/src/annotations.rs",
        "NonEmptyMap",
        "free-form",
    ),
    (
        "crates/cadmpeg-ir/src/container.rs",
        "ContainerKind",
        "keyless",
    ),
    ("crates/cadmpeg-ir/src/document.rs", "CadIr", "wire"),
    ("crates/cadmpeg-ir/src/repro.rs", "Bundle", "wire"),
    ("crates/cadmpeg-ir/src/document.rs", "CensusKey", "keyless"),
    (
        "crates/cadmpeg-ir/src/document.rs",
        "IrVersion",
        "validated-value",
    ),
    ("crates/cadmpeg-ir/src/document.rs", "Model", "wire"),
    ("crates/cadmpeg-ir/src/document.rs", "Topology", "wire"),
    (
        "crates/cadmpeg-ir/src/features/edge_treatments.rs",
        "FullRoundFilletGroup",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/features/form.rs",
        "FormConstruction",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/features/holes.rs",
        "HoleShape",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/features/patterns.rs",
        "PatternKind",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/features/replace_face.rs",
        "ReplaceFaceOperands",
        "wire",
    ),
    ("crates/cadmpeg-ir/src/features.rs", "$name", "wire"),
    (
        "crates/cadmpeg-ir/src/features/geometry.rs",
        "$name",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features/selections.rs",
        "BodyMember",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/features/selections.rs",
        "BodyMembers",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features/parameters.rs",
        "ConfigurationEvaluation",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/features/selections.rs",
        "DistinctMembers",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features/sweeps.rs",
        "FaceMaker",
        "keyless",
    ),
    ("crates/cadmpeg-ir/src/features.rs", "Feature", "wire"),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "FeatureContent",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features/profiles.rs",
        "GeometryImportPath",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "NativeFeatureKind",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features/selections.rs",
        "NativeSelections",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features/selections.rs",
        "NonEmptyMembers",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features/parameters.rs",
        "PolygonSideCount",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features/selections.rs",
        "SelectionMembers",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features/selections.rs",
        "SelectionReference",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "SewBodySelection",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/features/sheet_metal_forms.rs",
        "SheetMetalFlangeEdgeWidths",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features/profiles.rs",
        "SketchProfileRegions",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features/direct_edit.rs",
        "SplitFacePlanes",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features/sweeps.rs",
        "SweepCircularRegion",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/features/profiles.rs",
        "ThreePointSelection",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features/tree.rs",
        "TreeChildren",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry/nurbs.rs",
        "BsplineSurface",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry/nurbs.rs",
        "NurbsCurve",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry/nurbs.rs",
        "NurbsSurface",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry/pcurve.rs",
        "PcurveNurbs",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry/pcurve.rs",
        "PolarPcurveNurbs",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry/sampled.rs",
        "PolygonalSurface",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "DirectedParameterRange",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralSurfaceDefinition",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProjectionRole",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/hash/digest.rs",
        "Sha256Digest",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/native/bytes.rs",
        "NativeBytes",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/native/mod.rs",
        "NativeRecord",
        "free-form",
    ),
    ("crates/cadmpeg-ir/src/pmi.rs", "PmiMagnitude", "keyless"),
    ("crates/cadmpeg-ir/src/process/tool.rs", "Uuid", "keyless"),
    (
        "crates/cadmpeg-core/src/text.rs",
        "NonBlankString",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/provenance.rs",
        "CodecFormat",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/provenance.rs",
        "SourceOwner",
        "keyless",
    ),
    ("crates/cadmpeg-ir/src/provenance.rs", "Provenance", "wire"),
    ("crates/cadmpeg-ir/src/provenance.rs", "Provenance", "wire"),
    ("crates/cadmpeg-ir/src/scalar.rs", "$name", "keyless"),
    (
        "crates/cadmpeg-ir/src/scalar.rs",
        "FiniteBinary32",
        "keyless",
    ),
    ("crates/cadmpeg-ir/src/scalar.rs", "UnitBinary32", "keyless"),
    (
        "crates/cadmpeg-ir/src/sketches.rs",
        "SpatialSketchNurbsCurve",
        "wire",
    ),
    ("crates/cadmpeg-ir/src/tessellation.rs", "Strip", "keyless"),
    ("crates/cadmpeg-ir/src/tessellation.rs", "Strips", "keyless"),
    ("crates/cadmpeg-ir/src/transform.rs", "Transform", "keyless"),
    (
        "crates/cadmpeg-ir/src/transform.rs",
        "Transform2",
        "keyless",
    ),
    ("crates/cadmpeg-ir/src/units.rs", "$name", "keyless"),
];

#[test]
fn every_hand_written_deserialize_states_its_coverage() {
    let found = hand_written_impls();
    assert!(
        !found.is_empty(),
        "the hand-impl census found no impls to classify"
    );
    let listed: Vec<(String, String)> = HAND_IMPLS
        .iter()
        .map(|(path, name, _class)| ((*path).to_owned(), (*name).to_owned()))
        .collect();
    let mut found = found;
    let mut listed = listed;
    found.sort();
    listed.sort();
    let missing: Vec<String> = reader_routes::multiset_difference(&found, &listed)
        .into_iter()
        .map(|(path, name)| format!("{path} {name}"))
        .collect();
    assert!(
        missing.is_empty(),
        "{} hand-written Deserialize impl(s) state no coverage in HAND_IMPLS:\n{}",
        missing.len(),
        missing.join("\n")
    );
    let stale: Vec<String> = reader_routes::multiset_difference(&listed, &found)
        .into_iter()
        .map(|(path, name)| format!("{path} {name}"))
        .collect();
    assert!(
        stale.is_empty(),
        "{} HAND_IMPLS entry/entries name no hand-written Deserialize impl:\n{}",
        stale.len(),
        stale.join("\n")
    );

    reader_routes::classify::assert_hand_written_reader_routes();

    // Namespace and arena names are open. Their values still have fixed shapes:
    // a namespace is an arena map, and an arena is a list of native records.
    for wire in [
        serde_json::json!({"rhino": {"objects": []}, "zz_bogus": {}}),
        serde_json::json!({"rhino": {"objects": [], "zz_bogus": []}}),
    ] {
        let read: cadmpeg_ir::native::Native =
            serde_json::from_value(wire.clone()).expect("native store reads back");
        assert_eq!(
            serde_json::to_value(read).expect("native store writes back"),
            wire
        );
    }
    for wire in [
        serde_json::json!({"rhino": {"objects": []}, "zz_bogus": true}),
        serde_json::json!({"rhino": {"objects": [], "zz_bogus": true}}),
    ] {
        assert!(
            serde_json::from_value::<cadmpeg_ir::native::Native>(wire).is_err(),
            "native namespace and arena maps refuse invalid value shapes"
        );
    }
}

/// Exercise the compiled readers that the source census classifies at the two
/// boundaries most likely to be confused by lexical markers: the intentionally
/// open native field map and a closed hand-written object adapter.
#[test]
fn compiled_hand_readers_keep_their_admission_boundaries() {
    let native = serde_json::from_value::<cadmpeg_ir::native::NativeRecord>(serde_json::json!({
        "id": "test:source-census:record#0",
        "codec_field": {"preserved": true},
    }))
    .expect("NativeRecord's open field reader admits codec-owned fields");
    assert_eq!(
        native.field("codec_field"),
        Some(serde_json::json!({"preserved": true}))
    );

    let unknown_configuration = serde_json::from_value::<
        cadmpeg_ir::features::parameters::ConfigurationEvaluation,
    >(serde_json::json!({
        "kind": "suppressed",
        "unexpected": true,
    }));
    assert!(
        unknown_configuration.is_err(),
        "the compiled local wire adapter rejects an unknown object key"
    );
}

/// Where every hand-written `Deserialize` refuses `null` for an optional
/// field, as (file, type, field, guard site).
///
/// A hand impl states its own key reads, so the derive census cannot see
/// whether `null` and absence reach the same value there. The guard site names
/// the item the impl reads through: a wire struct, or a wire enum and the
/// variant, spelled `Wire::Variant`. The site is located by parsing the file,
/// so an edit anywhere above it moves nothing; the named field must exist
/// there and must state a helper [`null_refusing_helpers`] derives.
const HAND_IMPL_NULL_REFUSALS: &[(&str, &str, &str, &str)] = &[
    (
        "crates/cadmpeg-ir/src/features/sweeps.rs",
        "SweepCircularRegion",
        "wall_thickness",
        "SweepCircularRegionWire",
    ),
    (
        "crates/cadmpeg-ir/src/features/tree.rs",
        "TreeChildren",
        "active_child",
        "TreeChildrenWire",
    ),
    (
        "crates/cadmpeg-ir/src/features/holes.rs",
        "HoleShape",
        "exit_kind",
        "HoleShapeWire",
    ),
    (
        "crates/cadmpeg-ir/src/features/holes.rs",
        "HoleShape",
        "diameter",
        "HoleShapeWire",
    ),
    (
        "crates/cadmpeg-ir/src/features/replace_face.rs",
        "ReplaceFaceOperands",
        "tangent_chain",
        "ReplaceFaceOperandsWire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        "ProceduralCurveDefinitionWire::Exact",
    ),
    (
        "crates/cadmpeg-ir/src/geometry/nurbs.rs",
        "NurbsSurface",
        "cache_fit_tolerance",
        "NurbsSurfaceWire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "version",
        "ProceduralCurveDefinitionWire::Law",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        "ProceduralCurveDefinitionWire::Law",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        "ProceduralCurveDefinitionWire::Intersection",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "parameterization",
        "ProceduralCurveDefinitionWire::TolerantIntersection",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        "ProceduralCurveDefinitionWire::TolerantIntersection",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "blend_surface",
        "ProceduralCurveDefinitionWire::BlendSpine",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "native_kind",
        "ProceduralCurveDefinitionWire::Unknown",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "record",
        "ProceduralCurveDefinitionWire::Unknown",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        "ProceduralCurveDefinitionWire::Unknown",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralSurfaceDefinition",
        "cache",
        "ProceduralSurfaceDefinitionWire::Ruled",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralSurfaceDefinition",
        "record",
        "ProceduralSurfaceDefinitionWire::Unknown",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralSurfaceDefinition",
        "cache",
        "ProceduralSurfaceDefinitionWire::Unknown",
    ),
    (
        "crates/cadmpeg-ir/src/document.rs",
        "CadIr",
        "repro",
        "CadIrReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/repro.rs",
        "Bundle",
        "incomplete_reason",
        "BundleMetadata",
    ),
    (
        "crates/cadmpeg-ir/src/document.rs",
        "CadIr",
        "source",
        "CadIrReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "Feature",
        "name",
        "FeatureReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "Feature",
        "suppressed",
        "FeatureReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "Feature",
        "source_tag",
        "FeatureReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "Feature",
        "source_text",
        "FeatureReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "Feature",
        "native_ref",
        "FeatureReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "Feature",
        "product_definition",
        "FeatureReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/provenance.rs",
        "Provenance",
        "tag",
        "AnnotationProvenanceWire",
    ),
    (
        "crates/cadmpeg-ir/src/provenance.rs",
        "Provenance",
        "tag",
        "SourceProvenanceWire",
    ),
    (
        "crates/cadmpeg-asm/src/brep/records.rs",
        "FaceSidedness",
        "containment",
        "FaceSidednessWire",
    ),
];

/// The `deserialize_with` helper the field named `field` states at the guard
/// site `site` of `items`, or the reason no such field was reached.
///
/// `site` is a wire type name, or a wire enum name and a variant name joined
/// by `::`. The walk recurses into inline modules, so a wire declared beside
/// its impl inside a private module reads the same.
fn helper_at_guard_site(items: &[syn::Item], site: &str, field: &str) -> Result<String, String> {
    let (type_name, variant_name) = match site.split_once("::") {
        Some((type_name, variant_name)) => (type_name, Some(variant_name)),
        None => (site, None),
    };
    for item in items {
        let fields = match item {
            syn::Item::Struct(declaration) if declaration.ident == type_name => {
                if variant_name.is_some() {
                    return Err(format!(
                        "{site} names a variant, but {type_name} is a struct"
                    ));
                }
                &declaration.fields
            }
            syn::Item::Enum(declaration) if declaration.ident == type_name => {
                let Some(wanted) = variant_name else {
                    return Err(format!(
                        "{site} names no variant, but {type_name} is an enum"
                    ));
                };
                let Some(variant) = declaration
                    .variants
                    .iter()
                    .find(|variant| variant.ident == wanted)
                else {
                    return Err(format!("{type_name} declares no variant {wanted}"));
                };
                &variant.fields
            }
            syn::Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    if let Ok(found) = helper_at_guard_site(nested, site, field) {
                        return Ok(found);
                    }
                }
                continue;
            }
            _ => continue,
        };
        let Some(declared) = fields
            .iter()
            .find(|candidate| candidate.ident.as_ref().is_some_and(|name| name == field))
        else {
            return Err(format!("{site} declares no field {field}"));
        };
        return absence_spelling(declared)
            .ok_or_else(|| format!("{site} field {field} states no deserialize_with"));
    }
    Err(format!("no item named {type_name} was found"))
}

/// Every hand-written `Deserialize` reads its optional keys the one way.
///
/// For each impl the coverage census enumerates, the type either declares no
/// `Option` field -- nothing for `null` to spell twice -- or every such field
/// is listed in [`HAND_IMPL_NULL_REFUSALS`] with the guard site where the impl
/// refuses `null`. A type declared by a macro states `$name` in the census and
/// declares no fields of its own.
#[test]
fn every_hand_written_deserialize_refuses_a_null_spelling() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root sits two levels above the crate manifest")
        .to_path_buf();
    let declarations = Declarations::read(&root);
    let mut disagreements = Vec::new();
    for (path, name, _class) in HAND_IMPLS {
        if name.starts_with('$') {
            continue;
        }
        let text = read_source(&root.join(path));
        let parsed = syn::parse_file(&text).expect("every source file in the wire crates parses");
        let mut skipping = Vec::new();
        declared_optional_fields(
            &parsed.items,
            path,
            &module_scope(path),
            name,
            &declarations,
            &mut skipping,
        );
        let mut listed: Vec<String> = HAND_IMPL_NULL_REFUSALS
            .iter()
            .filter(|(listed_path, listed_name, _, _)| listed_path == path && listed_name == name)
            .map(|(_, _, field, site)| {
                match helper_at_guard_site(&parsed.items, site, field) {
                    Ok(helper) => assert!(
                        refuses_null(&declarations, path, &helper)
                            || states_null(&declarations, path, &module_scope(path), &helper),
                        "{path} {site} is listed as where {name} states the spelling of null for {field}, but it states the unlisted helper {helper}"
                    ),
                    Err(reason) => panic!(
                        "{path} {site} is listed as where {name} refuses null for {field}, but {reason}"
                    ),
                }
                (*field).to_owned()
            })
            .collect();
        skipping.sort();
        skipping.dedup();
        listed.sort();
        listed.dedup();
        if skipping != listed {
            disagreements.push(format!(
                "{path} {name}: optional fields {skipping:?}, guard sites listed for {listed:?}"
            ));
        }
    }
    assert!(
        disagreements.is_empty(),
        "{} hand-written Deserialize impl(s) disagree with the null-refusal list:\n{}",
        disagreements.len(),
        disagreements.join("\n")
    );
}

/// Records the name of every `Option` field of the type named `name` in
/// module `scope` of `relative`, recursing into inline modules. A field whose
/// optionality the census cannot read is recorded with the reason, which no
/// listed guard site matches.
fn declared_optional_fields(
    items: &[syn::Item],
    relative: &str,
    scope: &[String],
    name: &str,
    declarations: &Declarations,
    found: &mut Vec<String>,
) {
    let owner = Declaration {
        path: relative.to_owned(),
        scope: scope.to_owned(),
        name: name.to_owned(),
    };
    let record = |fields: &syn::Fields, generics: &syn::Generics, found: &mut Vec<String>| {
        for field in fields {
            let field_name = field
                .ident
                .as_ref()
                .map_or_else(|| "<tuple field>".to_owned(), syn::Ident::to_string);
            match declarations.is_optional(&owner, generics, &field.ty) {
                Ok(true) => found.push(field_name),
                Ok(false) => {}
                Err(reason) => found.push(format!("{field_name} ({reason})")),
            }
        }
    };

    for item in items {
        match item {
            syn::Item::Struct(declaration) if declaration.ident == name => {
                record(&declaration.fields, &declaration.generics, found);
            }
            syn::Item::Enum(declaration) if declaration.ident == name => {
                for variant in &declaration.variants {
                    record(&variant.fields, &declaration.generics, found);
                }
            }
            syn::Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    let mut nested_scope = scope.to_owned();
                    nested_scope.push(module.ident.to_string());
                    declared_optional_fields(
                        nested,
                        relative,
                        &nested_scope,
                        name,
                        declarations,
                        found,
                    );
                }
            }
            syn::Item::Const(syn::ItemConst { expr, .. })
            | syn::Item::Static(syn::ItemStatic { expr, .. }) => declared_optional_fields(
                &initializer_items(expr),
                relative,
                scope,
                name,
                declarations,
                found,
            ),
            _ => {}
        }
    }
}

/// `None` is spelled one way on every field a document can be read into, and
/// the reading declaration states the spelling its writer produces.
///
/// An optional key has two writers and so two admitted forms. A field carrying
/// `skip_serializing_if = "Option::is_none"` is written by omission: the
/// reading declaration states a `cadmpeg_core::named_optional_field!` shim and
/// a `default`, so absence is `None` and `null` is refused by a message that
/// names the key. A field with no skip is always written: the
/// reading declaration states `cadmpeg_core::absent_key::nullable` and no
/// `default`, so `null` is `None` and an absent key is a missing field serde
/// names. Either way one state has one spelling.
///
/// The census reads both halves. It finds the writing declaration of each read
/// type — the type itself when it derives `Serialize`, the type its container
/// `into` or `remote` names, the type whose `try_from` or `from` routes the
/// read, or the pair stated in [`WRITING_DECLARATIONS`] — and refuses a
/// reading declaration whose spelling that writer does not produce.
///
/// The duty follows the reader: a type carries it when it derives
/// `Deserialize` and reads its own keys. A type that derives none, and one
/// that derives it with `try_from` or `from`, both read a document through a
/// separate wire type; this census reaches that wire type on its own, and the
/// serialize-side attributes left on the outer type name no reader.
///
/// The source is parsed, not grepped, by the walker the hand-impl census uses,
/// so an attribute list wrapped over several lines, a field in an inline
/// module, or an optional field of an enum variant is read just the same.
/// Offenders are named by `file:line`.
#[test]
fn every_read_optional_field_refuses_a_null_spelling() {
    assert!(
        !null_refusing_helpers().is_empty(),
        "no source defines a named_optional_field! shim, so no key could refuse null"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root sits two levels above the crate manifest")
        .to_path_buf();
    let (readable, offenders) = optional_absence_census(&root);
    assert!(
        readable > 0,
        "the optional-field census read no types that derive or implement Deserialize"
    );
    assert!(
        offenders.is_empty(),
        "{} `Option` field(s) on a read type do not state the one spelling of `None` their writer produces, so a state reaches the reader two ways:\n{}",
        offenders.len(),
        offenders.join("\n")
    );
}

/// The macro path every field-deserializer shim is defined by, keyed by the
/// file it is declared in and the shim's own name.
///
/// A shim is a `named_field!` or `named_optional_field!` item invocation; the
/// macro's first token is the shim's own name. The source is parsed, so an
/// invocation wrapped over several lines or nested in an inline module reads
/// the same. A shim is a module-scoped item and a `deserialize_with` names it
/// without a path, so a name resolves within its own file: one file may read
/// `deserialize_source_id` as a required key and another as an optional one.
/// A name declared twice in one file carries both macros, which is what refuses
/// a shim only some of whose definitions guard.
///
/// The second map holds each function [`forwarded_shim`] reads as a forward
/// to another helper of its file, keyed the same way, with that helper's name.
fn shim_definitions() -> (ShimMacros, BTreeMap<(String, String), String>) {
    fn walk(
        items: &[syn::Item],
        relative: &str,
        found: &mut ShimMacros,
        forwarders: &mut BTreeMap<(String, String), String>,
    ) {
        for item in items {
            match item {
                syn::Item::Fn(function) => {
                    if let Some(callee) = forwarded_shim(function) {
                        forwarders.insert(
                            (relative.to_owned(), function.sig.ident.to_string()),
                            callee,
                        );
                    }
                }
                syn::Item::Macro(invocation) => {
                    let path = &invocation.mac.path;
                    let Some(macro_name) = path.segments.last() else {
                        continue;
                    };
                    let macro_name = macro_name.ident.to_string();
                    if macro_name != "named_field" && macro_name != "named_optional_field" {
                        continue;
                    }
                    let segments: Vec<String> = path
                        .segments
                        .iter()
                        .map(|segment| segment.ident.to_string())
                        .collect();
                    let leading = if path.leading_colon.is_some() {
                        "::"
                    } else {
                        ""
                    };
                    let mut tokens = Vec::new();
                    flatten_tokens(&invocation.mac.tokens, &mut tokens);
                    let Some(shim) = tokens.first() else { continue };
                    found
                        .entry((relative.to_owned(), shim.clone()))
                        .or_default()
                        .insert(format!("{leading}{}", segments.join("::")));
                }
                syn::Item::Mod(module) => {
                    if let Some((_, nested)) = &module.content {
                        walk(nested, relative, found, forwarders);
                    }
                }
                _ => {}
            }
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root sits two levels above the crate manifest")
        .to_path_buf();
    let mut found = BTreeMap::new();
    let mut forwarders = BTreeMap::new();
    for source in [
        "crates/cadmpeg-ir/src",
        "crates/cadmpeg-core/src",
        "crates/cadmpeg-asm/src",
    ] {
        let mut files = Vec::new();
        collect_rust_sources(&root.join(source), &mut files)
            .unwrap_or_else(|error| panic!("cannot collect {source}: {error}"));
        files.sort();
        for file in files {
            let relative = file
                .strip_prefix(&root)
                .expect("a collected source sits under the repository root")
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_path(&relative) {
                continue;
            }
            let text = read_source(&file);
            let parsed = syn::parse_file(&text)
                .map_err(|error| format!("{relative} does not parse: {error}"))
                .expect("every source file in the wire crates parses");
            walk(&parsed.items, &relative, &mut found, &mut forwarders);
        }
    }
    (found, forwarders)
}

/// Per shim site, the macro paths that define it.
type ShimMacros = BTreeMap<(String, String), BTreeSet<String>>;

/// One key's spelling of `None` on the writing declaration.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WriteSpelling {
    /// `skip_serializing_if` on the writing field: `None` leaves the key out.
    Omitted,
    /// An optional writing field with no skip: `None` writes the key as `null`.
    Null,
    /// A writing field that is not optional: the key always states a value.
    Always,
}

/// Whether this attribute is a `derive` (or a `cfg_attr` carrying one) whose
/// list names `Serialize`.
fn derive_list_names_serialize(attribute: &syn::Attribute) -> bool {
    if !attribute.path().is_ident("derive") && !attribute.path().is_ident("cfg_attr") {
        return false;
    }
    let Ok(list) = attribute.meta.require_list() else {
        return false;
    };
    let mut flat = Vec::new();
    flatten_tokens(&list.tokens, &mut flat);
    flat.iter().any(|token| token == "Serialize")
}

/// The count of readable types visited, and every `file:line` where one of
/// their fields skips on `None` without a `deserialize_with`, in the wire
/// crates under `root`.
fn optional_absence_census(root: &Path) -> (usize, Vec<String>) {
    let index = wire_index(root);
    let mut readable = 0_usize;
    let mut offenders = Vec::new();
    for source in [
        "crates/cadmpeg-ir/src",
        "crates/cadmpeg-core/src",
        "crates/cadmpeg-asm/src",
    ] {
        let mut files = Vec::new();
        collect_rust_sources(&root.join(source), &mut files)
            .unwrap_or_else(|error| panic!("cannot collect {source}: {error}"));
        files.sort();
        for file in files {
            let relative = file
                .strip_prefix(root)
                .expect("a collected source sits under the repository root")
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_path(&relative) {
                continue;
            }
            let text = read_source(&file);
            let parsed = syn::parse_file(&text)
                .map_err(|error| format!("{relative} does not parse: {error}"))
                .expect("every source file in the wire crates parses");
            collect_optional_fields(
                &parsed.items,
                &relative,
                &module_scope(&relative),
                &index,
                &mut readable,
                &mut offenders,
            );
        }
    }
    (readable, offenders)
}

/// Visits every struct and enum among `items`, recursing into inline modules,
/// and records the unguarded optional fields of the readable ones.
fn collect_optional_fields(
    items: &[syn::Item],
    relative: &str,
    scope: &[String],
    index: &WireIndex,
    readable: &mut usize,
    offenders: &mut Vec<String>,
) {
    let reader = |name: &syn::Ident| Declaration {
        path: relative.to_owned(),
        scope: scope.to_owned(),
        name: name.to_string(),
    };
    for item in items {
        match item {
            syn::Item::Struct(declaration) => {
                if !reads_its_own_keys(&declaration.attrs) {
                    continue;
                }
                *readable += 1;
                let absence = if is_newtype(&declaration.fields) {
                    NewtypeKey::Holder
                } else {
                    NewtypeKey::Own
                };
                record_unguarded_fields(
                    &declaration.fields,
                    absence,
                    &declaration.generics,
                    &reader(&declaration.ident),
                    index,
                    offenders,
                );
            }
            syn::Item::Enum(declaration) => {
                if !reads_its_own_keys(&declaration.attrs) {
                    continue;
                }
                *readable += 1;
                let adjacent = container_conversion(&declaration.attrs, "content").is_some();
                for variant in &declaration.variants {
                    let absence = if adjacent
                        && is_newtype(&variant.fields)
                        && !variant_states_reader(variant)
                    {
                        NewtypeKey::Content
                    } else {
                        NewtypeKey::Own
                    };
                    record_unguarded_fields(
                        &variant.fields,
                        absence,
                        &declaration.generics,
                        &reader(&declaration.ident),
                        index,
                        offenders,
                    );
                }
            }
            syn::Item::Mod(module) => {
                if is_test_module(module) {
                    continue;
                }
                if let Some((_, nested)) = &module.content {
                    let mut nested_scope = scope.to_owned();
                    nested_scope.push(module.ident.to_string());
                    collect_optional_fields(
                        nested,
                        relative,
                        &nested_scope,
                        index,
                        readable,
                        offenders,
                    );
                }
            }
            // A const block is no function body: the items it declares are
            // read in its module.
            syn::Item::Const(syn::ItemConst { expr, .. })
            | syn::Item::Static(syn::ItemStatic { expr, .. }) => collect_optional_fields(
                &initializer_items(expr),
                relative,
                scope,
                index,
                readable,
                offenders,
            ),
            _ => {}
        }
    }
}

/// Consumes whatever follows a nested-meta path: an `= value`, a
/// parenthesized list, or nothing.
fn skip_meta_value(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) {
        let _literal: syn::Lit = meta.value()?.parse()?;
    } else if meta.input.peek(syn::token::Paren) {
        let content;
        syn::parenthesized!(content in meta.input);
        let _rest: proc_macro2::TokenStream = content.parse()?;
    }
    Ok(())
}

/// Whether a document reaches this type's own field attributes: it derives
/// `Deserialize`, and its container attributes route no `try_from` or `from`
/// conversion through another type.
fn reads_its_own_keys(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(derive_list_names_deserialize) && !attrs.iter().any(routes_through_a_wire)
}

/// Whether this container-level `serde` attribute states `try_from` or `from`,
/// which makes another type the reader.
fn routes_through_a_wire(attribute: &syn::Attribute) -> bool {
    if !attribute.path().is_ident("serde") {
        return false;
    }
    let mut routed = false;
    attribute
        .parse_nested_meta(|meta| {
            if meta.path.is_ident("try_from") || meta.path.is_ident("from") {
                routed = true;
            }
            skip_meta_value(&meta)?;
            Ok(())
        })
        .expect("every container serde attribute in the wire crates parses as a nested meta list");
    routed
}

/// Whether this attribute is a `derive` (or a `cfg_attr` carrying one) whose
/// list names `Deserialize`.
fn derive_list_names_deserialize(attribute: &syn::Attribute) -> bool {
    if !attribute.path().is_ident("derive") && !attribute.path().is_ident("cfg_attr") {
        return false;
    }
    let Ok(list) = attribute.meta.require_list() else {
        return false;
    };
    let mut flat = Vec::new();
    flatten_tokens(&list.tokens, &mut flat);
    flat.iter().any(|token| token == "Deserialize")
}

/// The `deserialize_with` helpers that refuse an explicit `null` at their key.
///
/// The set is derived, not listed: `cadmpeg_core::named_optional_field!` is the
/// one declaration that reaches the refusal, because the visitor it forwards to
/// is private to `cadmpeg_core::absent_key`. Every shim that macro defines
/// refuses `null` and names its own key in the refusal. A name another macro
/// also defines is left out, so a shim only some of whose definitions guard is
/// no proof. A function that [`forwarded_shim`] reads as a forward to such a
/// shim of its own file refuses `null` too; it is how a field whose type is not
/// the shim's own reads through one, such as a set read with its refusal of a
/// repeated member.
///
/// Anything else on a field that skips on `None` is an offender, because a
/// `deserialize_with` that does not state the refusal gives `None` a second
/// spelling. Each shim maps to the macro path its invocation spells, and a
/// forward to the path of the shim it calls.
fn null_refusing_helpers() -> &'static BTreeMap<(String, String), String> {
    static HELPERS: OnceLock<BTreeMap<(String, String), String>> = OnceLock::new();
    HELPERS.get_or_init(|| {
        let (macros, forwarders) = shim_definitions();
        let shims: BTreeMap<(String, String), String> = macros
            .iter()
            .filter_map(|(site, macros)| {
                let [invoked] = macros.iter().collect::<Vec<_>>().try_into().ok()?;
                (invoked.rsplit("::").next() == Some("named_optional_field"))
                    .then(|| (site.clone(), invoked.clone()))
            })
            .collect();
        let forwarding: Vec<((String, String), String)> = forwarders
            .into_iter()
            .filter(|(site, _)| !macros.contains_key(site))
            .filter_map(|(site, callee)| {
                let invoked = shims.get(&(site.0.clone(), callee))?;
                Some((site, invoked.clone()))
            })
            .collect();
        shims.into_iter().chain(forwarding).collect()
    })
}

/// Whether `helper`, read in `relative`, is a shim that refuses `null`: one
/// the macro defines whose path, read in that file, is the real
/// `cadmpeg_core::named_optional_field!`. `crate::` and a bare name reach it
/// only inside `cadmpeg-core`.
fn refuses_null(declarations: &Declarations, relative: &str, helper: &str) -> bool {
    null_refusing_helpers()
        .get(&(relative.to_owned(), helper.to_owned()))
        .is_some_and(|invoked| {
            declarations.names_external(
                relative,
                &module_scope(relative),
                invoked,
                &["cadmpeg_core", "named_optional_field"],
            ) || (relative.starts_with("crates/cadmpeg-core/src/")
                && matches!(
                    invoked.as_str(),
                    "crate::named_optional_field" | "named_optional_field"
                ))
        })
}

/// Whether `helper`, read in module `scope` of `relative`, is one of
/// [`NULL_STATING_HELPERS`], its root bound to the real crate there.
fn states_null(
    declarations: &Declarations,
    relative: &str,
    scope: &[String],
    helper: &str,
) -> bool {
    NULL_STATING_HELPERS
        .iter()
        .any(|expected| declarations.names_external(relative, scope, helper, expected))
}

/// The `deserialize_with` helpers that state `null` is this key's own spelling
/// of `None`.
///
/// A field with no `skip_serializing_if` on its writing declaration is written
/// for every value, and as `null` for `None`, so `null` is the writer's
/// spelling there and the reader admits it. `cadmpeg_core::absent_key::nullable`
/// states that, and nothing else: it reads the field's own `Option`. The
/// reading declaration states no `default` beside it, so an absent key is a
/// missing field rather than a second spelling of `None`. A key that states
/// neither this nor a helper [`null_refusing_helpers`] derives declares no
/// spelling at all, which is what this census refuses.
const NULL_STATING_HELPERS: &[&[&str]] = &[&["cadmpeg_core", "absent_key", "nullable"]];

/// Records every `Option` field of `fields` that states no helper
/// [`null_refusing_helpers`] derives.
///
/// The key is the field's own type. A field declared `Option<T>` on a type a
/// document reads gives `null` a second spelling of absence unless a helper
/// refuses it, whatever the field's `skip_serializing_if` says: the writing
/// half and the reading half are separate declarations, and only the reading
/// half decides what `null` becomes. A field typed by a type parameter's
/// associated type holds an `Option` when any impl's type is one, and one
/// whose impls the census cannot read is an offender. `absence` says which
/// key spells the absence of a newtype's `Option`.
fn record_unguarded_fields(
    fields: &syn::Fields,
    absence: NewtypeKey,
    generics: &syn::Generics,
    reader: &Declaration,
    index: &WireIndex,
    offenders: &mut Vec<String>,
) {
    let relative = reader.path.as_str();
    for field in fields {
        let span = field
            .ident
            .as_ref()
            .map_or_else(|| syn::spanned::Spanned::span(&field.ty), syn::Ident::span);
        let name = field
            .ident
            .as_ref()
            .map_or_else(|| "<tuple field>".to_owned(), syn::Ident::to_string);
        let at = format!("{relative}:{} {}.{name}", span.start().line, reader.name);
        match index.declarations.is_optional(reader, generics, &field.ty) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(reason) => {
                offenders.push(format!("{at} {reason}"));
                continue;
            }
        }
        let helper = absence_spelling(field);
        let read = match helper.as_deref() {
            Some(helper) if refuses_null(&index.declarations, relative, helper) => {
                ReadSpelling::Present
            }
            Some(helper) if states_null(&index.declarations, relative, &reader.scope, helper) => {
                ReadSpelling::Nullable
            }
            Some(helper) => {
                offenders.push(format!("{at} states the unlisted helper {helper}"));
                continue;
            }
            None => {
                offenders.push(format!("{at} states no deserialize_with"));
                continue;
            }
        };
        if read == ReadSpelling::Present && absence == NewtypeKey::Holder {
            continue;
        }
        let defaulted = states_default(field) || absence == NewtypeKey::Content;
        match (read, defaulted) {
            (ReadSpelling::Present, false) => offenders.push(format!(
                "{at} reads the absent key as its one spelling of None but states no default"
            )),
            (ReadSpelling::Nullable, true) => offenders.push(format!(
                "{at} reads null as its one spelling of None and states a default, which admits the absent key as a second"
            )),
            (ReadSpelling::Present, true) | (ReadSpelling::Nullable, false) => {}
        }
        if let Some((_, _, _, written)) = HAND_WRITTEN_KEYS.iter().find(|(file, named, key, _)| {
            *file == reader.path && *named == reader.name && *key == name
        }) {
            record_spelling_match(&at, read, *written, "the hand-written writer", offenders);
            continue;
        }
        let writer = match index.writing_declaration(reader) {
            Ok(writer) => writer,
            Err(reason) => {
                offenders.push(format!("{at} {reason}"));
                continue;
            }
        };
        let written = if writer.owner == *reader && writer.variant.is_none() {
            // The reading and writing declarations are one declaration, so the
            // field states both halves and no key lookup is needed. A field
            // with no name states no key at all, and is only reachable here.
            write_spelling(field, true)
        } else {
            match index.spelling(&writer, &name) {
                Ok(written) => written,
                Err(reason) => {
                    offenders.push(format!("{at} {reason}"));
                    continue;
                }
            }
        };
        let source = match writer.variant {
            Some(variant) => format!("{}::{variant}", writer.owner),
            None => writer.owner.to_string(),
        };
        record_spelling_match(&at, read, written, &source, offenders);
    }
}

/// Records `at` when the reading and writing spellings of `None` disagree.
fn record_spelling_match(
    at: &str,
    read: ReadSpelling,
    written: WriteSpelling,
    writer: &str,
    offenders: &mut Vec<String>,
) {
    match (read, written) {
        (ReadSpelling::Present, WriteSpelling::Null) => offenders.push(format!(
            "{at} refuses null, but {writer} states no skip_serializing_if and writes null for None"
        )),
        (ReadSpelling::Nullable, WriteSpelling::Omitted) => offenders.push(format!(
            "{at} admits null, but {writer} skips the key for None and never writes null"
        )),
        (ReadSpelling::Nullable, WriteSpelling::Always) => offenders.push(format!(
            "{at} admits null, but {writer} states no optional value there and never writes null"
        )),
        (ReadSpelling::Present, WriteSpelling::Omitted | WriteSpelling::Always)
        | (ReadSpelling::Nullable, WriteSpelling::Null) => {}
    }
}

/// Which key spells the absence of a newtype's `Option`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NewtypeKey {
    /// The `Option` is a key of its own declaration.
    Own,
    /// A newtype struct writes its `Option` as itself, so the fields that hold
    /// the struct state its key, and `integration_tests::wire_form_census`
    /// requires each of them to skip `None` and state a default.
    Holder,
    /// An adjacently tagged newtype variant's payload is the `content` key.
    /// When the variant states no reader of its own, serde reads an absent
    /// `content` through `missing_field`, which is `None` for an `Option`.
    Content,
}

/// Whether these fields are one unnamed field: a newtype struct or variant.
fn is_newtype(fields: &syn::Fields) -> bool {
    matches!(fields, syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1)
}

/// Whether a `serde` attribute on the variant itself states `deserialize_with`
/// or `with`, which makes an absent `content` a missing field.
fn variant_states_reader(variant: &syn::Variant) -> bool {
    variant.attrs.iter().any(|attribute| {
        if !attribute.path().is_ident("serde") {
            return false;
        }
        let mut stated = false;
        attribute
            .parse_nested_meta(|meta| {
                if meta.path.is_ident("deserialize_with") || meta.path.is_ident("with") {
                    stated = true;
                }
                skip_meta_value(&meta)?;
                Ok(())
            })
            .expect("every serde attribute in the wire crates parses as a nested meta list");
        stated
    })
}

/// Which spelling of `None` a reading declaration admits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadSpelling {
    /// The key, when stated, states a value; absence is `None`.
    Present,
    /// The key is required and `null` is `None`.
    Nullable,
}

/// How the writing declaration spells a `None` at this field, which holds an
/// `Option` when `optional`.
fn write_spelling(field: &syn::Field, optional: bool) -> WriteSpelling {
    if states_skip(field) {
        WriteSpelling::Omitted
    } else if optional {
        WriteSpelling::Null
    } else {
        WriteSpelling::Always
    }
}

/// Whether this field states a serde `default`.
fn states_default(field: &syn::Field) -> bool {
    field_states(field, "default")
}

/// Whether this field states a serde `skip_serializing_if`.
fn states_skip(field: &syn::Field) -> bool {
    field_states(field, "skip_serializing_if")
}

/// Whether any `serde` attribute of this field states `name`.
fn field_states(field: &syn::Field, name: &str) -> bool {
    let mut stated = false;
    for attribute in &field.attrs {
        if !attribute.path().is_ident("serde") {
            continue;
        }
        attribute
            .parse_nested_meta(|meta| {
                if meta.path.is_ident(name) {
                    stated = true;
                }
                skip_meta_value(&meta)?;
                Ok(())
            })
            .expect("every serde attribute in the wire crates parses as a nested meta list");
    }
    stated
}

/// Whether `ty` is spelled `Option<..>`, through any path prefix and through
/// a borrow. A writing declaration states `&'a Option<T>` for a key it holds
/// by reference, which is the same optional key.
fn is_option_type(ty: &syn::Type) -> bool {
    if let syn::Type::Reference(borrowed) = ty {
        return is_option_type(&borrowed.elem);
    }
    let syn::Type::Path(path) = ty else {
        return false;
    };
    path.qself.is_none()
        && path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Option")
}

/// The `deserialize_with` (or `with`) path this field states.
fn absence_spelling(field: &syn::Field) -> Option<String> {
    let mut helper = None;
    for attribute in &field.attrs {
        if !attribute.path().is_ident("serde") {
            continue;
        }
        attribute
            .parse_nested_meta(|meta| {
                let named_helper =
                    meta.path.is_ident("deserialize_with") || meta.path.is_ident("with");
                if meta.input.peek(syn::Token![=]) {
                    let literal: syn::Lit = meta.value()?.parse()?;
                    if let syn::Lit::Str(text) = &literal {
                        if named_helper {
                            helper = Some(text.value());
                        }
                    }
                } else {
                    skip_meta_value(&meta)?;
                }
                Ok(())
            })
            .expect("every serde attribute in the wire crates parses as a nested meta list");
    }
    helper
}

/// Every hand-written `Deserialize` impl in the three wire crates, outside
/// test modules and test files, as (crate-relative path, type name).
///
/// The source is parsed, not grepped: `syn` reads each file and the walk
/// visits every `impl` item, including one nested in an inline `mod`, so a
/// header wrapped over several lines, a generic parameter list, or a
/// fully-qualified trait path is found just the same. An impl counts when the
/// trait path ends in `Deserialize` and carries its lifetime argument,
/// regardless of whether that lifetime is declared by the impl.
///
/// A `macro_rules!` body is token text, not items, so its impls are read from
/// the macro's own token stream under the same trait-and-lifetime rule; the
/// type name recorded there is the macro's metavariable.
fn hand_written_impls() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root sits two levels above the crate manifest")
        .to_path_buf();
    let mut found = Vec::new();
    for source in [
        "crates/cadmpeg-ir/src",
        "crates/cadmpeg-core/src",
        "crates/cadmpeg-asm/src",
    ] {
        let mut files = Vec::new();
        collect_rust_sources(&root.join(source), &mut files)
            .unwrap_or_else(|error| panic!("cannot collect {source}: {error}"));
        for file in files {
            let relative = file
                .strip_prefix(&root)
                .expect("a collected source sits under the repository root")
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_path(&relative) {
                continue;
            }
            let text = read_source(&file);
            let parsed = syn::parse_file(&text)
                .map_err(|error| format!("{relative} does not parse: {error}"))
                .expect("every source file in the wire crates parses");
            collect_hand_impls(&parsed.items, &relative, &mut found);
        }
    }
    found.sort();
    found
}

/// Records every hand-written `Deserialize` impl among `items`, recursing into
/// inline modules and `macro_rules!` bodies.
fn collect_hand_impls(items: &[syn::Item], relative: &str, found: &mut Vec<(String, String)>) {
    for item in items {
        match item {
            syn::Item::Impl(implementation) => {
                if let Some(name) = deserialize_impl_target(implementation) {
                    found.push((relative.to_owned(), name));
                }
            }
            syn::Item::Mod(module) => {
                if is_test_module(module) {
                    continue;
                }
                if let Some((_, nested)) = &module.content {
                    collect_hand_impls(nested, relative, found);
                }
            }
            syn::Item::Macro(macro_item) => {
                for name in macro_body_impl_targets(&macro_item.mac.tokens) {
                    found.push((relative.to_owned(), name));
                }
            }
            _ => {}
        }
    }
}

/// Whether this inline module is a test module.
fn is_test_module(module: &syn::ItemMod) -> bool {
    module.attrs.iter().any(|attribute| {
        if !attribute.path().is_ident("cfg") {
            return false;
        }
        let mut requires_test = false;
        attribute
            .parse_nested_meta(|meta| {
                requires_test = cfg_requires_test(&meta)?;
                Ok(())
            })
            .expect("every cfg attribute in the wire crates parses");
        requires_test
    })
}

/// Whether a `cfg` expression is restricted to test builds.
///
/// A substring search treats `feature = "testing"` as `cfg(test)`, and also
/// drops modules enabled by a production feature in `any(test, ...)`. The
/// recursive shape keeps only expressions that require `test` on every active
/// branch. Unknown expressions are retained for the census.
fn cfg_requires_test(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<bool> {
    if meta.path.is_ident("test") {
        return Ok(true);
    }
    if meta.path.is_ident("all") {
        let mut has_argument = false;
        let mut requires_test = false;
        meta.parse_nested_meta(|nested| {
            has_argument = true;
            requires_test |= cfg_requires_test(&nested)?;
            Ok(())
        })?;
        return Ok(has_argument && requires_test);
    }
    if meta.path.is_ident("any") {
        let mut has_argument = false;
        let mut every_branch_requires_test = true;
        meta.parse_nested_meta(|nested| {
            has_argument = true;
            every_branch_requires_test &= cfg_requires_test(&nested)?;
            Ok(())
        })?;
        return Ok(has_argument && every_branch_requires_test);
    }
    if meta.path.is_ident("not") {
        // `not(test)` is a production configuration. Retain it in the walk;
        // the source may be compiled outside the test configuration.
        meta.parse_nested_meta(|nested| {
            cfg_requires_test(&nested)?;
            Ok(())
        })?;
        return Ok(false);
    }
    if meta.input.peek(syn::Token![=]) {
        let _: syn::Lit = meta.value()?.parse()?;
    } else if meta.input.peek(syn::token::Paren) {
        meta.parse_nested_meta(|nested| {
            cfg_requires_test(&nested)?;
            Ok(())
        })?;
    }
    Ok(false)
}

/// The type a hand-written `Deserialize` impl is written for: the last segment
/// of its self type, without generic arguments.
///
/// `None` when the impl is for another trait, is an inherent impl, or has no
/// lifetime argument in the `Deserialize` trait path.
fn deserialize_impl_target(implementation: &syn::ItemImpl) -> Option<String> {
    let (path, _) = implementation.trait_.as_ref()?;
    let deserialize = path.segments.last()?;
    if deserialize.ident != "Deserialize" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &deserialize.arguments else {
        return None;
    };
    if !arguments
        .args
        .iter()
        .any(|argument| matches!(argument, syn::GenericArgument::Lifetime(_)))
    {
        return None;
    }
    match implementation.self_ty.as_ref() {
        syn::Type::Path(typed) => Some(typed.path.segments.last()?.ident.to_string()),
        _ => None,
    }
}

/// Every `Deserialize<'a> for …` target named in a `macro_rules!` body.
///
/// A macro body is token text, so it is scanned as tokens: the walk flattens
/// every delimited group and looks for `Deserialize<'a> for`, accepting
/// either tokenization proc-macro uses for a lifetime. A target spelled as a
/// metavariable is recorded with its sigil.
fn macro_body_impl_targets(tokens: &proc_macro2::TokenStream) -> Vec<String> {
    let mut flat = Vec::new();
    flatten_tokens(tokens, &mut flat);
    let mut targets = Vec::new();
    for index in 0..flat.len() {
        if flat.get(index).map(String::as_str) != Some("Deserialize")
            || flat.get(index + 1).map(String::as_str) != Some("<")
        {
            continue;
        }
        let Some(mut cursor) = index.checked_add(2) else {
            continue;
        };
        if flat
            .get(cursor)
            .is_some_and(|token| token.starts_with('\'') && token.len() > 1)
        {
            cursor += 1;
        } else if flat.get(cursor).map(String::as_str) == Some("'") {
            cursor += 2;
        } else {
            continue;
        }
        if flat.get(cursor).map(String::as_str) != Some(">")
            || flat.get(cursor + 1).map(String::as_str) != Some("for")
        {
            continue;
        }
        let after = cursor + 2;
        let Some(first) = flat.get(after) else {
            continue;
        };
        if first == "$" {
            if let Some(second) = flat.get(after + 1) {
                targets.push(format!("${second}"));
            }
        } else {
            targets.push(first.clone());
        }
    }
    targets
}

/// Appends every token of `tokens` as text, descending into delimited groups.
fn flatten_tokens(tokens: &proc_macro2::TokenStream, flat: &mut Vec<String>) {
    for tree in tokens.clone() {
        match tree {
            proc_macro2::TokenTree::Group(group) => flatten_tokens(&group.stream(), flat),
            other => flat.push(other.to_string()),
        }
    }
}

/// Strict and reserved keywords: written bare, each is the keyword.
const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "true", "try", "type",
    "typeof", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// Reads one source file with each raw identifier `r#name` spelled `name`,
/// the name rustc reads, so every comparison of names sees one spelling.
fn read_source(file: &Path) -> String {
    unraw(&std::fs::read_to_string(file).expect("read source"))
}

/// `text` with each raw identifier `r#name` spelled `name`. A keyword keeps
/// its prefix, since bare it would be the keyword. No line changes.
fn unraw(text: &str) -> String {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut unrawn = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("r#") {
        unrawn.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let length = after.find(|c: char| !is_ident(c)).unwrap_or(after.len());
        let name = &after[..length];
        let bounded = unrawn
            .chars()
            .next_back()
            .is_none_or(|c| !is_ident(c) && c != '#');
        let starts = name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_');
        if !(bounded && starts) || RUST_KEYWORDS.contains(&name) {
            unrawn.push_str("r#");
        }
        unrawn.push_str(name);
        rest = &after[length..];
    }
    unrawn.push_str(rest);
    unrawn
}

/// Whether this crate-relative path is a test file or sits in a test tree.
fn is_test_path(relative: &str) -> bool {
    relative.ends_with("/tests.rs")
        || relative.ends_with("_tests.rs")
        || relative.contains("/tests/")
        || relative.contains("/test_support")
}

/// Appends every `*.rs` under `directory`, recursively.
fn collect_rust_sources(directory: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", directory.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, found)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod scanner_tests {
    use super::{
        collect_hand_impls, collect_rust_sources, deserialize_impl_target, is_test_module,
        macro_body_impl_targets, reader_routes,
    };

    #[test]
    fn cfg_classification_requires_a_test_only_expression() {
        let feature: syn::ItemMod =
            syn::parse_str(r#"#[cfg(feature = "testing")] mod production {}"#)
                .expect("parse feature-gated module");
        assert!(!is_test_module(&feature));

        let mixed: syn::ItemMod =
            syn::parse_str(r#"#[cfg(any(test, feature = "production"))] mod production {}"#)
                .expect("parse mixed module");
        assert!(!is_test_module(&mixed));

        let test_only: syn::ItemMod =
            syn::parse_str(r#"#[cfg(all(test, feature = "test_helpers"))] mod tests {}"#)
                .expect("parse test-only module");
        assert!(is_test_module(&test_only));

        let production: syn::ItemMod =
            syn::parse_str("#[cfg(not(test))] mod production {}").expect("parse production module");
        assert!(!is_test_module(&production));
    }

    #[test]
    fn manual_deserialize_census_accepts_a_named_lifetime() {
        let implementation: syn::ItemImpl =
            syn::parse_str("impl<'wire> serde::Deserialize<'wire> for Manual {}")
                .expect("parse manual Deserialize impl");
        assert_eq!(
            deserialize_impl_target(&implementation),
            Some("Manual".into())
        );

        let static_impl: syn::ItemImpl =
            syn::parse_str("impl serde::Deserialize<'static> for Manual {}").expect("parse");
        assert_eq!(deserialize_impl_target(&static_impl), Some("Manual".into()));

        let elided_impl: syn::ItemImpl =
            syn::parse_str("impl serde::Deserialize<'_> for Manual {}").expect("parse");
        assert_eq!(deserialize_impl_target(&elided_impl), Some("Manual".into()));
    }

    #[test]
    fn macro_deserialize_census_accepts_a_named_lifetime() {
        let file: syn::File = syn::parse_str(
            r"
                macro_rules! make_reader {
                    ($name:ident) => {
                        impl<'wire> serde::Deserialize<'wire> for $name {}
                    };
                }
            ",
        )
        .expect("parse macro");
        let syn::Item::Macro(item) = &file.items[0] else {
            panic!("the fixture is a macro");
        };
        assert_eq!(macro_body_impl_targets(&item.mac.tokens), vec!["$name"]);
    }

    #[test]
    fn hand_impl_scanner_preserves_duplicate_impls_and_macro_routes() {
        let file: syn::File = syn::parse_str(
            r"
                impl<'wire> serde::Deserialize<'wire> for Manual {}
                impl<'wire> serde::Deserialize<'wire> for Manual {}
                mod nested {
                    impl<'wire> serde::Deserialize<'wire> for Nested {}
                }
                macro_rules! make_reader {
                    ($name:ident) => {
                        impl<'wire> serde::Deserialize<'wire> for $name {}
                    };
                }
            ",
        )
        .expect("parse duplicate reader fixture");
        let mut found = Vec::new();
        collect_hand_impls(&file.items, "fixture.rs", &mut found);
        assert_eq!(
            found,
            vec![
                ("fixture.rs".to_owned(), "Manual".to_owned()),
                ("fixture.rs".to_owned(), "Manual".to_owned()),
                ("fixture.rs".to_owned(), "Nested".to_owned()),
                ("fixture.rs".to_owned(), "$name".to_owned()),
            ]
        );
        assert_eq!(
            reader_routes::multiset_difference(
                &found,
                &[
                    ("fixture.rs".to_owned(), "Manual".to_owned()),
                    ("fixture.rs".to_owned(), "$name".to_owned()),
                ],
            ),
            vec![
                ("fixture.rs".to_owned(), "Manual".to_owned()),
                ("fixture.rs".to_owned(), "Nested".to_owned()),
            ]
        );
    }

    #[test]
    fn source_collector_reports_a_missing_root() {
        let missing = std::env::temp_dir().join(format!(
            "cadmpeg-unknown-key-sweep-missing-{}",
            std::process::id()
        ));
        assert!(
            !missing.exists(),
            "test path unexpectedly exists: {missing:?}"
        );

        let mut found = Vec::new();
        let error = collect_rust_sources(&missing, &mut found)
            .expect_err("a missing source root must fail the census");
        assert!(error.contains(&missing.display().to_string()));
        let cause = std::fs::read_dir(&missing).expect_err("the source root is absent");
        assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
        assert!(error.contains(&cause.to_string()), "{error}");
    }
}
