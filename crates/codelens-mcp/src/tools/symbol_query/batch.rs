//! Array / cursor / snapshot inputs for the read tools.
//!
//! ADR-0016 decision 6 (execution plan I2.3): the read surface has to be a safe
//! target for programmatic tool calling and parallel fan-out. That needs three
//! additive input shapes, none of which change the singular contract:
//!
//! * **array** — a plural alias of the tool's singular selector (`names`,
//!   `queries`, `symbol_names`). One handler invocation per element; a bad
//!   element becomes a per-item error entry instead of failing the batch.
//! * **cursor** — `page_size` + opaque `cursor`. Pagination is a pure slice of
//!   the single deterministic result, so stitched pages equal the unpaged
//!   response byte-for-byte. The cursor embeds the index generation and a
//!   fingerprint of the full list, so a page boundary can never straddle two
//!   different result sets.
//! * **snapshot** — every response advertises `index_snapshot`; supplying it
//!   back pins the read. A pin that is not the current committed generation is
//!   rejected with the existing retryable `IndexGenerationChanged` (-32011).
//!   Serving historical snapshots is deliberately out of scope — refusing is
//!   the correct answer.
//!
//! The layer sits between the dispatch table and the tool handlers
//! (`tools/mod.rs` wires it), so `search`'s mode routing inherits it for free:
//! `resolve_verb_target` forwards every non-`mode` argument unchanged.

use crate::AppState;
use crate::error::CodeLensError;
use crate::tool_runtime::ToolResult;
use serde_json::{Map, Value, json};

/// Per-tool binding of the singular selector, its array alias, and the primary
/// result array that cursors page over.
pub(crate) struct BatchSpec {
    pub(crate) tool: &'static str,
    /// Singular argument the handler already understands.
    pub(crate) singular: &'static str,
    /// Additive array alias advertised in `tools.toml`.
    pub(crate) plural: &'static str,
    /// Top-level payload array that single-call pagination slices.
    pub(crate) primary_array: &'static str,
}

const BATCH_SPECS: &[BatchSpec] = &[
    BatchSpec {
        tool: "find_symbol",
        singular: "name",
        plural: "names",
        primary_array: "symbols",
    },
    BatchSpec {
        tool: "get_ranked_context",
        singular: "query",
        plural: "queries",
        primary_array: "symbols",
    },
    BatchSpec {
        tool: "find_referencing_symbols",
        singular: "symbol_name",
        plural: "symbol_names",
        primary_array: "references",
    },
];

/// Keys consumed by this layer. They are stripped before the handler runs so
/// existing `unknown args ignored` warnings stay quiet.
const RESERVED_KEYS: &[&str] = &["snapshot", "cursor", "page_size"];

const SNAPSHOT_PREFIX: &str = "gen:";
const CURSOR_VERSION: &str = "cl1";

pub(crate) fn batch_spec(tool: &str) -> Option<&'static BatchSpec> {
    BATCH_SPECS.iter().find(|spec| spec.tool == tool)
}

/// Required-param hook for `dispatch::envelope::validate_required_params`:
/// the array form stands in for the singular required argument.
pub(crate) fn satisfies_required_via_batch(tool: &str, key: &str, arguments: &Value) -> bool {
    batch_spec(tool).is_some_and(|spec| {
        spec.singular == key && arguments.get(spec.plural).is_some_and(Value::is_array)
    })
}

