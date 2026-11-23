# a0

A historical simulation of the age of sail, from 1600. Watch real history play out
against a world that builds itself, follow its statistics year by year, and change
history to see how the future turns out differently. Peoples grow, ships arrive,
colonies and trading posts take root, states rise and fall, and harvests, winters,
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

## History and what-ifs

In the India scenario, real history from the Company's charter in 1600 to the Crown's
takeover in 1858 plays out as a timeline of events. Each event has conditions, and it
happens only if the simulated world still allows it. The Battle of Plassey needs an
English factory in Bengal, a Nawab of Bengal to fight, and Clive, who made his name at
Arcot in 1751. Events change the world: the Marathas, the Nawabs of Bengal and the
Nizam of Hyderabad take over towns as the Mughal Empire comes apart, and the Company
takes Bengal's revenues and then much of India.

The **History** tab lets you change history: give Aurangzeb a strong heir, have France
back Dupleix, keep Mir Jafar loyal at Plassey, bankrupt the Company, or fail the monsoon
or start a plague where you tap. Later events check their conditions against the world
as it now is. With a strong heir the empire holds together, so Bengal never breaks away,
there is no Nawab to fight at Plassey, and no Company Raj follows. The chronicle marks
each event ✓ as in history, ≠ went differently, ✗ didn't happen (and why), or ★ your
change, next to what really happened.

When you first change history, a second copy of the world starts from the same seed
and is left alone. It replays the years already played in the background, then keeps
pace with yours. The chronicle shows what happened there when it went differently, the
charts draw it as a dashed line, and **Show the unchanged world** puts it on the map.
Small chances (a fire, a failed monsoon, an epidemic) also fall differently once the
two worlds part, so not every difference comes from your change.

The great famines are calibrated to the usual death tolls: some 7.4 million in Gujarat
and the Deccan in 1630–32, and seven to ten million in Bengal in 1770.

## Economy

Each December the **Economy** tab adds up every Indian state with a Leontief
input–output model of four sectors: farming, textiles, crafts and mining, and trade.
Gross output is `x = (I − A)⁻¹ d`, where `A` holds what each sector buys from the others
per unit of output and `d` is what households eat and wear plus cloth bought for
export. GDP is the value added.

Farming can't produce more than each town's harvest that year. When it falls short,
households cut back on cloth and goods much more than on food, so a failed monsoon also
hits the weavers. A state the Company rules pays part of its spending as revenue, which
leaves as cloth shipped home.

Totals are scaled so 1600 matches Angus Maddison's estimates for India (135 million
people, $74 billion in 1990 dollars), and the charts set the timeline against his
figures for later years. Tap a chart to read off a year. The input coefficients are
round, illustrative numbers, not estimates for any real year.

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

Drag to pan, pinch or scroll to zoom, and ⤢ to see the whole map. The top bar shows the
date, with play/pause (or space) and a speed button. The ☰ menu holds the scenario, a
new random world, map colours and the **Land value** overlay, which colours town lots
from red (cheap) to green (dear). On a phone the panel is a bottom sheet: drag its
handle, or tap it, to peek at the latest event, open it halfway or fill the screen.

Tap a tile to inspect it, or a state in the World tab to go to it on the map.

Zoomed out, the map colours states boldly and names them, and each town is a marker
sized by its people (a white centre marks a city). Close in, the colours fade to show
fields, streets and buildings. **Map colours** can keep states bold or faint at every
zoom instead. A red ring marks a town with plague or another epidemic, an orange ring
a town going hungry, and a dashed orange ring the area where the monsoon (or the
harvest) has failed. Town names never overlap: towns in trouble and big towns are
named first.

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
| `src/history.rs` | Real history as a timeline of events with conditions and effects, the player's what-ifs, and the chronicle |
| `src/timelines.rs` | Your timeline and the unchanged one, replayed from the same seed after your first change |
| `src/economy.rs` | The yearly input–output (Leontief) accounts: output by sector, GDP, food, exports, Company revenue |
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
