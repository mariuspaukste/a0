# a0

The age of sail, from 1600: a world that builds itself. Pick a scenario and watch
peoples grow, ships arrive, colonies and trading posts take root, and harvests, winters,
famines and epidemics shape it all. Written in Rust with no dependencies and running in
the browser as WebAssembly.

**Play:** https://mariuspaukste.github.io/a0/

## Scenarios

**New World.** A generated continent across the Atlantic. Native nations already live
there in villages of longhouses and maize fields. Spain (1601), England (1607), France
(1608) and the Dutch (1614) arrive by ship at the latitudes they historically settled,
found coastal forts, and keep sending emigrants. England sends many, France very few.
Colonies grow from immigration and large families, expand inland, and export tobacco,
furs, timber and fish. Contact brings epidemics to native villages, which have no
immunity.

**India.** The subcontinent as it was, with the Mughal Empire, the Deccan sultanates,
the southern kingdoms and Kandy holding the land and its great cities in their real
places. The Portuguese already hold Goa, Cochin and Diu. The Dutch (1605), English
(1608), Danish (1620) and French (1668) East India Companies come to trade, not to
settle. They build fortified factories at their historical sites (Surat, Fort Geldria,
Tranquebar, Bombay, Chinsurah, Pondichéry…), which grow by drawing in Indian merchants
and weavers. A failed monsoon can bring famine.

Both run on a real calendar with seasons: one harvest a year to last until the next,
snow in northern winters, autumn colours, monsoon greens. Population numbers are scaled
down to fit the map but keep their proportions.

## Towns

Every lot in a town has a land value. Lots near the market square, the waterfront, the
wharves and the church are dear, and lots next to smithies, tanneries and burnt ruins are
cheap. Houses and the church take the best lots they can find. The trades take cheap
ground at the edge, and as a town grows around them they sell up and move out, leaving
their lots to houses.

Fires break out now and then, more often in winter and in bigger towns. Most are put out
after a house or two, but on a windy night a fire can leap the streets and take a whole
quarter. Timber towns burn worst; brick, stone and adobe much less. A colony that suffers a
great fire rebuilds in brick and stone, as Boston was ordered to after its fire of 1679. Burnt lots are cleared and built on again.

## Controls

Drag to pan, pinch or scroll to zoom, space to pause. Tap a tile to inspect it.
**Land value** colours town lots from red (cheap) to green (dear).

Ships and bands of settlers run themselves, but you can take over any of them. Tap one
to pick it (it's ringed in gold and its route is dotted), then tap where it should go:

- **Ship:** tap one of its own towns to land its passengers there, a stretch of coast to
  found a colony or factory, or open sea to sail there and wait.
- **Settlers:** tap a spot to walk there and found a town.

**Automatic** hands the unit back; **Done** lets go of it.

## Layout

| File | What it is |
| --- | --- |
| `src/ecs.rs` | A from-scratch Entity-Component-System: entities, sparse-set storages, resources, commands, schedule |
| `src/era.rs` | The historical settings: peoples, powers, arrival dates, real towns and places, names, calendar and climate |
| `src/world.rs` | Terrain: noise-generated continents, and the map of India |
| `src/sim.rs` | Components, resources and the systems that run every month, plus orders for units |
| `src/render.rs` | Software renderer that draws the world into a pixel buffer, with the seasons |
| `src/web.rs` | Functions exported to the browser (no wasm-bindgen) |
| `src/main.rs` | Runs the simulation in the terminal |
| `examples/snapshot.rs` | Renders a frame to an image file, without a browser |
| `web/index.html` | The page: copies pixels to a canvas, handles input, shows the side panel |

## Commands

```
make test                                  # run the tests
make run YEARS=100 SEED=7 SCENARIO=india   # simulate in the terminal and print a summary and map
make serve                                 # build for the browser and serve at http://localhost:8080
```

Pushing to `main` runs the tests and deploys to GitHub Pages.
