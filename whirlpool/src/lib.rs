
use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, EguiState, widgets};
use std::sync::{Arc, Mutex};
use rustfft::{FftPlanner, num_complex::Complex};
use rustfft::num_traits::Zero;
use std::collections::VecDeque;

// --- DSP CONSTANTS ---
const FFT_SIZE: usize = 1024; // ~23ms at 44.1k

struct Visuals {
    input_history: VecDeque<f32>,
    output_history: VecDeque<f32>,
}

impl Default for Visuals {
    fn default() -> Self {
        Self { 
            input_history: VecDeque::from(vec![0.0; 512]),
            output_history: VecDeque::from(vec![0.0; 512]),
        }
    }
}

struct Whirlpool {
    params: Arc<WhirlpoolParams>,
    visuals: Arc<Mutex<Visuals>>,
    
    // DSP State
    planner: FftPlanner<f32>,
    in_buf: Vec<f32>,   // Accumulate input
    out_buf: VecDeque<f32>, // Latency buffer
    window: Vec<f32>,
    scratch_in: Vec<Complex<f32>>,
    scratch_out: Vec<Complex<f32>>,
}

#[derive(Params)]
struct WhirlpoolParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "harmonics"] pub harmonics: FloatParam,
    #[id = "shift"] pub shift: FloatParam,
    #[id = "blur"] pub blur: FloatParam,
    #[id = "mix"] pub mix: FloatParam,
    #[id = "output_gain"] pub out_gain: FloatParam,
}

impl Default for Whirlpool {
    fn default() -> Self {
        let mut planner = FftPlanner::new();
        let window = (0..FFT_SIZE).map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE as f32 - 1.0)).cos())).collect();
        
        Self {
            params: Arc::new(WhirlpoolParams::default()),
            visuals: Arc::new(Mutex::new(Visuals::default())),
            planner,
            in_buf: Vec::with_capacity(FFT_SIZE),
            out_buf: VecDeque::from(vec![0.0; FFT_SIZE]), // Initial Latency
            window,
            scratch_in: vec![Complex::zero(); FFT_SIZE],
            scratch_out: vec![Complex::zero(); FFT_SIZE],
        }
    }
}

impl Default for WhirlpoolParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(500, 350),

            harmonics: FloatParam::new("Harmonics", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 }),
            shift: FloatParam::new("Shift (Oct)", 1.0, FloatRange::Linear { min: 0.5, max: 3.0 }),
            blur: FloatParam::new("Blur", 0.2, FloatRange::Linear { min: 0.0, max: 1.0 }),
            mix: FloatParam::new("Dry/Wet", 0.8, FloatRange::Linear { min: 0.0, max: 1.0 }),
            out_gain: FloatParam::new("Output", 1.0, FloatRange::Linear { min: 0.0, max: 2.0 }),
        }
    }
}

impl Plugin for Whirlpool {
    const NAME: &'static str = "Whirlpool Spectral";
    const VENDOR: &'static str = "Antigravity";
    const URL: &'static str = "https://example.com";
    const EMAIL: &'static str = "info@example.com";
    const VERSION: &'static str = "2.0.0";

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

    fn params(&self) -> Arc<dyn Params> { self.params.clone() }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let visuals = self.visuals.clone();
        
        create_egui_editor(
            self.params.editor_state.clone(),
            (),
            |_, _| {},
            move |ctx: &egui::Context, setter: &ParamSetter, _state: &mut ()| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("WHIRLPOOL SPECTRAL");
                    ui.separator();
                    
                    // --- WAVEFORM VISUALIZER ---
                    // Draw a dark background
                    let (rect, _resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 100.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 3.0, egui::Color32::from_rgb(10, 10, 15));

                    if let Ok(vis) = visuals.try_lock() {
                        // Safe to read visuals here
                        // Draw Input (Gray)
                        if vis.input_history.len() > 1 {
                             let points_in: Vec<egui::Pos2> = vis.input_history.iter().enumerate().map(|(i, &v)| {
                                 let x = rect.min.x + (i as f32 / vis.input_history.len() as f32) * rect.width();
                                 let y = rect.center().y - v * 40.0;
                                 egui::pos2(x, y)
                             }).collect();
                             ui.painter().add(egui::Shape::line(points_in, egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 80, 80))));
                        }

