//go:build js && wasm

package main

import "syscall/js"

func main() {
	// Expose Greet to JavaScript as window.greet(name).
	js.Global().Set("greet", js.FuncOf(func(this js.Value, args []js.Value) any {
		name := ""
		if len(args) > 0 {
			name = args[0].String()
		}
		return Greet(name)
	}))

	// Block forever so the exported function stays callable.
	select {}
}
