// SPDX-License-Identifier: Apache-2.0
//! The PSB ("Pro/E Session Binary") container: the `#UGC:2` header, the ASCII
//! table of contents, and the run of named binary sections (spec §2).
//!
//! A `.prt` opens with an ASCII header block (`#UGC:2 …` magic line through
//! `#-END_OF_UGC_HEADER`), an ASCII table of contents (`#UGC_TOC` …
//! `#END_OF_TOC_HEADER`), then a sequence of named binary sections. A real body
//! section header is the byte sequence `#\n#<name>\n` (spec §2.1): the preceding
//! byte must be the literal `#` terminator and the name a printable run, which is
//! how a `\n`-plus-name coincidence inside feature data is excluded.
//!
//! This module locates the header/TOC boundaries, enumerates and classifies the
//! sections, identifies the layout family (ND vs DEPDB), and reads the byte-backed
//! `srf_array`/`crv_array` count headers out of the visible-geometry section. It
//! does not decode geometry; that is the (mostly ungated) layer reported as loss
//! in [`crate::decode`].

use std::collections::BTreeMap;

use cadmpeg_ir::codec::{CodecError, ContainerEntry, ContainerSummary, ReadSeek};

use crate::curve::{self, CurvePrototype, CurveTopologyRow};
use crate::datum::{self, DatumPlane};
use crate::psb;
use crate::surface::{self, SurfacePrototype, SurfaceRow};
use crate::topology::{self, HalfEdge, Loop};

/// The PSB magic: every Creo `.prt` opens with this ASCII framing line.
pub const MAGIC: &[u8] = b"#UGC:2";

/// End of the UGC header block.
const UGC_HEADER_END: &[u8] = b"#-END_OF_UGC_HEADER";
/// Start of the ASCII table of contents.
const TOC_START: &[u8] = b"#UGC_TOC";
/// End of the ASCII table of contents.
const TOC_END: &[u8] = b"#END_OF_TOC_HEADER";
/// JPEG SOI magic, marking the `THMB_IMG_MAIN` preview payload (never geometry).
const JPEG_MAGIC: &[u8] = &[0xff, 0xd8, 0xff];

/// ASCII names that appear in the header/TOC framing and look like section
/// headers but are structural markers, not binary sections (spec §2.1).
const FRAMING_NAMES: &[&str] = &[
    "-END_OF_UGC_HEADER",
    "END_OF_UGC",
    "UGC_TOC",
    "END_OF_TOC_HEADER",
    "NEXT_TOC_ENTRY",
];

/// Codec-defined role labels for [`ContainerEntry::role`], grouping sections by
/// what they carry (spec §2.2).
pub mod role {
    /// Primary/invisible PSB geometry (`VisibGeom`, `NovisGeom`).
    pub const GEOMETRY: &str = "psb-geometry";
    /// Feature rows, definitions, history, datums, body counts.
    pub const MODEL_DATA: &str = "model-data";
    /// Materials, display, persistence, and other auxiliary metadata.
    pub const METADATA: &str = "metadata";
    /// The JPEG thumbnail preview (`THMB_IMG_MAIN`), excluded from geometry.
    pub const THUMBNAIL: &str = "thumbnail";
    /// A section name this codec does not classify.
    pub const OPAQUE: &str = "opaque";
}

/// The visible-geometry section name whose `srf_array`/`crv_array` counts drive
/// the inspect census.
const VISIBGEOM: &str = "VisibGeom";
/// Named active-unit selector. Unit-definition tables can contain inactive
/// systems, so this selector—not a unit-name string elsewhere in the file—is
/// authoritative.
const PRINCIPAL_UNIT_ID: &[u8] = b"_principal_sys_units_id\0";

/// The two layout families (spec §1). Dispatched structurally, not per-file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Dense PSB rows in `VisibGeom` (~40+ sections; `ND:` name decoration).
    Nd,
    /// Sparse PSB views plus a persistence database (`DEPDB_DATA`, ~12 sections).
    Depdb,
    /// Neither signature was conclusive.
    Unknown,
}

impl Layout {
    /// A short, stable token for reports and source attributes.
    pub fn token(self) -> &'static str {
        match self {
            Layout::Nd => "ND",
            Layout::Depdb => "DEPDB",
            Layout::Unknown => "unknown",
        }
    }
}