                        // Draw Output (Cyan)
                        if vis.output_history.len() > 1 {
                            let points_out: Vec<egui::Pos2> = vis.output_history.iter().enumerate().map(|(i, &v)| {
                                let x = rect.min.x + (i as f32 / vis.output_history.len() as f32) * rect.width();
                                let y = rect.center().y - v * 40.0;
                                egui::pos2(x, y)
                            }).collect();
                            ui.painter().add(egui::Shape::line(points_out, egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 255, 255))));
                        }
                        
                        // Request repaint for animation
                        ui.ctx().request_repaint();
                    }

                    ui.separator();

                    // --- CONTROLS ---
                    ui.label("Harmonics");
                    ui.add(widgets::ParamSlider::for_param(&params.harmonics, setter));
                    ui.label("Shift");
                    ui.add(widgets::ParamSlider::for_param(&params.shift, setter));
                    ui.label("Blur");
                    ui.add(widgets::ParamSlider::for_param(&params.blur, setter));
                    ui.separator();
                    ui.label("Dry/Wet");
                    ui.add(widgets::ParamSlider::for_param(&params.mix, setter));
                    ui.label("Volume");
                    ui.add(widgets::ParamSlider::for_param(&params.out_gain, setter));
                });
            },
        )
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

        // Mono processing buffer loop
        for channel_samples in buffer.iter_samples() {
            // Collect samples to avoid double-borrow issue
            let mut samples: Vec<&mut f32> = channel_samples.into_iter().collect();
            if samples.is_empty() { continue; }

            // Calculate Mono Input
            let mut input_mono = 0.0;
            for s in samples.iter() { input_mono += **s; }
            input_mono /= samples.len() as f32;

            // STFT Buffering
            self.in_buf.push(input_mono);

            // Output sample (Latency compensation: pop from out_buf)
            // If out_buf is empty (underrun or initial), return 0
            // But we pre-filled it in Default.
            // If push rate > pop rate? No, 1:1 ratio.
            
            // PROCESS FRAME if enough input
            if self.in_buf.len() >= FFT_SIZE {
                 // 1. Prepare FFT (Copy windowed input to scratch)
                 for i in 0..FFT_SIZE {
                     self.scratch_in[i] = Complex::new(self.in_buf[i] * self.window[i], 0.0);
                 }
                 
                 // 2. Forward FFT
                 self.planner.plan_fft_forward(FFT_SIZE).process(&mut self.scratch_in);

                 // 3. Spectral Processing
                 // Zero out output scratch
                 for x in self.scratch_out.iter_mut() { *x = Complex::zero(); }
                 
                 let half = FFT_SIZE / 2;
                 
                 for i in 0..half {
                     let bin = self.scratch_in[i];
                     if bin.norm_sqr() < 1e-6 { continue; } // Noise floor

                     // Fundamental (Mix with Harmonics later via 'mix' param is Dry/Wet?)
                     // Actually 'harmonics' param adds harmonics. Fundamental is always there?
                     // Let's say Harmonics param controls the ADDED harmonics level.
                     self.scratch_out[i] += bin;

                     // Generate Harmonics
                     if harmonics > 0.01 {
                         // Shift logic
                         // e.g. Shift 1.0 = Octave (+100% freq -> 2x freq)
                         // e.g. Shift 0.5 = Fifth (+50% -> 1.5x)
                         let target_idx = (i as f32 * (1.0 + shift)).round() as usize; 
                         
                         if target_idx < half {
                             let mag = bin.norm();
                             let phase = bin.arg();
                             
                             // Blur phase
                             let new_phase = if blur > 0.0 { 
                                 // Random phase or linear phase shift? Random is nicer for "wash"
                                 // But we don't have rand here easily (unless we add valid Rng).
                                 // Deterministic blur: phase + i * blur
                                 phase + (i as f32 * blur * 0.1)
                             } else { phase };
                             
                             self.scratch_out[target_idx] += Complex::from_polar(mag * harmonics, new_phase);
                         }
                     }
                 }
                 
                 // Conjugate Symmetry for Real Output
                 for i in 1..half {
                     self.scratch_out[FFT_SIZE - i] = self.scratch_out[i].conj();
                 }

                 // 4. Inverse FFT
                 self.planner.plan_fft_inverse(FFT_SIZE).process(&mut self.scratch_out);

                 // 5. Output Overlap-Add (Simplified: Window + Norm)
                 // Just normalize by FFT_SIZE for now.
                 // Ideally we'd window again, but let's stick to basic reconstruction.
                 let norm = 1.0 / FFT_SIZE as f32;
                 
                 for i in 0..FFT_SIZE {
                     self.out_buf.push_back(self.scratch_out[i].re * norm);
                 }
                 
                 // Clear Input Buffer (Hop = Size)
                 self.in_buf.clear(); 
            }
            
            // Pop output
            let wet_sig = self.out_buf.pop_front().unwrap_or(0.0);
            let final_wet = (wet_sig * 2.0).tanh(); // Saturation / Limiter ("Not clippy")

            // Send to visuals
            if let Ok(mut vis) = self.visuals.try_lock() {
                 if vis.input_history.len() >= 512 { vis.input_history.pop_front(); }
                 vis.input_history.push_back(input_mono);

                 if vis.output_history.len() >= 512 { vis.output_history.pop_front(); }
                 vis.output_history.push_back(final_wet);
            }

            // Mix
            let output_sig = input_mono * (1.0 - mix) + final_wet * mix;
            
            // Write to all channels
            for sample in samples.iter_mut() {
                **sample = output_sig * gain;
            }
        }

        ProcessStatus::Normal
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
    const VST3_CLASS_ID: [u8; 16] = *b"WhirlpoolSPECTRA";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Instrument];
}

nih_export_clap!(Whirlpool);
nih_export_vst3!(Whirlpool);
