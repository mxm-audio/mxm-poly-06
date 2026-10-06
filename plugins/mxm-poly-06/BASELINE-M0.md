# mxm-poly-06 — pre-conversion reference, captured at M0

`plans/plan-mxm-poly-06-modulation.md` M0. **These figures stop existing once the routing
conversion starts**, which is why they are captured first and committed rather than re-derived.

Produced by `plugins/mxm-poly-06/src/lib.rs`'s `baseline` module, through `render_block_for_test` —
the plugin's own per-sample path: the switches once per block, `next_patch()` per sample (every
smoother and the `Patch` rebuild), then `Synth::process`. It omits the wrapper's event handling,
telemetry and buffer plumbing, which is not where the routing work lands.

```bash
cargo test -p mxm-poly-06 --release baseline -- --ignored --nocapture
```

## Throughput — not recorded at M0, deliberately

The capture ran while `mxm-mono-02`'s conversion was building on the same machine, and **a timing
taken on a machine that is also building is not a measurement** (mxm-kit's
[`docs/code-review-notes.md`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/code-review-notes.md) §3).
So the pre-conversion figure is taken **from this commit, on a quiet machine, beside the
post-conversion figure** — the same conditions for both, which is what a before/after gate needs.

`baseline::throughput` measures three cases at 48 kHz in 64-sample blocks:

| Case | Why |
|---|---|
| Init, idle | The free case |
| Init, six-note chord held (notes 60–65) | **The case routing multiplies by six**, and the one the cost gate turns on |
| The routed patch, the same chord | `dcolfo` 0.3, `pwmmode` LFO, `pwmdepth` 0.5, `envamount` 0.6, `vcflfo` 0.4, `keytrack` 0.5 — normalised values. After the conversion the routes that replace them are set to the same depths |

## Factory bank — reference digests

FNV-1a over the raw sample bits, left and right interleaved — the digest
`apps/mxm-player/tests/t4_golden_audio_poly_06.rs` already uses. A C-major triad (48, 52, 55) held
1.5 s then released with 2.5 s of tail at 48 kHz, identical for every sound, so a digest change is
attributable to the patch and not to the playing.

*Since 2026-10-05:* that player test is this repository's
[`host-tests/tests/golden_audio.rs`](host-tests/tests/golden_audio.rs), with the same FNV-1a digest.

**Verified reproducible**: two consecutive runs produced identical digests for all 51 renders. Every
preset applied **all 31 parameters** the instrument has at M0.

