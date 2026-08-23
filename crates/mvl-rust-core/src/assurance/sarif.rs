//! Translates [`DiagnosticRecord`]s into a SARIF 2.1.0 log, for CI tools
//! (GitHub code scanning, etc.) that consume that format rather than the
//! project's own assurance-JSON schema (#117).
//!
//! **Deliberately minimal**, matching the same "checked, not proved" spirit
//! as the tools that produce these diagnostics: one `run` per report, a
//! single generic rule per tool (`DiagnosticRecord` carries no category to
//! key a real rule taxonomy on), and one `result` per diagnostic with
//! exactly one physical location. `provenance` is parsed back out of its
//! `"file:line:col"` string form (the same format [`super::report::diagnostic_to_record`]
//! builds), so a provenance string that doesn't parse falls back to line 1,
//! column 1 rather than failing the whole conversion — SARIF output is for
//! human/CI consumption, not itself a proof artifact, so a best-effort
//! location beats no output at all.

use serde::Serialize;

use super::schema::DiagnosticRecord;

/// A full SARIF 2.1.0 log with exactly one run.
#[derive(Debug, Clone, Serialize)]
pub struct SarifLog {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub version: String,
    pub runs: Vec<SarifRun>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SarifRun {
    pub tool: SarifTool,
    pub results: Vec<SarifResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SarifTool {
    pub driver: SarifDriver,
}

#[derive(Debug, Clone, Serialize)]
pub struct SarifDriver {
    pub name: String,
    #[serde(rename = "informationUri")]
    pub information_uri: String,
    pub rules: Vec<SarifRule>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SarifRule {
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SarifResult {
    #[serde(rename = "ruleId")]
    pub rule_id: String,
    pub level: String,
    pub message: SarifMessage,
    pub locations: Vec<SarifLocation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SarifMessage {
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SarifLocation {
    #[serde(rename = "physicalLocation")]
    pub physical_location: SarifPhysicalLocation,
}

#[derive(Debug, Clone, Serialize)]
pub struct SarifPhysicalLocation {
    #[serde(rename = "artifactLocation")]
    pub artifact_location: SarifArtifactLocation,
    pub region: SarifRegion,
}

#[derive(Debug, Clone, Serialize)]
pub struct SarifArtifactLocation {
    pub uri: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SarifRegion {
    #[serde(rename = "startLine")]
    pub start_line: u64,
    #[serde(rename = "startColumn")]
    pub start_column: u64,
}

/// The single generic rule id every diagnostic reports against — there is
/// no per-diagnostic category to key a real rule taxonomy on yet.
const GENERIC_RULE_ID: &str = "mvl-diagnostic";

/// `"file:line:col"` -> `(file, line, col)`, falling back to `(provenance,
/// 1, 1)` if it doesn't parse in that shape (e.g. a bare error-record
/// provenance with no line/col, as [`super::report::build_check_report`]'s
/// error path can produce).
fn parse_provenance(provenance: &str) -> (String, u64, u64) {
    let mut parts = provenance.rsplitn(3, ':');
    let (Some(col), Some(line), Some(file)) = (parts.next(), parts.next(), parts.next()) else {
        return (provenance.to_string(), 1, 1);
    };
    match (line.parse::<u64>(), col.parse::<u64>()) {
        (Ok(line), Ok(col)) => (file.to_string(), line, col),
        _ => (provenance.to_string(), 1, 1),
    }
}

fn sarif_level(level: &str) -> String {
    match level {
        "error" => "error",
        "warning" => "warning",
        "note" => "note",
        _ => "warning",
    }
    .to_string()
}

/// Builds a one-run SARIF log for `tool_name`'s diagnostics.
pub fn build_sarif_log(tool_name: &str, records: &[DiagnosticRecord]) -> SarifLog {
    let results = records
        .iter()
        .map(|record| {
            let (file, line, column) = parse_provenance(&record.provenance);
            SarifResult {
                rule_id: GENERIC_RULE_ID.to_string(),
                level: sarif_level(&record.level),
                message: SarifMessage {
                    text: record.message.clone(),
                },
                locations: vec![SarifLocation {
                    physical_location: SarifPhysicalLocation {
                        artifact_location: SarifArtifactLocation { uri: file },
                        region: SarifRegion {
                            start_line: line,
                            start_column: column,
                        },
                    },
                }],
            }
        })
        .collect();

    SarifLog {
        schema: "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json".to_string(),
        version: "2.1.0".to_string(),
        runs: vec![SarifRun {
            tool: SarifTool {
                driver: SarifDriver {
                    name: tool_name.to_string(),
                    information_uri: "https://github.com/mvl-lang/mvl-rust".to_string(),
                    rules: vec![SarifRule {
                        id: GENERIC_RULE_ID.to_string(),
                    }],
                },
            },
            results,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(level: &str, message: &str, provenance: &str) -> DiagnosticRecord {
        DiagnosticRecord {
            level: level.to_string(),
            message: message.to_string(),
            provenance: provenance.to_string(),
            label: None,
            suggestion: None,
        }
    }

    #[test]
    fn builds_one_run_with_the_tool_name() {
        let log = build_sarif_log("rust-total", &[]);
        assert_eq!(log.runs.len(), 1);
        assert_eq!(log.runs[0].tool.driver.name, "rust-total");
        assert!(log.runs[0].results.is_empty());
    }

    #[test]
    fn parses_file_line_col_provenance() {
        let log = build_sarif_log("rust-total", &[record("error", "boom", "src/lib.rs:12:5")]);
        let loc = &log.runs[0].results[0].locations[0].physical_location;
        assert_eq!(loc.artifact_location.uri, "src/lib.rs");
        assert_eq!(loc.region.start_line, 12);
        assert_eq!(loc.region.start_column, 5);
    }

    #[test]
    fn falls_back_to_line_one_column_one_on_unparseable_provenance() {
        let log = build_sarif_log("rust-total", &[record("error", "boom", "some read error")]);
        let loc = &log.runs[0].results[0].locations[0].physical_location;
        assert_eq!(loc.artifact_location.uri, "some read error");
        assert_eq!(loc.region.start_line, 1);
        assert_eq!(loc.region.start_column, 1);
    }

    #[test]
    fn maps_levels_through_unchanged() {
        let log = build_sarif_log(
            "rust-total",
            &[
                record("error", "e", "f:1:1"),
                record("warning", "w", "f:1:1"),
                record("note", "n", "f:1:1"),
            ],
        );
        let levels: Vec<_> = log.runs[0]
            .results
            .iter()
            .map(|r| r.level.as_str())
            .collect();
        assert_eq!(levels, ["error", "warning", "note"]);
    }

    #[test]
    fn serializes_with_sarif_field_names() {
        let log = build_sarif_log("rust-total", &[record("error", "boom", "f.rs:1:1")]);
        let json = serde_json::to_value(&log).unwrap();
        assert_eq!(json["version"], "2.1.0");
        assert!(json["$schema"].is_string());
        assert_eq!(json["runs"][0]["tool"]["driver"]["name"], "rust-total");
        assert_eq!(json["runs"][0]["results"][0]["ruleId"], "mvl-diagnostic");
        assert_eq!(
            json["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]
                ["uri"],
            "f.rs"
        );
    }
}
