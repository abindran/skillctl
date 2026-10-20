//! `telemetry serve|enable|disable|status|report`: a local OpenTelemetry
//! collector for Claude Code, so tokens and cost per model, skill and agent are
//! recorded without running any other service.
//!
//! Claude Code exports OTLP over HTTP/JSON with a `Content-Length` on every
//! request, so the receiver is a small HTTP/1.1 server on 127.0.0.1. Each log
//! record and metric data point becomes one JSON line under
//! `$SKILLCTL_HOME/telemetry/<UTC date>.jsonl`. Identity and content attributes
//! (email, account and organisation ids, prompts, responses, tool inputs and
//! error text) are dropped before anything is written; only names, models,
//! counts, durations and costs stay.
//!
//! `enable` writes the export variables into the `env` block of
//! `~/.claude/settings.json` (merged and backed up like hook registrations)
//! and, on macOS, runs `skillctl telemetry serve` as a login service through a
//! LaunchAgent, so the collector is up whenever Claude Code is.
use crate::{
    cli::TelemetryCommand,
    context::Context,
    error::Error,
    harness::{self, claude},
};
use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::Duration as StdDuration,
};

/// The OTLP/HTTP default port.
pub const DEFAULT_PORT: u16 = 4318;
/// The LaunchAgent label; also its file name under `~/Library/LaunchAgents`.
pub const LABEL: &str = "dev.abindran.skillctl.collector";
/// Answered on `GET /health` so `status` and `doctor` can tell this collector
/// from another program on the same port.
const SERVICE: &str = "skillctl-collector";
const MAX_HEADERS: usize = 64 * 1024;
const MAX_BODY: usize = 32 * 1024 * 1024;
const READ_TIMEOUT: StdDuration = StdDuration::from_secs(30);

/// Attributes never written: who the user is and what they or the model said.
const DROPPED: &[&str] = &[
    "user.email",
    "user.account_uuid",
    "user.account_id",
    "user.id",
    "organization.id",
    "prompt",
    "prompt_text",
    "response",
    "response_text",
    "tool_parameters",
    "tool_input",
    "error",
];
/// Startup events that describe configuration, not work: one per hook and
/// plugin on every session start, so they would dominate the files.
const SKIPPED: &[&str] = &[
    "hook_registered",
    "plugin_loaded",
    "managed_settings_resolved",
];
/// Names worth keeping from `tool_parameters` before it is dropped.
const PARAMETERS_KEPT: &[&str] = &[
    "skill_name",
    "subagent_type",
    "mcp_server_name",
    "mcp_tool_name",
];

/// The variables `enable` writes, in the order they are reported.
pub fn exports(port: u16) -> Vec<(&'static str, String)> {
    vec![
        ("CLAUDE_CODE_ENABLE_TELEMETRY", "1".to_owned()),
        ("OTEL_METRICS_EXPORTER", "otlp".to_owned()),
        ("OTEL_LOGS_EXPORTER", "otlp".to_owned()),
        ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/json".to_owned()),
        ("OTEL_EXPORTER_OTLP_ENDPOINT", endpoint(port)),
        // Without it skill.name and agent.name are exported as "custom".
        ("OTEL_LOG_TOOL_DETAILS", "1".to_owned()),
    ]
}

pub fn endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn data_dir(home: &Path) -> PathBuf {
    home.join("telemetry")
}

// MARK: Records

/// One log record or metric data point, as written to the day file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Record {
    /// When the event happened (RFC 3339, UTC).
    pub ts: String,
    /// `log` or `metric`.
    pub signal: String,
    /// The event name without its `claude_code.` prefix (`api_request`), or
    /// the metric name (`claude_code.cost.usage`).
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// `delta` or `cumulative`, on sums.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporality: Option<String>,
    #[serde(default)]
    pub attrs: Map<String, Value>,
}

fn int(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

/// An OTLP `AnyValue` as plain JSON; 64-bit integers arrive as strings.
fn any_value(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return Value::Null;
    };
    if let Some(text) = object.get("stringValue") {
        text.clone()
    } else if let Some(flag) = object.get("boolValue") {
        flag.clone()
    } else if let Some(number) = object.get("intValue") {
        int(number).map_or(Value::Null, Value::from)
    } else if let Some(number) = object.get("doubleValue") {
        number
            .as_f64()
            .or_else(|| number.as_str().and_then(|text| text.parse().ok()))
            .map_or(Value::Null, Value::from)
    } else if let Some(array) = object.get("arrayValue") {
        Value::Array(
            array
                .get("values")
                .and_then(Value::as_array)
                .map(|values| values.iter().map(any_value).collect())
                .unwrap_or_default(),
        )
    } else if let Some(list) = object.get("kvlistValue") {
        Value::Object(attributes(list.get("values")))
    } else {
        Value::Null
    }
}

fn attributes(list: Option<&Value>) -> Map<String, Value> {
    let mut map = Map::new();
    for item in list.and_then(Value::as_array).into_iter().flatten() {
        if let (Some(key), Some(value)) =
            (item.get("key").and_then(Value::as_str), item.get("value"))
        {
            map.insert(key.to_owned(), any_value(value));
        }
    }
    map
}

fn nanos_to_time(value: Option<&Value>) -> Option<String> {
    let nanos = value.and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    })?;
    if nanos == 0 {
        return None;
    }
    let time = DateTime::<Utc>::from_timestamp(
        i64::try_from(nanos / 1_000_000_000).ok()?,
        u32::try_from(nanos % 1_000_000_000).ok()?,
    )?;
    Some(time.to_rfc3339_opts(SecondsFormat::Millis, true))
}

/// Drop identity and content, keeping the names inside `tool_parameters`.
fn scrub(attrs: &mut Map<String, Value>) {
    if let Some(Value::String(parameters)) = attrs.get("tool_parameters") {
        if let Ok(Value::Object(parameters)) = serde_json::from_str::<Value>(parameters) {
            for key in PARAMETERS_KEPT {
                if let Some(value) = parameters.get(*key).filter(|value| value.is_string()) {
                    attrs.insert((*key).to_owned(), value.clone());
                }
            }
        }
    }
    for key in DROPPED {
        attrs.remove(*key);
    }
    attrs.retain(|key, _| !key.starts_with("telemetry.sdk."));
}

