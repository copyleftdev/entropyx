//! JSON Schema for the tq1 `Summary` envelope.
//!
//! The schema is hand-written (not derived) for a reason: this is the
//! tq1 protocol's on-the-wire contract. Consumers in other languages
//! read it to validate `Summary` payloads, and the description fields
//! double as protocol documentation. Changes to this schema must move
//! in lock-step with `Dict::METRIC_COLUMNS`, the `Event` variants, and
//! `entropyx_core::CONTRACT_VERSION` — they are one atomic surface.
//!
//! Emitted as JSON Schema draft 2020-12 with an `$id` pinned to the
//! current contract version, so two Summary files produced against
//! different schema revisions are distinguishable by their schemas'
//! `$id` alone.

use crate::Dict;
use entropyx_core::CONTRACT_VERSION;
use serde_json::{Value, json};

/// Build the JSON Schema for a tq1 `Summary` as a `serde_json::Value`.
///
/// Uses `$defs` / `$ref` rather than inlining every nested type: the
/// `Event` union references each variant once, the per-file metric
/// array is defined once and referenced from `FileRow`, etc.
pub fn schema_json() -> Value {
    let metric_columns: Vec<Value> = Dict::METRIC_COLUMNS.iter().map(|s| json!(s)).collect();

    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": format!("https://entropyx.dev/schema/tq1-{CONTRACT_VERSION}.json"),
        "title": "tq1 Summary",
        "description": "Token-efficient summary protocol (RFC-009) emitted by `entropyx scan`. The Summary is a dense, dictionary-encoded envelope with handle-addressable drill-down.",
        "type": "object",
        "required": ["schema", "dict", "files", "events", "handles"],
        "additionalProperties": false,
        "properties": {
            "schema": {"$ref": "#/$defs/Schema"},
            "dict": {"$ref": "#/$defs/Dict"},
            "files": {
                "type": "array",
                "description": "One row per file trajectory (RFC-004 lineage).",
                "items": {"$ref": "#/$defs/FileRow"}
            },
            "events": {
                "type": "array",
                "description": "Chronological per-file events carrying a commit `sha` for enrichment joins.",
                "items": {"$ref": "#/$defs/Event"}
            },
            "handles": {
                "type": "object",
                "description": "Self-indexed handle table keyed by Handle::key() — `file:...`, `commit:...`, `range:...`.",
                "additionalProperties": {"$ref": "#/$defs/Handle"}
            },
            "enrichments": {
                "$ref": "#/$defs/Enrichments",
                "description": "Optional external-source sidecar keyed by commit SHA; present when `scan --github` ran."
            }
        },
        "$defs": {
            "Schema": {
                "type": "object",
                "required": ["name", "version"],
                "additionalProperties": false,
                "properties": {
                    "name": {"type": "string", "description": "Protocol name, e.g. `tq1`."},
                    "version": {"type": "string", "description": "Contract version; see entropyx_core::CONTRACT_VERSION."}
                }
            },
            "Dict": {
                "type": "object",
                "required": ["files", "authors", "metrics"],
                "additionalProperties": false,
                "properties": {
                    "files": {
                        "type": "array",
                        "description": "FileId index → canonical path at observation time.",
                        "items": {"type": "string"}
                    },
                    "authors": {
                        "type": "array",
                        "description": "AuthorId index → identity-normalized author key (RFC-010).",
                        "items": {"type": "string"}
                    },
                    "metrics": {
                        "type": "array",
                        "description": "Column order for FileRow.values. Load-bearing — changes bump CONTRACT_VERSION.",
                        "items": {"type": "string", "enum": metric_columns.clone()},
                        "minItems": 8,
                        "maxItems": 8
                    }
                }
            },
            "FileRow": {
                "type": "object",
                "required": ["file", "values", "lineage_confidence"],
                "additionalProperties": false,
                "properties": {
                    "file": {"$ref": "#/$defs/FileId"},
                    "values": {
                        "type": "array",
                        "description": "Dense metric vector; positions align to Dict.metrics column order.",
                        "items": {"type": "number"},
                        "minItems": 8,
                        "maxItems": 8
                    },
                    "lineage_confidence": {
                        "type": "number",
                        "description": "Confidence in the lineage resolver for this row, in [0, 1].",
                        "minimum": 0.0,
                        "maximum": 1.0
                    },
                    "signal_class": {
                        "oneOf": [
                            {"type": "null"},
                            {"$ref": "#/$defs/SignalClass"}
                        ],
                        "description": "RFC-008 classification; null when no label applies."
                    }
                }
            },
            "FileId": {
                "type": "integer",
                "description": "Interned FileId — index into Dict.files.",
                "minimum": 0
            },
            "AuthorId": {
                "type": "integer",
                "description": "Interned AuthorId — index into Dict.authors.",
                "minimum": 0
            },
            "Timestamp": {
                "type": "integer",
                "description": "Seconds since the UNIX epoch."
            },
            "SignalClass": {
                "type": "string",
                "enum": [
                    "refactor_convergence",
                    "api_drift",
                    "ownership_fragmentation",
                    "incident_aftershock",
                    "coupled_amplifier",
                    "frozen_neglect"
                ],
                "description": "RFC-008 taxonomy of file dynamics signatures."
            },
            "Event": {
                "description": "Tagged union; discriminator is `kind`.",
                "oneOf": [
                    {"$ref": "#/$defs/EventHotspot"},
                    {"$ref": "#/$defs/EventOwnershipSplit"},
                    {"$ref": "#/$defs/EventApiDrift"},
                    {"$ref": "#/$defs/EventRename"},
                    {"$ref": "#/$defs/EventIncidentAftershock"}
                ]
            },
            "EventHotspot": {
                "type": "object",
                "required": ["kind", "file", "at", "reason"],
                "additionalProperties": false,
                "properties": {
                    "kind": {"const": "hotspot"},
                    "file": {"$ref": "#/$defs/FileId"},
                    "at": {"$ref": "#/$defs/Timestamp"},
                    "sha": {"type": "string", "description": "Full 40-char commit SHA; default empty when unknown."},
                    "reason": {"type": "string"}
                }
            },
            "EventOwnershipSplit": {
                "type": "object",
                "required": ["kind", "file", "at", "authors"],
                "additionalProperties": false,
                "properties": {
                    "kind": {"const": "ownership_split"},
                    "file": {"$ref": "#/$defs/FileId"},
                    "at": {"$ref": "#/$defs/Timestamp"},
                    "sha": {"type": "string"},
                    "authors": {
                        "type": "array",
                        "items": {"$ref": "#/$defs/AuthorId"}
                    }
                }
            },
            "EventApiDrift": {
                "type": "object",
                "required": ["kind", "file", "at", "pub_items_changed"],
                "additionalProperties": false,
                "properties": {
                    "kind": {"const": "api_drift"},
                    "file": {"$ref": "#/$defs/FileId"},
                    "at": {"$ref": "#/$defs/Timestamp"},
                    "sha": {"type": "string"},
                    "pub_items_changed": {"type": "integer", "minimum": 0}
                }
            },
            "EventRename": {
                "type": "object",
                "required": ["kind", "file", "at", "from", "to"],
                "additionalProperties": false,
                "properties": {
                    "kind": {"const": "rename"},
                    "file": {"$ref": "#/$defs/FileId"},
                    "at": {"$ref": "#/$defs/Timestamp"},
                    "sha": {"type": "string"},
                    "from": {"type": "string"},
                    "to": {"type": "string"}
                }
            },
            "EventIncidentAftershock": {
                "type": "object",
                "required": ["kind", "file", "at", "window_days"],
                "additionalProperties": false,
                "properties": {
                    "kind": {"const": "incident_aftershock"},
                    "file": {"$ref": "#/$defs/FileId"},
                    "at": {"$ref": "#/$defs/Timestamp"},
                    "sha": {"type": "string"},
                    "window_days": {"type": "integer", "minimum": 0}
                }
            },
            "Handle": {
                "description": "Tagged union addressing evidence; discriminator is `kind`.",
                "oneOf": [
                    {
                        "type": "object",
                        "required": ["kind", "file", "blob_prefix"],
                        "additionalProperties": false,
                        "properties": {
                            "kind": {"const": "file"},
                            "file": {"$ref": "#/$defs/FileId"},
                            "blob_prefix": {
                                "type": "string",
                                "description": "12-char git blob SHA prefix — unambiguous byte-exact lookup."
                            }
                        }
                    },
                    {
                        "type": "object",
                        "required": ["kind", "commit", "sha"],
                        "additionalProperties": false,
                        "properties": {
                            "kind": {"const": "commit"},
                            "commit": {"type": "integer", "minimum": 0, "description": "Interned CommitId."},
                            "sha": {"type": "string", "description": "Full 40-char git commit SHA."}
                        }
                    },
                    {
                        "type": "object",
                        "required": ["kind", "base", "head"],
                        "additionalProperties": false,
                        "properties": {
                            "kind": {"const": "range"},
                            "base": {"type": "string"},
                            "head": {"type": "string"}
                        }
                    }
                ]
            },
            "Enrichments": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "pull_requests": {
                        "type": "object",
                        "description": "Commit SHA → PullRequestRef.",
                        "additionalProperties": {"$ref": "#/$defs/PullRequestRef"}
                    }
                }
            },
            "PullRequestRef": {
                "type": "object",
                "required": ["number", "title", "state", "merged"],
                "additionalProperties": false,
                "properties": {
                    "number": {"type": "integer", "minimum": 0},
                    "title": {"type": "string"},
                    "state": {"type": "string", "description": "GitHub PR state: `open`, `closed`."},
                    "merged": {"type": "boolean"},
                    "merged_at": {
                        "oneOf": [{"type": "null"}, {"type": "string"}],
                        "description": "ISO-8601 merge timestamp; null when not merged."
                    },
                    "author": {
                        "oneOf": [{"type": "null"}, {"type": "string"}]
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Dict;
    use entropyx_core::CONTRACT_VERSION;

    #[test]
    fn schema_is_valid_json_and_has_toplevel_shape() {
        let s = schema_json();
        assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
        assert!(s["$id"].as_str().unwrap().contains(CONTRACT_VERSION));
        assert_eq!(s["type"], "object");
        let required: Vec<&str> = s["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for key in ["schema", "dict", "files", "events", "handles"] {
            assert!(required.contains(&key), "{key} must be required");
        }
    }

    #[test]
    fn metric_column_enum_stays_in_lockstep_with_dict() {
        // The schema's enum of metric column names must exactly mirror
        // Dict::METRIC_COLUMNS — a drift here means the schema lies
        // about the on-the-wire column order.
        let s = schema_json();
        let enum_vals: Vec<String> = s["$defs"]["Dict"]["properties"]["metrics"]["items"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let expected: Vec<String> = Dict::METRIC_COLUMNS.iter().map(|s| s.to_string()).collect();
        assert_eq!(enum_vals, expected);
    }

    #[test]
    fn every_event_variant_has_a_def() {
        let s = schema_json();
        let defs = &s["$defs"];
        for v in [
            "EventHotspot",
            "EventOwnershipSplit",
            "EventApiDrift",
            "EventRename",
            "EventIncidentAftershock",
        ] {
            assert!(defs[v].is_object(), "missing $def for {v}");
        }
        // And the union itself must list all five.
        let one_of = s["$defs"]["Event"]["oneOf"].as_array().unwrap();
        assert_eq!(one_of.len(), 5);
    }

    #[test]
    fn signal_class_enum_covers_all_rfc008_labels() {
        let s = schema_json();
        let vals: Vec<&str> = s["$defs"]["SignalClass"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        // These six strings are the serde snake_case form of SignalClass.
        for label in [
            "refactor_convergence",
            "api_drift",
            "ownership_fragmentation",
            "incident_aftershock",
            "coupled_amplifier",
            "frozen_neglect",
        ] {
            assert!(vals.contains(&label), "missing label {label}");
        }
        assert_eq!(vals.len(), 6);
    }

    #[test]
    fn schema_round_trips_through_string_pretty_print() {
        // `serde_json::to_string_pretty` on the Value must succeed and
        // the result must parse back — cheap sanity check that the
        // schema builder didn't produce invalid JSON shapes.
        let s = schema_json();
        let text = serde_json::to_string_pretty(&s).expect("serialize");
        let back: serde_json::Value = serde_json::from_str(&text).expect("round-trip");
        assert_eq!(back, s);
    }
}
