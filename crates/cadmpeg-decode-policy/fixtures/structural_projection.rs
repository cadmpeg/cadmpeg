// SPDX-License-Identifier: Apache-2.0
#![feature(allocator_api)]
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::structural::project;
use serde::ser::Serializer;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Serialize)]
pub struct BoundedSource<'a> {
    pub label: &'a str,
    pub values: Vec<Option<Box<u32>>>,
    pub entries: BTreeMap<String, String>,
    pub unique: BTreeSet<u32>,
    pub recursive: Option<Box<RecursiveSource<'a>>>,
}

#[derive(Serialize)]
pub struct RecursiveSource<'a> {
    pub label: &'a str,
    pub children: Vec<Box<RecursiveSource<'a>>>,
}

#[derive(Serialize)]
pub struct ExpandingSource<T> {
    pub next: Option<Box<ExpandingSource<Vec<T>>>>,
    pub value: T,
}

#[derive(Serialize)]
#[serde(transparent)]
pub struct TransparentReferenceCycle(pub &'static TransparentReferenceCycle);

pub static TRANSPARENT_REFERENCE_CYCLE: TransparentReferenceCycle =
    TransparentReferenceCycle(&TRANSPARENT_REFERENCE_CYCLE);

#[derive(Serialize)]
pub struct HashMapSource {
    pub entries: HashMap<String, String>,
}

#[derive(Serialize)]
pub struct HashSetSource {
    pub entries: HashSet<String>,
}

#[derive(Serialize)]
pub struct PredicateSource {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub label: String,
}

#[derive(Serialize)]
pub struct BoundedPredicateSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
}

mod shadowed_skip {
    use serde::Serialize;

    pub struct Option;

    impl Option {
        pub fn is_none(value: &String) -> bool {
            for byte in value.as_bytes() {
                std::hint::black_box(byte);
            }
            false
        }
    }

    #[derive(Serialize)]
    pub struct Source {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub value: String,
    }
}

fn serialize_text<S: Serializer>(value: &String, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(value)
}

#[derive(Serialize)]
pub struct CustomFieldSource {
    #[serde(serialize_with = "serialize_text")]
    pub label: String,
}

mod string_wire {
    use serde::de::Deserializer;
    use serde::ser::Serializer;
    use serde::{Deserialize, Serialize};

    pub fn serialize<S: Serializer>(value: &String, serializer: S) -> Result<S::Ok, S::Error> {
        value.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(parser: D) -> Result<String, D::Error> {
        String::deserialize(parser)
    }
}

#[derive(Serialize)]
pub struct ModuleFieldSource {
    #[serde(with = "string_wire")]
    pub label: String,
}

#[derive(Serialize)]
pub struct FlattenedSource {
    #[serde(flatten)]
    pub entries: BTreeMap<String, String>,
}

#[derive(Serialize)]
pub struct FixedFlattenedFields {
    pub label: u32,
    pub children: Vec<String>,
}

#[derive(Serialize)]
pub struct FixedFlattenedSource {
    #[serde(flatten)]
    pub fields: FixedFlattenedFields,
}

pub fn project_fixed_flattened_record(
    ctx: &DecodeContext<'_>,
    source: &FixedFlattenedSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "fixed flattened record")?);
    Ok(())
}

#[derive(Serialize)]
pub struct IntoTarget {
    pub label: String,
}

#[derive(Clone, Serialize)]
#[serde(into = "IntoTarget")]
pub struct IntoConversionSource {
    pub label: String,
}

impl From<IntoConversionSource> for IntoTarget {
    fn from(source: IntoConversionSource) -> Self {
        let length = source.label.chars().count();
        Self {
            label: format!("{length}:{}", source.label),
        }
    }
}

pub struct RemoteSource {
    pub label: String,
}

fn remote_label(source: &RemoteSource) -> String {
    source.label.chars().collect()
}

#[derive(Serialize)]
#[serde(remote = "RemoteSource")]
struct RemoteWire {
    #[serde(getter = "remote_label")]
    label: String,
}

impl Serialize for RemoteSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RemoteWire::serialize(self, serializer)
    }
}

pub struct ScanningSource<'a>(pub &'a str);

#[automatically_derived]
impl Serialize for ScanningSource<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let copy: String = self.0.chars().collect();
        serializer.serialize_str(&copy)
    }
}