| Sound | Digest | Peak |
|---|---|---|
| *(Init)* | `033244de2ccb807d` | 0.1832 |
| `bell-keys` | `d6822292cb73f13d` | 0.2209 |
| `brass` | `387f8d2317d55d34` | 0.4067 |
| `bright-lead` | `1909e9469ca3e33d` | 0.1887 |
| `cello` | `561b390d7a850083` | 0.4541 |
| `choir` | `9a2efdd39a47ea25` | 0.1932 |
| `chorus-pad` | `9ff62b82b29b54b9` | 0.4003 |
| `cinematic-swell` | `9507e893197e9dfb` | 0.3984 |
| `clav` | `3d10b677300f8e35` | 0.1740 |
| `dark-pad` | `8290a5ca8aba3fb2` | 0.1955 |
| `drone-chord` | `9660c04bd9e9585e` | 0.1799 |
| `electric-piano` | `addaf1ec39cafff7` | 0.3652 |
| `evolving-pad` | `0a1850c516f220b0` | 0.2313 |
| `fanfare` | `9170167b209811fe` | 0.1848 |
| `gate-pad` | `2ebe10cfc79b5a6b` | 0.2122 |
| `glass-pad` | `95c378224f88abd3` | 0.3982 |
| `harp` | `87959a8119c4110b` | 0.4450 |
| `harpsichord` | `26e7c50a0b097a81` | 0.2749 |
| `high-pad` | `ea0cf2d8407f8cdd` | 0.2559 |
| `hollow-pad` | `5d5cdfccec072ed1` | 0.3351 |
| `juno-piano` | `2d9e5f0fa58c4e6f` | 0.2064 |
| `music-box` | `ace5b7f98ba20161` | 0.3813 |
| `noise-hat` | `d6ead752568713a5` | 0.0632 |
| `noise-wash` | `aeb27df2b9824e1d` | 0.1935 |
| `organ` | `a48f2ee484101e7b` | 0.5959 |
| `pad-strings` | `e8ec20e6a854ed1c` | 0.1915 |
| `pluck` | `60ef042ee4bfbd2f` | 0.1579 |
| `poly-lead` | `fc80ed3daf95e157` | 0.1764 |
| `pulse-bass` | `c60e99fcbff724ed` | 0.1732 |
| `rain-drops` | `0de4cef314ee8d51` | 0.1003 |
| `rubber-bass` | `9bd41c7e181959a9` | 0.4605 |
| `slow-sweep` | `43297be0de2335e2` | 0.2327 |
| `snare` | `16da9fb4bb22ed45` | 0.1443 |
| `soft-horn` | `d00bdb0100c90ce1` | 0.2606 |
| `soft-keys` | `ce302db772bb54fe` | 0.1427 |
| `soft-lead` | `0ccbea1478efde25` | 0.4159 |
| `solo-violin` | `1098cf656742aad0` | 0.6920 |
| `square-bass` | `acb72b156660b9b9` | 0.6874 |
| `stacked-lead` | `2d3f995cf8beeaf9` | 0.4881 |
| `strings` | `202eb47f251b369e` | 0.2255 |
| `sub-bass` | `962a024a9aae5e15` | 0.5542 |
| `sub-kick` | `a914dc9af5a0fa35` | 0.4664 |
| `sweep-bass` | `6dbc9e0319d1f645` | 0.5234 |
| `synth-brass` | `9f0d2de8f0bb19dd` | 0.1473 |
| `thin-pulse` | `7879a18c6df6da19` | 0.1469 |
| `tom` | `92fd98337953a589` | 0.1780 |
| `unison-bass` | `688e45f758a70c05` | 0.4643 |
| `vibrato-lead` | `e12265111aa1c289` | 0.1719 |
| `warm-pad` | `1af254f64a742c3e` | 0.1805 |
| `wide-chords` | `2869eeec424d8b00` | 0.3660 |
| `wobble` | `7021dfccfd5d25b9` | 0.2182 |

Every peak is non-zero and below unity.

## After the conversion — what moved, and why

The same `baseline` module on the converted tree, with `FACTORY_DESIGN` translated by
`plans/plan-mxm-poly-06-modulation.md` §4.1 and every preset applying all **111** parameters.
**51 renders: 22 bit-identical, 29 moved, and no peak changed at four decimals.**

**Bit-identical:** *(Init)*, `bell-keys`, `chorus-pad`, `cinematic-swell`, `drone-chord`, `fanfare`, `gate-pad`, `high-pad`, `hollow-pad`, `music-box`, `noise-hat`, `noise-wash`, `organ`, `pluck`, `rubber-bass`, `slow-sweep`, `strings`, `sub-kick`, `sweep-bass`, `thin-pulse`, `tom`, `wobble`.