/// Entry point wired into the dispatch table for every programmatic read tool.
pub(crate) fn run_programmatic(
    tool: &'static str,
    handler: fn(&AppState, &Value) -> ToolResult,
    state: &AppState,
    arguments: &Value,
) -> ToolResult {
    let Some(spec) = batch_spec(tool) else {
        return handler(state, arguments);
    };
    let generation = state.symbol_index().committed_generation();
    let project = || state.project().as_path().display().to_string();

    if let Some(pinned) = arguments.get("snapshot") {
        let requested = parse_snapshot(pinned)?;
        if requested != generation {
            return Err(CodeLensError::IndexGenerationChanged {
                project: project(),
                before: requested,
                after: generation,
            });
        }
    }

    let page_size = parse_page_size(arguments.get("page_size"))?;
    let cursor = match arguments.get("cursor") {
        Some(value) => Some(parse_cursor(value, generation, &project())?),
        None => None,
    };

    let inner_args = strip_layer_keys(arguments, spec);
    let items = arguments.get(spec.plural);

    let (mut payload, meta) = match items {
        Some(Value::Array(elements)) => run_batch(spec, handler, state, &inner_args, elements)?,
        Some(_) => {
            return Err(CodeLensError::Validation(format!(
                "`{}` must be an array of strings",
                spec.plural
            )));
        }
        None => handler(state, &inner_args)?,
    };

    let array_key = if items.is_some() {
        "batch"
    } else {
        spec.primary_array
    };
    if page_size.is_some() || cursor.is_some() {
        apply_pagination(
            &mut payload,
            array_key,
            page_size,
            cursor.as_ref(),
            generation,
            &project(),
        )?;
    }
    if let Some(map) = payload.as_object_mut() {
        map.insert(
            "index_snapshot".to_owned(),
            json!(snapshot_token(generation)),
        );
    }
    Ok((payload, meta))
}

/// One handler invocation per array element, keyed by the singular selector.
/// A failing element is an entry, never a batch-wide failure.
fn run_batch(
    spec: &BatchSpec,
    handler: fn(&AppState, &Value) -> ToolResult,
    state: &AppState,
    inner_args: &Value,
    elements: &[Value],
) -> Result<(Value, crate::protocol::ToolResponseMeta), CodeLensError> {
    let mut entries = Vec::with_capacity(elements.len());
    let mut first_meta: Option<crate::protocol::ToolResponseMeta> = None;
    let mut error_count = 0usize;

    for element in elements {
        let mut entry = Map::new();
        entry.insert(spec.singular.to_owned(), element.clone());
        let Some(selector) = element.as_str() else {
            error_count += 1;
            entry.insert("ok".to_owned(), json!(false));
            entry.insert(
                "error".to_owned(),
                json!({
                    "code": CodeLensError::Validation(String::new()).jsonrpc_code(),
                    "message": format!("`{}` entries must be strings", spec.plural),
                }),
            );
            entries.push(Value::Object(entry));
            continue;
        };

        let mut args = inner_args.clone();
        if let Some(map) = args.as_object_mut() {
            map.insert(spec.singular.to_owned(), json!(selector));
        }
        match handler(state, &args) {
            Ok((payload, meta)) => {
                entry.insert("ok".to_owned(), json!(true));
                entry.insert("result".to_owned(), payload);
                if first_meta.is_none() {
                    first_meta = Some(meta);
                }
            }
            Err(error) => {
                error_count += 1;
                entry.insert("ok".to_owned(), json!(false));
                entry.insert(
                    "error".to_owned(),
                    json!({
                        "code": error.jsonrpc_code(),
                        "message": error.to_string(),
                    }),
                );
            }
        }
        entries.push(Value::Object(entry));
    }

    let total = entries.len();
    let payload = json!({
        "batch": entries,
        "batch_count": total,
        "ok_count": total - error_count,
        "error_count": error_count,
    });
    let meta = first_meta.unwrap_or_else(|| {
        crate::tool_runtime::success_meta(crate::protocol::BackendKind::TreeSitter, 0.0)
    });
    Ok((payload, meta))
}

