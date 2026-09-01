// SPDX-License-Identifier: Apache-2.0
//! Named projections over cadmpeg JSON artifacts.
//!
//! `cadmpeg query` reads one of the three JSON artifact kinds the CLI
//! produces — a decoded CADIR document, a versioned command report, or a
//! `<stem>.fidelity.json` decode sidecar — detects which one it was given, and prints one
//! named view. Aggregate views print tab-separated rows; `item`, `graph`, and
//! `join` print pretty-printed JSON records (or a TSV projection with `--fields`). It
//! replaces ad-hoc `jq` path exploration: the view names are stable and each
//! view's help states which artifact kinds it accepts.

mod document;
mod fidelity;
mod graph;
mod item;
mod join;
mod schema;
mod schema_infer;

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use serde::de::{IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::commands::CLI_SCHEMA_VERSION;

pub use fidelity::FidelityArgs;
pub use graph::GraphArgs;
pub use item::ItemArgs;
pub use join::JoinArgs;
pub use schema::SchemaArgs;

/// One named projection over a cadmpeg JSON artifact.
#[derive(Debug, Subcommand)]
pub enum QueryView {
    /// What this JSON file is.
    ///
    /// Accepts every artifact kind.
    Summary(QueryArgs),
    /// Decode coverage counts.
    ///
    /// Accepts a command report or a decode sidecar. Empty coverage is not an error.
    Coverage(QueryArgs),
    /// Check errors and warnings.
    ///
    /// Accepts a command report written by `check` or `convert`.
    Findings(QueryArgs),
    /// What was dropped or reduced.
    ///
    /// Accepts a command report or a decode sidecar.
    Losses(QueryArgs),
    /// Entity counts.
    ///
    /// Accepts a CADIR document (arena lengths, including `native.<codec>`
    /// arenas) or a command report (`entity_counts`).
    #[command(visible_alias = "arenas")]
    Counts(QueryArgs),
    /// One record by id.
    ///
    /// Arena names are the dotted keys from `query counts --json`
    /// (`model.<arena>` or `native.<codec>.<arena>`; a bare name means
    /// `model.<arena>`). IDs match exactly, or as a unique suffix of the
    /// JSON-string `id` field. With no IDs, prints the first record;
    /// `--head N` prints the first N. Default output is pretty-printed JSON
    /// (blank-line separated), not TSV — nested records do not fit the other
    /// views' table convention. `--fields a,b.c` projects those paths as TSV
    /// (null/absent → empty cell; arrays/objects → compact JSON; tab/newline
    /// in strings → `\t`/`\n`). Alias: `record` (also `get`).
    #[command(visible_alias = "record", alias = "get")]
    Item(ItemArgs),
    /// Fields of an entity type.
    ///
    /// With no FILE this prints what this binary's IR types allow — every
    /// field of an arena's element type, which fields are optional, and
    /// every variant of a tagged union (`FaceSelection`'s `value` is
    /// absent, a string, an array, or an object depending on `kind`; the
    /// discriminator of a feature's `definition` is
    /// `.definition.definition`). Bare `query schema` lists every model
    /// arena and its element type; `sidecar` prints the
    /// `<stem>.fidelity.json` decode-sidecar shape.
    ///
    /// Native arena records are per-document. `query schema FILE ARENA`
    /// infers each dotted path's presence, JSON type, an example, and a
    /// `relation` column (`id` / `ref` / `refs`) from the records
    /// (`layout_prefix  435/710  array`). Unknown arena names
    /// list every addressable arena and its entry count. Which arenas a
    /// given document actually has also comes from `query counts FILE`.
    Schema(SchemaArgs),
    /// Walk identity references from start records.
    ///
    /// Accepts a CADIR document. Arena names and ID selection match
    /// `query item` (`model.<arena>` or `native.<codec>.<arena>`; suffix
    /// IDs; `--head` when IDs are omitted). `--hops N` (default 1) is the
    /// maximum edge length from a start; `0` emits only the starts.
    /// Default walk follows every string that equals another record's `id`
    /// in this document. The record's own `id` is not an outgoing edge.
    /// Array members use the array's path (`links`), not `links.0`; an
    /// array of objects uses the nested field path without indices
    /// (`pcurves.pcurve`). `--follow PATH,...` keeps only those field
    /// paths (exact match). `--reverse` walks incoming references.
    /// Discover follow paths with `query schema FILE ARENA` (`relation`
    /// column). Arena names come from `query counts`. `--max-paths`
    /// (default 10000) caps explosion: a truncated walk prints a note on
    /// standard error and still exits 0. A record with no string `id`
    /// uses `ARENA#<index>` as its locator and cannot be selected by id.
    Graph(GraphArgs),
    /// Join two arenas on named key paths.
    ///
    /// Accepts a CADIR document. `--left-key` and `--right-key` are
    /// required dotted paths (no default). `--mode matched` (default) is
    /// an inner join (one row per pair). `--mode unmatched` is a left
    /// anti-join. `--mode all` keeps every left record with matching
    /// rights as an array. `--right-file` joins two documents by key
    /// value only. Arena names match `query item`. Discover key paths
    /// with `query schema FILE ARENA`. This is not SQL: no expressions,
    /// no WHERE, no three-way join.
    Join(JoinArgs),
    /// Retained source bytes.
    ///
    /// The bare view lists `retained_records` as a table (stream, offset,
    /// bytes, whether the bytes are retained, id) with annotation counts
    /// on standard error. `--stream NAME` reassembles that stream's
    /// retained bytes byte-exactly into `-o FILE` (or stdout with
    /// `--binary-stdout`) — replacing the
    /// `jq '.fidelity.retained_records[].data' | base64 -d` pipeline.
    Fidelity(FidelityArgs),
}

/// Input selection and output format for one query view.
#[derive(Debug, Args)]
pub struct QueryArgs {
    /// JSON file, or `-` for standard input.
    pub file: PathBuf,
    /// Print the projected subtree as JSON instead of the table.
    #[arg(long)]
    pub json: bool,
}

impl QueryView {
    fn args(&self) -> &QueryArgs {
        match self {
            Self::Summary(args)
            | Self::Coverage(args)
            | Self::Findings(args)
            | Self::Losses(args)
            | Self::Counts(args) => args,
            Self::Item(_) => unreachable!("item uses ItemArgs"),
            Self::Schema(_) => unreachable!("schema uses SchemaArgs"),
            Self::Graph(_) => unreachable!("graph uses GraphArgs"),
            Self::Join(_) => unreachable!("join uses JoinArgs"),
            Self::Fidelity(_) => unreachable!("fidelity uses FidelityArgs"),
        }
    }
}

/// Which artifact kind a JSON file turned out to be.
enum Artifact {
    /// A versioned CLI command report (`--report`/`-o`).
    Report(ReportProbe),
    /// A decoded CADIR document.
    Cadir(CadirProbe),
    /// A `<stem>.fidelity.json` decode sidecar.
    Sidecar(SidecarProbe),
}

impl Artifact {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Report(_) => "command report",
            Self::Cadir(_) => "CADIR document",
            Self::Sidecar(_) => "decode sidecar",
        }
    }
}

