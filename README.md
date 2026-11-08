# a0

A world that builds itself: civilizations found cities, cities grow street by street,
and settlers spread across a generated continent. Part Civilization, part SimCity,
written in Rust with no dependencies and running in the browser as WebAssembly.

**Play:** https://mariuspaukste.github.io/a0/

## Layout

| File | What it is |
| --- | --- |
| `src/ecs.rs` | A from-scratch Entity-Component-System: entities, sparse-set storages, resources, commands, schedule |
| `src/world.rs` | Terrain and noise-based map generation |
| `src/sim.rs` | Components, resources and the systems that run every month |
| `src/render.rs` | Software renderer that draws the world into a pixel buffer |
| `src/web.rs` | Functions exported to the browser (no wasm-bindgen) |
| `src/main.rs` | Runs the simulation in the terminal |
| `web/index.html` | The page: copies pixels to a canvas, handles input, shows the side panel |

## Commands

```
make test                    # run the tests
make run YEARS=100 SEED=7    # simulate in the terminal and print a summary and map
make serve                   # build for the browser and serve at http://localhost:8080
```

Pushing to `main` runs the tests and deploys to GitHub Pages.
