# NOTES.md — plugins/mxm-poly-06

The detail behind this folder's AGENTS.md: history, measurements, rationale and worked examples.
AGENTS.md is the contract; this file is the reference it links to.

## BASELINE-M0.md

**`BASELINE-M0.md` is the routing conversion's reference**, captured before any of it
(`plans/plan-mxm-poly-06-modulation.md` M0): the factory bank's digests through
`render_block_for_test`, the plugin's own per-sample path, and the throughput cases the cost gate
compares. `lib.rs`'s `#[ignore]`d `baseline` module produces both. The seam is a measurement seam, not
a second `process()`. Its last section accounts for every factory digest the conversion moved.

## The LFO's tempo sync

**The LFO rate has the collection's one tempo sync** (`lfosync`, 2026-09-25;
`plans/plan-tempo-sync-controls.md`): the quarter note beside Rate on the LFO card, on
`params::LFO_SYNC` (1/32 to four bars, the top the fastest). `MxmPoly06Params::synced_lfo_rate`
resolves it once a buffer from the modulated position; it replaces the unsmoothed free rate while
synced, and `Telemetry::tempo` lets the knob read its division.

## The chorus is inside, with the evidence

The parent's rule: an instrument ships the effects its original had, and no others. **The JUNO-106
shipped with a stereo BBD chorus, wired after the voice sum and the patch's VCA** — read off the
service manual's Jack Board schematic (`research:instruments/juno-106.md` §2, §6;
`research:effects/juno-chorus.md`). So it is part of the machine and lives in
`crates/mxm-poly-06-dsp/src/chorus.rs`, with the output stereo because the chorus makes it so.

**It has one control, because the circuit has one.** Off, I, II, or both: the rate is all a mode
changes; depth and mix are fixed. Adding a depth or a mix knob is the plug-out's move and makes it
not a JUNO chorus. *A chorus should have a depth* is the argument that will be offered, and it is
about choruses in general, not this one.

## The machine's modulation is routing, and its wiring is the init patch

`routes.rs` declares a presence and a signed amount for every *(target, source)* pair — four targets
and eleven sources, `mxm-mono-01`'s shape — and the DSP's `routing` module evaluates them in every
voice. Nothing asks whether a route is the machine's own. **The six paths the JUNO wires are present
in the init patch at zero depth** — (Pitch ← LFO), (Pulse width ← LFO), (Cutoff ← Envelope),
(Cutoff ← LFO), (Cutoff ← Key) and (Cutoff ← Bend) — so a fresh instance is the machine:
`the_init_patch_wires_exactly_the_machines_own_routes` here, and
`the_init_routes_at_zero_depth_render_bit_identically_to_nothing_routed` in the DSP.

**A route's amount reads what its pair delivers**, in the target's own unit — semitones, a percentage
of width, octaves, a percentage of level, and per octave of keyboard for a Key route — so the
machine's own routes read their retired sliders' numbers at full: +7.00 st, +45 %, +7.00 and
+3.00 oct, +1.00 oct/oct and +4.00 oct. **A route the JUNO never had reads the collection's
standard reach** — +12.00 st, +12.00 st/oct from Key, +4.00 oct, +100 % — the DSP's `FULL_SCALE`
being where that is decided. `a_route_reads_what_its_pair_delivers_and_reads_back` holds each, and
that a typed reading lands back on its amount.
**Every amount is the collection's one route parameter** — `mxm_modulation_params::reading`'s
`amount_param`, on the travel the pair's offer allows (both halves, on every pair here) — so its
reading, its parse and its negative-zero rule are the shared ones.
`every_reading_survives_the_hosts_round_trip_a_rounded_zero_included` sends every amount through
the host's own conversion either side of zero, where a plain signed format printed `-0`, which
`clap-validator`'s `param-conversions` fails whenever its random values land there.

**The parameters are held to the DSP** (`plans/plan-modulation-standard.md`, which this instrument
piloted): `every_route_parameter_says_what_the_dsp_does` runs `mxm_plugin_test::routing_checks`
against `mxm_poly_06_dsp::conformance::Declared` — each pair's travel is its offer's, a performance
route's reading carries its target's unit and states what the voice's own graph delivers, and every
reading survives the host's round trip. `[dev-dependencies]` turns on the DSP's and
`mxm-modulation`'s `conformance` features for it; the bundle carries neither.

