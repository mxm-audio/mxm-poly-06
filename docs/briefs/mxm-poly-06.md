# mxm-poly-06 — UI design brief

Required by `MXM_DESIGN_SYSTEM.md` §14, written before implementation. Answers the ten questions in
order, then records the deliberate deviations and the decisions the plan (`plans/plan-mxm-poly-06.md`
§9) hands to this document.

**Instrument:** six-voice polysynth with a built-in chorus. Architecture inspired by the Roland
JUNO-106; the interface is not.

---

## 1. Primary sound-design task

**Voicing a chord and deciding how wide it is.** This machine's whole character is six oscillators
that do not drift behind a chorus that decorrelates them — so the task is not performing a filter
(mono-03) or tweaking while playing (mono-01), but setting an envelope shape, a filter position and a
chorus button, and then playing chords into it. Most of the work is done once per patch; the chorus
button is the one thing changed mid-performance.

## 2. The three to five parameters users reach for most

1. **Cutoff** — where the chord sits, tonally.
2. **Resonance** — its partner.
3. **Chorus mode** — off, I, II or both. The width, and the one control performed live.
4. **Attack** and **Release** — a polysynth's envelope is mostly its onset and its tail.
5. **HPF position** — position 0 is the bass boost that makes a 106 sound bigger than a 60.

Cutoff and Resonance take **Primary** sizing. Everything else is Standard, the bender's two
depths included, so each shows its value.

## 3. Signal flow that must be visible

```
DCO ─► VCF ─► VCA ─┐  x6
       ▲     ▲     │
 one ENV ────┘     │ sum ─► HPF ─► Level ─► Chorus ─► Volume ─► L / R
 routes ─► pitch · width · cutoff · amplitude   (the LFO, ENV, key and bender at Init)
```

Three things must read without a manual, and each is something people get wrong about this machine:

- **One envelope per voice, and it drives both the filter and the amplifier.** Anyone expecting a
  filter envelope and an amplifier envelope must see there is one card called Envelope.
- **The HPF is global and after the sum, and its bottom position boosts.** The filter card draws the
  HPF and the lowpass as one response, so the boost is seen lifting the low end under the corner.
- **The chorus is at the end of the chain and is the machine's own.** One control, beneath the
  amplifier's level that drives it, on *Amplifier and chorus* (R2, `plans/plan-editor-standard.md`:
  hugged, a card of its own was one switch alone).

**Since the routing conversion the modulation is routes**, and the six the machine wires are present
at Init. Each target's stack sits under the controls it moves — Pitch and Pulse width on the
Oscillator card, Cutoff on the Filter card, Amplitude on the Amplifier card — so the flow still reads
left to right and the envelope's sweep is still visibly the filter's.

## 4. Which controls belong in Play view

**Not applicable — no `Play` view.** See §6.

## 5. Advanced controls and their disclosure

**Two, on the Voice card since 2026-09-28** (the owner: *no reason to put them behind a bender
knob*), where they were disclosed behind a labelled expander in the Amplifier card's footer: the
bender's range into the DCO, and how much vibrato the mod wheel adds. The bender's depth into the VCF was a third
until the routing conversion made it the (Cutoff ← Bend) route, which the Filter card's stack shows. `mxm-mono-01`'s idiom for its bend
range, and §3.3's card footer is where the design system puts advanced disclosure.

**Not a second zone**, unlike `mxm-mono-03`. That editor shows eight fixed constants below a
divider because hiding a third of a small instrument costs more than it saves; here the disclosed
set is two depths from the left cheek of the panel, on an instrument with twenty-three parameters of
its own and eighty-eight routing ones.
The expander is collapsed by default, and **every disclosed control defaults to a value that
changes nothing** about the panel's sound: bend range 2 semitones, and the wheel's reach at a useful
depth with the wheel itself at rest.

**What the split means**: on the panel is what the machine's panel had; in the footer is what its
left cheek had. Provenance, not frequency of use.

## 6. Views