/// Every log record in an `ExportLogsServiceRequest`.
pub fn parse_logs(body: &Value, received: &str) -> Vec<Record> {
    let mut records = Vec::new();
    for resource in body
        .get("resourceLogs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let shared = attributes(resource.pointer("/resource/attributes"));
        for scope in resource
            .get("scopeLogs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for log in scope
                .get("logRecords")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let mut attrs = shared.clone();
                attrs.extend(attributes(log.get("attributes")));
                let name = attrs
                    .remove("event.name")
                    .and_then(|name| name.as_str().map(str::to_owned))
                    .or_else(|| {
                        log.get("eventName")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .or_else(|| {
                        log.pointer("/body/stringValue")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_default();
                let name = name
                    .strip_prefix("claude_code.")
                    .unwrap_or(&name)
                    .to_owned();
                if name.is_empty() || SKIPPED.contains(&name.as_str()) {
                    continue;
                }
                scrub(&mut attrs);
                records.push(Record {
                    ts: nanos_to_time(log.get("timeUnixNano"))
                        .or_else(|| nanos_to_time(log.get("observedTimeUnixNano")))
                        .unwrap_or_else(|| received.to_owned()),
                    signal: "log".into(),
                    name,
                    value: None,
                    temporality: None,
                    attrs,
                });
            }
        }
    }
    records
}

/// Every sum and gauge data point in an `ExportMetricsServiceRequest`.
pub fn parse_metrics(body: &Value, received: &str) -> Vec<Record> {
    let mut records = Vec::new();
    for resource in body
        .get("resourceMetrics")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let shared = attributes(resource.pointer("/resource/attributes"));
        for scope in resource
            .get("scopeMetrics")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for metric in scope
                .get("metrics")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(name) = metric.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let (data, temporality) = if let Some(sum) = metric.get("sum") {
                    let temporality = match sum.get("aggregationTemporality") {
                        Some(value) if int(value) == Some(2) => "cumulative",
                        Some(Value::String(text)) if text.ends_with("CUMULATIVE") => "cumulative",
                        _ => "delta",
                    };
                    (sum, Some(temporality.to_owned()))
                } else if let Some(gauge) = metric.get("gauge") {
                    (gauge, None)
                } else {
                    continue;
                };
                for point in data
                    .get("dataPoints")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let value = point
                        .get("asDouble")
                        .and_then(|value| {
                            value
                                .as_f64()
                                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
                        })
                        .or_else(|| point.get("asInt").and_then(int).map(|value| value as f64));
                    let mut attrs = shared.clone();
                    attrs.extend(attributes(point.get("attributes")));
                    scrub(&mut attrs);
                    records.push(Record {
                        ts: nanos_to_time(point.get("timeUnixNano"))
                            .unwrap_or_else(|| received.to_owned()),
                        signal: "metric".into(),
                        name: name.to_owned(),
                        value,
                        temporality: temporality.clone(),
                        attrs,
                    });
                }
            }
        }
    }
    records
}

// MARK: Receiver

/// Appends records to the day file; one writer at a time so lines never interleave.
struct Sink {
    dir: PathBuf,
    lock: Mutex<()>,
}

impl Sink {
    fn write(&self, records: &[Record]) -> io::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let mut text = String::new();
        for record in records {
            text.push_str(&serde_json::to_string(record).map_err(io::Error::other)?);
            text.push('\n');
        }
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        std::fs::create_dir_all(&self.dir)?;
        let day = now().format("%Y-%m-%d").to_string();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(format!("{day}.jsonl")))?;
        file.write_all(text.as_bytes())
    }
}

fn now() -> DateTime<Utc> {
    DateTime::<Utc>::from(std::time::SystemTime::now())
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    close: bool,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

fn read_line(reader: &mut impl BufRead, budget: &mut usize) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    if reader
        .by_ref()
        .take(*budget as u64 + 1)
        .read_until(b'\n', &mut line)?
        == 0
    {
        return Ok(None);
    }
    if line.len() > *budget {
        return Err(invalid("request headers too large"));
    }
    *budget -= line.len();
    let text = String::from_utf8(line).map_err(|_| invalid("header is not UTF-8"))?;
    Ok(Some(text.trim_end_matches(['\r', '\n']).to_owned()))
}

/// Read one HTTP/1.1 request; `None` when the client closed the connection.
fn read_request(reader: &mut impl BufRead) -> io::Result<Option<Request>> {
    let mut budget = MAX_HEADERS;
    let line = loop {
        match read_line(reader, &mut budget)? {
            None => return Ok(None),
            Some(line) if line.is_empty() => continue,
            Some(line) => break line,
        }
    };
    let mut parts = line.split_whitespace();
    let (Some(method), Some(path), version) = (parts.next(), parts.next(), parts.next()) else {
        return Err(invalid("malformed request line"));
    };
    let mut headers = Vec::new();
    loop {
        let Some(line) = read_line(reader, &mut budget)? else {
            return Err(invalid("connection closed inside the headers"));
        };
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    let mut request = Request {
        method: method.to_owned(),
        path: path.split('?').next().unwrap_or(path).to_owned(),
        headers,
        body: Vec::new(),
        close: false,
    };
    let connection = request.header("connection").map(str::to_ascii_lowercase);
    request.close = connection.as_deref() == Some("close")
        || (version == Some("HTTP/1.0") && connection.as_deref() != Some("keep-alive"));
    if request
        .header("transfer-encoding")
        .is_some_and(|value| value.to_ascii_lowercase().contains("chunked"))
    {
        loop {
            let Some(size) = read_line(reader, &mut budget)? else {
                return Err(invalid("connection closed inside a chunk"));
            };
            let size = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16)
                .map_err(|_| invalid("malformed chunk size"))?;
            if size == 0 {
                // Trailers, then the blank line.
                while read_line(reader, &mut budget)?.is_some_and(|line| !line.is_empty()) {}
                break;
            }
            if request.body.len() + size > MAX_BODY {
                return Err(invalid("request body too large"));
            }
            let start = request.body.len();
            request.body.resize(start + size, 0);
            reader.read_exact(&mut request.body[start..])?;
            read_line(reader, &mut budget)?;
        }
    } else if let Some(length) = request.header("content-length") {
        let length: usize = length
            .parse()
            .map_err(|_| invalid("malformed Content-Length"))?;
        if length > MAX_BODY {
            return Err(invalid("request body too large"));
        }
        request.body.resize(length, 0);
        reader.read_exact(&mut request.body)?;
    }
    Ok(Some(request))
}