pub struct BorrowedTextSource {
    pub text: String,
}

impl Serialize for BorrowedTextSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.text.as_str())
    }
}

pub struct StaticTextSource;

impl Serialize for StaticTextSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("fixed schema label")
    }
}

pub struct ScalarFieldSource(pub i64);

impl Serialize for ScalarFieldSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(self.0)
    }
}

#[derive(Clone, Copy)]
pub enum FixedFormatSource {
    Step,
    Rhino,
}

impl FixedFormatSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Step => "step",
            Self::Rhino => "rhino",
        }
    }
}

impl Serialize for FixedFormatSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

pub struct AllocatingAccessorSource {
    pub text: String,
}

impl AllocatingAccessorSource {
    fn owned_text(&self) -> String {
        self.text.clone()
    }
}

impl Serialize for AllocatingAccessorSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let text = self.owned_text();
        serializer.serialize_str(&text)
    }
}

pub struct ScanningAccessorSource {
    pub text: String,
}

impl ScanningAccessorSource {
    fn as_str(&self) -> &str {
        for byte in self.text.as_bytes() {
            std::hint::black_box(byte);
        }
        self.text.as_str()
    }
}

impl Serialize for ScanningAccessorSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

pub struct SwallowedSerializerResultSource {
    pub text: String,
}

pub struct LazyLockTextSource {
    pub text: std::sync::LazyLock<String>,
}

impl Serialize for LazyLockTextSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.text.as_str())
    }
}

impl Serialize for SwallowedSerializerResultSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let _ignored = serializer.serialize_str(self.text.as_str());
        Err(<S::Error as serde::ser::Error>::custom(
            "discarded serializer result",
        ))
    }
}

pub fn project_bounded_source(
    ctx: &DecodeContext<'_>,
    source: &BoundedSource<'_>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "bounded structural source")?);
    Ok(())
}

pub fn project_recursive_source(
    ctx: &DecodeContext<'_>,
    source: &RecursiveSource<'_>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "recursive structural source")?);
    Ok(())
}

pub fn project_expanding_source(
    ctx: &DecodeContext<'_>,
    source: &ExpandingSource<u32>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "expanding structural source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_transparent_reference_cycle(
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    drop(project( // finding: unproven_decode_charge
        ctx,
        &TRANSPARENT_REFERENCE_CYCLE,
        "transparent reference cycle",
    )?);
    Ok(())
}

pub fn project_ir_severity(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::report::Severity,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "IR severity structural source")?);
    Ok(())
}

pub fn project_ir_finding(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::report::check::Finding,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "IR finding structural source")?);
    Ok(())
}

pub fn project_hash_map_source(
    ctx: &DecodeContext<'_>,
    source: &HashMapSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "hash map structural source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_hash_set_source(
    ctx: &DecodeContext<'_>,
    source: &HashSetSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "hash set structural source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_predicate_source(
    ctx: &DecodeContext<'_>,
    source: &PredicateSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "predicate structural source")?);
    Ok(())
}

pub fn project_bounded_predicate_source(
    ctx: &DecodeContext<'_>,
    source: &BoundedPredicateSource,
) -> Result<(), CodecError> {
    drop(project(
        ctx,
        source,
        "bounded predicate structural source",
    )?);
    Ok(())
}

pub fn project_shadowed_skip_source(
    ctx: &DecodeContext<'_>,
    source: &shadowed_skip::Source,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "shadowed skip helper")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_custom_field_source(
    ctx: &DecodeContext<'_>,
    source: &CustomFieldSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "custom field structural source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_module_field_source(
    ctx: &DecodeContext<'_>,
    source: &ModuleFieldSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "module field structural source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_flattened_source(
    ctx: &DecodeContext<'_>,
    source: &FlattenedSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "flattened structural source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_into_source(
    ctx: &DecodeContext<'_>,
    source: &IntoConversionSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "into structural source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_remote_source(
    ctx: &DecodeContext<'_>,
    source: &RemoteSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "remote structural source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_custom_serializer_source(
    ctx: &DecodeContext<'_>,
    source: &ScanningSource<'_>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "custom serializer structural source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_borrowed_text_source(
    ctx: &DecodeContext<'_>,
    source: &BorrowedTextSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "borrowed text structural source")?);
    Ok(())
}