**Eight ids retired for these**, each an owner decision under the governing plan's decision 1.13
(`plans/plan-mxm-poly-06-modulation.md` §4.1), and every state they could reach is reachable: the
DCO's LFO, the PWM in LFO mode, the envelope amount with its polarity and the VCF's LFO are the same
arithmetic to the bit (`the_machines_own_routes_are_bit_identical_to_the_expressions_they_replace`),
key tracking and the bender's filter depth to rounding
(`key_tracking_and_bend_reach_the_old_numbers_to_rounding`). What changed on the way, deliberately:

- **The PWM mode switch is the (Pulse width ← LFO) route's presence**, and LFO mode's depth is its
  amount. The route sums onto `pulsewidth`, so a design that was in LFO mode stores a width of 0.5,
  which is what that mode played.
- **The envelope's polarity is the sign of (Cutoff ← Envelope)** (ruling X2): one authority over the
  sign, rather than a switch beside a signed amount.
- **The factory designs were translated** by plan §4.1's table and regenerated; a signed amount is
  stored as `(a + 1) / 2`. What that moved is accounted for sound by sound in `BASELINE-M0.md`.

**Decision, X1 — the owner, 2026-09-15: no `filter_state` is built.** A project or host state saved
before the conversion keeps every surviving parameter and **loses what the eight retired ids held**,
and an automation lane on a retired id is not carried — as on `mxm-mono-02`.

**Once per block, then once per sample**, through `resolve_topology` and `render_sample` in `lib.rs`,
which `process()` and the measurement seam `render_block_for_test` both call:

- `Routes::topology_from` builds the topology against last block's and **snaps each newly present
  route's smoother** to its stored depth —
  `a_re_added_route_arrives_at_its_stored_depth_rather_than_ramping_from_a_stale_one`.
- `Synth::set_topology` arms **every voice, idle ones included**, and each voice clears a source that
  has just become read — the DSP's contract.
- `Routes::advance`, per sample, advances each live route's smoother and applies the mod wheel's push.
- `a_route_arriving_after_an_idle_span_is_block_partition_invariant` renders that path at 64, 37 and
  1024 samples a block, with a route arriving mid-ramp after an idle span.

**What the routing costs is not yet measured**: `BASELINE-M0.md` says why, and how to measure it.

## Every parameter's text survives the host's conversion

Format, parse and format again through the normalised conversion gives the same text, which
`clap-validator`'s `param-conversions` checks only at its own grid, so a clean run does not prove it.
A time chooses `ms` or `s` from its *rounded* milliseconds: chosen from the raw value, 0.9995 s
printed `1000 ms` and read back `1.00 s` (Attack, LFO delay, Portamento), and Decay and Release,
whose inverse of one second lands just below it, read `1.00 s` back as `1000 ms`. Cutoff uses
`mxm-mono-pr1`'s reading — the `1.0 kHz` bucket in whole hertz — because nice-plug's
`v2s_f32_hz_then_khz` chooses its unit from the raw value and printed `1000.0 Hz` at 999.95 Hz, which
read back `1.0 kHz`. `params::tests::every_parameter_text_is_idempotent_through_the_hosts_conversion`
walks every parameter with the unit on, over clap-validator 0.4.1's own grid, the `i / 19` grid and
both sides of every unit, precision and sign switch.

## Two volumes, in the schematic's two places

`level` is the patch's VCA, **before** the chorus: it sets how hard the BBD is driven, so it is part
of the sound and travels with a preset. `volume` is the master, **after** the chorus. Both are
ordinary parameters under the shared preset contract; what keeps Volume out of the *sound* of a
preset is content — `no_factory_preset_sets_the_master_volume`. The editor draws them apart for
the same reason: Level on the Amplifier card, Volume in the app bar (*The editor, and its brief*).

## The wheel's reach is a configuration, not an amount

`lfomod` is how much vibrato the mod wheel adds when pushed all the way, and it starts at a useful
depth rather than zero. The **wheel** is the amount, and it rests at zero; at `lfomod` zero the wheel
would do nothing, which reads as a broken wheel rather than a neutral patch.
`the_wheel_reaches_something_at_init` pins it. Every other amount, every route's included, starts at
zero — `every_amount_starts_at_zero`.