/// Status line and JSON body for one request.
fn route(request: &Request, sink: &Sink) -> (u16, Value) {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/health") => (
            200,
            json!({"service": SERVICE, "version": env!("CARGO_PKG_VERSION")}),
        ),
        ("POST", path @ ("/v1/logs" | "/v1/metrics" | "/v1/traces")) => {
            if request
                .header("content-encoding")
                .is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
            {
                return (
                    415,
                    json!({"error": "compressed bodies are not supported; unset OTEL_EXPORTER_OTLP_COMPRESSION"}),
                );
            }
            if request
                .header("content-type")
                .is_some_and(|value| value.contains("protobuf"))
            {
                return (
                    415,
                    json!({"error": "this collector reads OTLP JSON; set OTEL_EXPORTER_OTLP_PROTOCOL=http/json"}),
                );
            }
            let body: Value = match serde_json::from_slice(&request.body) {
                Ok(body) => body,
                Err(error) => return (400, json!({"error": format!("invalid JSON: {error}")})),
            };
            let received = now().to_rfc3339_opts(SecondsFormat::Millis, true);
            let records = match path {
                "/v1/logs" => parse_logs(&body, &received),
                "/v1/metrics" => parse_metrics(&body, &received),
                _ => Vec::new(),
            };
            match sink.write(&records) {
                Ok(()) => (200, json!({})),
                Err(error) => {
                    eprintln!("skillctl collector: could not write records: {error}");
                    (503, json!({"error": "could not write records"}))
                }
            }
        }
        _ => (404, json!({"error": "not found"})),
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        415 => "Unsupported Media Type",
        _ => "Service Unavailable",
    }
}

fn connection(stream: TcpStream, sink: &Sink) -> io::Result<()> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    loop {
        let request = match read_request(&mut reader) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                let body = json!({"error": error.to_string()}).to_string();
                write!(
                    writer,
                    "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )?;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let (status, body) = route(&request, sink);
        let body = body.to_string();
        write!(
            writer,
            "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n{body}",
            reason(status),
            body.len(),
            if request.close { "close" } else { "keep-alive" }
        )?;
        writer.flush()?;
        if request.close {
            return Ok(());
        }
    }
}

/// Run the receiver until the process is stopped. `ready` gets the bound
/// address, which matters when `port` is 0.
pub fn serve(home: &Path, port: u16, ready: impl FnOnce(SocketAddr)) -> Result<(), Error> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|source| Error::Io {
        operation: format!("Listen on 127.0.0.1:{port}"),
        hint: if source.kind() == io::ErrorKind::AddrInUse {
            format!(
                "Another program uses port {port}; skillctl telemetry status says whether it is this collector. Pick another with --port."
            )
        } else {
            "Check that loopback networking is available.".into()
        },
        source,
    })?;
    let address = listener.local_addr().map_err(|source| Error::Io {
        operation: "Read the listening address".into(),
        source,
        hint: "Try again.".into(),
    })?;
    ready(address);
    let sink = Arc::new(Sink {
        dir: data_dir(home),
        lock: Mutex::new(()),
    });
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let sink = Arc::clone(&sink);
        std::thread::spawn(move || {
            // A dropped or timed-out client is not the collector's problem.
            let _ = connection(stream, &sink);
        });
    }
    Ok(())
}

/// What answers on the port: `None` when nothing listens, `Some(true)` when
/// it is this collector, `Some(false)` for another program.
pub fn probe(port: u16) -> Option<bool> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, StdDuration::from_millis(500)).ok()?;
    let _ = stream.set_read_timeout(Some(StdDuration::from_secs(2)));
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return Some(false);
    }
    let mut response = String::new();
    let _ = stream.take(4096).read_to_string(&mut response);
    Some(response.contains(SERVICE))
}

/// The port of a local endpoint (`http://127.0.0.1:4318`, `http://localhost:4317/`),
/// or `None` for a remote one.
pub fn local_port(endpoint: &str) -> Option<u16> {
    let rest = endpoint
        .strip_prefix("http://")
        .or_else(|| endpoint.strip_prefix("https://"))?;
    let authority = rest.split('/').next()?;
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => (host, port.parse().ok()?),
        _ => (authority, 80),
    };
    matches!(host, "127.0.0.1" | "localhost" | "[::1]").then_some(port)
}

// MARK: Recorded data

/// Every record in the day files from `from` on, in file order.
pub fn read_records(home: &Path, from: Option<NaiveDate>) -> Vec<Record> {
    let mut files: Vec<(NaiveDate, PathBuf)> = std::fs::read_dir(data_dir(home))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let day = NaiveDate::parse_from_str(name.strip_suffix(".jsonl")?, "%Y-%m-%d").ok()?;
            Some((day, entry.path()))
        })
        // A record is filed by the day it arrived, which can follow the day it happened.
        .filter(|(day, _)| from.is_none_or(|from| *day >= from - Duration::days(1)))
        .collect();
    files.sort();
    files
        .iter()
        .filter_map(|(_, path)| std::fs::read_to_string(path).ok())
        .flat_map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str::<Record>(line).ok())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn number(attrs: &Map<String, Value>, key: &str) -> f64 {
    attrs
        .get(key)
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .unwrap_or(0.0)
}