/// One enumerated binary section.
#[derive(Debug, Clone)]
pub struct Section {
    /// Section name with any `ND:0:` prefix / `ModelView#N` suffix stripped.
    pub name: String,
    /// Raw name as it appeared in the header, when decorated.
    pub raw_name: String,
    /// Byte offset of the section header within the file.
    pub offset: usize,
    /// Payload length in bytes (header to the next section, or EOF).
    pub length: usize,
    /// Role classification.
    pub role: &'static str,
}

/// The byte-backed count headers read from the visible-geometry section.
#[derive(Debug, Clone, Default)]
pub struct GeomCensus {
    /// `srf_array\0 f8 <count>` surface-namespace count, when present.
    pub srf_array_count: Option<u32>,
    /// `crv_array\0 [f3] f8 <count>` curve-namespace count, when present.
    pub crv_array_count: Option<u32>,
}

/// Everything read from a `.prt`, shared by `inspect` and `decode`.
pub struct ContainerScan {
    /// The whole file image.
    pub data: Vec<u8>,
    /// The magic/version header line, ASCII, trimmed.
    pub version_line: String,
    /// Enumerated sections in file order.
    pub sections: Vec<Section>,
    /// Identified layout family.
    pub layout: Layout,
    /// Visible-geometry namespace census, when a `VisibGeom` section was found.
    pub census: GeomCensus,
    /// Active Creo principal coordinate unit system, when its selector is
    /// present. Both currently defined systems store model lengths in mm.
    pub principal_unit: Option<String>,
    /// Typed fixed-prefix surface rows from visible and invisible geometry
    /// sections. Parameter bodies are decoded separately.
    pub surface_rows: Vec<SurfaceRow>,
    /// Labeled surface prototypes with fully decoded scalar fields.
    pub surface_prototypes: Vec<SurfacePrototype>,
    /// Labeled curve prototypes from geometry sections. The curve body and
    /// its analytic interpretation are decoded separately.
    pub curve_prototypes: Vec<CurvePrototype>,
    /// Curve rows with an unambiguous canonical four-reference topology
    /// suffix. These rows define the native half-edge adjacency graph.
    pub curve_topology_rows: Vec<CurveTopologyRow>,
    /// Resolved native half-edges and closed loops built from curve rows.
    pub half_edges: Vec<HalfEdge>,
    /// Closed rings of half-edges, one per resolved face loop.
    pub loops: Vec<Loop>,
    /// Model-space standard datum planes decoded from `ActDatums` outlines.
    pub datum_planes: Vec<DatumPlane>,
    /// Feature IDs that own decoded geometry rows.
    pub feature_ids: Vec<u32>,
}

/// Whether a byte prefix is a Creo PSB `.prt`: the `#UGC:2` ASCII magic is the
/// container signature (spec §2.1). Detection is magic-based, never
/// extension-based, because `.prt` is shared with Siemens NX (spec §1).
pub fn looks_like_creo(prefix: &[u8]) -> bool {
    prefix.starts_with(MAGIC)
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

fn line_at(data: &[u8], start: usize) -> String {
    let end = find(data, b"\n", start).unwrap_or(data.len());
    String::from_utf8_lossy(&data[start..end])
        .trim()
        .to_string()
}

/// Normalize a decorated section name to its base (spec §2.1): strip a
/// `ModelView#N` suffix and an `ND:0:<Name>:N` decoration.
fn normalize_name(raw: &str) -> String {
    let base = raw.split('#').next().unwrap_or(raw);
    if let Some(rest) = base.strip_prefix("ND:") {
        // ND:0:Name:N  ->  parts = ["0", "Name", "N"]; the name is index 1.
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() >= 2 {
            return parts[1].to_string();
        }
    }
    base.to_string()
}

/// Classify a normalized section name by what it carries (spec §2.2).
fn classify(name: &str) -> &'static str {
    match name {
        "VisibGeom" | "NovisGeom" | "ActDatums" => role::GEOMETRY,
        "AllFeatur" | "FeatDefs" | "FeatDefsIndex" | "FeatDefsDtm" | "Geomlists" | "GeomDepen"
        | "Model_L05_PX" | "Model_L05P" | "BasicData" | "BasBasData" | "BasFullData"
        | "FullMData" => role::MODEL_DATA,
        "THMB_IMG_MAIN" => role::THUMBNAIL,
        "NeuPrtSld" | "NeuAsmSld" | "SolidPersistTable" | "SolidPrimdata" | "DEPDB_DATA"
        | "UnitSystemDef_L03" | "PDMTrail_L03" | "ActEntity" | "MdlStatus" | "MdlRefInfo"
        | "DispCntrl" | "ColorSchemeInfo" | "LargeText" | "BasicText" | "IdsGenInfoDb" => {
            role::METADATA
        }
        _ => role::OPAQUE,
    }
}

