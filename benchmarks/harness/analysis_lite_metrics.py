"""Provider accounting only; server estimates never enter actual token totals."""

from __future__ import annotations


def normalize_provider_usage(raw, *, source=None, input_includes_cache=None):
    """Retain raw evidence; callers must declare whether input includes cache.

    Inclusive input is Codex-style total input. Exclusive input is Claude-style
    uncached input and requires both cache-read and cache-creation counts. Missing
    components stay unknown. This function accepts one provider usage record;
    aggregation of multiple turns belongs to the run-level evaluator.
    """
    result = {
        "source": source, "raw": raw, "input_includes_cache": input_includes_cache,
        "status": "unknown", "input_tokens": None, "output_tokens": None,
        "cached_input_tokens": None, "cache_creation_input_tokens": None,
        "uncached_input_tokens": None, "total_tokens": None,
    }
    if raw is None:
        return result
    if not isinstance(raw, dict) or not source:
        result.update(status="error", error="usage must be an object with a source")
        return result
    try:
        def count(key):
            value = raw.get(key)
            if value is not None and (type(value) is not int or value < 0):
                raise ValueError(f"invalid count: {key}")
            return value

        base = count("input_tokens")
        output = count("output_tokens")
        cached = count("cached_input_tokens")
        cache_read = count("cache_read_input_tokens")
        created = count("cache_creation_input_tokens")
        if cached is not None and cache_read is not None and cached != cache_read:
            raise ValueError("conflicting cache-read counts")
        cached = cached if cached is not None else cache_read
        actual = uncached = None
        if input_includes_cache is True:
            actual = base
            if base is not None and cached is not None:
                known_cache = cached + (created if created is not None else 0)
                if known_cache > base:
                    raise ValueError("cache counts exceed inclusive input")
                # Creation is not supplied by all inclusive providers. Do not
                # claim a complete uncached split when creation is unknown.
                uncached = base - known_cache if created is not None else None
                if "cached_input_tokens" in raw and "cache_creation_input_tokens" not in raw:
                    uncached = base - cached  # Codex's sole cache component.
        elif input_includes_cache is False:
            uncached = base
            if all(value is not None for value in (base, cached, created)):
                actual = base + cached + created
        result.update(input_tokens=actual, output_tokens=output,
                      cached_input_tokens=cached, cache_creation_input_tokens=created,
                      uncached_input_tokens=uncached,
                      total_tokens=actual + output if actual is not None and output is not None else None,
                      status="ok" if actual is not None and output is not None else "partial")
    except ValueError as exc:
        result.update(status="error", error=str(exc))
    return result
