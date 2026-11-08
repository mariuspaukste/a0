package main

import "testing"

func TestGreet(t *testing.T) {
	tests := []struct {
		name, in, want string
	}{
		{"with name", "Marius", "Hello, Marius! (from Go + WebAssembly)"},
		{"trims spaces", "  Ada  ", "Hello, Ada! (from Go + WebAssembly)"},
		{"empty falls back", "", "Hello, world! (from Go + WebAssembly)"},
		{"blank falls back", "   ", "Hello, world! (from Go + WebAssembly)"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := Greet(tt.in); got != tt.want {
				t.Errorf("Greet(%q) = %q, want %q", tt.in, got, tt.want)
			}
		})
	}
}