/// Enumerate binary sections from `body_start` to EOF by the `\n#<name>\n`
/// header rule (spec §2.1). A candidate header is accepted only when its name is
/// a printable run and is not one of the header/TOC framing markers.
fn scan_sections(data: &[u8], body_start: usize) -> Vec<Section> {
    // Collect header hits as (offset_of_section_hash, raw_name).
    let mut hits: Vec<(usize, String)> = Vec::new();
    let search_start = body_start.saturating_sub(1);
    let mut i = search_start;
    while i + 1 < data.len() {
        if data[i] != b'\n' || data[i + 1] != b'#' {
            i += 1;
            continue;
        }
        let hash_off = i + 1; // offset of the section-header '#'
        let name_start = i + 2;
        let Some(nl) = find(data, b"\n", name_start) else {
            break;
        };
        let name_bytes = &data[name_start..nl];
        i = nl; // continue scanning after this line regardless of acceptance
                // A real section name is a printable run with at least one alphanumeric
                // character; this rejects TOC/EOF padding lines made only of `#`.
        if !name_bytes.iter().all(|&b| is_name_byte(b))
            || name_bytes.len() < 2
            || !name_bytes.iter().any(u8::is_ascii_alphanumeric)
        {
            continue;
        }
        let raw = String::from_utf8_lossy(name_bytes).to_string();
        if FRAMING_NAMES.contains(&raw.as_str()) {
            continue;
        }
        hits.push((hash_off, raw));
    }

    let mut sections = Vec::with_capacity(hits.len());
    for (idx, (hdr_off, raw)) in hits.iter().enumerate() {
        let end = hits.get(idx + 1).map_or(data.len(), |(next, _)| *next);
        let name = normalize_name(raw);
        let role = classify(&name);
        sections.push(Section {
            name,
            raw_name: raw.clone(),
            offset: *hdr_off,
            length: end.saturating_sub(*hdr_off),
            role,
        });
    }
    sections
}

/// Section-name bytes: printable ASCII minus space, plus the `ND:` decoration
/// punctuation and the `ModelView#N` separator.
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'.' | b'-' | b'#')
}

/// Identify the layout family structurally (spec §1). ND is signalled by `ND:`
/// name decoration or a large section count; DEPDB by a `DEPDB_DATA` section
/// with a sparse section list.
fn identify_layout(sections: &[Section]) -> Layout {
    let has_nd_decoration = sections.iter().any(|s| s.raw_name.starts_with("ND:"));
    let has_depdb = sections.iter().any(|s| s.name == "DEPDB_DATA");
    if has_nd_decoration {
        Layout::Nd
    } else if has_depdb && sections.len() <= 24 {
        Layout::Depdb
    } else if sections.len() >= 32 {
        Layout::Nd
    } else if has_depdb {
        Layout::Depdb
    } else {
        Layout::Unknown
    }
}

/// Sum every valid `<label>\0 [skip] f8 <count>` header in `region`.
/// After the label's NUL terminator, up to two optional non-`f8` framing bytes
/// (e.g. the `f3`/`f2` `crv_array` discriminators) are skipped before the
/// required `f8` opener, whose compact-integer count is then decoded (spec §4,
/// §5).
fn read_array_count(region: &[u8], label: &[u8]) -> Option<u32> {
    let mut from = 0;
    let mut total = 0u32;
    let mut found = false;
    while let Some(pos) = find(region, label, from) {
        let mut p = pos + label.len();
        // Require the NUL that terminates the namespace label.
        if region.get(p) == Some(&0) {
            p += 1;
            // Skip up to two framing bytes before the array opener.
            for _ in 0..3 {
                match region.get(p) {
                    Some(&psb::token::ARRAY_OPEN) => {
                        let (count, _) = psb::compact_int(region, p + 1);
                        total = total.saturating_add(count);
                        found = true;
                        break;
                    }
                    Some(_) => p += 1,
                    None => break,
                }
            }
        }
        from = pos + 1;
    }
    found.then_some(total)
}

/// Read the visible-geometry namespace census from the `VisibGeom` section body.
fn geom_census(data: &[u8], sections: &[Section]) -> GeomCensus {
    let Some(vg) = sections
        .iter()
        .find(|s| s.name == VISIBGEOM)
        .or_else(|| sections.iter().find(|s| s.name == "DEPDB_DATA"))
    else {
        return GeomCensus::default();
    };
    let region = &data[vg.offset..(vg.offset + vg.length).min(data.len())];
    GeomCensus {
        srf_array_count: read_array_count(region, b"srf_array"),
        crv_array_count: read_array_count(region, b"crv_array"),
    }
}

