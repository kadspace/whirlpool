# Whirlpool

A spectral harmonizer audio effect (VST3 and CLAP), written in Rust with [nih-plug](https://github.com/robbert-vdh/nih-plug) and [rustfft](https://crates.io/crates/rustfft).

It takes 1024-point FFTs of the input with 4x overlap (Hann window, 256-sample hop), adds a pitch-shifted copy of each frequency bin on top of the original, optionally randomizes the phases to smear the sound, and resynthesizes with overlap-add. The wet signal goes through `tanh` soft clipping before the dry/wet mix.

## Parameters

| Parameter | Range | What it does |
|---|---|---|
| Harmonics | 0–1 | Level of the shifted copy added to each bin |
| Shift | 0.5–2.0 | Where the copy lands: bin `i` goes to bin `i × (1 + shift)` |
| Blur | 0–1 | Amount of random phase per bin; higher values turn tones into a wash |
| Dry/Wet | 0–1 | Mix between the input and the processed signal |
| Volume | 0–2 | Output gain |

There is no custom GUI; the plugin uses the host's generic parameter controls.

## Build (Windows)

Needs Rust and the Visual Studio 2022 Build Tools (MSVC).

```bat
build_whirlpool.bat
```

This runs `cargo build --release --target x86_64-pc-windows-msvc` and bundles the DLL as `whirlpool\target\bundled\Whirlpool.vst3`. Copy that folder to `C:\Program Files\Common Files\VST3` and rescan in your DAW.

`check_whirlpool.bat` runs `cargo check` for a quick compile check.