/// Top-level key sniff. Every field optional; unknown fields ignored.
#[derive(Deserialize)]
struct KindProbe {
    schema_version: Option<u32>,
    command: Option<String>,
    ir_version: Option<String>,
    version: Option<String>,
    ir_sha256: Option<String>,
}

/// Lenient shape of a CLI command report. Sections the command did not run
/// are JSON `null`; sections this build does not know stay unparsed.
#[derive(Deserialize)]
struct ReportProbe {
    schema_version: u32,
    command: String,
    /// Binary that wrote the report; absent in reports from older builds.
    #[serde(default)]
    generator: Option<String>,
    /// Present from `schema_version` 6 (`ok` | `refused`).
    #[serde(default)]
    status: Option<String>,
    /// Present from `schema_version` 6; null on success.
    #[serde(default)]
    refusal: Option<RefusalProbe>,
    #[serde(default)]
    decode_report: Option<DecodeReportProbe>,
    #[serde(default)]
    check_report: Option<CheckReportProbe>,
}

#[derive(Deserialize)]
struct RefusalProbe {
    #[serde(default)]
    stage: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// Lenient decode report: enum-valued fields read as strings so future
/// severities and loss kinds do not break projection.
#[derive(Deserialize)]
struct DecodeReportProbe {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    container_only: Option<bool>,
    #[serde(default)]
    geometry_transferred: Option<bool>,
    #[serde(default)]
    coverage: BTreeMap<String, u64>,
    #[serde(default)]
    losses: Vec<LossProbe>,
    #[serde(default)]
    dialects: Option<DialectLayersProbe>,
}

/// Lenient dialect identity. Admission stays as JSON so a future admission
/// variant does not make an otherwise projectable artifact unreadable.
#[derive(Deserialize)]
struct DialectMatchProbe {
    #[serde(default)]
    dialect: Option<String>,
    #[serde(default)]
    admission: Option<Value>,
}

#[derive(Deserialize)]
struct DialectLayersProbe {
    primary: DialectMatchProbe,
    #[serde(default)]
    extra: Vec<DialectMatchProbe>,
}

#[derive(Deserialize)]
struct CheckReportProbe {
    #[serde(default)]
    entity_counts: BTreeMap<String, u64>,
    #[serde(default)]
    findings: Vec<FindingProbe>,
    #[serde(default)]
    losses: Vec<LossProbe>,
}

#[derive(Deserialize)]
struct FindingProbe {
    #[serde(default)]
    check: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    entity: Option<String>,
}

#[derive(Deserialize)]
struct LossProbe {
    #[serde(default)]
    code: Option<LossCodeProbe>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// Accepts v1 bare strings and v2 `{ namespace, code, kind }` objects.
#[derive(Deserialize)]
#[serde(untagged)]
enum LossCodeProbe {
    Legacy(String),
    Namespaced { namespace: String, code: String },
}

impl LossCodeProbe {
    fn display(&self) -> String {
        match self {
            Self::Legacy(code) => cadmpeg_ir::LossKind::from_v1_str(code)
                .map_or_else(|| code.clone(), |kind| kind.to_string()),
            Self::Namespaced { namespace, code } => format!("{namespace}/{code}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LossCodeProbe;

    #[test]
    fn legacy_loss_codes_use_the_shared_migration_spelling() {
        assert_eq!(
            LossCodeProbe::Legacy("metadata_not_transferred".to_owned()).display(),
            "shared/metadata_not_transferred"
        );
    }

    #[test]
    fn unknown_legacy_loss_codes_remain_visible_in_lenient_queries() {
        assert_eq!(
            LossCodeProbe::Legacy("future_loss".to_owned()).display(),
            "future_loss"
        );
    }
}

/// Lenient CADIR document probe: arena contents are counted, never
/// materialized, so a large document costs one parse pass and no entity
/// allocation.
#[derive(Deserialize)]
struct CadirProbe {
    ir_version: String,
    #[serde(default)]
    source: Option<SourceProbe>,
    #[serde(default)]
    model: BTreeMap<String, ArenaLen>,
    #[serde(default)]
    native: BTreeMap<String, NativeNamespaceProbe>,
}

#[derive(Deserialize)]
struct SourceProbe {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    dialect: Option<DialectMatchProbe>,
}

#[derive(Deserialize)]
struct NativeNamespaceProbe {
    #[serde(default)]
    arenas: BTreeMap<String, ArenaLen>,
}

/// Lenient decode sidecar probe. Deliberately not `DecodeSidecar::from_json`,
/// which rejects unknown sidecar versions; query projects whatever it can.
#[derive(Deserialize)]
struct SidecarProbe {
    version: String,
    #[serde(default)]
    report: Option<DecodeReportProbe>,
}

/// Length of a JSON array, counted without materializing its elements.
/// A non-array value counts as zero instead of failing the projection.
struct ArenaLen(u64);

impl<'de> Deserialize<'de> for ArenaLen {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LenVisitor;

        impl<'de> Visitor<'de> for LenVisitor {
            type Value = ArenaLen;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an arena array")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ArenaLen, A::Error> {
                let mut len = 0;
                while seq.next_element::<IgnoredAny>()?.is_some() {
                    len += 1;
                }
                Ok(ArenaLen(len))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<ArenaLen, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(ArenaLen(0))
            }

            fn visit_unit<E>(self) -> Result<ArenaLen, E> {
                Ok(ArenaLen(0))
            }

            fn visit_bool<E>(self, _: bool) -> Result<ArenaLen, E> {
                Ok(ArenaLen(0))
            }

            fn visit_i64<E>(self, _: i64) -> Result<ArenaLen, E> {
                Ok(ArenaLen(0))
            }

            fn visit_u64<E>(self, _: u64) -> Result<ArenaLen, E> {
                Ok(ArenaLen(0))
            }

            fn visit_f64<E>(self, _: f64) -> Result<ArenaLen, E> {
                Ok(ArenaLen(0))
            }

            fn visit_str<E>(self, _: &str) -> Result<ArenaLen, E> {
                Ok(ArenaLen(0))
            }
        }

        deserializer.deserialize_any(LenVisitor)
    }
}

/// Runs one query view against one artifact file.
pub fn run(view: &QueryView) -> Result<()> {
    if let QueryView::Item(args) = view {
        return item::run(args);
    }
    if let QueryView::Schema(args) = view {
        return schema::run(args);
    }
    if let QueryView::Graph(args) = view {
        return graph::run(args);
    }
    if let QueryView::Join(args) = view {
        return join::run(args);
    }
    if let QueryView::Fidelity(args) = view {
        return fidelity::run(args);
    }
    let args = view.args();
    let bytes = read_input(&args.file)?;
    let artifact = detect(&bytes, &args.file)?;
    match view {
        QueryView::Summary(args) => {
            summary(&artifact, args);
            Ok(())
        }
        QueryView::Coverage(args) => coverage(&artifact, args),
        QueryView::Findings(args) => findings(&artifact, args),
        QueryView::Losses(args) => losses(&artifact, args),
        QueryView::Counts(args) => counts(&artifact, args),
        QueryView::Item(_)
        | QueryView::Schema(_)
        | QueryView::Graph(_)
        | QueryView::Join(_)
        | QueryView::Fidelity(_) => {
            unreachable!("handled above")
        }
    }
}

fn read_input(path: &Path) -> Result<Vec<u8>> {
    if path == Path::new("-") {
        let mut bytes = Vec::new();
        std::io::stdin()
            .lock()
            .read_to_end(&mut bytes)
            .context("reading standard input")?;
        Ok(bytes)
    } else {
        std::fs::read(path).with_context(|| format!("reading {}", path.display()))
    }
}

fn detect(bytes: &[u8], path: &Path) -> Result<Artifact> {
    let sniff: KindProbe = serde_json::from_slice(bytes).with_context(|| {
        format!(
            "{} is not a JSON object; query reads a command report (--report/-o), \
             a decoded CADIR document, or a .fidelity.json decode sidecar",
            path.display()
        )
    })?;
    if sniff.schema_version.is_some() && sniff.command.is_some() {
        let report: ReportProbe = serde_json::from_slice(bytes)
            .with_context(|| format!("parsing the command report {}", path.display()))?;
        return Ok(Artifact::Report(report));
    }
    if sniff.ir_version.is_some() {
        let cadir: CadirProbe = serde_json::from_slice(bytes)
            .with_context(|| format!("parsing the CADIR document {}", path.display()))?;
        return Ok(Artifact::Cadir(cadir));
    }
    if sniff.version.is_some() && sniff.ir_sha256.is_some() {
        let sidecar: SidecarProbe = serde_json::from_slice(bytes)
            .with_context(|| format!("parsing the decode sidecar {}", path.display()))?;
        return Ok(Artifact::Sidecar(sidecar));
    }
    bail!(
        "{} is JSON but not a recognized artifact; query reads a command report \
         (top-level `schema_version` and `command`), a decoded CADIR document \
         (`ir_version` and `model`), or a .fidelity.json decode sidecar (`version` and \
         `ir_sha256`)",
        path.display()
    )
}

/// Replaces TSV structure characters in free-form text with spaces.
fn cell(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

fn opt(text: Option<&String>) -> String {
    text.map(|value| cell(value)).unwrap_or_default()
}

fn print_json(view: &str, payload: &serde_json::Value) {
    let envelope = serde_json::json!({
        "schema_version": CLI_SCHEMA_VERSION,
        "command": format!("query {view}"),
        view: payload,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&envelope).expect("envelope serializes")
    );
}

fn counts_json(map: &BTreeMap<String, u64>) -> serde_json::Value {
    serde_json::json!(map)
}

fn summary(artifact: &Artifact, args: &QueryArgs) {
    let mut rows: Vec<(String, String)> =
        vec![("kind".to_owned(), artifact.kind_name().to_owned())];
    match artifact {
        Artifact::Report(report) => {
            rows.push((
                "schema_version".to_owned(),
                report.schema_version.to_string(),
            ));
            rows.push(("command".to_owned(), cell(&report.command)));
            if let Some(status) = &report.status {
                rows.push(("status".to_owned(), cell(status)));
            }
            if let Some(refusal) = &report.refusal {
                if let Some(stage) = &refusal.stage {
                    rows.push(("refusal_stage".to_owned(), cell(stage)));
                }
                if let Some(code) = &refusal.code {
                    rows.push(("refusal_code".to_owned(), cell(code)));
                }
                if let Some(message) = &refusal.message {
                    rows.push(("refusal_message".to_owned(), cell(message)));
                }
            }
            if let Some(generator) = &report.generator {
                rows.push(("generator".to_owned(), cell(generator)));
            }
            match &report.decode_report {
                Some(decode) => {
                    if let Some(format) = &decode.format {
                        rows.push(("decode_format".to_owned(), cell(format)));
                    }
                    if let Some(container_only) = decode.container_only {
                        rows.push(("container_only".to_owned(), container_only.to_string()));
                    }
                    if let Some(geometry) = decode.geometry_transferred {
                        rows.push(("geometry_transferred".to_owned(), geometry.to_string()));
                    }
                    rows.push((
                        "coverage_rows".to_owned(),
                        decode.coverage.len().to_string(),
                    ));
                    rows.push(("decode_losses".to_owned(), decode.losses.len().to_string()));
                    push_decode_dialect_summary(&mut rows, decode);
                }
                None => rows.push(("decode_report".to_owned(), "null".to_owned())),
            }
            match &report.check_report {
                Some(validation) => {
                    rows.push((
                        "findings".to_owned(),
                        format!(
                            "{} ({} error, {} warning)",
                            validation.findings.len(),
                            count_severity(&validation.findings, &["error", "blocking"]),
                            count_severity(&validation.findings, &["warning"]),
                        ),
                    ));
                    rows.push((
                        "check_losses".to_owned(),
                        validation.losses.len().to_string(),
                    ));
                    rows.push((
                        "entity_count_rows".to_owned(),
                        validation.entity_counts.len().to_string(),
                    ));
                }
                None => rows.push(("check_report".to_owned(), "null".to_owned())),
            }
        }
        Artifact::Cadir(cadir) => {
            rows.push(("ir_version".to_owned(), cell(&cadir.ir_version)));
            match &cadir.source {
                Some(source) => {
                    if let Some(format) = &source.format {
                        rows.push(("source_format".to_owned(), cell(format)));
                    }
                    push_dialect_summary(&mut rows, "source", source.dialect.as_ref());
                }
                None => rows.push(("source".to_owned(), "null".to_owned())),
            }
            rows.push(("model_arenas".to_owned(), cadir.model.len().to_string()));
            rows.push((
                "model_entities".to_owned(),
                cadir
                    .model
                    .values()
                    .map(|len| len.0)
                    .sum::<u64>()
                    .to_string(),
            ));
            for (namespace, probe) in &cadir.native {
                rows.push((
                    format!("native.{namespace}.arenas"),
                    probe.arenas.len().to_string(),
                ));
                rows.push((
                    format!("native.{namespace}.entities"),
                    probe
                        .arenas
                        .values()
                        .map(|len| len.0)
                        .sum::<u64>()
                        .to_string(),
                ));
            }
        }
        Artifact::Sidecar(sidecar) => {
            rows.push(("sidecar_version".to_owned(), cell(&sidecar.version)));
            match &sidecar.report {
                Some(decode) => {
                    if let Some(format) = &decode.format {
                        rows.push(("decode_format".to_owned(), cell(format)));
                    }
                    rows.push((
                        "coverage_rows".to_owned(),
                        decode.coverage.len().to_string(),
                    ));
                    rows.push(("decode_losses".to_owned(), decode.losses.len().to_string()));
                    push_decode_dialect_summary(&mut rows, decode);
                }
                None => rows.push(("report".to_owned(), "null".to_owned())),
            }
        }
    }
    if args.json {
        let map: serde_json::Map<String, serde_json::Value> = rows
            .into_iter()
            .map(|(field, value)| (field, serde_json::Value::String(value)))
            .collect();
        print_json("summary", &serde_json::Value::Object(map));
    } else {
        println!("field\tvalue");
        for (field, value) in rows {
            println!("{field}\t{value}");
        }
    }
}

fn push_decode_dialect_summary(rows: &mut Vec<(String, String)>, decode: &DecodeReportProbe) {
    match &decode.dialects {
        Some(layers) => {
            rows.push((
                "decode_dialect_layers".to_owned(),
                (1 + layers.extra.len()).to_string(),
            ));
            push_dialect_summary(rows, "decode", Some(&layers.primary));
        }
        None => push_dialect_summary(rows, "decode", None),
    }
}

fn push_dialect_summary(
    rows: &mut Vec<(String, String)>,
    prefix: &str,
    matched: Option<&DialectMatchProbe>,
) {
    let Some(matched) = matched else {
        rows.push((format!("{prefix}_dialect"), "null".to_owned()));
        return;
    };
    rows.push((
        format!("{prefix}_dialect"),
        matched
            .dialect
            .as_ref()
            .map_or_else(|| "null".to_owned(), |dialect| cell(dialect)),
    ));
    if let Some(admission) = &matched.admission {
        let value = admission
            .as_str()
            .map_or_else(|| admission.to_string(), cell);
        rows.push((format!("{prefix}_dialect_admission"), value));
    }
}

fn count_severity(findings: &[FindingProbe], severities: &[&str]) -> usize {
    findings
        .iter()
        .filter(|finding| {
            finding
                .severity
                .as_deref()
                .is_some_and(|severity| severities.contains(&severity))
        })
        .count()
}

fn coverage(artifact: &Artifact, args: &QueryArgs) -> Result<()> {
    let decode = match artifact {
        Artifact::Report(report) => report.decode_report.as_ref(),
        Artifact::Sidecar(sidecar) => sidecar.report.as_ref(),
        Artifact::Cadir(_) => bail!(
            "a CADIR document has no decode report; coverage is in the report \
             written by `dump --report` or in the `.fidelity.json` sidecar"
        ),
    };
    let (coverage, note): (&BTreeMap<String, u64>, Option<&str>) = match decode {
        Some(decode) if decode.coverage.is_empty() => {
            (&decode.coverage, Some("(coverage is empty)"))
        }
        Some(decode) => (&decode.coverage, None),
        None => {
            static EMPTY: BTreeMap<String, u64> = BTreeMap::new();
            (
                &EMPTY,
                Some("(no decode report — this command did not decode)"),
            )
        }
    };
    if args.json {
        print_json("coverage", &counts_json(coverage));
    } else {
        println!("measure\tcount");
        for (measure, count) in coverage {
            println!("{}\t{count}", cell(measure));
        }
    }
    if let Some(note) = note {
        eprintln!("{note}");
    }
    Ok(())
}

fn findings(artifact: &Artifact, args: &QueryArgs) -> Result<()> {
    let report = match artifact {
        Artifact::Report(report) => report,
        Artifact::Cadir(_) => bail!(
            "a CADIR document has no findings; run: cadmpeg check FILE -o report.json \
             && cadmpeg query findings report.json"
        ),
        Artifact::Sidecar(_) => bail!(
            "a decode sidecar has no check findings; run: cadmpeg check FILE \
             -o report.json && cadmpeg query findings report.json \
             (a sidecar carries `coverage` and `losses`)"
        ),
    };
    let (rows, note): (&[FindingProbe], Option<&str>) = match &report.check_report {
        Some(validation) => (&validation.findings, None),
        None => (&[], Some("(no check report; this command did not check)")),
    };
    if args.json {
        let payload = rows
            .iter()
            .map(|finding| {
                serde_json::json!({
                    "check": finding.check,
                    "severity": finding.severity,
                    "message": finding.message,
                    "entity": finding.entity,
                })
            })
            .collect();
        print_json("findings", &serde_json::Value::Array(payload));
    } else {
        println!("severity\tcheck\tentity\tmessage");
        for finding in rows {
            println!(
                "{}\t{}\t{}\t{}",
                opt(finding.severity.as_ref()),
                opt(finding.check.as_ref()),
                opt(finding.entity.as_ref()),
                opt(finding.message.as_ref()),
            );
        }
    }
    if let Some(note) = note {
        eprintln!("{note}");
    }
    Ok(())
}

fn losses(artifact: &Artifact, args: &QueryArgs) -> Result<()> {
    let (rows, note): (&[LossProbe], Option<&str>) = match artifact {
        Artifact::Report(report) => match (&report.check_report, &report.decode_report) {
            (Some(validation), _) => (&validation.losses, None),
            (None, Some(decode)) => (&decode.losses, None),
            (None, None) => (&[], Some("(this report has no decode or check stage)")),
        },
        Artifact::Sidecar(sidecar) => match &sidecar.report {
            Some(decode) => (&decode.losses, None),
            None => (&[], Some("(this sidecar has no decode report)")),
        },
        Artifact::Cadir(_) => bail!(
            "a CADIR document has no loss notes; losses are in the report written \
             by `--report` or in the `.fidelity.json` sidecar"
        ),
    };
    if args.json {
        let payload = rows
            .iter()
            .map(|loss| {
                serde_json::json!({
                    "code": loss.code.as_ref().map(LossCodeProbe::display),
                    "severity": loss.severity,
                    "message": loss.message,
                })
            })
            .collect();
        print_json("losses", &serde_json::Value::Array(payload));
    } else {
        println!("severity\tcode\tmessage");
        for loss in rows {
            println!(
                "{}\t{}\t{}",
                opt(loss.severity.as_ref()),
                loss.code
                    .as_ref()
                    .map(|code| cell(&code.display()))
                    .unwrap_or_default(),
                opt(loss.message.as_ref()),
            );
        }
    }
    if let Some(note) = note {
        eprintln!("{note}");
    }
    Ok(())
}

fn counts(artifact: &Artifact, args: &QueryArgs) -> Result<()> {
    match artifact {
        Artifact::Cadir(cadir) => {
            let mut rows: Vec<(String, String, u64)> = cadir
                .model
                .iter()
                .map(|(arena, len)| ("model".to_owned(), arena.clone(), len.0))
                .collect();
            for (namespace, probe) in &cadir.native {
                for (arena, len) in &probe.arenas {
                    rows.push((format!("native.{namespace}"), arena.clone(), len.0));
                }
            }
            if args.json {
                let mut map = serde_json::Map::new();
                for (namespace, arena, entries) in &rows {
                    map.insert(format!("{namespace}.{arena}"), serde_json::json!(entries));
                }
                print_json("counts", &serde_json::Value::Object(map));
            } else {
                println!("namespace\tarena\tentries");
                for (namespace, arena, entries) in rows {
                    println!("{}\t{}\t{entries}", cell(&namespace), cell(&arena));
                }
            }
            Ok(())
        }
        Artifact::Report(report) => {
            let (entity_counts, note): (&BTreeMap<String, u64>, Option<&str>) =
                match &report.check_report {
                    Some(validation) => (&validation.entity_counts, None),
                    None => {
                        static EMPTY: BTreeMap<String, u64> = BTreeMap::new();
                        (
                            &EMPTY,
                            Some("(no check report; this command did not check)"),
                        )
                    }
                };
            if args.json {
                print_json("counts", &counts_json(entity_counts));
            } else {
                println!("namespace\tarena\tentries");
                for (arena, entries) in entity_counts {
                    println!("model\t{}\t{entries}", cell(arena));
                }
            }
            if let Some(note) = note {
                eprintln!("{note}");
            }
            Ok(())
        }
        Artifact::Sidecar(_) => bail!(
            "a decode sidecar has no entity counts; use `cadmpeg query coverage` or \
             `cadmpeg query losses` on it, or run `cadmpeg check` for entity counts"
        ),
    }
}