/// Decode the active unit-system selector. `51` is millimeter-Newton-Second
/// and `55` is millimeter-Kilogram-Second; both use millimeters for lengths.
fn principal_unit(data: &[u8]) -> Option<String> {
    let start = find(data, PRINCIPAL_UNIT_ID, 0)? + PRINCIPAL_UNIT_ID.len();
    match *data.get(start)? {
        51 => Some("mmNs".to_string()),
        55 => Some("mmKs".to_string()),
        value => Some(format!("unknown:{value}")),
    }
}

fn surface_rows(data: &[u8], sections: &[Section]) -> Vec<SurfaceRow> {
    let mut rows = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY)
    {
        let end = (section.offset + section.length).min(data.len());
        rows.extend(
            surface::rows(&data[section.offset..end])
                .into_iter()
                .map(|mut row| {
                    row.offset += section.offset;
                    row
                }),
        );
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn surface_prototypes(data: &[u8], sections: &[Section]) -> Vec<SurfacePrototype> {
    let mut prototypes = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY)
    {
        let end = (section.offset + section.length).min(data.len());
        prototypes.extend(
            surface::prototypes(&data[section.offset..end])
                .into_iter()
                .map(|mut prototype| {
                    prototype.offset += section.offset;
                    prototype
                }),
        );
    }
    prototypes.sort_by_key(|prototype| prototype.offset);
    prototypes
}

fn curve_prototypes(data: &[u8], sections: &[Section]) -> Vec<CurvePrototype> {
    let mut prototypes = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY)
    {
        let end = (section.offset + section.length).min(data.len());
        prototypes.extend(
            curve::prototypes(&data[section.offset..end])
                .into_iter()
                .map(|mut prototype| {
                    prototype.offset += section.offset;
                    prototype
                }),
        );
    }
    prototypes.sort_by_key(|prototype| prototype.offset);
    prototypes
}

fn curve_topology_rows(data: &[u8], sections: &[Section]) -> Vec<CurveTopologyRow> {
    let mut rows = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY)
    {
        let end = (section.offset + section.length).min(data.len());
        rows.extend(
            curve::topology_rows(&data[section.offset..end])
                .into_iter()
                .map(|mut row| {
                    row.offset += section.offset;
                    row
                }),
        );
    }
    rows.sort_by_key(|row| row.offset);
    rows
}

fn datum_planes(data: &[u8], sections: &[Section]) -> Vec<DatumPlane> {
    let mut planes = Vec::new();
    for section in sections
        .iter()
        .filter(|section| section.name == "ActDatums")
    {
        let end = (section.offset + section.length).min(data.len());
        planes.extend(
            datum::planes(&data[section.offset..end])
                .into_iter()
                .map(|mut plane| {
                    plane.offset_in_payload += section.offset;
                    plane
                }),
        );
        if let Some(mut plane) = datum::named_zero_plane(&data[section.offset..end]) {
            plane.offset_in_payload += section.offset;
            planes.push(plane);
        }
    }
    planes.sort_by_key(|plane| plane.offset_in_payload);
    planes
}

fn feature_ids(data: &[u8], sections: &[Section], rows: &[SurfaceRow]) -> Vec<u32> {
    let mut ids = std::collections::BTreeSet::new();
    ids.extend(rows.iter().map(|row| row.feature_id).filter(|id| *id != 0));
    for section in sections
        .iter()
        .filter(|section| section.role == role::GEOMETRY)
    {
        let end = (section.offset + section.length).min(data.len());
        let payload = &data[section.offset..end];
        let mut from = 0;
        while let Some(found) = find(payload, b"parent_feats\0", from) {
            let start = found + b"parent_feats\0".len();
            let Some(&psb::token::ARRAY_OPEN) = payload.get(start) else {
                from = start;
                continue;
            };
            let (count, mut cursor) = psb::compact_int(payload, start + 1);
            for _ in 0..count {
                let (id, next) = psb::compact_int(payload, cursor);
                if next == cursor {
                    break;
                }
                if id != 0 {
                    ids.insert(id);
                }
                cursor = next;
            }
            from = start;
        }
    }
    ids.into_iter().collect()
}

