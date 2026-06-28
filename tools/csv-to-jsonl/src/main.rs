use anyhow::{Context, Result};
use clap::Parser;
use serde::Serialize;
use serde_json;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Write};

/// Convert a CSV of LLM provider endpoints into our JSONL source schema.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Input CSV path. Omit to read from stdin.
    #[arg(short, long)]
    input: Option<String>,

    /// Output JSONL path. Omit to write to stdout.
    #[arg(short, long)]
    output: Option<String>,

    /// Comma-separated list of required columns that must be non-empty.
    #[arg(long, value_delimiter = ',', default_value = "provider,name,api_type,api_base")]
    required: Vec<String>,
}

#[derive(Serialize, Debug)]
struct SourceEntry {
    provider: String,
    name: String,
    api_type: String,
    api_base: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    models_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version_header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    models: Option<Vec<String>>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut reader: Box<dyn io::Read> = match args.input {
        Some(path) => Box::new(File::open(&path).with_context(|| format!("open {path}"))?),
        None => Box::new(io::stdin()),
    };

    let mut csv_reader = csv::Reader::from_reader(&mut reader);
    let headers = csv_reader.headers()?.clone();

    let mut writer: Box<dyn Write> = match args.output {
        Some(path) => Box::new(BufWriter::new(
            File::create(&path).with_context(|| format!("create {path}"))?,
        )),
        None => Box::new(BufWriter::new(io::stdout())),
    };

    for (idx, record) in csv_reader.records().enumerate() {
        let record = record.with_context(|| format!("parse row {}", idx + 2))?;
        let get = |name: &str| {
            headers
                .iter()
                .position(|h| h.eq_ignore_ascii_case(name))
                .and_then(|i| {
                    let v = record.get(i).unwrap_or("").trim();
                    if v.is_empty() { None } else { Some(v.to_string()) }
                })
        };

        for req in &args.required {
            if get(req).is_none() {
                anyhow::bail!(
                    "row {} is missing required column '{}'",
                    idx + 2,
                    req
                );
            }
        }

        let mut extra = HashMap::new();
        for (i, header) in headers.iter().enumerate() {
            let key = header.trim().to_lowercase().replace(' ', "_");
            let known = matches!(
                key.as_str(),
                "provider" | "name" | "api_type" | "api_base" | "key_env"
                    | "models_endpoint" | "auth_header" | "version_header" | "models"
            );
            if !known {
                let v = record.get(i).unwrap_or("").trim();
                if !v.is_empty() {
                    extra.insert(key, serde_json::Value::String(v.to_string()));
                }
            }
        }

        let models: Option<Vec<String>> = get("models").map(|s| {
            s.split(',')
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect()
        });

        let entry = SourceEntry {
            provider: get("provider").unwrap_or_default(),
            name: get("name").unwrap_or_default(),
            api_type: get("api_type").unwrap_or_default(),
            api_base: get("api_base").unwrap_or_default(),
            key_env: get("key_env"),
            models_endpoint: get("models_endpoint"),
            auth_header: get("auth_header"),
            version_header: get("version_header"),
            models,
            extra,
        };

        let line = serde_json::to_string(&entry)?;
        writeln!(writer, "{line}")?;
    }

    writer.flush()?;
    Ok(())
}
