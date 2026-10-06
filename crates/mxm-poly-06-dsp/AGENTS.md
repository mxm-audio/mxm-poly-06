# AGENTS.md — crates/mxm-poly-06-dsp

Parent: [`../../AGENTS.md`](../../AGENTS.md)

# Purpose

The complete mxm-poly-06 instrument as plain Rust: six voices — each a DCO, an IR3109 ladder with the
JUNO's compensation, and one ADSR shared by filter and amplifier — the ledger that hands keys to
voices, one global LFO, the four-position HPF, and the BBD chorus, summed and wired in the
schematic's order. Framework-free, so the whole thing is testable with `cargo test` and no host.

**The instrument is a six-voice polysynth with a built-in chorus**, architecture inspired by the
Roland JUNO-106. `research:instruments/juno-106.md` and `research:effects/juno-chorus.md` are the research it rests on; `plans/plan-mxm-poly-06.md` is the completed implementation record.

# Ownership

Owns `src/` (`lib.rs`, `dco.rs`, `filter.rs`, `envelope.rs`, `lfo.rs`, `onepole.rs`, `hpf.rs`,
`chorus.rs`, `voice.rs`, `poly.rs`, `routing.rs`, `conformance.rs`), `examples/` (`juno_demo.rs`; `common/wav.rs` retired to
`mxm-measure`'s encoder), and
`Cargo.toml`.

Does **not** own parameter definitions, ranges, smoothing or the editor — those belong to
[`plugins/mxm-poly-06/AGENTS.md`](../../plugins/mxm-poly-06/AGENTS.md). This crate takes plain
values and a sample rate.

# Local Contracts

## The press ledger owns voice allocation

Nothing else in the collection allocates voices. `poly.rs` keeps a **ledger of presses**: one entry
per note-on until its note-off, carrying the key, the host's note id if any, and the **set of
voices** it went to. A press whose voices were all stolen stays as a **tombstone**, so its late
note-off lands there and not on the voice now sounding a re-pressed key. The module doc carries the whole table. Protect these regression-prone rules:

- **A steal shrinks a press's voice set by one; only an emptied set becomes a tombstone.** Leaving
  UNISON with the key held and then pressing a new key takes *one* of the six; the held key's
  note-off later releases the other five. Tombstoning the whole press stranded them —
  `leaving_unison_and_stealing_one_voice_leaves_five_to_release_with_the_held_key`.
- **An id-less note-off retires the oldest press of that key, tombstones first.** Key 60 stolen,
  key 60 pressed again, then the first press's late note-off: by-key matching would have released
  the new note — `a_late_id_less_note_off_for_a_stolen_key_does_not_release_its_re_press`.
- **An id is authoritative when both sides have one; otherwise the key decides** — the mono
  instruments' `NoteId::matches`, so a note-off carrying an id the press does not carry is not about
  it, whatever the key says.
- **A repeated press of a held key joins the voice and retriggers nothing.** The collection's legato
  joint, not the hardware's: the player's export merges a same-pitch joint into a hold because this
  collection's voices define the joint as no-retrigger, and a voice that retriggered here would sound
  different live from its own export. What a 106 does with a MIDI double-press is unverified.
- **The ledger is bounded and its overflow is a policy**: evict the oldest tombstone, else the oldest
  press, retiring it as a note-off would. A release one press early, never a stuck note, never an
  allocation. A host can send fresh-id note-ons for one key for ever and never a note-off, so the
  bound is not a claim that it cannot fill.
- **The assign mode governs future allocation only.** Nothing is re-voiced or retired when it moves.
- **A non-finite expression is dropped**, and every press keeps the offset it had: a NaN in a voice's
  pitch sum would stay in its DCO's phase —
  `a_non_finite_expression_is_dropped_and_the_pitch_stays_finite`.

## The post-mix chain settles for a fixed time, and nothing tries to detect exact zeros along it

`Synth::is_active` is *any voice active, or `POST_TAIL_S` not yet elapsed since the last one went
idle*. The fixed post-envelope settle covers the 15 Hz DC blocker and noise fade without fragile
exact-zero observation; `the_tail_covers_the_actual_decay` guards it.

**And idle is defined.** A GATE-mode voice's envelope keeps running behind the closed amplifier for
as long as `process` is called — the machine's envelope never stops — but once the instrument
reports idle a host may stop calling it, and a retrigger that depended on how long the host slept
would be worse than one that always starts from zero. So the moment the settle elapses every hidden
envelope is retired: an idle instrument has every envelope at zero, and the next note starts its
filter sweep from the bottom. `when_the_instrument_goes_idle_every_hidden_envelope_is_at_zero`
holds it; `a_gate_mode_voices_hidden_envelope_keeps_time_while_it_is_idle` holds the other half.

**A panic is idle at once, so it clears what an idle voice would carry into its next note.**
`Synth::all_sound_off` (CC 120) empties the ledger; through `Voice::all_sound_off` silences every
voice's envelope, key and gate and resets its ladder; resets the DC blocker and the HPF; silences the
chorus's audio path; and ends the settle, so the plugin reports `Normal` on the next block. An idle
voice skips its filter until its next note, so a ladder left as the panic found it would be where
that note starts, whether or not the host keeps calling. It keeps each voice's DCO phase, sub state
and noise (which idle freezes rather than resets), its portamento lag and its card's trims, the
global LFO, and the chorus modulator's phase. **A choke is not a panic**: it silences one voice
through `Voice::silence` and leaves the ladder as a release to idle does.
`panic_clears_every_voices_filter_and_a_late_release_of_an_old_press_moves_nothing` plays the same
keys for the same time through different cutoffs, resonances, mixes and sustains, panics both
instruments, calls them through a silent gap, and requires the same chord to render bit-identically
and a late release carrying a pre-panic id to leave the new press held; without the ladder's reset
it differs from the first sample.

## Two reusable exact-silence rules

- **Flushing a biquad's two states separately produces a limit cycle at the flush threshold.**
  `z2 = -a2·y` fell under 1e-20 and was zeroed while `z1 = -a1·y` (with `|a1| > 1`) survived and
  grew, which removed the damping term; the output hovered between 1e-20 and 5e-20 for ever. Two
  states of one recursion go to zero together (`chorus.rs`, `Biquad::process`). The mono crates have
  no biquad, so nothing to port — but the rule is general.
- **The envelope's decay can stall one ulp above its sustain threshold in `f32`.** With decay 0.4 s
  and sustain 0.8 the per-sample step falls under half an ulp of the level, and the stage never
  reaches `Sustain`. Inaudible, and it left the stage wrong. Fixed here by also switching when the
  step stopped moving the level; **`crates/mxm-mono-01-dsp/src/envelope.rs` has the same stall.**
- **The portamento lag is carried as its remaining distance**, `glide_offset`, by the same
  mechanism's lesson. `glide = note + (glide − note) × coef` stopped moving once the step was under
  half an ulp of the note, so after a glide the pitch rested short of the key until the next note —
  1.8 cents at 0.1 s, 18 at 1 s and 37 at the 2 s maximum at 48 kHz, twice that at 96 kHz. Found by
  the modulation standard's Key check and fixed on 2026-09-26 at the owner's request, here and in
  the three other instruments with the same form (`crates/mxm-mono-01-dsp/AGENTS.md`, *Numeric
  contracts*); `a_glide_lands_exactly_on_its_note` holds it. Four factory designs in UNISON — whose
  chord glides every voice — render differently by the glide's rounding.

## What is chosen, not measured

Two tables and a list. The first table is every number **about the machine** that the research does
not have, taken so the instrument could be built, and to be listened to in the fidelity gate. The
second is this crate's own implementation constants: audible in principle, chosen for the model
rather than the machine. The list after them names the constants **copied from `mxm-mono-01-dsp`
unchanged**, which that crate's doc owns and this one does not re-argue: `envelope.rs`'s
`ZERO_THRESHOLD`, `ATTACK_OVERSHOOT`, `ATTACK_TAUS`, `DECAY_TAUS` and `MIN_TIME_S`; `filter.rs`'s
`K_MAX`, `EXCITATION_THRESHOLD`, `EXCITATION_LEVEL`, `CUTOFF_MIN_HZ`, `NEWTON_ITERATIONS` and
`NYQUIST_FRACTION`; `dco.rs`'s `FREQ_MIN_HZ` and `NYQUIST_FRACTION`; and the two Padé approximants.
Everything else numeric in `src/` is a derived quantity or a test tolerance. None is a parameter.

| Where | What | Chosen |
|---|---|---|
| `filter.rs` | `COMP_AMOUNT` — how much of the ladder's droop the external circuit puts back | 0.8, the IR3109 research's Juno-6/60 starting point; the 106's side is unverified |
| `filter.rs` | `CAPACITOR_SPREAD` per voice card | ±2%, research §10 |
| `voice.rs` | `CUTOFF_OFFSET_CENTS`, `LEVEL_OFFSET_DB` per card | six small constants, fixed across instances, platforms and builds |
| `voice.rs` | `FILTER_ENV_OCTAVES`, `FILTER_LFO_OCTAVES`, `DCO_LFO_SEMITONES`, `PWM_SWING` — each now the full scale of the route that replaced its slider | 7 oct, 3 oct, 7 st, 0.45 |
| `routing.rs` | `BEND_OCTAVES` — the (Cutoff ← Bend) route's reach | 4 oct, the range the retired `bendfilter` sensitivity had |
| `hpf.rs` | `BOOST_DB`, `BOOST_CORNER_HZ` | +10 dB (Electric Druid's figure over a forum's +3), corner 250 Hz. The two cut corners are **computed** from the RC products under a stated topology assumption |
| `lfo.rs` | `FADE_FRACTION` — the delay's fade-in time constant | half the delay time |
| `poly.rs` | Voice stealing, UNISON priority, LFO-delay retrigger | POLY 1 steals the oldest press; POLY 2 takes the next in rotation, free or not; UNISON is **last-trigger with no fallback** — a second key takes all six, the first key's press becomes a tombstone, and releasing the second does not return to the first, unlike a conventional last-note-priority monosynth; the delay restarts on the first key after all were released |
| `poly.rs` | `VOICE_SUM_GAIN`, `POST_TAIL_S` | 1/3; 0.6 s |
| `chorus.rs` | `DELAY_CENTRE_MS`, `DELAY_DEPTH_MS` | 2.8 ± 1.2 ms, inside the research's inferred 1.3–4.3 ms |
| `chorus.rs` | `RATE_BOTH_HZ` | 1.3 Hz — faster than II, because a second current path into the gate can only raise it |
| `chorus.rs` | `NOISE_LEVEL`, and that the two BBDs' noise is independent | −75 dBFS RMS; independence assumed |
| `chorus.rs` | `WET_SWITCH_S`, `NOISE_FADE_S` | 5 ms, 50 ms |
| `dco.rs` | The finite reset and the residual amplitude variation | **Not modelled.** The reset is derived to be −0.16 dB at 20 kHz; the variation's shape is undocumented |

| Where | Implementation constant | Value, and where it comes from |
|---|---|---|
| `voice.rs` | `GATE_TIME_S` — the gate's rise and fall, and the VCA-source crossfade | 2 ms, mono-01's |
| `routing.rs` | `KEY_UNIT_SEMITONES` — the Key source's unit | 60: five octaves either side of middle C, so `KEY_OCTAVES` 5 is unity tracking. Middle C itself is the collection's (`mxm_modulation::standard::key`) |
| `poly.rs` | `DcBlocker::CUTOFF_HZ` — the output's AC coupling | 15 Hz, mono-01's; below the lowest 16' fundamental |
| `poly.rs` | `LEDGER_CAPACITY` | 64 presses; the overflow rule is what matters, not the number |
| `filter.rs` | `DRIVE` — how hard the input saturator is hit at unity | 1.0, mono-01's; the compensation multiplies it |
| `dco.rs` | The mixer's `0.5` headroom | mono-01's: four sources at full level must not slam the saturator |
| `chorus.rs` | `NOISE_SNAP` — where a fading noise gain becomes exactly zero | 1e-7, about −140 dBFS |
| `chorus.rs` | `DEPTH_MAX_MS` — the widest swing a standalone may ask for, and the line's size | exactly twice the circuit's depth, so the circuit is a linear control's midpoint and a half is exact in `f32` |
| `chorus.rs` | `CONTROL_SLEW_S` — the glide for a moving rate or depth | 20 ms, the middle of the range `plugins/AGENTS.md` measured; `set_mode` does not glide |
| `chorus.rs` | `PRE_FILTER_HZ`, `RECON_1`, `RECON_2`, `WET_GAIN` | **Computed** from the schematic's component values, not chosen — see the effects reference |

## The chorus is a module with a plain-values API, and it serves two products

Mono in, stereo out; it knows nothing about voices. `plugins/mxm-chorus-06` **depends on this crate
in place** and calls this module—no move or copy—because the chorus support code also serves the
instrument voice.

**Depth, mix and noise are fixed in this instrument and are inputs in the module** — the owner's
ruling, 2026-09-03, unlocking them for the standalone. `Chorus::set_mode` is the synth's whole
interface: it writes the rate from the three constants, without a glide, and touches nothing else,
so the synth's arithmetic is the same to the bit as before the inputs existed —
`the_inputs_at_the_circuits_values_render_identically_to_the_mode_api` holds it here, and the
chorus-on golden scores in `plugins/mxm-poly-06/host-tests/tests/golden_audio.rs` (I, II and Both,
pinned before the inputs were added) hold it through the real bundle. The standalone drives
`set_rate_hz`, `set_depth_ms`, `set_wet_level` and `set_noise_level`; the two that glide keep the
glide as the **remaining distance**, because the value form stalls in `f32` (measured: 1.29994 for
a target of 1.3, for ever). Adding a depth or a mix knob to *this instrument* still makes it not a
JUNO chorus; that is what the two products are for.

## Exact silence at idle is a labelled deviation

A real 106 hisses through its chorus with no key pressed. This instrument's noise floor is present
while anything sounds and fades to exact zero once nothing does — `Chorus::set_active`. The plugin's
AGENTS.md owns the ruling; this crate implements it, and `silence_in_gives_exactly_zero_out_after_the_tail`
holds it.

## Modulation is routing: eleven sources, four targets, a frame in every voice

`routing.rs` is this instrument's declaration on the shared
[`mxm-modulation`](https://github.com/mxm-audio/mxm-kit/blob/main/crates/mxm-modulation/AGENTS.md): its sources, its targets, each pair's full scale and
which routes the init patch holds (`plans/plan-mxm-poly-06-modulation.md` §2 and §3). **Nothing in
the voice asks whether a route is the machine's own**; `INIT_PRESENT` is the whole of that. The
routing travels **beside** `Patch`, never inside it: `Synth` keeps one `Routing`, and each voice a
`Graph` — its own frame and its compacted routes. The one global source, the LFO, is published into
every voice's frame, `mxm-creative-sampler`'s choice (governing plan §4.1).

| # | Source | Value |
|---|---|---|
| 0 | Key | The voice's **glided** note from middle C over `KEY_UNIT_SEMITONES`, 60 — five octaves either side at unit |
| 1 | Envelope | The voice's one ADSR, before the VCA's ENV/GATE choice, 0…1 |
| 2 | LFO | The global LFO with its delay fade, ±1 — the same value in every voice |
| 3 | Velocity | `v − 1` of the press that last triggered the envelope: zero at the hardest note |
| 4–6 | Wheel, Pressure, Bend | The channel the plugin reduces to, 0…1; the bend lever ±1 |
| 7–9 | Saw, Pulse, Sub | The DCO's components, before the mixer |
| 10 | Noise | The mixer's own noise sample: publishing it draws nothing more |

**The performance sources — Key, Velocity, Wheel, Pressure, Bend — are published through
`mxm_modulation::standard`**, so each is zero at its rest and means what it means on every
instrument (`plans/plan-modulation-standard.md`; this instrument was its pilot).

| Target | Law | Full scale at amount one — the machine's paths, then every added one |
|---|---|---|
| Pitch | Sum in semitones, onto glide, range, tune and expression | LFO 7 (`DCO_LFO_SEMITONES`); added: 12, and Key 12 per octave |
| Pulse width | Sum onto the width parameter; the DCO clamps what it reaches | 0.45 (`PWM_SWING`), which the standard also takes; Key 9 % per octave |
| Cutoff | Sum in octaves | Envelope 7 (`FILTER_ENV_OCTAVES`), LFO 3 (`FILTER_LFO_OCTAVES`), Key one octave per octave (`KEY_OCTAVES`), Bend 4 (`BEND_OCTAVES`); added: 4 |
| Amplitude | `amp × standard::amplitude_factor(Σ)` on the ENV/GATE result: `1 + clamp(Σ, ±1)` | 1, Key 20 % per octave: a route scales the envelope and never adds to it |

**A path the JUNO has keeps its slider's reach; every path it does not have takes the collection's
standard reach** (`standard::reach`). Until the standard, an added pitch route took the LFO slider's
seven semitones — so Pitch ← Key at full was 1.4 semitones per octave — and an added cutoff route the
envelope's seven octaves. No factory design used an added route, so no sound moved.

Each route is `(amount × source) × scale`, summed over live routes **in source order** — the order
the old cutoff expression added key, envelope and LFO in, so the machine's own routes associate as
that expression did. Every sum is bounded at 64, generously, because the filter clamps its cutoff and
the DCO its width; **Amplitude's is bounded at one by the standard's factor**, so several amplitude
routes together reach no further than one (`plans/plan-collection-sync.md` D8, closed 2026-09-26).
The factor equals the old `max(0, 1 + Σ)` wherever Σ ≤ 1, so a single route renders to the bit as it
did.

**The evaluation order is a contract**, and
`a_route_from_the_voices_audio_is_a_sample_late_into_pitch_and_on_time_into_cutoff` reads it off real
samples:

| Published | Read by Pitch and Pulse width | Read by Cutoff and Amplitude |
|---|---|---|
| Key, Envelope, LFO, Velocity, Wheel, Pressure, Bend — before the pitch sum | this sample | this sample |
| Saw, Pulse, Sub, Noise — after the DCO, before the cutoff sum | last sample | this sample |

**The init patch is the machine.** The six routes the JUNO wires — (Pitch ← LFO), (Pulse width ← LFO),
(Cutoff ← Envelope), (Cutoff ← LFO), (Cutoff ← Key) and (Cutoff ← Bend) — are present at zero depth.
`the_init_routes_at_zero_depth_render_bit_identically_to_nothing_routed` holds that they render what
nothing routed renders; `the_machines_own_routes_are_bit_identical_to_the_expressions_they_replace`
that at depth they compute the retired sliders' expressions to the bit, and
`key_tracking_and_bend_reach_the_old_numbers_to_rounding` that the two which re-associate stay within
1e-5 octaves. `each_route_the_machine_wires_reads_the_reach_its_slider_had` holds the scales.

**What the routing owes, each a test run against the defect it names:**

- **A source that becomes read starts from silence**: `Graph::set_topology` clears it in every voice's
  frame — `a_source_that_becomes_needed_starts_from_silence_not_from_an_old_phrase`.
- **A voice starting from idle reads nothing of its last note.** An idle voice returns before it
  publishes, so the fresh path resets its whole frame —
  `a_voice_starting_from_idle_reads_no_audio_from_its_last_note`.
- **Every voice is armed with the topology, idle ones included** — the sampler's lesson —
  `every_voice_is_armed_with_the_topology_idle_ones_included`.
- **Amplitude routes scale the envelope and never add to it**, so a released voice still ends exactly
  — `an_amplitude_route_scales_the_envelope_and_a_released_voice_still_ends_exactly`; and a negative
  envelope route inverts the sweep — `a_negative_envelope_route_inverts_the_sweep`.
- **No key down is exact silence whatever is routed**, every source into every target at full with the
  wheel, pressure and bender parked — `every_source_into_every_target_with_no_key_down_is_exact_silence`.
- **The voice's audio summed into the cutoff at the top of resonance stays bounded** at 8, 48 and
  192 kHz — `summed_audio_into_cutoff_stays_bounded_at_the_top_of_resonance` — and every pair at full
  stays finite at four rates — `no_nan_with_everything_at_its_limit`.
- **Every pair means what the standard says**, through `conformance.rs`'s `Declared` over the real
  tables and a real `Graph` (`mxm_modulation::conformance`): offered as `standard::offer` says,
  nothing at a source's rest, a meaningful move at full, the standard reach where the JUNO has no
  path — `every_pair_means_what_the_standard_says`; a voice publishes the standard's values —
  `a_voice_publishes_what_the_standard_says`; Key follows the portamento —
  `key_follows_the_portamento`; and after a release no performance route holds a note open, in
  either VCA mode — `after_a_release_no_performance_route_holds_a_note_open`. Each was falsified
  once: an added pitch reach of seven semitones, a raw velocity, the key published before the lag,
  a note never released.

## Dependencies: one at runtime

**One runtime dependency, [`mxm-modulation`](https://github.com/mxm-audio/mxm-kit/blob/main/crates/mxm-modulation/AGENTS.md)**, which has none of its own
and holds this same 1.87 floor — the routing conversion added it, and the MSRV override still rests on
the shipped graph staying that small. `cargo tree -p mxm-poly-06-dsp -e normal` shows that crate and
nothing else; `plugins/mxm-chorus-06`, which depends on this crate in place, inherits it.

`[dev-dependencies]` holds **`mxm-measure`**, the collection's measurement rulers — zero dependencies
at this same floor, reaching only tests and `examples/`, never a shipped `.clap`.

**The `conformance` feature** compiles `conformance.rs` outside this crate's own tests, for the
plugin's, which hold its route readings to `Declared::deliver`; it turns on `mxm-modulation`'s
feature of the same name. Only `[dev-dependencies]` enable either — this crate's own
`mxm-modulation` entry there, and the plugin's — and resolver 3 keeps a dev-dependency's features
out of a shipped build: `cargo tree -p mxm-poly-06 -e normal -f "{p} {f}"` shows `mxm-modulation`
with no features.
[`../mxm-measure/AGENTS.md`](https://github.com/mxm-audio/mxm-kit/blob/main/crates/mxm-measure/AGENTS.md)'s verification section checks that rather than
asserting it.

It also holds **`mxm-audio-file`**, which writes the listening demo, and **`mxm-audio-file-decode`**,
which its test reads the file back through — test-only edges on the same terms. The decoder's
MPL-2.0 symphonia therefore reaches this crate's tests and never its shipped graph.

**`juno_demo` no longer writes its WAV by hand**; it encodes through `mxm_audio_file` and applies
its own headroom at the call site.

## Everything else the collection's DSP already requires

No framework types; realtime rules on every per-sample path; denormals flushed in the DSP itself;
`f32` in the audio path and `f64` for prewarping and biquad coefficients; every saturator bounded
exactly and monotonic; a stated `pub const` output bound; deterministic seeded randomness — and here
that last one is load-bearing beyond testability: the export renders through a second instance, so
every seed and every per-voice constant is fixed. These are the parent's and the two mono crates',
not restated.

**`MIN_SAMPLE_RATE` (1 kHz) is the lowest rate the plugin activates at.** `f32::clamp` panics on a
NaN or crossed bound, and each voice's `20 Hz ..= 0.45 × rate` cutoff crosses below 44.4 Hz.

# Work Guidance

- Keep this cross-crate comparison until a shared-DSP proposal uses or rejects it; resemblance alone
  does not authorize extraction.

  | Candidate | Standing against `mxm-mono-01-dsp` |
  |---|---|
  | `flush`, `Rng` | **Identical**, byte for byte |
  | PolyBLEP residual, `clamp_pulse_width` | **Identical** |
  | `tan_approx`, `tanh_approx` | **Identical** |
  | `Adsr` | Identical **except the stall fix above**, which mono-01 should take |
  | The DC blocker | Identical in form; here on the voice sum rather than per voice |
  | The ladder core (TPT + Newton) | **The same equation with per-stage trims and input-side compensation added** — similar, not identical; the external circuit is exactly what the research says differs between machines |
  | `OnePole` | New here; mono-03's diode ladder has its own one-poles |

  Any extraction must be into a separately planned shared crate, not a move out of this one.
- Filter theory is [`docs/filters/`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/filters/README.md); the DCO is
  `research:oscillators/05-machines.md` §5.3; envelopes and
  the LFO are [`docs/modulation/`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/modulation/README.md). Read the relevant chapter before
  changing a module.
- Prefer a clear implementation to a clever one. This is reference-quality open source.

# Verification

**The rulers are shared, the thresholds are not.** `mxm-measure` is a `[dev-dependencies]` entry —
zero dependencies at this same 1.87 floor, and **not in the shipped graph**, which is what the
manifest's *no runtime dependencies* comment means. Measurements come from there; every bound and
its headroom stays in the test that argues for it.

**Tuning is asserted at a tenth of a cent, and the bound was re-derived rather than rescaled.** It
read one cent while being measured by a crossing *count*, which quantises to ±1 cycle — ±9 cents at
55 Hz over two seconds — so it could not have failed for any tuning error smaller than its own
ruler's. With `mxm-measure`'s interpolated ruler the worst case over the twenty rate/pitch
combinations is **0.009 cents**.

```bash
cargo test -p mxm-poly-06-dsp
cargo clippy -p mxm-poly-06-dsp --all-targets
cargo run -p mxm-poly-06-dsp --release --example juno_demo   # four passes over four chords
cargo +1.87.0 test -p mxm-poly-06-dsp                        # the MSRV this crate claims
cargo tree -p mxm-poly-06-dsp -e normal                      # mxm-modulation, and nothing else
```

The demo renders a 33-second stereo WAV.

Properties the tests must keep asserting, because each regresses silently:

- silence in gives **exactly** zero out after the tail, chorus and boost on — and two instances
  render bit-identically
- no NaN or inf across a sample-rate × cutoff × resonance sweep, per voice and with all six sounding
- output within the stated bound under overdrive past the oscillation threshold
- `reset()` leaves no tail — the chorus's delay lines and noise, every voice's envelope
- a panic is silence on the next sample and idle at once, and leaves no voice's ladder for the next
  note, across two histories
- the filter's threshold, **measured** at four sample rates, and that a voice card's spread moves the
  peak and not the threshold
- **the compensation keeps the body as resonance rises** — the JUNO's whole difference from the
  SH-101, as a number
- saw, pulse and sub **do not beat**; the sub toggles exactly at the ramp's reset
- the ledger, row by row: six then a seventh steals the oldest; the late id-less note-off; the
  repeated press; the id rules; overflow; UNISON then POLY; POLY 1 reuses and POLY 2 rotates; **POLY 1
  and POLY 2 render differently and the same mode renders identically**
- the chorus: off is dual mono; the wet is band-limited and the dry is not; **switching off and on
  does not restart the modulator**, proven against an always-on instance; the noise is present only
  with a mode on and a note sounding, and is two hisses, not one inverted; the modulator is a
  triangle with two corners per cycle
- the chorus's inputs: **at the circuit's values they render identically to the mode API**; a
  knob's rate lands exactly and a switch's at once; depth zero collapses the two clocks onto one
  delay; wet level zero is bit-exact dry; noise zero leaves no floor; a sweep of any input does not
  click; `advance` moves the modulator as processing would have
- the HPF: position 0 boosts, 1 is flat, only 2 and 3 cut, and the display curve agrees with the
  running filter

## An oracle that cannot fail is not an oracle

`poly_1_and_poly_2_are_audibly_different` asserts the two modes' renders differ **and** that the
same mode renders bit-identically twice, so the difference is not noise. Removing the per-voice
offsets *and* the capacitor spread would make it fail; either alone leaves the other to carry it.
`the_wet_path_is_band_limited_and_the_dry_is_not` measures the wet component's energy above 12 kHz
against white noise's own, and asserts the wet carries real signal, so an empty wet path cannot pass.

## What is not verified, and must not be claimed

**No hardware was measured, here or in any source this instrument rests on.** The table above is
every constant that was chosen. The tests prove the model is self-consistent; **they do not
establish that it sounds like the machine.** The gate that would is a **listening comparison against
reference recordings**, and it has **not been run**. Fidelity is UNVERIFIED.

**The DCO's residual amplitude variation and the BBD's distortion are not modelled**, because the
research has no shape for either and an unmarked guess is worse than a gap.

Linux and macOS are unverified — there is no CI (root *Windows, Linux and macOS*) — and the
development machine is Windows.

# Child DOX Index

No child AGENTS.md files. `src/` and `examples/` are covered by this doc.