**Space-derived pages**, following design-system §3.2. Voice is Performance;
LFO/Envelope are Modulators; Oscillator is Generators; Filter and *Amplifier and chorus* —
the chorus on the amplifier's card since R2 (`plans/plan-editor-standard.md`) — are Tone and a
preferred group. The bender's two depths are on the Voice card (2026-09-28).
The master Volume is on no card: it is the app bar's output control, an inline slider beside the
level meter (design system §3.1), and the keyboard cursor reaches it there. The Amplifier card
keeps the patch's Level, which is before the chorus and part of the sound.
Full names, no fixed page count, no bar for one page. Parameters stays separately reachable at
developer CC 119 value 127 without a tab. Controller roles/pages remain unchanged.

## 7. Identity accent

**Rose.** Dark `#FF7EB3`, light `#A8135E`.

Measured with `mxm_ui::theme::contrast` against the surfaces it is drawn on, never judged by eye:

| | vs `surface-1` | vs `surface-2` |
|---|---:|---:|
| Dark `#FF7EB3` | **7.38 : 1** | **6.75 : 1** |
| Light `#A8135E` | **7.19 : 1** | **6.03 : 1** |

Both themes clear 4.5 : 1 for text and 3 : 1 for control boundaries.

**The alternatives, all of which passed the gate**, so the choice was made on hue separation and
not on contrast:

| Candidate | Dark s1 / s2 | Light s1 / s2 | Why not |
|---|---:|---:|---|
| Sky `#4FC3F7` / `#01579B` | 8.70 / 7.96 | 7.40 / 6.20 | ~15° from `mod-lfo` blue; an LFO-coloured knob arc on a knob that also draws LFO modulation arcs is the one collision that confuses |
| Ice `#7FD8FF` / `#0A5F86` | 10.93 / 10.00 | 7.01 / 5.88 | Same neighbourhood as Sky |
| Mint `#4FE3C1` / `#0B6E56` | 10.86 / 9.94 | 6.21 / 5.21 | ~15° from `mod-performance` |
| Peach `#FFB37A` / `#9A4A00` | 9.94 / 9.09 | 6.26 / 5.25 | Between `danger` and `warning`, and beside coral, which is mono-01's candidate |
| **Rose** | 7.38 / 6.75 | 7.19 / 6.03 | ~20° from `danger` — the same distance orchid and coral sit from their neighbours, which mono-03's brief accepted as the cost of a crowded wheel — and further from every modulation hue than the others |

**What was already taken:** lime is `mxm-mono-03`'s; orchid and coral are on `mxm-mono-01`'s candidate
list and this brief leaves them there; blue, red, amber, green, violet and teal are the modulation and
status colours. **This brief does not edit mono-01's.**

**§5.3's trade-dress rule is satisfied**: the hardware's arrangement is black with coloured slider
caps and an orange wordmark, and this is none of those.

**The plan records this as the owner's choice.** The candidates were measured and rose was taken so
the editor could be built; the owner may pick another passing candidate, and the change is one
constant in mxm-kit's `crates/ui/src/theme.rs` and this table.

## 8. Live visualizations

Three, and the test each had to pass is whether it answers a question the controls cannot.

1. **The filter section's response, as one curve** — the global HPF and the per-voice lowpass
   together, with the envelope's reach as a secondary trace. It shows the bass boost lifting the low
   end under the corner, which is the fact about this machine most often got wrong.
2. **The six voices.** Which card each note landed on and how loud it is. POLY 1 reuses low-numbered
   cards and POLY 2 rotates, and because the cards differ slightly the two modes sound different —
   and nothing on the panel shows *which* card a note took. A seventh key stealing a voice reads as
   the machine's behaviour rather than a dropout, because the cell it took is visibly the one that
   changed. Six cells, a bar for the envelope level, the key as text while it sounds.
3. **Output level with clip indication** in the app bar beside the master Volume, per §3.1.

Deliberately **not** included: an envelope display (four knobs describe one ADSR fully), an LFO
display (one triangle), and a chorus modulator display (a triangle at 0.5 Hz is not information).

### Ownership

A single `Telemetry` struct, `Arc`-shared, **atomics only**, written once per block — the pattern the
mono instruments set, including a **peak that is max-combined and reset on read** and a **clip that
latches** until acknowledged.

| Visualization | Writer | Truth model |
|---|---|---|
| Filter response | UI thread, from parameters | **Declared approximation** — the linear analytic response, ignoring the compensation's saturation and the per-voice spread |
| Voices | Audio thread, once per block | **Exact** — each voice's envelope level and key |
| Output level + clip | Audio thread, once per block | **Exact** |

