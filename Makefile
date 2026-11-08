export GO111MODULE := on

.PHONY: test build serve clean

test:
	go test ./...

build:
	mkdir -p dist
	cp web/* dist/
	cp "$$(go env GOROOT)/lib/wasm/wasm_exec.js" dist/
	GOOS=js GOARCH=wasm go build -o dist/main.wasm .

serve: build
	@echo "Open http://localhost:8080"
	cd dist && python3 -m http.server 8080

clean:
	rm -rf dist