/// Read the whole file and parse its container framing.
pub fn scan(reader: &mut dyn ReadSeek) -> Result<ContainerScan, CodecError> {
    reader
        .seek(std::io::SeekFrom::Start(0))
        .map_err(CodecError::Io)?;
    let mut data = Vec::new();
    reader.read_to_end(&mut data).map_err(CodecError::Io)?;
    Ok(scan_bytes(data))
}

/// Parse a whole `.prt` byte image. Split out so tests drive it from a synthetic
/// buffer without a reader.
pub fn scan_bytes(data: Vec<u8>) -> ContainerScan {
    let version_line = line_at(&data, 0);

    // The binary body begins after the ASCII header and TOC. Prefer the TOC end
    // marker; fall back to the header end; fall back to the magic line.
    let header_end = find(&data, UGC_HEADER_END, 0)
        .and_then(|p| find(&data, b"\n", p))
        .map(|nl| nl + 1);
    let toc_end = find(&data, TOC_START, 0)
        .and_then(|toc| find(&data, TOC_END, toc))
        .and_then(|p| find(&data, b"\n", p))
        .map(|nl| nl + 1);
    let body_start = toc_end.or(header_end).unwrap_or(0);

    let sections = scan_sections(&data, body_start);
    let layout = identify_layout(&sections);
    let census = geom_census(&data, &sections);
    let principal_unit = principal_unit(&data);
    let surface_rows = surface_rows(&data, &sections);
    let surface_prototypes = surface_prototypes(&data, &sections);
    let curve_prototypes = curve_prototypes(&data, &sections);
    let curve_topology_rows = curve_topology_rows(&data, &sections);
    let (half_edges, loops) = topology::build(&curve_topology_rows);
    let datum_planes = datum_planes(&data, &sections);
    let feature_ids = feature_ids(&data, &sections, &surface_rows);

    ContainerScan {
        data,
        version_line,
        sections,
        layout,
        census,
        principal_unit,
        surface_rows,
        surface_prototypes,
        curve_prototypes,
        curve_topology_rows,
        half_edges,
        loops,
        datum_planes,
        feature_ids,
    }
}

/// Whether the file carries a JPEG thumbnail payload (spec §2.2). Informational
/// only; the thumbnail is never geometry.
pub fn has_thumbnail(scan: &ContainerScan) -> bool {
    scan.sections
        .iter()
        .filter(|s| s.role == role::THUMBNAIL)
        .any(|s| {
            let region = &scan.data[s.offset..(s.offset + s.length).min(scan.data.len())];
            find(region, JPEG_MAGIC, 0).is_some()
        })
}

/// Build a [`ContainerSummary`] enumerating sections and reporting the layout,
/// census, and the honest scope of what decode can and cannot do.
pub fn summarize(scan: &ContainerScan) -> ContainerSummary {
    let entries = scan
        .sections
        .iter()
        .map(|s| {
            let mut attributes = BTreeMap::new();
            attributes.insert("offset".to_string(), s.offset.to_string());
            if s.raw_name != s.name {
                attributes.insert("raw_name".to_string(), s.raw_name.clone());
            }
            ContainerEntry {
                name: s.name.clone(),
                role: s.role.to_string(),
                compression: "none".to_string(),
                compressed_size: s.length as u64,
                uncompressed_size: s.length as u64,
                attributes,
            }
        })
        .collect();

    let mut notes = vec![
        format!("PSB container: {}", scan.version_line),
        format!(
            "layout: {}; {} section(s) enumerated",
            scan.layout.token(),
            scan.sections.len()
        ),
    ];

    match (scan.census.srf_array_count, scan.census.crv_array_count) {
        (None, None) => {
            notes.push("no VisibGeom srf_array/crv_array count header was located".to_string());
        }
        (srf, crv) => {
            notes.push(format!(
                "VisibGeom namespace census: srf_array={}, crv_array={} (byte-backed count \
                 headers; per-instance row geometry is not decoded)",
                srf.map_or_else(|| "n/a".to_string(), |c| c.to_string()),
                crv.map_or_else(|| "n/a".to_string(), |c| c.to_string()),
            ));
        }
    }

    if has_thumbnail(scan) {
        notes.push("THMB_IMG_MAIN carries a JPEG preview (excluded from geometry)".to_string());
    }

    notes.push(
        "container-level enumeration; `decode` performs an honest structural decode only: PSB \
         geometry is preserved as unknown records with counted loss notes, no geometry is \
         transferred (per-instance model-space geometry is gated behind undecoded PSB layers)"
            .to_string(),
    );

    ContainerSummary {
        format: "creo".to_string(),
        container_kind: "psb".to_string(),
        entries,
        notes,
    }
}
