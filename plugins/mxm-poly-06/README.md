# mxm-poly-06

A six-voice polysynth with a built-in chorus. Architecture inspired by the Roland JUNO-106; the
interface is not, and the name is not. Not affiliated with or endorsed by Roland.

The collection's first polyphonic instrument, and the first to carry an effect.

## What it is

| | |
|---|---|
| Voices | **Six**, fixed. A seventh key steals one. Three assign modes: Poly 1 (first free), Poly 2 (rotate), Unison (all six on one key, in phase, no detune — loud, not wide) |
| Oscillator | A **DCO** per voice: rising saw, a pulse cut from the same ramp, a sub that is the ramp's reset clock divided by two, and noise. **It does not drift**, and saw + pulse + sub is one waveform — nothing beats |
| Filter | The IR3109 core `mxm-mono-01` ships, with the JUNO's external circuit around it: input-side compensation, so the filter keeps its body as the resonance rises and gets dirtier as it does. Six cards, six slightly different tolerances |
| HPF | **Global**, after the voices are summed, four stepped positions — and **position 0 is a bass boost**, not "off". Only the top two cut |
| Envelope | **One** per voice, shared by the filter and the amplifier. That is the machine's defining constraint, and there is no second one |
| LFO | **One**, global, triangle only, with a delayed fade-in. Every voice's vibrato is in phase |
| Chorus | The machine's own: two bucket brigades, one triangle in antiphase, a wet path rolled off above 10 kHz and a dry path that is not. **Off / I / II / I + II** — the rate is all a mode changes. Depth and mix are fixed, as on the machine |

## The controls

**LFO** rate, delay · **DCO** range, pulse width, pulse, saw, sub, noise · **HPF** · **VCF** cutoff,
resonance · **VCA** level, envelope/gate · **ENV** A D S R · **Chorus** · portamento, key assign,
volume — and **modulation routes** into pitch, pulse width, cutoff and amplitude.

Two things this copy does that the panel did not:

- **Its modulation is routing.** The panel's modulation sliders — the DCO's LFO depth, PWM in LFO
  mode, and the VCF's envelope, LFO and keyboard amounts — and the bender's depth into the filter
  are routes, present in a fresh instance at zero depth, each reaching what its slider did. The key,
  envelope, LFO, velocity, mod wheel, pressure, bender and the voice's own oscillators can each reach
  pitch, pulse width, cutoff or amplitude. The envelope's polarity switch is the sign of its route,
  and the PWM mode switch is whether that route is there.
- **The bender's range and the wheel's vibrato are disclosed** behind an expander in the Amplifier
  card; bend into the filter is a route.

**Level is before the chorus and Volume is after it**, as on the machine: Level sets how hard the
chorus is driven, so it is part of the sound and travels with a preset; Volume is yours. So Level
is on the Amplifier card and Volume is in the app bar, beside the output meter.

## What is deliberately not here

A second envelope, an arpeggiator, extra DCO ranges, LFO shapes, a legato switch, any
effect but the chorus, and every modelling knob. None was on the machine. See
`research:instruments/juno-106.md` §9. Velocity and pressure were not on it either; they are
routing sources here, and nothing routes them in a fresh instance.

**One deviation from the hardware, labelled:** a real 106 hisses through its chorus with no key
pressed. This one reaches exact digital silence at idle — the noise floor is present while anything
sounds and fades once nothing does — because a plugin that never falls silent never reports idle
and breaks every host's tail handling. See the plugin's `AGENTS.md`.

## Presets

Fifty factory sounds, compiled into the plugin — copy the `.clap` alone and the sounds travel with
it. **Init is not a file**: it is generated from the parameter defaults, so it cannot be deleted and
cannot drift from them. Your own presets are saved as readable JSON under the platform config
directory, browsed from the same bar, and can be starred to sort first.

## Status

**The editor is built, and the design system's QA gate has not been run on it.** Seven cards on pages derived from the window, the combined HPF-and-lowpass response, a
six-voice display showing which card each note landed on, and an app bar with the preset browser,
the master volume, the output meter and a scale control (75–200%).

**Fidelity is UNVERIFIED.** No hardware was measured, here or in any source this instrument rests
on. The tests prove the model is self-consistent — not that it sounds like the machine. The listening
comparison against reference recordings has not been run. The chorus's delay range, depth, noise
level and I + II rate, the HPF's boost, the voice-steal policy and the LFO-delay retrigger are all
**chosen, not measured**, and the DSP crate's `AGENTS.md` lists each.

## Building

```bash
cargo xtask bundle mxm-poly-06 --release
clap-validator validate "target/bundled/mxm-poly-06.clap"
```

MIT licensed — see [LICENSE](LICENSE). All code is original.
