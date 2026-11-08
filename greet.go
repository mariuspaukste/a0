package main

import "strings"

// Greet returns a greeting for name, falling back to "world" when it's blank.
func Greet(name string) string {
	name = strings.TrimSpace(name)
	if name == "" {
		name = "world"
	}
	return "Hello, " + name + "! (from Go + WebAssembly)"
}