pub fn project_static_text_source(
    ctx: &DecodeContext<'_>,
    source: &StaticTextSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "static text structural source")?);
    Ok(())
}

pub fn project_scalar_field_source(
    ctx: &DecodeContext<'_>,
    source: &ScalarFieldSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "scalar field structural source")?);
    Ok(())
}

pub fn project_fixed_format_source(
    ctx: &DecodeContext<'_>,
    source: &FixedFormatSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "finite enum structural source")?);
    Ok(())
}

pub fn project_allocating_accessor_source(
    ctx: &DecodeContext<'_>,
    source: &AllocatingAccessorSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "allocating accessor structural source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_scanning_accessor_source(
    ctx: &DecodeContext<'_>,
    source: &ScanningAccessorSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "scanning accessor structural source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_swallowed_result_source(
    ctx: &DecodeContext<'_>,
    source: &SwallowedSerializerResultSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "swallowed serializer result")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_lazy_lock_source(
    ctx: &DecodeContext<'_>,
    source: &LazyLockTextSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "lazy lock structural source")?); // finding: unproven_decode_charge
    Ok(())
}

#[derive(serde::Serialize)]
pub struct CustomCowValue {
    text: String,
}

pub struct CustomCowStorage {
    value: CustomCowValue,
}

impl std::borrow::Borrow<CustomCowValue> for CustomCowStorage {
    fn borrow(&self) -> &CustomCowValue {
        for byte in self.value.text.bytes() {
            std::hint::black_box(byte);
        }
        &self.value
    }
}

impl std::borrow::ToOwned for CustomCowValue {
    type Owned = CustomCowStorage;

    fn to_owned(&self) -> Self::Owned {
        CustomCowStorage {
            value: CustomCowValue {
                text: self.text.clone(),
            },
        }
    }
}

pub fn project_custom_cow(
    ctx: &DecodeContext<'_>,
    source: &std::borrow::Cow<'_, CustomCowValue>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "custom cow structural source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_text_cow(
    ctx: &DecodeContext<'_>,
    source: &std::borrow::Cow<'_, str>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "text cow structural source")?);
    Ok(())
}

pub fn project_slice_cow(
    ctx: &DecodeContext<'_>,
    source: &std::borrow::Cow<'_, [String]>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "slice cow structural source")?);
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(untagged)]
pub enum UnchargedVariantCycle<'a> {
    Scalar(u32),
    Recursive(&'a UnchargedVariantCycle<'a>),
}

pub fn project_uncharged_variant_cycle(
    ctx: &DecodeContext<'_>,
    source: &UnchargedVariantCycle<'_>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "uncharged variant cycle")?); // finding: unproven_decode_charge
    Ok(())
}

#[derive(Serialize)]
pub struct AllocatorBoxPayload {
    pub text: String,
}

#[derive(Clone, Copy)]
pub struct LocalAllocator;

unsafe impl std::alloc::Allocator for LocalAllocator {
    fn allocate(
        &self,
        layout: std::alloc::Layout,
    ) -> Result<std::ptr::NonNull<[u8]>, std::alloc::AllocError> {
        std::alloc::Allocator::allocate(&std::alloc::Global, layout)
    }

    unsafe fn deallocate(&self, pointer: std::ptr::NonNull<u8>, layout: std::alloc::Layout) {
        unsafe { std::alloc::Allocator::deallocate(&std::alloc::Global, pointer, layout); }
    }
}

impl Serialize for Box<AllocatorBoxPayload, LocalAllocator> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        for byte in self.text.bytes() {
            std::hint::black_box(byte);
        }
        serializer.serialize_str(&self.text)
    }
}

pub fn project_custom_allocator_box(
    ctx: &DecodeContext<'_>,
    source: &Box<AllocatorBoxPayload, LocalAllocator>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "custom allocator box")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn project_global_allocator_box(
    ctx: &DecodeContext<'_>,
    source: &Box<AllocatorBoxPayload>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "global allocator box")?);
    Ok(())
}

#[derive(Serialize)]
pub struct BoxedFixedFlattenedSource {
    #[serde(flatten)]
    pub fields: Box<FixedFlattenedFields>,
}

pub fn project_boxed_fixed_flattened_record(
    ctx: &DecodeContext<'_>,
    source: &BoxedFixedFlattenedSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "boxed fixed flattened record")?);
    Ok(())
}
