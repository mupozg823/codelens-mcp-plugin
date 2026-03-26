#!/usr/bin/env python3
"""
Minimal stdio-to-SSE bridge for the JetBrains MCP Server.

It forwards MCP JSON-RPC messages received on stdio to the IDE's local
SSE session endpoint and relays IDE responses back to stdout.
"""

import argparse
import http.client
import json
import sys
import threading
import urllib.parse


class BridgeError(Exception):
    pass


class JetBrainsSseBridge:
    def __init__(self, host: str, port: int, sse_path: str):
        self.host = host
        self.port = port
        self.sse_path = sse_path
        self.message_path = None
        self.stdout_lock = threading.Lock()
        self.stop_event = threading.Event()

    def connect(self):
        conn = http.client.HTTPConnection(self.host, self.port, timeout=None)
        conn.request("GET", self.sse_path, headers={"Accept": "text/event-stream"})
        response = conn.getresponse()
        if response.status != 200:
            raise BridgeError(f"SSE connect failed: {response.status} {response.reason}")

        self.message_path = self._read_endpoint_event(response)
        if not self.message_path:
            raise BridgeError("Did not receive /message endpoint from SSE stream")

        thread = threading.Thread(target=self._sse_reader, args=(response,), daemon=True)
        thread.start()

    def _read_endpoint_event(self, response):
        current_event = None
        data_lines = []

        while True:
            raw = response.readline()
            if not raw:
                return None

            line = raw.decode("utf-8", errors="ignore").rstrip("\r\n")
            if line == "":
                if current_event == "endpoint":
                    payload = "\n".join(data_lines)
                    parsed = urllib.parse.urlparse(payload)
                    path = parsed.path
                    if parsed.query:
                        path += f"?{parsed.query}"
                    return path
                current_event = None
                data_lines = []
                continue

            if line.startswith("event:"):
                current_event = line.split(":", 1)[1].strip()
            elif line.startswith("data:"):
                data_lines.append(line.split(":", 1)[1].lstrip())

    def _sse_reader(self, response):
        current_event = None
        data_lines = []

        try:
            while not self.stop_event.is_set():
                raw = response.readline()
                if not raw:
                    break

                line = raw.decode("utf-8", errors="ignore").rstrip("\r\n")
                if line == "":
                    if current_event == "message":
                        payload = "\n".join(data_lines)
                        try:
                            message = json.loads(payload)
                        except json.JSONDecodeError:
                            print(f"Bridge received invalid JSON from SSE: {payload[:200]}", file=sys.stderr)
                        else:
                            self._write_message(message)
                    current_event = None
                    data_lines = []
                    continue

                if line.startswith("event:"):
                    current_event = line.split(":", 1)[1].strip()
                elif line.startswith("data:"):
                    data_lines.append(line.split(":", 1)[1].lstrip())
        finally:
            self.stop_event.set()

    def _write_message(self, payload):
        body = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        with self.stdout_lock:
            sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode("ascii"))
            sys.stdout.buffer.write(body)
            sys.stdout.buffer.flush()

    def _write_error(self, request_id, message):
        if request_id is None:
            print(message, file=sys.stderr)
            return

        payload = {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": -32000,
                "message": message,
            },
        }
        self._write_message(payload)

    def _post_message(self, payload):
        body = json.dumps(payload).encode("utf-8")
        conn = http.client.HTTPConnection(self.host, self.port, timeout=10)
        conn.request(
            "POST",
            self.message_path,
            body=body,
            headers={"Content-Type": "application/json"},
        )
        response = conn.getresponse()
        response.read()
        conn.close()
        if response.status not in (200, 202):
            raise BridgeError(f"POST failed: {response.status} {response.reason}")

    def run(self):
        while not self.stop_event.is_set():
            message = read_stdio_message()
            if message is None:
                self.stop_event.set()
                return

            method = message.get("method")
            if method == "exit":
                self.stop_event.set()
                return

            request_id = message.get("id")
            try:
                self._post_message(message)
            except Exception as exc:
                self._write_error(request_id, f"Failed to forward MCP message: {exc}")


def read_stdio_message():
    headers = {}

    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            if headers:
                break
            continue

        text = line.decode("ascii", errors="ignore").strip()
        if ":" not in text:
            continue
        key, value = text.split(":", 1)
        headers[key.strip().lower()] = value.strip()

    length_header = headers.get("content-length")
    if not length_header:
        raise BridgeError("Missing Content-Length header")

    length = int(length_header)
    body = sys.stdin.buffer.read(length)
    if len(body) != length:
        raise BridgeError("Unexpected EOF while reading MCP message body")

    return json.loads(body.decode("utf-8"))


def parse_args():
    parser = argparse.ArgumentParser(description="Bridge stdio MCP to the JetBrains IDE SSE endpoint")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=64342)
    parser.add_argument("--sse-path", default="/sse")
    return parser.parse_args()


def main():
    args = parse_args()
    bridge = JetBrainsSseBridge(args.host, args.port, args.sse_path)

    try:
        bridge.connect()
        print(
            f"Connected to JetBrains MCP SSE at http://{args.host}:{args.port}{args.sse_path}",
            file=sys.stderr,
        )
        bridge.run()
    except Exception as exc:
        print(f"Bridge startup failed: {exc}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
