# AGENTS.md — crates/mxm-poly-06-dsp

Parent: [`../../AGENTS.md`](../../AGENTS.md)

# Purpose

The complete mxm-poly-06 instrument as plain Rust: six voices — each a DCO, an IR3109 ladder with the
JUNO's compensation, and one ADSR shared by filter and amplifier — the ledger that hands keys to
voices, one global LFO, the four-position HPF, and the BBD chorus, summed and wired in the
schematic's order. Framework-free, so the whole thing is testable with `cargo test` and no host.

**The instrument is a six-voice polysynth with a built-in chorus**, architecture inspired by the
Roland JUNO-106. `research:instruments/juno-106.md` and `research:effects/juno-chorus.md` are the
research it rests on; `plans/plan-mxm-poly-06.md` is the completed implementation record. Rationale,
measurements and full test lists behind the rules below: [NOTES.md](NOTES.md).

# Ownership

Owns `src/` (`lib.rs`, `dco.rs`, `filter.rs`, `envelope.rs`, `lfo.rs`, `onepole.rs`, `hpf.rs`,
`chorus.rs`, `voice.rs`, `poly.rs`, `routing.rs`, `conformance.rs`), `examples/` (`juno_demo.rs`;
`common/wav.rs` retired to `mxm-measure`'s encoder), and `Cargo.toml`.

Does **not** own parameter definitions, ranges, smoothing or the editor — those belong to
[`plugins/mxm-poly-06/AGENTS.md`](../../plugins/mxm-poly-06/AGENTS.md). This crate takes plain
values and a sample rate.

# Local Contracts

## The press ledger owns voice allocation

Nothing else allocates voices. `poly.rs` keeps one entry per press (key, host note id, **set of
voices**); a press whose voices were all stolen stays as a **tombstone**. The module doc carries the
whole table; the regression-prone rules ([NOTES.md § The press ledger](NOTES.md#the-press-ledger-owns-voice-allocation)):

- **A steal shrinks a press's voice set by one; only an emptied set becomes a tombstone**
  (`leaving_unison_and_stealing_one_voice_leaves_five_to_release_with_the_held_key`).
- **An id-less note-off retires the oldest press of that key, tombstones first**
  (`a_late_id_less_note_off_for_a_stolen_key_does_not_release_its_re_press`).
- **An id is authoritative when both sides have one; otherwise the key decides** (`NoteId::matches`).
- **A repeated press of a held key joins the voice and retriggers nothing** (the collection's legato
  joint, which the player's export relies on).
- **The ledger is bounded and its overflow is a policy**: evict the oldest tombstone, else the oldest
  press. A release one press early, never a stuck note, never an allocation.
- **The assign mode governs future allocation only.** Nothing is re-voiced or retired when it moves.
- **A non-finite expression is dropped**, and every press keeps the offset it had
  (`a_non_finite_expression_is_dropped_and_the_pitch_stays_finite`).

## Settle, idle and panic

- `Synth::is_active` is *any voice active, or `POST_TAIL_S` not yet elapsed since the last went
  idle*; nothing detects exact zeros along the post-mix chain (`the_tail_covers_the_actual_decay`;
  [NOTES.md § The post-mix chain settles](NOTES.md#the-post-mix-chain-settles-for-a-fixed-time-and-nothing-tries-to-detect-exact-zeros-along-it)).
- **Idle is defined**: when the settle elapses every hidden GATE-mode envelope is retired to zero
  (`when_the_instrument_goes_idle_every_hidden_envelope_is_at_zero`); until then it keeps time.
- **A panic is idle at once**: `Synth::all_sound_off` (CC 120) empties the ledger, silences every
  voice and resets its ladder, the DC blocker, the HPF and the chorus's audio path, and ends the
  settle; it keeps oscillator, lag, trim, LFO and chorus-modulator state. **A choke is not a panic**
  (`Voice::silence` leaves the ladder). `panic_clears_every_voices_filter_and_a_late_release_of_an_old_press_moves_nothing`.

## Exact-silence and `f32` rules

- **Two states of one recursion go to zero together** (`chorus.rs`, `Biquad::process`), or a biquad
  limit-cycles at the flush threshold ([NOTES.md § Two reusable exact-silence rules](NOTES.md#two-reusable-exact-silence-rules)).
- **The envelope switches stage when the step stopped moving the level**, or decay stalls one ulp
  above sustain. `crates/mxm-mono-01-dsp/src/envelope.rs` has the same stall.
- **The portamento lag is carried as its remaining distance** (`glide_offset`), never as the value
  form (`a_glide_lands_exactly_on_its_note`).

## What is chosen, not measured

Every machine number the research lacks, and every implementation constant, is tabled with its value
and source in [NOTES.md § What is chosen, not measured](NOTES.md#what-is-chosen-not-measured): keep it
complete. Constants copied from `mxm-mono-01-dsp` unchanged are argued there. Everything else numeric
in `src/` is a derived quantity or a test tolerance. None is a parameter.

## The chorus serves two products

- Mono in, stereo out; it knows nothing about voices. `plugins/mxm-chorus-06` **depends on this
  crate in place** and calls this module: no move or copy
  ([NOTES.md § The chorus is a module](NOTES.md#the-chorus-is-a-module-with-a-plain-values-api-and-it-serves-two-products)).
- **Depth, mix and noise are fixed in this instrument and are inputs in the module** (the owner,
  2026-09-03). `Chorus::set_mode` is the synth's whole interface and touches only the rate, without a
  glide (`the_inputs_at_the_circuits_values_render_identically_to_the_mode_api`, and the chorus-on
  golden scores). Gliding inputs keep the **remaining distance**. Never add a depth or mix knob here.

## Exact silence at idle is a labelled deviation

A real 106 hisses through its chorus with no key pressed. This instrument's noise floor is present
while anything sounds and fades to exact zero once nothing does — `Chorus::set_active`. The plugin's
AGENTS.md owns the ruling; this crate implements it, and `silence_in_gives_exactly_zero_out_after_the_tail`
holds it.

## Modulation is routing: eleven sources, four targets, a frame in every voice

`routing.rs` declares this instrument on the shared `mxm-modulation`. **Nothing in the voice asks
whether a route is the machine's own**; `INIT_PRESENT` is the whole of that. Routing travels
**beside** `Patch`: `Synth` keeps one `Routing`, each voice a `Graph`; the LFO is published into
every voice's frame. Sources and detail: [NOTES.md § Modulation is routing](NOTES.md#modulation-is-routing-eleven-sources-four-targets-a-frame-in-every-voice).

| Target | Law | Full scale at amount one — the machine's paths, then every added one |
|---|---|---|
| Pitch | Sum in semitones, onto glide, range, tune and expression | LFO 7 (`DCO_LFO_SEMITONES`); added: 12, and Key 12 per octave |
| Pulse width | Sum onto the width parameter; the DCO clamps what it reaches | 0.45 (`PWM_SWING`), which the standard also takes; Key 9 % per octave |
| Cutoff | Sum in octaves | Envelope 7 (`FILTER_ENV_OCTAVES`), LFO 3 (`FILTER_LFO_OCTAVES`), Key one octave per octave (`KEY_OCTAVES`), Bend 4 (`BEND_OCTAVES`); added: 4 |
| Amplitude | `amp × standard::amplitude_factor(Σ)` on the ENV/GATE result: `1 + clamp(Σ, ±1)` | 1, Key 20 % per octave: a route scales the envelope and never adds to it |

- Performance sources go through `mxm_modulation::standard`. **A path the JUNO has keeps its
  slider's reach; every other takes the standard reach** (`standard::reach`).
- Each route is `(amount × source) × scale`, summed **in source order**; every sum is bounded at 64,
  **Amplitude's at one** by the standard's factor.
- **The init patch is the machine**: (Pitch ← LFO), (Pulse width ← LFO), (Cutoff ← Envelope),
  (Cutoff ← LFO), (Cutoff ← Key) and (Cutoff ← Bend) present at zero depth, bit-identical to the
  retired sliders (`the_machines_own_routes_are_bit_identical_to_the_expressions_they_replace`).

**The evaluation order is a contract**, and
`a_route_from_the_voices_audio_is_a_sample_late_into_pitch_and_on_time_into_cutoff` reads it off real
samples:

| Published | Read by Pitch and Pulse width | Read by Cutoff and Amplitude |
|---|---|---|
| Key, Envelope, LFO, Velocity, Wheel, Pressure, Bend — before the pitch sum | this sample | this sample |
| Saw, Pulse, Sub, Noise — after the DCO, before the cutoff sum | last sample | this sample |

**What the routing owes**, each tested against its defect (names in NOTES.md): a source that becomes
read starts from silence (`Graph::set_topology` clears it); a voice from idle resets its whole frame;
every voice is armed with the topology, idle ones included; Amplitude routes scale and never add; no
key down is exact silence; audio into cutoff stays bounded; every pair means what the standard says.

## Dependencies: one at runtime

- **One runtime dependency, `mxm-modulation`**, dependency-free at the same 1.87 floor; the MSRV
  rests on the shipped graph staying that small ([NOTES.md § Dependencies](NOTES.md#dependencies-one-at-runtime)).
- `[dev-dependencies]` only: `mxm-measure`, `mxm-audio-file`, `mxm-audio-file-decode` (MPL-2.0
  symphonia reaches tests only). **The `conformance` feature** compiles `conformance.rs` for the
  plugin's tests; only dev-dependencies enable it, so a shipped build has `mxm-modulation` featureless.

## Everything else the collection's DSP already requires

No framework types; realtime rules on every per-sample path; denormals flushed in the DSP itself;
`f32` audio, `f64` prewarping and biquad coefficients; every saturator bounded exactly and monotonic;
a stated `pub const` output bound; **deterministic seeded randomness, load-bearing here**: the export
renders through a second instance, so every seed and per-voice constant is fixed.
**`MIN_SAMPLE_RATE` (1 kHz) is the lowest rate the plugin activates at** ([NOTES.md § Everything else](NOTES.md#everything-else-the-collections-dsp-already-requires)).

# Work Guidance

- Keep the cross-crate comparison with `mxm-mono-01-dsp` ([NOTES.md](NOTES.md#the-cross-crate-comparison))
  until a shared-DSP proposal uses or rejects it; resemblance alone does not authorize extraction.
  Any extraction must be into a separately planned shared crate, not a move out of this one.
- Filter theory is [`docs/filters/`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/filters/README.md); the DCO is
  `research:oscillators/05-machines.md` §5.3; envelopes and
  the LFO are [`docs/modulation/`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/modulation/README.md). Read the relevant chapter before
  changing a module.
- Prefer a clear implementation to a clever one. This is reference-quality open source.
- Every bound and its headroom stays in the test that argues for it; measurements come from
  `mxm-measure` ([NOTES.md § Measurement](NOTES.md#measurement)).

# Verification

```bash
cargo test -p mxm-poly-06-dsp
cargo clippy -p mxm-poly-06-dsp --all-targets
cargo run -p mxm-poly-06-dsp --release --example juno_demo   # four passes over four chords
cargo +1.87.0 test -p mxm-poly-06-dsp                        # the MSRV this crate claims
cargo tree -p mxm-poly-06-dsp -e normal                      # mxm-modulation, and nothing else
cargo tree -p mxm-poly-06 -e normal -f "{p} {f}"             # mxm-modulation with no features
```

The demo renders a 33-second stereo WAV. The properties the tests must keep asserting, because each
regresses silently, are listed in [NOTES.md § Properties](NOTES.md#properties-the-tests-must-keep-asserting-in-full):
exact silence and bit-identical instances, no NaN, the output bound, reset and panic, the filter
threshold and compensation, the DCO, the ledger row by row, the chorus and its inputs, the HPF.

- **An oracle must be able to fail**: a "differs" assertion also proves the same input renders
  identically ([NOTES.md § An oracle](NOTES.md#an-oracle-that-cannot-fail-is-not-an-oracle)).
- **Fidelity is UNVERIFIED and must not be claimed**: no hardware was measured and no listening
  comparison has run ([NOTES.md § What is not verified](NOTES.md#what-is-not-verified-and-must-not-be-claimed)).

# Child DOX Index

No child AGENTS.md files. `src/` and `examples/` are covered by this doc.
