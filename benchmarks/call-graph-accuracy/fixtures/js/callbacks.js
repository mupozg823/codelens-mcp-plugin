// Call-graph accuracy fixture (JS).
// Patterns:
//   - direct call: validateUser(req) inside handleRequest
//   - method:      service.run(req)
//   - function reference (v1.11.1+): setTimeout(timeoutHandler), arr.map(parseLine), bus.on("evt", onEvent)

function parseLine(line) {
  return line.trim();
}

function onEvent(payload) {
  return payload;
}

function timeoutHandler() {
  return 1;
}

function validateUser(req) {
  return req && req.user;
}

const handleRequest = async (req) => {
  validateUser(req);
  service.run(req);
  const lines = ["a", "bb"];
  const parsed = lines.map(parseLine);
  bus.on("evt", onEvent);
  setTimeout(timeoutHandler, 100);
  return parsed;
};