/// Cursors are pure slices of the deterministic full list: page N ++ page N+1
/// reconstructs the unpaged array exactly.
fn apply_pagination(
    payload: &mut Value,
    array_key: &str,
    page_size: Option<usize>,
    cursor: Option<&CursorState>,
    generation: u64,
    project: &str,
) -> Result<(), CodeLensError> {
    let Some(map) = payload.as_object_mut() else {
        return Ok(());
    };
    let Some(Value::Array(full)) = map.get(array_key) else {
        return Ok(());
    };
    let full = full.clone();
    let fingerprint = fingerprint_array(&full);
    let offset = match cursor {
        Some(state) => {
            if state.fingerprint != fingerprint {
                return Err(CodeLensError::Validation(format!(
                    "cursor does not match this result set (arguments changed between pages); \
                     restart pagination for `{array_key}`"
                )));
            }
            state.offset.min(full.len())
        }
        None => 0,
    };
    let window = page_size.or_else(|| cursor.map(|state| state.page_size));
    let end = match window {
        Some(size) => (offset + size).min(full.len()),
        None => full.len(),
    };
    let page: Vec<Value> = full[offset..end].to_vec();
    let returned = page.len();
    map.insert(array_key.to_owned(), Value::Array(page));
    map.insert(
        "page".to_owned(),
        json!({
            "offset": offset,
            "returned": returned,
            "total": full.len(),
        }),
    );
    if end < full.len() {
        let next = CursorState {
            generation,
            fingerprint,
            offset: end,
            page_size: window.unwrap_or(full.len()),
        };
        map.insert("next_cursor".to_owned(), json!(next.encode()));
    }
    let _ = project;
    Ok(())
}

#[derive(Debug)]
struct CursorState {
    generation: u64,
    fingerprint: u64,
    offset: usize,
    page_size: usize,
}

impl CursorState {
    fn encode(&self) -> String {
        format!(
            "{CURSOR_VERSION}.{}.{:016x}.{}.{}",
            self.generation, self.fingerprint, self.offset, self.page_size
        )
    }

    fn decode(raw: &str) -> Option<Self> {
        let mut parts = raw.split('.');
        if parts.next()? != CURSOR_VERSION {
            return None;
        }
        let generation = parts.next()?.parse().ok()?;
        let fingerprint = u64::from_str_radix(parts.next()?, 16).ok()?;
        let offset = parts.next()?.parse().ok()?;
        let page_size = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            generation,
            fingerprint,
            offset,
            page_size,
        })
    }
}

fn parse_cursor(
    value: &Value,
    generation: u64,
    project: &str,
) -> Result<CursorState, CodeLensError> {
    let raw = value
        .as_str()
        .ok_or_else(|| CodeLensError::Validation("`cursor` must be an opaque string".to_owned()))?;
    let state = CursorState::decode(raw).ok_or_else(|| {
        CodeLensError::Validation(format!(
            "`cursor` is not a CodeLens pagination token: {raw}"
        ))
    })?;
    if state.generation != generation {
        return Err(CodeLensError::IndexGenerationChanged {
            project: project.to_owned(),
            before: state.generation,
            after: generation,
        });
    }
    Ok(state)
}

fn parse_page_size(value: Option<&Value>) -> Result<Option<usize>, CodeLensError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .filter(|size| *size > 0)
            .map(|size| Some(size as usize))
            .ok_or_else(|| {
                CodeLensError::Validation("`page_size` must be a positive integer".to_owned())
            }),
        Some(_) => Err(CodeLensError::Validation(
            "`page_size` must be a positive integer".to_owned(),
        )),
    }
}

fn parse_snapshot(value: &Value) -> Result<u64, CodeLensError> {
    let parsed = match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text
            .strip_prefix(SNAPSHOT_PREFIX)
            .unwrap_or(text)
            .parse()
            .ok(),
        _ => None,
    };
    parsed.ok_or_else(|| {
        CodeLensError::Validation(
            "`snapshot` must be an index token from a previous response (`gen:<n>`)".to_owned(),
        )
    })
}

fn snapshot_token(generation: u64) -> String {
    format!("{SNAPSHOT_PREFIX}{generation}")
}

fn strip_layer_keys(arguments: &Value, spec: &BatchSpec) -> Value {
    let mut cloned = arguments.clone();
    if let Some(map) = cloned.as_object_mut() {
        for key in RESERVED_KEYS {
            map.remove(*key);
        }
        map.remove(spec.plural);
    }
    cloned
}

