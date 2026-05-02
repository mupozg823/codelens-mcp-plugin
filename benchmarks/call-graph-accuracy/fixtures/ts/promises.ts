// Call-graph accuracy fixture (TypeScript, no JSX).
// Patterns:
//   - direct call:        decode(payload)
//   - method:             api.fetch()
//   - function reference (v1.11.1+): Promise.then(success).catch(handleError),
//                                    arr.map(parseLine), bus.on("evt", onEvent)

function decode(payload: string): string {
  return payload.trim();
}

function parseLine(line: string): string {
  return line.split(",")[0];
}

function onEvent(payload: unknown): unknown {
  return payload;
}

function handleError(err: Error): void {
  return undefined;
}

function success(value: string): string {
  return value;
}

export const handleRequest = async (payload: string): Promise<string> => {
  decode(payload);
  api.fetch().then(success).catch(handleError);
  const lines = ["a", "b"];
  const parsed = lines.map(parseLine);
  bus.on("evt", onEvent);
  return parsed.join(",");
};
