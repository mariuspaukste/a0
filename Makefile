.PHONY: test run web serve clean

test:
	cargo test --release

# Simulate in the terminal: make run YEARS=100 SEED=7
run:
	cargo run --release -- $(or $(YEARS),50) $(or $(SEED),1)

web:
	cargo build --release --lib --target wasm32-unknown-unknown
	mkdir -p dist
	cp web/* dist/
	cp target/wasm32-unknown-unknown/release/a0.wasm dist/

serve: web
	@echo "Open http://localhost:8080"
	cd dist && python3 -m http.server 8080

clean:
	cargo clean
	rm -rf dist
