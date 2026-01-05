use nih_plug::prelude::*;
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use rustfft::num_traits::Zero;
use std::collections::VecDeque;
use std::f32::consts::PI;
use std::sync::Arc;

// --- DSP CONSTANTS for OVERLAP-ADD ---
const FFT_SIZE: usize = 1024;
const HOP_SIZE: usize = 256; // 4x Overlap (1024 / 256 = 4)
const WINDOW_SIZE: usize = 1024;

fn fast_rand(x: usize, seed: u32) -> f32 {
    let mut n = (x as u32).wrapping_mul(374761393).wrapping_add(seed);
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    (n as f32) / (u32::MAX as f32)
}

struct Whirlpool {
    params: Arc<WhirlpoolParams>,

    forward_fft: Arc<dyn Fft<f32>>,
    inverse_fft: Arc<dyn Fft<f32>>,

    channels: Vec<ChannelState>,
    window: Vec<f32>,
}

struct ChannelState {
    input_ring: VecDeque<f32>,
    output_accum: VecDeque<f32>,
    scratch_in: Vec<Complex<f32>>,
    scratch_out: Vec<Complex<f32>>,
    hop_counter: usize,
    rng_state: u32,
}

#[derive(Params)]
struct WhirlpoolParams {
    #[id = "harmonics"]
    pub harmonics: FloatParam,
    #[id = "shift"]
    pub shift: FloatParam,
    #[id = "blur"]
    pub blur: FloatParam,
    #[id = "mix"]
    pub mix: FloatParam,
    #[id = "output_gain"]
    pub out_gain: FloatParam,
}

impl Default for Whirlpool {
    fn default() -> Self {
        let mut planner = FftPlanner::new();
        let forward_fft = planner.plan_fft_forward(FFT_SIZE);
        let inverse_fft = planner.plan_fft_inverse(FFT_SIZE);

        // Hanning Window for Smooth OLA
        let window: Vec<f32> = (0..WINDOW_SIZE)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (WINDOW_SIZE as f32 - 1.0)).cos()))
            .collect();

        Self {
            params: Arc::new(WhirlpoolParams::default()),
            forward_fft,
            inverse_fft,
            channels: vec![ChannelState::new(), ChannelState::new()],
            window,
        }
    }
}

impl ChannelState {
    fn new() -> Self {
        Self {
            input_ring: VecDeque::from(vec![0.0; FFT_SIZE]),
            output_accum: VecDeque::from(vec![0.0; FFT_SIZE]),
            scratch_in: vec![Complex::zero(); FFT_SIZE],
            scratch_out: vec![Complex::zero(); FFT_SIZE],
            hop_counter: 0,
            rng_state: 0,
        }
    }
}

impl Default for WhirlpoolParams {
    fn default() -> Self {
        Self {
            harmonics: FloatParam::new(
                "Harmonics",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            ),
            shift: FloatParam::new(
                "Shift",
                1.0,
                FloatRange::Linear { min: 0.5, max: 2.0 },
            ),
            blur: FloatParam::new(
                "Blur",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            ),
            mix: FloatParam::new(
                "Dry/Wet",
                0.8,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            ),
            out_gain: FloatParam::new(
                "Volume",
                1.0,
                FloatRange::Linear { min: 0.0, max: 2.0 },
            ),
        }
    }
}

