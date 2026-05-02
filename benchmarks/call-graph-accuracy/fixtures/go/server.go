// Call-graph accuracy fixture (Go).
// Patterns:
//   - direct call:    handler(w, r) inside run
//   - selector:       server.Listen()
//   - function reference (v1.11.2+): Register("/api", handler), Schedule(teardown)
package main

func handler(w int, r int) {}

func teardown() {}

func setup() {
	Register("/api", handler)
	Schedule(teardown)
}

func run() {
	handler(0, 0)
	setup()
}