## 9. What is removed from the source hardware layout, and why

**Kept:** the control set, and the signal flow it implies.
**Removed:** the panel layout, appearance, geometry, control style, colour arrangement, typography,
trade dress.

The hardware's panel was consulted, through `research:instruments/juno-106.md` §3, for the **control set
and its grouping**: LFO · DCO · HPF · VCF · VCA · ENV · CHORUS left to right, with portamento, key
assign and volume on the left cheek and the bender's depths beside them. Nothing about how it looks
was taken, and no image of it is kept in this repository.

| Removed | Why |
|---|---|
| The panel layout and its sliders | §2 forbids copying the inspiring instrument's panel. Controls are grouped by task; the grouping survives because it is the signal flow |
| The patch memory, bank and manual buttons, tape and MIDI | The preset system is the collection's, in the app bar |
| The keyboard, key transpose and the bender lever | §2 forbids a decorative keyboard; note input and transposition are the host's. The lever's range survives as a disclosed parameter, and its depth into the VCF as a route |
| Black-and-coloured trade dress, the wordmark | §2 and §5.3 |

**Two interface improvements, each a limit of the panel and not of the circuit** (the plan's §6):

- **The PWM slider's two meanings and the envelope's polarity switch are routes.** A width is
  `pulsewidth`; its LFO depth is (Pulse width ← LFO), whose presence is what the mode switch was; the
  polarity is the sign of (Cutoff ← Envelope). One slider meant two things on the machine, and a
  switch beside a signed amount would be two authorities over one sign.
- **The bender's depths are on the Voice card**, not hidden on a side cheek or behind a disclosure.

## 10. Minimum size and 200% scale

**Resizable: the editor's `REFERENCE` and `MINIMUM`, derived and held by its tests**, with every
route revealed. Category/card order is §6's.
`every_dynamic_page_fits_and_every_card_is_reachable` checks all pages in both themes, at opening size, the quarter-4K content size and the minimum. Component floor/row tests
remain separate from physical fit.

Zoom is independently chosen at **75–200%**; only indivisible overflow scrolls. Keep the physical
window fixed for §15's DPI/zoom gate. Native-window, real-DAW and owner inspection remain open.

---

## 10a. Coherence with the mono editors

| Taken | Why |
|---|---|
| **`SECTIONS` as a `const` array, with the order stated as the contract** | It is the information architecture |
| **`mxm_ui::ModuleCard` per section**, names from the shared vocabulary | §3.3's grouping, already themed |
| **`knob_row`: columns of their own width**, capped at what the knobs need | `mxm-mono-03`'s answer to two knobs 190 points apart |
| **One place brackets gestures** — `binding::Bound::apply` | Load-bearing for the player's step editing. The third verbatim copy of `binding.rs` |
| **A `Parameters` view, and Init in the utility menu** | Same pair, same placement |
| **`Telemetry` as the only DSP → editor channel** | Same rules |
| **The zoom control, 75–200%** | One size that is always right, absorbed by zoom rather than reflow |
| **The disclosed expander** | `mxm-mono-01`'s idiom, chosen over mono-03's zone for the reason §5 gives |

**Where it deliberately differs**: the amplifier and the chorus it drives share a card; and a voice display, because six voices is the thing
the mono instruments did not have to show.

---

## 11. Decisions the plan handed to this brief

The plan's §9 named three owner decisions and four implementer's. Where they stand:

| Decision | Standing |
|---|---|
| **Exact silence at idle over the hardware's chorus hiss** | **Built under the plan's working assumption**: the noise floor is present while anything sounds and fades to exact zero once nothing does. Labelled in `plugins/mxm-poly-06/AGENTS.md`. The owner may reverse it; reversing changes the tail, the export and the golden score |
| **The identity accent** | **Rose, measured above**, taken so the editor could be built. The owner's to change |
| **The two prerequisite plans** (tolerant control-map loader; shared preset crate) | **Not run first.** `preset.rs` and `binding.rs` are third copies; the shipped map omits the three roles the standard did not have |
| Voice stealing and UNISON's priority | **POLY 1 steals the oldest press; POLY 2 takes the next in rotation, free or not; UNISON is last-trigger with no fallback: a second key takes all six and releasing it does not return to the first.** Chosen, UNVERIFIED on a 106 |
| What retriggers the LFO delay | **The first key after every key was released.** Chosen, UNVERIFIED |
| Which side the compensation sits on; the DCO's finite reset | **Input side, from the IR3109 research's Juno-6/60 configuration; the reset is not modelled** — a `sinc(5.3 µs · f)` rolloff of −0.16 dB at 20 kHz, derived, two orders under the aliasing floor |
| The chorus's delay range, depth and noise level | **2.8 ± 1.2 ms, −75 dBFS**, chosen; the I+II rate 1.3 Hz, chosen. The fidelity gate is where they are heard |

---

## 12. The recognisability trial

§9 makes a recognisable *control set* the requirement — not a recognisable panel, which §2 forbids.

**The trial patch:** saw and sub, attack 40%, release 60%, cutoff 60%, HPF at position 0, chorus I,
a four-note chord held and a fifth note added.

Run with **someone who has used a 106 or a clone**, without showing them the hardware.

### Stage 1 — before composition, on a wireframe

1. *"How many envelopes does this have?"* → **one**, shared. Two is a fail of the drawing.
2. *"What does the lowest HPF position do?"* → it **boosts**. "Turns the filter off" is a fail.
3. *"Where is the chorus?"* → at the end, one control. "Which knob is the chorus depth?" is a fail
   of the drawing only if the person believes there should be one.

### Stage 2 — on the finished editor

| Task | Control |
|---|---|
| *Make it wider.* | `chorus` |
| *Make it darker.* | `cutoff` |
| *Give the chord more bass.* | `hpf` to Boost |
| *Make the notes swell in.* | `attack` |
| *Make it a bit quieter overall.* | `volume` |
| *Stack all six voices on one note.* | `keyassign` to Unison |

**Per task:** found within ten seconds, entering at most one wrong card. **Gate: five of six.**

*Results: not yet run.* An unrun trial is recorded as unmet, never as passed.

---

## Deliberate deviations from the design system

### ~~A one-line caption under most cards~~ — withdrawn (§7.6)

Five cards carried a quiet line of explanation until the owner ruled out help text on the panel
(2026-09-27). Each fact is now its control's tooltip, written for the player: that saw, pulse and
sub are one waveform (Sub), that one envelope shapes the filter and the volume (Attack), what the two
levels do (Level), that the LFO is one triangle moving every voice (Rate), and how the assign modes
differ (Key assign).

### No undo/redo (§3.1)

The only undoable events are parameter edits, which hosts already track.

---

## The fidelity gate

**UNVERIFIED.** No hardware was measured for the research this instrument rests on, and no
listening comparison against reference recordings has been run. The DSP's tests prove the model is
self-consistent — six pitches in a chord, a seventh stealing, the chorus band-limited and in
antiphase, the boost boosting — not that it sounds like the machine. The three things the research
names as worth measuring first if a unit appears — the envelope curves, the BBD's noise and
bandwidth, the HPF's four responses — are also the three most likely to move on a listening
comparison.

---

## Sign-off checklist

- [x] Signal flow readable without documentation: **one envelope**, the HPF **after the sum and boosting at 0**, the chorus **last**
- [x] Cutoff and Resonance at Primary sizing, adjacent
- [x] The disclosed expander collapsed by default, and every disclosed control changing nothing at Init
- [x] Identity accent applied, with the measured ratios above holding against `surface-2` as well
- [x] Every parameter present — 20 on the cards, Volume in the app bar, 2 disclosed, 88 routing parameters in four stacks — `every_parameter_is_drawn_exactly_once`, `the_master_volume_is_drawn_once_in_the_app_bar` and `the_keyboard_cursor_reaches_and_operates_every_route_revealed`
- [x] Card names match the collection's vocabulary; only *Amplifier and chorus* is new
- [x] No row of knobs spreads to fill its card
- [x] Both views reachable, `Synth` active on open
- [x] Height measured and pinned by a test rather than assumed
- [ ] Verified by eye at 75%, 100%, 150% and 200% — **not yet done**
- [ ] Dark and light both complete, with all control states — **light is the editor's default; dark not yet checked by eye**
- [ ] §12's trial run and recorded — **unmet**
- [ ] §15 QA gate passed in full — **unmet**
