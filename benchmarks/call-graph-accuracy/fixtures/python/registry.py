# Call-graph accuracy fixture (Python).
# Patterns:
#   - direct call:    parse_line() inside setup()
#   - decorator:      @log_calls
#   - function reference (v1.11.1+): register("evt", on_event), map(parse_line, ...)


def log_calls(fn):
    def wrap(*a, **kw):
        return fn(*a, **kw)

    return wrap


def parse_line(line):
    return line.strip()


def on_event(payload):
    return payload


@log_calls
def setup():
    register("evt", on_event)
    pipe = list(map(parse_line, ["a", "b"]))
    parse_line("seed")
    return pipe