**The wheel's push is a named legacy path** (plan D5): at play time `wheel × lfomod` is added into
(Pitch ← LFO)'s amount **whichever way that amount points**, clamped to one, and writes no parameter —
what the wheel always did to the DCO's LFO depth, so a wheel at rest is exactly the stored depth. A
negative route is partly cancelled rather than deepened, as the machine's two depths would sum, and
the depth stays continuous through zero (`the_wheel_push_adds_the_same_whichever_way_the_route_points`).
The raw wheel is a Wheel source beside it.

## Velocity and pressure are routing sources; per note, only pitch is answered

**The machine sent no velocity and had no aftertouch, and both are sources anyway** — the governing
plan's decision 1.7, *the performance inputs are sources on every instrument*. A voice holds its
note-on's velocity; channel pressure, the wheel and the bender are reduced to the channel of the
latest note-on, as the bend always was. Nothing routes them at Init, so a fresh instance still
ignores them.

**They mean what they mean on every instrument** (`mxm_modulation::standard`, the owner's ruling of
2026-09-26, found here: *Amplitude ← Velocity* at +100 % was inaudible when Velocity was published
0…1). Velocity is `v − 1`, so a route does nothing at the hardest note and **+100 % into Amplitude
makes the gain follow velocity**; Key is the glided note; each gesture is zero at rest; Amplitude
is the standard factor, silence to double however many routes are summed.

Per-note **pitch** expression is honoured and is per-voice state, routed by the ledger. Per-note
pressure, vibrato, brightness and expression are still dropped (plan D6): decision 1.7 names
*channel* pressure, and MPE is out of scope. This is the polyphonic answer the parent's *Per-note
pitch expression is accepted* deferred, and it is per instrument.

## Polyphonic parameter modulation is not declared

nice-plug couples the CLAP voice-info extension to `CLAP_POLY_MODULATION_CONFIG`, which advertises a
host offsetting a **parameter** for one voice. The routing conversion added per-voice *sources* —
key, envelope, velocity, the voice's own audio — not per-voice parameter destinations, so the
governing plan's expectation that decision 1.7 would invert this did not come true (plan D7; its
open decision 8). Declaring it would promise honouring `PolyModulation` for every parameter in
every voice, which is a separate feature; honest is better than advertised-and-dropped.

## The control map omits three roles, and says so

`control-map.json` claims only roles the standard declared before this instrument. **`filter.hpf`,
`filter_env.polarity` and `fx.chorus` are absent** — the chorus being the one a player would want —
because an instrument map naming a role the player's compiled standard does not declare is refused
whole (mxm-kit's [`docs/MXM_CONTROL_MAP.md`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/MXM_CONTROL_MAP.md) §9). The three roles exist in the standard now, appended into free
slots and onto a new Effects page; this file claims them once an unknown role is inert rather than
fatal.

**The saw and pulse switches fill `mixer.src1` and `mixer.src2`**, as `mxm-mono-01`'s saw and pulse
*levels* do: on a machine with switches, the switch is the source's level.

**Every depth role names a route the init patch wires** — `osc1.pwm_depth`, `filter.env_amount`,
`filter.key_track`, `filter.lfo_amount`, `lfo1.to_pitch` and `lfo1.to_filter` — so each knob is live
on a fresh instance (`a_control_map_role_never_points_at_a_dead_route`). `osc1.pwm_source` is
unfilled, its switch being the width route's presence, and **`filter_env.polarity` is unclaimable
here for good**: the polarity is a route's sign.

## `preset.rs` is this instrument's `Instrument` impl and its factory set

The preset system is mxm-kit's `crates/mxm-preset` (since 2026-09-04): this file was the third copy, and with
three the extraction rule was met and the extraction made. `editor/binding.rs` re-exports
`mxm_preset::binding`, the collection's one binding, since 2026-09-24; it was one of eighteen drifted
copies. What is local: **fifty factory sounds** in `presets/`, each with its category, generated from
`FACTORY_DESIGN` in `preset.rs`'s test module (`write_the_factory_presets`, `#[ignore]`d;
`the_factory_files_match_the_design_they_were_generated_from` catches a stale file). Init has no
file.

## The editor, and its brief