fn text(attrs: &Map<String, Value>, key: &str) -> Option<String> {
    attrs
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn round(value: f64, places: i32) -> f64 {
    let factor = 10f64.powi(places);
    (value * factor).round() / factor
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Bucket {
    pub key: String,
    pub requests: u64,
    pub cost: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

impl Bucket {
    fn add(&mut self, other: &Bucket) {
        self.requests += other.requests;
        self.cost += other.cost;
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Count {
    pub key: String,
    pub count: u64,
    /// Failed tool results; zero elsewhere.
    pub failures: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Report {
    pub from: String,
    pub to: String,
    /// `logs` (API request events), `metrics` (cost and token counters, when
    /// the logs exporter is off) or `none`.
    pub source: String,
    pub records: usize,
    pub last_record: Option<String>,
    pub total: Bucket,
    pub by_model: Vec<Bucket>,
    /// `(none)` for requests made outside any skill.
    pub by_skill: Vec<Bucket>,
    /// `main` for the main conversation.
    pub by_agent: Vec<Bucket>,
    /// `main`, `subagent` or `auxiliary`.
    pub by_source: Vec<Bucket>,
    pub daily: Vec<Bucket>,
    pub tools: Vec<Count>,
    pub commands: Vec<Count>,
    pub api_errors: u64,
    /// Seconds from `claude_code.active_time.total`, by `type` (user, cli).
    pub active_seconds: BTreeMap<String, f64>,
}

fn sorted(map: BTreeMap<String, Bucket>) -> Vec<Bucket> {
    let mut buckets: Vec<Bucket> = map
        .into_iter()
        .map(|(key, mut bucket)| {
            bucket.key = key;
            bucket.cost = round(bucket.cost, 4);
            bucket
        })
        .collect();
    buckets.sort_by(|left, right| {
        right
            .cost
            .total_cmp(&left.cost)
            .then(right.requests.cmp(&left.requests))
            .then(left.key.cmp(&right.key))
    });
    buckets
}

fn counts(map: BTreeMap<String, Count>) -> Vec<Count> {
    let mut counts: Vec<Count> = map
        .into_iter()
        .map(|(key, mut count)| {
            count.key = key;
            count
        })
        .collect();
    counts.sort_by(|left, right| right.count.cmp(&left.count).then(left.key.cmp(&right.key)));
    counts
}

/// Totals for the records that happened between `from` and `to` (inclusive dates, UTC).
pub fn summarize(records: &[Record], from: NaiveDate, to: NaiveDate) -> Report {
    let in_window: Vec<&Record> = records
        .iter()
        .filter(|record| {
            crate::eventlog::parse_time(&record.ts).is_some_and(|time| {
                let day = time.date_naive();
                day >= from && day <= to
            })
        })
        .collect();
    let requests: Vec<&Record> = in_window
        .iter()
        .copied()
        .filter(|record| record.signal == "log" && record.name == "api_request")
        .collect();
    let mut by_model = BTreeMap::<String, Bucket>::new();
    let mut by_skill = BTreeMap::<String, Bucket>::new();
    let mut by_agent = BTreeMap::<String, Bucket>::new();
    let mut by_source = BTreeMap::<String, Bucket>::new();
    let mut daily = BTreeMap::<String, Bucket>::new();
    let mut total = Bucket::default();
    let mut file = |record: &Record, bucket: &Bucket| {
        let attrs = &record.attrs;
        total.add(bucket);
        for (map, key) in [
            (
                &mut by_model,
                text(attrs, "model").unwrap_or_else(|| "unknown".into()),
            ),
            (
                &mut by_skill,
                text(attrs, "skill.name").unwrap_or_else(|| "(none)".into()),
            ),
            (
                &mut by_agent,
                text(attrs, "agent.name").unwrap_or_else(|| "main".into()),
            ),
            (
                &mut by_source,
                text(attrs, "query_source").unwrap_or_else(|| "main".into()),
            ),
            (
                &mut daily,
                record.ts.get(..10).unwrap_or_default().to_owned(),
            ),
        ] {
            map.entry(key).or_default().add(bucket);
        }
    };
    let source = if !requests.is_empty() {
        for record in &requests {
            let attrs = &record.attrs;
            let bucket = Bucket {
                requests: 1,
                cost: number(attrs, "cost_usd"),
                input_tokens: number(attrs, "input_tokens") as u64,
                output_tokens: number(attrs, "output_tokens") as u64,
                cache_read_tokens: number(attrs, "cache_read_tokens") as u64,
                cache_creation_tokens: number(attrs, "cache_creation_tokens") as u64,
                ..Default::default()
            };
            file(record, &bucket);
        }
        "logs"
    } else {
        // The counters carry the same attributes, one data point per series and
        // export interval; only delta points can be summed.
        let mut any = false;
        for record in in_window.iter().filter(|record| {
            record.signal == "metric" && record.temporality.as_deref() == Some("delta")
        }) {
            let value = record.value.unwrap_or(0.0);
            let bucket = match record.name.as_str() {
                "claude_code.cost.usage" => Bucket {
                    cost: value,
                    ..Default::default()
                },
                "claude_code.token.usage" => {
                    let tokens = value as u64;
                    match text(&record.attrs, "type").as_deref() {
                        Some("input") => Bucket {
                            input_tokens: tokens,
                            ..Default::default()
                        },
                        Some("output") => Bucket {
                            output_tokens: tokens,
                            ..Default::default()
                        },
                        Some("cacheRead") => Bucket {
                            cache_read_tokens: tokens,
                            ..Default::default()
                        },
                        Some("cacheCreation") => Bucket {
                            cache_creation_tokens: tokens,
                            ..Default::default()
                        },
                        _ => continue,
                    }
                }
                _ => continue,
            };
            any = true;
            file(record, &bucket);
        }
        if any { "metrics" } else { "none" }
    };
    let mut tools = BTreeMap::<String, Count>::new();
    let mut commands = BTreeMap::<String, Count>::new();
    let mut api_errors = 0;
    let mut active_seconds = BTreeMap::<String, f64>::new();
    for record in &in_window {
        match (record.signal.as_str(), record.name.as_str()) {
            ("log", "tool_result") => {
                let attrs = &record.attrs;
                let Some(tool) = text(attrs, "tool_name") else {
                    continue;
                };
                // A skill or subagent run is reported under its own name.
                let key = match (text(attrs, "skill_name"), text(attrs, "subagent_type")) {
                    (Some(skill), _) => format!("{tool}:{skill}"),
                    (_, Some(agent)) => format!("{tool}:{agent}"),
                    _ => tool,
                };
                let count = tools.entry(key).or_default();
                count.count += 1;
                let failed = match attrs.get("success") {
                    Some(Value::Bool(success)) => !success,
                    Some(Value::String(success)) => success == "false",
                    _ => false,
                };
                count.failures += u64::from(failed);
            }
            ("log", "user_prompt") => {
                if let Some(command) = text(&record.attrs, "command_name") {
                    commands.entry(command).or_default().count += 1;
                }
            }
            ("log", "api_error") => api_errors += 1,
            ("metric", "claude_code.active_time.total")
                if record.temporality.as_deref() == Some("delta") =>
            {
                *active_seconds
                    .entry(text(&record.attrs, "type").unwrap_or_else(|| "all".into()))
                    .or_default() += record.value.unwrap_or(0.0);
            }
            _ => {}
        }
    }
    for seconds in active_seconds.values_mut() {
        *seconds = round(*seconds, 1);
    }
    total.cost = round(total.cost, 4);
    total.key = "total".into();
    let mut daily = sorted(daily);
    daily.sort_by(|left, right| left.key.cmp(&right.key));
    Report {
        from: from.to_string(),
        to: to.to_string(),
        source: source.into(),
        records: in_window.len(),
        last_record: in_window.iter().map(|record| record.ts.clone()).max(),
        total,
        by_model: sorted(by_model),
        by_skill: sorted(by_skill),
        by_agent: sorted(by_agent),
        by_source: sorted(by_source),
        daily,
        tools: counts(tools),
        commands: counts(commands),
        api_errors,
        active_seconds,
    }
}

fn tokens(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn report_text(report: &Report) -> String {
    if report.source == "none" {
        return format!(
            "Telemetry {} to {}: no API requests recorded{}. Run skillctl telemetry status to check the collector; Claude Code exports events every 5 s and metrics every 60 s, and only sessions started after telemetry was enabled export at all.",
            report.from,
            report.to,
            if report.records > 0 {
                format!(" ({} other record(s))", report.records)
            } else {
                String::new()
            }
        );
    }
    let mut lines = vec![format!(
        "Telemetry {} to {} (from {}): ${:.2}, {} request(s), {} output tokens, {} cache-read tokens",
        report.from,
        report.to,
        if report.source == "logs" {
            "API request events"
        } else {
            "cost and token counters"
        },
        report.total.cost,
        report.total.requests,
        tokens(report.total.output_tokens),
        tokens(report.total.cache_read_tokens)
    )];
    for (title, buckets) in [
        ("By model", &report.by_model),
        ("By skill", &report.by_skill),
        ("By agent", &report.by_agent),
        ("By source", &report.by_source),
    ] {
        lines.push(String::new());
        lines.push(format!(
            "{title:<34} {:>9} {:>10} {:>10} {:>9}",
            "requests", "output", "cache rd", "cost"
        ));
        for bucket in buckets.iter().take(15) {
            lines.push(format!(
                "  {:<32} {:>9} {:>10} {:>10} {:>9}",
                bucket.key,
                bucket.requests,
                tokens(bucket.output_tokens),
                tokens(bucket.cache_read_tokens),
                format!("${:.2}", bucket.cost)
            ));
        }
    }
    if !report.tools.is_empty() {
        lines.push(String::new());
        lines.push("Tools (runs, failures)".into());
        for tool in report.tools.iter().take(15) {
            lines.push(format!(
                "  {:<32} {:>6} {:>6}",
                tool.key, tool.count, tool.failures
            ));
        }
    }
    if report.api_errors > 0 {
        lines.push(format!("\nAPI errors: {}", report.api_errors));
    }
    lines.join("\n")
}

// MARK: Settings and service

#[derive(Debug, Clone, Serialize)]
pub struct SettingsState {
    pub path: PathBuf,
    /// Every variable `enable` writes is present with skillctl's value.
    pub configured: bool,
    /// Variables missing or set to something else.
    pub missing: Vec<String>,
    /// The endpoint Claude Code exports to, wherever it was set.
    pub endpoint: Option<String>,
}

fn settings_state(settings: &Value, path: PathBuf, port: u16) -> SettingsState {
    let env = settings.get("env");
    let value = |key: &str| {
        std::env::var(key).ok().or_else(|| {
            env.and_then(|env| env.get(key))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
    };
    let missing: Vec<String> = exports(port)
        .into_iter()
        .filter(|(key, expected)| value(key).as_deref() != Some(expected.as_str()))
        .map(|(key, _)| key.to_owned())
        .collect();
    SettingsState {
        path,
        configured: missing.is_empty(),
        missing,
        endpoint: value("OTEL_EXPORTER_OTLP_ENDPOINT"),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceState {
    /// LaunchAgents exist on macOS only; elsewhere run `telemetry serve` yourself.
    pub supported: bool,
    pub path: Option<PathBuf>,
    pub installed: bool,
    /// The binary the LaunchAgent runs.
    pub program: Option<String>,
    pub program_exists: bool,
    pub loaded: bool,
}

fn plist_path() -> Result<PathBuf, Error> {
    Ok(crate::paths::user_home_dir()?
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The LaunchAgent that keeps `<program> --home <home> telemetry serve` running.
pub fn plist(program: &Path, home: &Path, port: u16) -> String {
    let log = data_dir(home).join("collector.log");
    let arguments = [
        program.display().to_string(),
        "--home".into(),
        home.display().to_string(),
        "telemetry".into(),
        "serve".into(),
        "--port".into(),
        port.to_string(),
    ]
    .iter()
    .map(|argument| format!("    <string>{}</string>", xml_escape(argument)))
    .collect::<Vec<_>>()
    .join("\n");
    let log = xml_escape(&log.display().to_string());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
{arguments}
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#
    )
}

fn domain() -> Option<String> {
    let output = Command::new("id").arg("-u").output().ok()?;
    let uid = String::from_utf8(output.stdout).ok()?;
    Some(format!("gui/{}", uid.trim()))
}

fn launchctl(arguments: &[&str]) -> io::Result<std::process::Output> {
    Command::new("launchctl").args(arguments).output()
}

fn service_state() -> Result<ServiceState, Error> {
    if !cfg!(target_os = "macos") {
        return Ok(ServiceState {
            supported: false,
            path: None,
            installed: false,
            program: None,
            program_exists: false,
            loaded: false,
        });
    }
    let path = plist_path()?;
    let contents = std::fs::read_to_string(&path).ok();
    // The first <string> of ProgramArguments is the binary.
    let program = contents.as_deref().and_then(|text| {
        let arguments = text.split("<key>ProgramArguments</key>").nth(1)?;
        let start = arguments.find("<string>")? + "<string>".len();
        let end = arguments[start..].find("</string>")? + start;
        Some(
            arguments[start..end]
                .replace("&quot;", "\"")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&"),
        )
    });
    let loaded = contents.is_some()
        && domain().is_some_and(|domain| {
            launchctl(&["print", &format!("{domain}/{LABEL}")])
                .is_ok_and(|output| output.status.success())
        });
    Ok(ServiceState {
        supported: true,
        installed: contents.is_some(),
        program_exists: program
            .as_deref()
            .is_some_and(|program| Path::new(program).is_file()),
        program,
        path: Some(path),
        loaded,
    })
}

fn service_error(operation: &str, detail: String) -> Error {
    Error::Io {
        operation: operation.to_owned(),
        source: io::Error::other(detail),
        hint: "Run skillctl telemetry serve in a terminal instead, or check launchctl print gui/$(id -u)/dev.abindran.skillctl.collector.".into(),
    }
}

fn install_service(home: &Path, port: u16) -> Result<PathBuf, Error> {
    let program = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|source| Error::Io {
            operation: "Locate the skillctl binary".into(),
            source,
            hint: "Run skillctl from its installed location.".into(),
        })?;
    let path = plist_path()?;
    let io = |source| Error::Io {
        operation: format!("Write {}", path.display()),
        source,
        hint: "Check that ~/Library/LaunchAgents is writable.".into(),
    };
    std::fs::create_dir_all(path.parent().expect("plist folder")).map_err(io)?;
    std::fs::create_dir_all(data_dir(home)).map_err(io)?;
    std::fs::write(&path, plist(&program, home, port)).map_err(io)?;
    let domain =
        domain().ok_or_else(|| service_error("Find the login session", "id -u failed".into()))?;
    // Replace a running copy so a new binary or port takes effect.
    let _ = launchctl(&["bootout", &format!("{domain}/{LABEL}")]);
    let output = launchctl(&["bootstrap", &domain, &path.display().to_string()])
        .map_err(|error| service_error("Run launchctl", error.to_string()))?;
    if !output.status.success() {
        return Err(service_error(
            "Start the collector service",
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(path)
}

fn remove_service() -> Result<Option<PathBuf>, Error> {
    if !cfg!(target_os = "macos") {
        return Ok(None);
    }
    let path = plist_path()?;
    if !path.exists() {
        return Ok(None);
    }
    if let Some(domain) = domain() {
        let _ = launchctl(&["bootout", &format!("{domain}/{LABEL}")]);
    }
    std::fs::remove_file(&path).map_err(|source| Error::Io {
        operation: format!("Remove {}", path.display()),
        source,
        hint: "Delete the file by hand.".into(),
    })?;
    Ok(Some(path))
}

fn env_object(settings: &mut Value) -> Result<&mut Map<String, Value>, Error> {
    let root = settings.as_object_mut().expect("settings object");
    root.entry("env")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| Error::Usage {
            message: "settings.json has an \"env\" key that is not an object".into(),
            hint: "Fix the file by hand so env is an object of strings.".into(),
        })
}

// MARK: Commands

pub fn run(context: &mut Context<'_>, command: TelemetryCommand) -> Result<(), Error> {
    match command {
        TelemetryCommand::Serve { port } => {
            super::init::ensure_home(context)?;
            let home = context.home.home().to_path_buf();
            let dir = data_dir(&home);
            serve(&home, port, |address| {
                // stderr, so a parent can read the bound port without a JSON document.
                eprintln!(
                    "skillctl collector listening on http://{address}; writing to {}",
                    dir.display()
                );
            })
        }
        TelemetryCommand::Enable { port, no_service } => enable(context, port, no_service),
        TelemetryCommand::Disable { port } => disable(context, port),
        TelemetryCommand::Status { port } => status(context, port),
        TelemetryCommand::Report { days } => {
            let days = days.max(1);
            let to = now().date_naive();
            let from = to - Duration::days(i64::from(days) - 1);
            let report = summarize(&read_records(context.home.home(), Some(from)), from, to);
            context
                .output
                .result(&report, &report_text(&report))
                .map_err(super::output_error)
        }
    }
}

fn enable(context: &mut Context<'_>, port: u16, no_service: bool) -> Result<(), Error> {
    super::init::ensure_home(context)?;
    let settings_path = harness::claude_settings().expand()?;
    let mut settings = claude::load(context.fs.as_ref(), &settings_path)?;
    let env = env_object(&mut settings)?;
    let mut changed = Vec::new();
    for (key, value) in exports(port) {
        if env.get(key).and_then(Value::as_str) != Some(value.as_str()) {
            env.insert(key.to_owned(), Value::String(value));
            changed.push(key);
        }
    }
    let backup = if changed.is_empty() {
        None
    } else {
        Some(claude::save(
            context.fs.as_ref(),
            &settings_path,
            &settings,
        )?)
    };
    let service = if no_service || context.flags.dry_run || !cfg!(target_os = "macos") {
        None
    } else {
        Some(install_service(context.home.home(), port)?)
    };
    // launchd starts the process asynchronously (it can take a few seconds when
    // it replaces a running copy); wait up to 5 s before probing.
    let mut collector = probe(port);
    for _ in 0..50 {
        if service.is_none() || collector == Some(true) {
            break;
        }
        std::thread::sleep(StdDuration::from_millis(100));
        collector = probe(port);
    }
    let verb = if context.flags.dry_run {
        "Would point"
    } else {
        "Pointed"
    };
    let mut text = format!(
        "{verb} Claude Code at {} ({})",
        endpoint(port),
        if changed.is_empty() {
            format!("{} already set", settings_path.display())
        } else {
            format!("set {} in {}", changed.join(", "), settings_path.display())
        }
    );
    if let Some(service) = &service {
        text.push_str(&format!(
            "\n  service: {} (starts at login)",
            service.display()
        ));
    } else if !no_service && !cfg!(target_os = "macos") {
        text.push_str("\n  service: not available on this system; run skillctl telemetry serve");
    }
    text.push_str(&format!(
        "\n  collector: {}",
        match collector {
            Some(true) => "running".to_owned(),
            Some(false) => format!("port {port} is taken by another program"),
            None => "not running; start it with skillctl telemetry serve".to_owned(),
        }
    ));
    text.push_str(
        "\n  Sessions already open keep their old settings; new Claude Code sessions export.",
    );
    context
        .output
        .result(
            &json!({
                "endpoint": endpoint(port),
                "settings": settings_path,
                "changed": changed,
                "backup": backup,
                "service": service,
                "collector": collector.map(|ours| if ours { "running" } else { "port-taken" }).unwrap_or("stopped"),
                "dry_run": context.flags.dry_run,
            }),
            &text,
        )
        .map_err(super::output_error)
}

fn disable(context: &mut Context<'_>, port: u16) -> Result<(), Error> {
    let settings_path = harness::claude_settings().expand()?;
    let mut settings = claude::load(context.fs.as_ref(), &settings_path)?;
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    if let Some(env) = settings.get_mut("env").and_then(Value::as_object_mut) {
        for (key, value) in exports(port) {
            match env.get(key).and_then(Value::as_str) {
                Some(current) if current == value => {
                    env.remove(key);
                    removed.push(key);
                }
                // Set to something else: the user's own configuration.
                Some(_) => kept.push(key),
                None => {}
            }
        }
        if env.is_empty() {
            settings
                .as_object_mut()
                .expect("settings object")
                .remove("env");
        }
    }
    let backup = if removed.is_empty() {
        None
    } else {
        Some(claude::save(
            context.fs.as_ref(),
            &settings_path,
            &settings,
        )?)
    };
    let service = if context.flags.dry_run {
        None
    } else {
        remove_service()?
    };
    let mut text = format!(
        "{} {}",
        if context.flags.dry_run {
            "Would remove"
        } else {
            "Removed"
        },
        if removed.is_empty() {
            "no telemetry settings (none were skillctl's)".to_owned()
        } else {
            format!("{} from {}", removed.join(", "), settings_path.display())
        }
    );
    if !kept.is_empty() {
        text.push_str(&format!(
            "\n  kept (set to other values): {}",
            kept.join(", ")
        ));
    }
    if let Some(service) = &service {
        text.push_str(&format!(
            "\n  service stopped and removed: {}",
            service.display()
        ));
    }
    text.push_str("\n  Recorded data stays under the telemetry folder of the skillctl home.");
    context
        .output
        .result(
            &json!({
                "settings": settings_path,
                "removed": removed,
                "kept": kept,
                "backup": backup,
                "service": service,
                "dry_run": context.flags.dry_run,
            }),
            &text,
        )
        .map_err(super::output_error)
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub endpoint: String,
    pub port: u16,
    pub settings: SettingsState,
    pub service: ServiceState,
    /// `running` (this collector answers), `port-taken` or `stopped`.
    pub collector: String,
    pub data_dir: PathBuf,
    pub records_today: usize,
    pub last_record: Option<String>,
}

fn status(context: &mut Context<'_>, port: u16) -> Result<(), Error> {
    let settings_path = harness::claude_settings().expand()?;
    let settings = claude::load(context.fs.as_ref(), &settings_path).unwrap_or(json!({}));
    let today = now().date_naive();
    let records = read_records(context.home.home(), Some(today));
    let state = Status {
        endpoint: endpoint(port),
        port,
        settings: settings_state(&settings, settings_path, port),
        service: service_state()?,
        collector: match probe(port) {
            Some(true) => "running",
            Some(false) => "port-taken",
            None => "stopped",
        }
        .into(),
        data_dir: data_dir(context.home.home()),
        records_today: records.len(),
        last_record: records.iter().map(|record| record.ts.clone()).max(),
    };
    let mut text = vec![
        format!(
            "Claude Code settings: {}",
            if state.settings.configured {
                format!("export to {}", state.endpoint)
            } else {
                format!(
                    "not pointed at the collector (missing {})",
                    state.settings.missing.join(", ")
                )
            }
        ),
        format!(
            "Service: {}",
            match (
                &state.service.supported,
                state.service.installed,
                state.service.loaded
            ) {
                (false, _, _) => "not available on this system".to_owned(),
                (true, false, _) => "not installed".to_owned(),
                (true, true, loaded) => format!(
                    "{} ({}{})",
                    if loaded {
                        "loaded"
                    } else {
                        "installed, not loaded"
                    },
                    state.service.program.as_deref().unwrap_or("unknown binary"),
                    if state.service.program_exists {
                        ""
                    } else {
                        ", missing"
                    }
                ),
            }
        ),
        format!("Collector: {} on port {port}", state.collector),
        format!(
            "Data: {} record(s) today in {}{}",
            state.records_today,
            state.data_dir.display(),
            state
                .last_record
                .as_deref()
                .map(|last| format!(", last {last}"))
                .unwrap_or_default()
        ),
    ];
    if !state.settings.configured || state.collector != "running" {
        text.push("Run skillctl telemetry enable to set it all up.".into());
    }
    context
        .output
        .result(&state, &text.join("\n"))
        .map_err(super::output_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(key: &str, value: Value) -> Value {
        json!({"key": key, "value": value})
    }

    fn logs() -> Value {
        json!({"resourceLogs": [{
            "resource": {"attributes": [kv("service.name", json!({"stringValue": "claude-code"})), kv("telemetry.sdk.name", json!({"stringValue": "opentelemetry"}))]},
            "scopeLogs": [{"logRecords": [
                {"timeUnixNano": "1792483200000000000", "body": {"stringValue": "claude_code.api_request"}, "attributes": [
                    kv("event.name", json!({"stringValue": "api_request"})),
                    kv("model", json!({"stringValue": "claude-opus-5-5"})),
                    kv("cost_usd", json!({"doubleValue": 0.25})),
                    kv("input_tokens", json!({"intValue": "10"})),
                    kv("output_tokens", json!({"intValue": 400})),
                    kv("cache_read_tokens", json!({"intValue": "90000"})),
                    kv("cache_creation_tokens", json!({"intValue": "500"})),
                    kv("skill.name", json!({"stringValue": "onezoho-client-ember"})),
                    kv("query_source", json!({"stringValue": "main"})),
                    kv("user.email", json!({"stringValue": "someone@example.com"})),
                    kv("session.id", json!({"stringValue": "s1"}))
                ]},
                {"timeUnixNano": "1792483260000000000", "attributes": [
                    kv("event.name", json!({"stringValue": "api_request"})),
                    kv("model", json!({"stringValue": "claude-haiku-4-5"})),
                    kv("cost_usd", json!({"doubleValue": 0.01})),
                    kv("output_tokens", json!({"intValue": "100"})),
                    kv("agent.name", json!({"stringValue": "onezoho-explore"})),
                    kv("query_source", json!({"stringValue": "subagent"}))
                ]},
                {"timeUnixNano": "1792483270000000000", "attributes": [
                    kv("event.name", json!({"stringValue": "tool_result"})),
                    kv("tool_name", json!({"stringValue": "Skill"})),
                    kv("success", json!({"stringValue": "true"})),
                    kv("tool_parameters", json!({"stringValue": "{\"skill_name\":\"onezoho-client-ember\",\"args\":\"secret\"}"}))
                ]},
                {"timeUnixNano": "1792483280000000000", "attributes": [
                    kv("event.name", json!({"stringValue": "tool_result"})),
                    kv("tool_name", json!({"stringValue": "Bash"})),
                    kv("success", json!({"boolValue": false}))
                ]},
                {"timeUnixNano": "1792483280000000000", "attributes": [
                    kv("event.name", json!({"stringValue": "hook_registered"}))
                ]},
                {"timeUnixNano": "1792483290000000000", "attributes": [
                    kv("event.name", json!({"stringValue": "user_prompt"})),
                    kv("prompt", json!({"stringValue": "do the thing"})),
                    kv("prompt_text", json!({"stringValue": "do the thing"})),
                    kv("command_name", json!({"stringValue": "oz-art-gen-pom"}))
                ]}
            ]}]
        }]})
    }

    #[test]
    fn log_records_are_flattened_and_scrubbed() {
        let records = parse_logs(&logs(), "2026-10-20T00:00:00.000Z");
        assert_eq!(records.len(), 5);
        let first = &records[0];
        assert_eq!(first.name, "api_request");
        assert_eq!(first.ts, "2026-10-20T08:00:00.000Z");
        assert_eq!(first.attrs["input_tokens"], json!(10));
        assert_eq!(first.attrs["service.name"], json!("claude-code"));
        assert!(!first.attrs.contains_key("user.email"));
        assert!(!first.attrs.contains_key("telemetry.sdk.name"));
        assert!(!first.attrs.contains_key("event.name"));
        let skill = &records[2];
        assert_eq!(skill.attrs["skill_name"], json!("onezoho-client-ember"));
        assert!(
            !skill.attrs.contains_key("tool_parameters"),
            "tool inputs are dropped"
        );
        assert!(!records[4].attrs.contains_key("prompt"));
        assert!(!records[4].attrs.contains_key("prompt_text"));
        assert!(
            records
                .iter()
                .all(|record| record.name != "hook_registered"),
            "configuration events are skipped"
        );
    }

    #[test]
    fn report_groups_cost_by_model_skill_agent_and_source() {
        let records = parse_logs(&logs(), "2026-10-20T00:00:00.000Z");
        let day = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let report = summarize(&records, day, day);
        assert_eq!(report.source, "logs");
        assert_eq!(report.total.requests, 2);
        assert_eq!(report.total.cost, 0.26);
        assert_eq!(report.total.output_tokens, 500);
        assert_eq!(report.by_model[0].key, "claude-opus-5-5");
        assert_eq!(report.by_skill[0].key, "onezoho-client-ember");
        assert_eq!(report.by_skill[1].key, "(none)");
        assert_eq!(report.by_agent[0].key, "main");
        assert_eq!(report.by_agent[1].key, "onezoho-explore");
        assert_eq!(report.by_source[1].key, "subagent");
        assert_eq!(report.daily.len(), 1);
        assert_eq!(report.tools[0].key, "Bash");
        assert_eq!(report.tools[0].failures, 1);
        assert_eq!(report.tools[1].key, "Skill:onezoho-client-ember");
        assert_eq!(report.commands[0].key, "oz-art-gen-pom");
        let rendered = report_text(&report);
        assert!(rendered.contains("$0.26, 2 request(s)"), "{rendered}");
        let empty = summarize(&records, day - Duration::days(9), day - Duration::days(2));
        assert_eq!(empty.source, "none");
    }

    #[test]
    fn metrics_fill_in_when_the_logs_exporter_is_off() {
        let body = json!({"resourceMetrics": [{"scopeMetrics": [{"metrics": [
            {"name": "claude_code.cost.usage", "sum": {"aggregationTemporality": 1, "dataPoints": [
                {"timeUnixNano": "1792483200000000000", "asDouble": 1.5, "attributes": [kv("model", json!({"stringValue": "claude-opus-5-5"})), kv("skill.name", json!({"stringValue": "skill-check"}))]}
            ]}},
            {"name": "claude_code.token.usage", "sum": {"aggregationTemporality": "AGGREGATION_TEMPORALITY_DELTA", "dataPoints": [
                {"timeUnixNano": "1792483200000000000", "asInt": "2000", "attributes": [kv("type", json!({"stringValue": "output"})), kv("model", json!({"stringValue": "claude-opus-5-5"}))]}
            ]}},
            {"name": "claude_code.token.usage", "sum": {"aggregationTemporality": 2, "dataPoints": [
                {"timeUnixNano": "1792483200000000000", "asInt": "999999", "attributes": [kv("type", json!({"stringValue": "output"}))]}
            ]}},
            {"name": "claude_code.active_time.total", "sum": {"aggregationTemporality": 1, "dataPoints": [
                {"timeUnixNano": "1792483200000000000", "asDouble": 12.34, "attributes": [kv("type", json!({"stringValue": "user"}))]}
            ]}}
        ]}]}]});
        let records = parse_metrics(&body, "2026-10-20T00:00:00.000Z");
        assert_eq!(records.len(), 4);
        assert_eq!(records[2].temporality.as_deref(), Some("cumulative"));
        let day = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let report = summarize(&records, day, day);
        assert_eq!(report.source, "metrics");
        assert_eq!(report.total.cost, 1.5);
        assert_eq!(
            report.total.output_tokens, 2000,
            "cumulative points are not summed"
        );
        assert_eq!(report.by_skill[0].key, "skill-check");
        assert_eq!(report.active_seconds["user"], 12.3);
    }

    #[test]
    fn requests_are_read_with_content_length_and_chunks() {
        let raw = b"POST /v1/logs HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}GET /health HTTP/1.1\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n0\r\n\r\n";
        let mut reader = BufReader::new(&raw[..]);
        let first = read_request(&mut reader).unwrap().unwrap();
        assert_eq!(
            (first.method.as_str(), first.path.as_str()),
            ("POST", "/v1/logs")
        );
        assert_eq!(first.body, b"{}");
        assert!(!first.close);
        let second = read_request(&mut reader).unwrap().unwrap();
        assert_eq!(second.body, b"abc");
        assert!(second.close);
        assert!(read_request(&mut reader).unwrap().is_none());
    }

    #[test]
    fn local_endpoints_are_recognised() {
        assert_eq!(local_port("http://127.0.0.1:4318"), Some(4318));
        assert_eq!(local_port("http://localhost:4317/"), Some(4317));
        assert_eq!(local_port("http://localhost"), Some(80));
        assert_eq!(local_port("https://otel.example.com:4318"), None);
        assert_eq!(local_port("grpc"), None);
    }

    #[test]
    fn plist_runs_serve_with_the_home_and_port() {
        let text = plist(
            Path::new("/Apps/Skillctl.app/Contents/Helpers/skillctl"),
            Path::new("/Users/me/.skills"),
            4318,
        );
        assert!(text.contains("<string>dev.abindran.skillctl.collector</string>"));
        assert!(text.contains("<string>/Apps/Skillctl.app/Contents/Helpers/skillctl</string>\n    <string>--home</string>\n    <string>/Users/me/.skills</string>\n    <string>telemetry</string>\n    <string>serve</string>\n    <string>--port</string>\n    <string>4318</string>"));
        assert!(text.contains("/Users/me/.skills/telemetry/collector.log"));
        assert_eq!(xml_escape("a&b<c>"), "a&amp;b&lt;c&gt;");
    }
}