| Moved | M0 | Converted |
|---|---|---|
| `brass` | `387f8d2317d55d34` | `bb0b965348a71ed2` |
| `bright-lead` | `1909e9469ca3e33d` | `98612c58ad74cbf1` |
| `cello` | `561b390d7a850083` | `f237f8706bd5a6bb` |
| `choir` | `9a2efdd39a47ea25` | `3d12e98c17823095` |
| `clav` | `3d10b677300f8e35` | `ae9bacf79407ea01` |
| `dark-pad` | `8290a5ca8aba3fb2` | `c95f65a5e6ca9f17` |
| `electric-piano` | `addaf1ec39cafff7` | `3d53b275323b20d3` |
| `evolving-pad` | `0a1850c516f220b0` | `18975135ca1a63ab` |
| `glass-pad` | `95c378224f88abd3` | `3c470c220ddb2b69` |
| `harp` | `87959a8119c4110b` | `1bc2fee56fa6955c` |
| `harpsichord` | `26e7c50a0b097a81` | `5dd1d5408ce3f64d` |
| `juno-piano` | `2d9e5f0fa58c4e6f` | `b28e8052b51465ff` |
| `pad-strings` | `e8ec20e6a854ed1c` | `8a3fba8d0ddfa16b` |
| `poly-lead` | `fc80ed3daf95e157` | `8abd348adeca098f` |
| `pulse-bass` | `c60e99fcbff724ed` | `09bd4cac01af6cd1` |
| `rain-drops` | `0de4cef314ee8d51` | `3edad94d2a22c985` |
| `snare` | `16da9fb4bb22ed45` | `ed06cf932c280fa1` |
| `soft-horn` | `d00bdb0100c90ce1` | `b9ab9b2c6199683b` |
| `soft-keys` | `ce302db772bb54fe` | `52c4d1c9a0b7d756` |
| `soft-lead` | `0ccbea1478efde25` | `1504b0fbc2f92ab9` |
| `solo-violin` | `1098cf656742aad0` | `4ffe3b66cc9892b3` |
| `square-bass` | `acb72b156660b9b9` | `bfc32f2d5a10bb31` |
| `stacked-lead` | `2d3f995cf8beeaf9` | `d54634b5610c6d15` |
| `sub-bass` | `962a024a9aae5e15` | `3c1815640b582e29` |
| `synth-brass` | `9f0d2de8f0bb19dd` | `0baba1b9323c67e1` |
| `unison-bass` | `688e45f758a70c05` | `eefa21aa4bc7c7bd` |
| `vibrato-lead` | `e12265111aa1c289` | `36d3342aa961b445` |
| `warm-pad` | `1af254f64a742c3e` | `447bf2958b2f8343` |
| `wide-chords` | `2869eeec424d8b00` | `89df41540939d102` |

**Every move is one of the two rounding causes plan §4.1 names, and nothing else.** A rule that
predicts a move exactly when a design has either one agrees with the measurement on all fifty sounds:

- **Key tracking re-associates.** The slider's `keytrack × (note − 60) / 12` octaves became the
  route's `(amount × (note − 60) / 60) × 5`: a different division, a few ulps of octave, which
  `key_tracking_and_bend_reach_the_old_numbers_to_rounding` bounds at 1e-5. Every design that sets
  `mod_cutoff_key` moved.
- **A translated depth that does not survive the normalised round trip.** A retired slider's depth
  `d` was its own plain value; its route stores `(d + 1) / 2`, and the parameter reads back
  `−1 + 2 × that` in `f32`. Where that is not `d` to the bit, the route's product differs in its
  last ulps.

A design with neither is bit-identical, which is the conversion's arithmetic claim —
`the_machines_own_routes_are_bit_identical_to_the_expressions_they_replace` — holding across the
factory bank. **The player's golden digests did not move**: the default patch and each chorus mode,
through the real bundle (`t4_golden_audio_poly_06`). The moved sounds are for the owner's listening
pass (plan P5), where the factory digests are re-pinned and not before.

## What this does not establish

- **Nothing about how it sounds.** A digest proves *unchanged*; it cannot prove *good*.
- **Nothing about the bundle or the host.** This is the plugin library, in process. The player's own
  golden (`apps/mxm-player/tests/t4_golden_audio_poly_06.rs`) covers the real bundle at its defaults
  and with each chorus mode, and must not move. *Since 2026-10-05* that golden is
  [`host-tests/tests/golden_audio.rs`](host-tests/tests/golden_audio.rs) here, run with
  `cargo test -p mxm-poly-06-host-tests` after a release bundle.