Carries the collection's **developer channel** (mxm-kit's
[`docs/plugin-conventions.md`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/plugin-conventions.md#a-developer-channel-in-every-editor),
*A developer channel in every editor*; `plugins/AGENTS.md` holds its one-line contract): with `MXM_DEV_CC` in the process environment, CC 119 selects a category (0–5) or
Parameters (127), as defined by the parent, CC 117 opens and closes the preset browser. CC 118 is
consumed and changes nothing: nothing is disclosed since the Bender's two controls moved onto the
Voice card (2026-09-28). CC 116 sets the theme by index — 0 light, 1 dark, 2 system — without saving
it.

Six `page_items` keys, the cards' positions in `SECTIONS`: Voice is Performance; LFO/Envelope
are Modulators; Oscillator is Generators; Filter and *Amplifier and chorus* are Tone and a preferred
group. No controller-map page changes. **The chorus is on the amplifier's card** (R2's call, the
owner's to overrule — `plans/plan-editor-standard.md` A4): hugged, it was one switch alone, and the
level beside it is what drives it. **The bend range and the wheel's vibrato (`sections::BEND`) are
on the Voice card**, a knob row under Key assign and Portamento, each showing its value (the owner,
2026-09-28: *no reason to put them behind a bender knob*); they were a `tree::disclosure` in the
Amplifier card's footer. The LFO paints *Rate* and *Delay* (`sections::panel_label`); the two-module
card keeps *VCA mode* and *Chorus mode*; a switch's cells are its parameter's own option text. **The
master `volume` is no card's** (owner, 2026-09-18: an instrument's master output is in the app bar):
an inline slider beside the level meter (design system §3.1), drawn inside
`mxm_ui::navigation::bar_card` under key 64, outside the paging keys, and reached by the keyboard
cursor through `navigation::paged_with_bar`.
`the_master_volume_is_drawn_once_in_the_app_bar` holds that it registers only there, whichever card
is requested. The opening size is the quarter-4K budget hugged (`REFERENCE`, held by
`the_opening_size_is_the_budget_hugged`); the minimum is at least one widest card (the Oscillator)
plus gutters (`MINIMUM`, exercised by `every_dynamic_page_fits_and_every_card_is_reachable`), and
the app bar at its last compact step is wider and sets it (`the_app_bar_holds_in_the_minimum_window`).
**Each target's routes are a stack under the controls they move** — Pitch and Pulse width on the
Oscillator card, Cutoff on the Filter card, Amplitude on the *Amplifier and chorus* — drawn by
`mxm_modulation_params::ui::stack`. The filter curve reads the envelope's reach from
(Cutoff ← Envelope)'s signed amount and draws none while that route is absent.

**Every card is a `mxm_ui::tree`, and every floor is computed** (`plans/plan-layout-tree.md`;
mxm-kit's [`crates/ui/AGENTS.md`](https://github.com/mxm-audio/mxm-kit/blob/main/crates/ui/AGENTS.md), *A card body as data*). `sections::card` describes each of the six cards
once; `paging::editor::show` measures that tree for the card's floor and height, and
`sections::paint` draws it leaf by leaf through the same bindings, so the controls, their gestures
and their names are unchanged. `page_items` computes each floor every frame and passes it as the
card's ceiling too: every card is exactly as wide as its content (`plans/plan-editor-standard.md`
A1), with no usability minimum (A2).

- **A route stack's floor is every route revealed at its widest reading**
  (`mxm_modulation_params::ui::stack_size`): the source and its reading side by side, never
  overlapping, over a `TRACK_MIN` track.
- Knob rows are the collection's `mxm_ui::tree::knob_row` (at `mxm_ui::control::knob_column`);
  the switch beside a knob stands on its grid (`Kind::Segmented` with `beside`); the Oscillator's
  two picture toggles are a row under a *Waveform* line, `SPACE_2` apart as the hand layout spaced
  them.
- **The displays state their sizes** in `visuals`: the filter curve `HEIGHT` tall with no
  minimum width of its own (`RESPONSE_MIN_WIDTH`), the voice display `VOICES_HEIGHT` tall and
  `voices_min_width` wide — six cells, each the widest key name in the caption style with
  `SPACE_2` either side. Both fill the card's width. A voice cell's number and the key beneath it
  each keep a line of their own, so a note sounding never moves the number (design system §7.5).