/// FNV-1a over the canonical serialization of the full list. Hand-rolled so the
/// value is stable across processes (unlike `DefaultHasher`), which is what
/// makes a cursor safe to hand back to a different worker.
fn fingerprint_array(items: &[Value]) -> u64 {
    let serialized = Value::Array(items.to_vec()).to_string();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in serialized.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload() -> Value {
        json!({
            "symbols": [json!({"n": 1}), json!({"n": 2}), json!({"n": 3})],
            "count": 3,
        })
    }

    #[test]
    fn single_call_pages_are_pure_slices_of_the_full_array() {
        let generation = 7;
        let mut first = sample_payload();
        apply_pagination(
            &mut first,
            "symbols",
            Some(2),
            None,
            generation,
            "/tmp/project",
        )
        .expect("first page");
        let cursor = first["next_cursor"].as_str().expect("cursor").to_owned();
        assert_eq!(first["symbols"].as_array().expect("page").len(), 2);
        assert_eq!(first["page"]["total"], json!(3));

        let state = parse_cursor(&json!(cursor), generation, "/tmp/project").expect("decode");
        let mut second = sample_payload();
        apply_pagination(
            &mut second,
            "symbols",
            None,
            Some(&state),
            generation,
            "/tmp/project",
        )
        .expect("second page");

        let mut stitched = first["symbols"].as_array().expect("page").clone();
        stitched.extend(second["symbols"].as_array().expect("page").clone());
        assert_eq!(Value::Array(stitched), sample_payload()["symbols"]);
        assert!(second["next_cursor"].is_null());
    }

    #[test]
    fn cursor_from_a_different_result_set_is_rejected() {
        let mut payload = sample_payload();
        apply_pagination(&mut payload, "symbols", Some(1), None, 7, "/tmp/project")
            .expect("first page");
        let state = parse_cursor(&payload["next_cursor"], 7, "/tmp/project").expect("decode");

        let mut other = json!({"symbols": [json!({"n": 9})]});
        let error = apply_pagination(&mut other, "symbols", None, Some(&state), 7, "/tmp/project")
            .expect_err("fingerprint mismatch must be rejected");
        assert!(matches!(error, CodeLensError::Validation(_)), "{error:?}");
    }

    #[test]
    fn cursor_from_another_generation_is_retryable() {
        let mut payload = sample_payload();
        apply_pagination(&mut payload, "symbols", Some(1), None, 7, "/tmp/project")
            .expect("first page");
        let error = parse_cursor(&payload["next_cursor"], 9, "/tmp/project")
            .expect_err("stale cursor must be rejected");
        assert_eq!(error.jsonrpc_code(), -32011);
    }

    #[test]
    fn snapshot_tokens_round_trip_in_both_accepted_forms() {
        assert_eq!(parse_snapshot(&json!("gen:42")).expect("prefixed"), 42);
        assert_eq!(parse_snapshot(&json!("42")).expect("bare"), 42);
        assert_eq!(parse_snapshot(&json!(42)).expect("numeric"), 42);
        assert!(parse_snapshot(&json!("not-a-token")).is_err());
        assert_eq!(snapshot_token(42), "gen:42");
    }

    #[test]
    fn layer_keys_never_reach_the_handler() {
        let spec = batch_spec("find_symbol").expect("spec");
        let stripped = strip_layer_keys(
            &json!({
                "name": "sample",
                "names": ["sample"],
                "snapshot": "gen:1",
                "cursor": "cl1.1.0.0.1",
                "page_size": 2,
                "include_body": true,
            }),
            spec,
        );
        let map = stripped.as_object().expect("object");
        assert_eq!(map.len(), 2, "only handler-owned keys survive: {map:?}");
        assert!(map.contains_key("name") && map.contains_key("include_body"));
    }
}
