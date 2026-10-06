# AGENTS.md — plugins/mxm-poly-06

Parent: [`../AGENTS.md`](../AGENTS.md)

# Purpose

The nice-plug shell for **mxm-poly-06**, a six-voice polysynth with a built-in chorus, inspired by the
Roland JUNO-106. Identity, parameters, MIDI, presets, telemetry and the editor. The whole instrument
is [`crates/mxm-poly-06-dsp`](../../crates/mxm-poly-06-dsp/AGENTS.md).

Shared conventions — nice-plug's API, the init-patch contract, preset rules, `process()` realtime
rules, the editor contract — live in the parent and are not restated here. This doc holds what is
**local to this plugin**; the evidence, history and test detail behind it are in [NOTES.md](NOTES.md).

# Ownership

`Cargo.toml`, `LICENSE`, `README.md`, `BASELINE-M0.md`, `control-map.json`, `presets/`, and `src/` —
`lib.rs`, `params.rs`, `routes.rs`, `preset.rs`, `telemetry.rs`, and `editor.rs` with its
`editor/{binding, sections, visuals}.rs`.

**`BASELINE-M0.md` is the routing conversion's reference**, produced by `lib.rs`'s `#[ignore]`d
`baseline` module: a measurement seam, not a second `process()`
([NOTES.md § BASELINE-M0.md](NOTES.md#baseline-m0md)).

# Local Contracts

## Permanent identifiers

| What | Value |
|---|---|
| `CLAP_ID` | `dk.mxm.mxm-poly-06` — assembled from `plugin_name!` in `src/lib.rs`, **not** from `CARGO_PKG_NAME` |
| Parameter `#[id]`s | **LFO:** `lforate` `lfosync` `lfodelay` · **DCO:** `range` `pulsewidth` `pulse` `saw` `sub` `noise` · **HPF:** `hpf` · **VCF:** `cutoff` `resonance` · **VCA:** `level` `vcamode` · **ENV:** `attack` `decay` `sustain` `release` · **Chorus:** `chorus` · **Voice:** `portamento` `keyassign` `volume` · **Disclosed:** `bendrange` `lfomod` |
| Routing `#[id]`s | `mod_<target>_<source>`, the amount, and `mod_<target>_<source>on`, the presence — targets `pitch` `width` `cutoff` `amp`, sources `key` `env` `lfo` `vel` `wheel` `press` `bend` `saw` `pulse` `sub` `noise`. Written out in `routes::ROUTE_IDS`, which `the_id_table_is_what_the_derive_actually_produces` holds to the derive |
| Retired `#[id]`s | `dcolfo` `pwmdepth` `pwmmode` `envamount` `envpolarity` `vcflfo` `keytrack` `bendfilter` — never to be reused (`no_retired_id_reappears`) |

Treat all three as public interface. **The LFO rate has the collection's one tempo sync** (`lfosync`,
`params::LFO_SYNC`), resolved once a buffer by `MxmPoly06Params::synced_lfo_rate`
([NOTES.md § The LFO's tempo sync](NOTES.md#the-lfos-tempo-sync)).

## The chorus is inside, with one control

The JUNO-106 shipped with a stereo BBD chorus after the voice sum and the patch's VCA, so it is part
of the machine (`crates/mxm-poly-06-dsp/src/chorus.rs`) and the output is stereo. **It has one
control, because the circuit has one**: Off, I, II or both. Never add a depth or mix knob here; that
is the standalone's move ([NOTES.md § The chorus is inside](NOTES.md#the-chorus-is-inside-with-the-evidence)).

## Exact silence at idle — a recorded deviation from warts-and-all

A real 106 hisses through its chorus with no key pressed. This plugin reaches **exact digital
silence** at idle: the BBD's noise floor is present while anything sounds and fades once nothing
does, and `ProcessStatus::Normal` follows. The collection's numeric contract — silence in gives
exactly zero out — wins over a wart, because a plugin that never falls silent never reports idle,
never reaches the export's silence threshold, and keeps every host's CPU on for a hiss.

**This is local, and a fidelity call the owner may reverse.** It was the plan's working assumption
and was built under it. Reversing it changes the tail, the export and the golden score.

## The machine's modulation is routing, and its wiring is the init patch

- `routes.rs` declares a presence and a signed amount for every *(target, source)* pair (four
  targets, eleven sources); nothing asks whether a route is the machine's own. **The six paths the
  JUNO wires are present in the init patch at zero depth**
  (`the_init_patch_wires_exactly_the_machines_own_routes`). Detail and the retirement record:
  [NOTES.md § The machine's modulation is routing](NOTES.md#the-machines-modulation-is-routing-and-its-wiring-is-the-init-patch).
- **A route's amount reads what its pair delivers**, in the target's own unit; a route the JUNO
  never had reads the standard reach (`a_route_reads_what_its_pair_delivers_and_reads_back`).
  **Every amount is the collection's one route parameter** (`mxm_modulation_params::reading`'s
  `amount_param`) and survives the host's round trip, `-0` included
  (`every_reading_survives_the_hosts_round_trip_a_rounded_zero_included`).
- **The parameters are held to the DSP**: `every_route_parameter_says_what_the_dsp_does` runs
  `mxm_plugin_test::routing_checks` against `mxm_poly_06_dsp::conformance::Declared`.
  `[dev-dependencies]` turns the `conformance` features on; the bundle carries neither.
- **The PWM mode switch is the (Pulse width ← LFO) route's presence**; **the envelope's polarity is
  the sign of (Cutoff ← Envelope)** (X2). A signed amount is stored as `(a + 1) / 2`.
- **No `filter_state` is built** (X1, the owner, 2026-09-15): old state loses what the retired ids held.
- **Once per block, then once per sample** (`resolve_topology`, `render_sample`, shared by
  `process()` and `render_block_for_test`): `Routes::topology_from` snaps each newly present route's
  smoother to its stored depth; `Synth::set_topology` arms every voice, idle ones included;
  `Routes::advance` advances live smoothers and applies the wheel's push.
  `a_route_arriving_after_an_idle_span_is_block_partition_invariant`.
- What the routing costs is not yet measured: `BASELINE-M0.md` says why, and how to measure it.

## Parameters, volumes and the wheel

- **Every parameter's text survives the host's conversion**
  (`params::tests::every_parameter_text_is_idempotent_through_the_hosts_conversion`); a clean
  `clap-validator` run does not prove it. A time chooses `ms`/`s` from its rounded milliseconds;
  Cutoff uses `mxm-mono-pr1`'s reading ([NOTES.md § Parameter text](NOTES.md#every-parameters-text-survives-the-hosts-conversion)).
- **Two volumes**: `level` (the VCA, before the chorus, part of the sound) and `volume` (the master,
  after it). No factory preset sets the master volume (`no_factory_preset_sets_the_master_volume`).
- **`lfomod` is the wheel's reach and starts useful**; the wheel is the amount and rests at zero
  (`the_wheel_reaches_something_at_init`). Every other amount starts at zero (`every_amount_starts_at_zero`).
- **The wheel's push is a named legacy path** (D5): `wheel × lfomod` is added into (Pitch ← LFO)'s
  amount whichever way it points, clamped to one, writing no parameter
  (`the_wheel_push_adds_the_same_whichever_way_the_route_points`). The raw wheel is a Wheel source
  ([NOTES.md § The wheel's reach](NOTES.md#the-wheels-reach-is-a-configuration-not-an-amount)).

## Performance input and polyphony

- **Velocity and pressure are routing sources** (decision 1.7) though the machine had neither; they
  mean what they mean on every instrument (`mxm_modulation::standard`). Nothing routes them at Init.
  Channel pressure, the wheel and the bender reduce to the channel of the latest note-on.
- Per-note **pitch** expression is honoured, per voice, routed by the ledger. Per-note pressure,
  vibrato, brightness and expression are dropped (D6); MPE is out of scope
  ([NOTES.md § Velocity and pressure](NOTES.md#velocity-and-pressure-are-routing-sources-per-note-only-pitch-is-answered)).
- **Polyphonic parameter modulation is not declared** (`CLAP_POLY_MODULATION_CONFIG`): the routing
  added per-voice sources, not per-voice parameter destinations
  ([NOTES.md](NOTES.md#polyphonic-parameter-modulation-is-not-declared)).

## The control map omits three roles, and says so

- `control-map.json` claims only roles the standard declared before this instrument:
  **`filter.hpf`, `filter_env.polarity` and `fx.chorus` are absent** until an unknown role is inert
  rather than fatal. **`filter_env.polarity` is unclaimable here for good** (it is a route's sign).
- The saw and pulse switches fill `mixer.src1` and `mixer.src2`; `osc1.pwm_source` is unfilled.
- **Every depth role names a route the init patch wires**
  (`a_control_map_role_never_points_at_a_dead_route`). Detail:
  [NOTES.md § The control map](NOTES.md#the-control-map-omits-three-roles-and-says-so).

## Presets

The preset system is `crates/mxm-preset`; `editor/binding.rs` re-exports `mxm_preset::binding`.
Local: **fifty factory sounds** in `presets/`, generated from `FACTORY_DESIGN` in `preset.rs`'s test
module (`write_the_factory_presets`, `#[ignore]`d); `the_factory_files_match_the_design_they_were_generated_from`
catches a stale file. Init has no file ([NOTES.md § preset.rs](NOTES.md#presetrs-is-this-instruments-instrument-impl-and-its-factory-set)).

## The editor, and its brief

- The brief is [`docs/briefs/mxm-poly-06.md`](../../docs/briefs/mxm-poly-06.md); layout rulings and
  history are in [NOTES.md § The editor](NOTES.md#the-editor-and-its-brief).
- **Developer channel** (parent's rule) with `MXM_DEV_CC`: CC 119 selects a category (0–5) or
  Parameters (127), CC 117 toggles the preset browser, CC 118 changes nothing, CC 116 sets the theme
  by index without saving it.
- Six `page_items` keys, the cards' positions in `SECTIONS`. The chorus is on the amplifier's card;
  the bend range and the wheel's vibrato are on the Voice card.
- **The master `volume` is no card's**: an inline slider in the app bar under key 64
  (`the_master_volume_is_drawn_once_in_the_app_bar`). Each target's routes are a stack under the
  controls they move.
- The opening size is the quarter-4K budget hugged (`REFERENCE`, `the_opening_size_is_the_budget_hugged`);
  `MINIMUM` is set by the app bar (`the_app_bar_holds_in_the_minimum_window`) and exercised by
  `every_dynamic_page_fits_and_every_card_is_reachable`.
- **Every card is a `mxm_ui::tree`, and every floor is computed**: `sections::card` describes each of
  the six cards once; `page_items` passes the floor as the ceiling too. A route stack's floor is every
  route revealed at its widest reading; the displays state their sizes in `visuals`.
- **A row that mixes knobs and switches is top-aligned, and each switch names the knob it sits
  beside.** `editor::tests::every_card_passes_the_tree_checks_in_every_state` runs the shared checks.
- `src/telemetry.rs` is the **only** DSP → editor channel. The editor is what raised this crate's
  MSRV to **1.95**; the DSP crate stays at 1.87.

## The keyboard coverage check reveals every route

The parent's contract applies; `REVEAL` does nothing. **It runs in two frames**, at defaults and with
every routing pair present (`the_keyboard_cursor_reaches_and_operates_every_route_revealed`).

## `editor`, `params`, `routes` and `telemetry` are public

They are `pub` (with `Section`, `SECTIONS`, `title()` and the card grouping), permanently, so the
layout lab draws **these real cards** rather than copies ([NOTES.md](NOTES.md#editor-params-routes-and-telemetry-are-public)).

## Activation refuses a rate the DSP cannot hold

`activate` returns `false`, before anything changes, for a non-finite host rate or one below
`mxm_poly_06_dsp::MIN_SAMPLE_RATE` (`activation_refuses_a_non_finite_rate_and_any_below_the_floor`,
`the_rate_floor_activates_and_plays_at_every_parameter_extreme`).

# Work Guidance

# Verification

```bash
cargo test -p mxm-poly-06
cargo test -p mxm-poly-06 --lib every_route_parameter_says_what_the_dsp_does
cargo test -p mxm-poly-06 --lib every_card_passes_the_tree_checks_in_every_state
# Every page, light and dark, for review -> target/layout-tree/mxm-poly-06/<MXM_PICTURES tag>/
MXM_PICTURES=after cargo test -p mxm-poly-06 --lib tree_pictures -- --ignored
cargo clippy -p mxm-poly-06 --all-targets
cargo xtask bundle mxm-poly-06 --release
cargo xtask bundle mxm-poly-06                 # debug too: assert_process_allocs only fires there
clap-validator validate "target/bundled/mxm-poly-06.clap"
```

Run the debug bundle as well as release because `assert_process_allocs` is debug-only.

Through the player: `behaviour.rs`, `golden_audio.rs` and `t7_editor.rs`. **Not signed off**: no
real DAW, listening comparison, §12 trial or §15 QA gate by eye, so the editor is built, not done;
fidelity is UNVERIFIED ([NOTES.md § Through the player](NOTES.md#through-the-player-and-what-was-not-run)).

# Child DOX Index

No child AGENTS.md files.