- `sections::draw`, which the layout lab calls, builds the section's tree and `tree::show`s it;
  its `spare` goes into the filter curve's height. `the_lab_entry_point_draws_every_cards_tree`
  holds it to the tree's height and the card's bounds.

`editor::tests::every_card_passes_the_tree_checks_in_every_state` runs the shared checks
(`mxm_plugin_test::tree_checks`) over every card at Init; with every route revealed at full
negative depth; and with every voice sounding the key with the longest name.
The brief is [`docs/briefs/mxm-poly-06.md`](../../docs/briefs/mxm-poly-06.md).
`every_dynamic_page_fits_and_every_card_is_reachable` checks every page in both themes at opening,
quarter-4K content and minimum sizes; component floor/row checks remain. **A row that mixes knobs
and switches is top-aligned, and each switch names the knob it sits beside**, so the shared control
lays it out on that knob's grid — label on the knob's name line, cells on its circle;
mxm-kit's [`crates/ui/AGENTS.md`](https://github.com/mxm-audio/mxm-kit/blob/main/crates/ui/AGENTS.md)
(*A control beside a knob sits on the knob's grid*) has the rule, and its `NOTES.md` the
measurement. Reported twice from screenshots of the Oscillator card; the first fix top-aligned the
rows, which was necessary and not enough. `src/telemetry.rs` is the **only** DSP → editor channel.
The editor is what raised this crate's MSRV to **1.95**; the DSP crate stays at 1.87.

## The keyboard coverage check reveals every route

The parent's *Keyboard cursor* rule (in full: mxm-kit's `docs/plugin-conventions.md`, *The keyboard
cursor runs in every editor*) owns the contract; `REVEAL` does nothing,
since nothing is disclosed (the bend controls are on the Voice card). The app bar's Volume registers
on every page, through its bar card. **And it runs in two frames**, as the governing plan's §8a
asks: at defaults, and with every routing pair present, which is what paints every route's row
(`the_keyboard_cursor_reaches_and_operates_every_route_revealed`).

## `editor`, `params`, `routes` and `telemetry` are public

They are `pub`, with the `Section` enum, its `SECTIONS`, `title()` and the card grouping the flow
reads, so `apps/mxm-layout-lab` (`apps/mxm-layout-lab/AGENTS.md` in the private archive)
can draw **these real cards** on its bench instead of copying the section code, which would then
drift.

It began as a branch-only change for that lab and **is now permanent**, because the reflowing layout
the lab was built to judge shipped on 2026-09-04: the same section data that feeds
the paging renderer in this editor is what the bench re-draws. Nothing else changes — no item's own
behaviour moves, and the shipped `cdylib` and its CLAP entry point are untouched.

## Activation refuses a rate the DSP cannot hold

`activate` returns `false`, before anything changes, for a non-finite host rate or one below
`mxm_poly_06_dsp::MIN_SAMPLE_RATE`, 1 kHz: a NaN rate, or one low enough for a corner's floor to
cross 0.45 of it, panicked on the audio thread.
`activation_refuses_a_non_finite_rate_and_any_below_the_floor` holds the refusal, and
`the_rate_floor_activates_and_plays_at_every_parameter_extreme` a held note at the floor with every
parameter at its default and at either end.

## Through the player, and what was not run

Through the player: `plugins/mxm-poly-06/host-tests/tests/behaviour.rs` measures the instrument on
rendered audio — the routing's headline gesture among it, (Cutoff ← Envelope) opening a closed
filter over a chord, and (Amplitude ← LFO), a route the machine never had, making a tremolo —
`plugins/mxm-poly-06/host-tests/tests/golden_audio.rs` hashes a fixed score — from day one, because
mxm-mono-03's `crates/mxm-mono-03-dsp/NOTES.md` records what its absence cost — and the
mxm-player repository's `apps/mxm-player/tests/t7_editor.rs` asserts the
floating editor is advertised and, `#[ignore]`d because it opens a real window, that the editor
opens, closes and **reopens** through the player's hosting path. That last one was run by hand on
2026-09-02 and passed.

**Not run:** a real DAW, a listening comparison against hardware, the brief's §12 trial, and the §15
QA gate by eye at every zoom and in both themes — so the editor is **built, not signed off**;
mxm-kit's [`docs/AGENTS.md`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/AGENTS.md) makes that gate the condition for calling an editor done. Fidelity is UNVERIFIED —
see the DSP crate's doc.