impl Plugin for Whirlpool {
    const NAME: &'static str = "Whirlpool Spectral";
    const VENDOR: &'static str = "Antigravity";
    const URL: &'static str = "https://example.com";
    const EMAIL: &'static str = "info@example.com";
    const VERSION: &'static str = "2.5.0";

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
    ];
    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        None
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let harmonics = self.params.harmonics.value();
        let shift = self.params.shift.value();
        let blur = self.params.blur.value();
        let mix = self.params.mix.value();
        let gain = self.params.out_gain.value();

        for mut channel_samples in buffer.iter_samples() {
            for (ch, sample) in channel_samples.iter_mut().enumerate() {
                if ch >= self.channels.len() {
                    continue;
                }
                let state = &mut self.channels[ch];
                let input = *sample;

                let wet = Self::process_sample(
                    state,
                    input,
                    harmonics,
                    shift,
                    blur,
                    self.forward_fft.as_ref(),
                    self.inverse_fft.as_ref(),
                    &self.window,
                );
                let final_wet = wet.tanh();
                let output = input * (1.0 - mix) + final_wet * mix;

                *sample = output * gain;
            }
        }

        ProcessStatus::Normal
    }
}

impl Whirlpool {
    fn process_sample(
        state: &mut ChannelState,
        input: f32,
        harmonics: f32,
        shift: f32,
        blur: f32,
        forward_fft: &dyn Fft<f32>,
        inverse_fft: &dyn Fft<f32>,
        window: &[f32],
    ) -> f32 {
        state.input_ring.push_back(input);
        if state.input_ring.len() > FFT_SIZE {
            state.input_ring.pop_front();
        }

        state.hop_counter += 1;
        if state.hop_counter >= HOP_SIZE && state.input_ring.len() == FFT_SIZE {
            state.hop_counter = 0;
            let frame_seed = state.rng_state;

            for i in 0..FFT_SIZE {
                state.scratch_in[i] = Complex::new(state.input_ring[i] * window[i], 0.0);
            }

            forward_fft.process(&mut state.scratch_in);

            for x in state.scratch_out.iter_mut() {
                *x = Complex::zero();
            }
            let half = FFT_SIZE / 2;

            for i in 0..half {
                let bin = state.scratch_in[i];
                if bin.norm_sqr() < 1e-6 {
                    continue;
                }

                let mag = bin.norm();
                let phase = bin.arg();

                if blur > 0.0 {
                    let r = fast_rand(i + frame_seed as usize, frame_seed);
                    let new_phase = phase + (r * 2.0 * PI * blur);
                    state.scratch_out[i] += Complex::from_polar(mag, new_phase);
                } else {
                    state.scratch_out[i] += bin;
                }

                if harmonics > 0.01 {
                    let target_idx = (i as f32 * (1.0 + shift)).round() as usize;
                    if target_idx < half {
                        let mag_h = mag * harmonics;
                        let r = fast_rand(target_idx + frame_seed as usize, frame_seed.wrapping_mul(2));
                        let phase_h = if blur > 0.0 {
                            phase + (r * 2.0 * PI * blur)
                        } else {
                            phase
                        };
                        state.scratch_out[target_idx] += Complex::from_polar(mag_h, phase_h);
                    }
                }
            }

            for i in 1..half {
                state.scratch_out[FFT_SIZE - i] = state.scratch_out[i].conj();
            }

            inverse_fft.process(&mut state.scratch_out);

            let norm = 1.0 / FFT_SIZE as f32;
            for i in 0..FFT_SIZE {
                let val = state.scratch_out[i].re * norm * window[i];
                if i < state.output_accum.len() {
                    state.output_accum[i] += val;
                } else {
                    state.output_accum.push_back(val);
                }
            }
        }

        let wet_sig = state.output_accum.pop_front().unwrap_or(0.0);
        state.output_accum.push_back(0.0);
        while state.output_accum.len() < FFT_SIZE {
            state.output_accum.push_back(0.0);
        }

        state.rng_state = state.rng_state.wrapping_add(1);
        wet_sig
    }
}

impl ClapPlugin for Whirlpool {
    const CLAP_ID: &'static str = "com.antigravity.whirlpool";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Whirlpool Spectral Harmonizer");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Stereo];
}

impl Vst3Plugin for Whirlpool {
    const VST3_CLASS_ID: [u8; 16] = *b"WhirlpoolOlaV2__";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Modulation];
}

nih_export_clap!(Whirlpool);
nih_export_vst3!(Whirlpool);
