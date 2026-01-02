
use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, EguiState, widgets};
use rustfft::{FftPlanner, num_complex::Complex};
use rustfft::num_traits::Zero;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use std::f32::consts::PI;

// --- DSP CONSTANTS ---
const FFT_SIZE: usize = 1024;
const WINDOW_SIZE: usize = 1024;

// Simple pseudo-random for phase blur in DSP thread
// x: bin index, seed: rolling counter
fn fast_rand(x: usize, seed: u32) -> f32 {
    let mut n = (x as u32).wrapping_mul(374761393).wrapping_add(seed);
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    (n as f32) / (u32::MAX as f32)
}

struct Visuals {
    input_history: VecDeque<f32>,
    output_history: VecDeque<f32>,
}

impl Default for Visuals {
    fn default() -> Self {
        Self { 
            input_history: VecDeque::from(vec![0.0; 256]), // Smaller for separate windows? 256 is fine
            output_history: VecDeque::from(vec![0.0; 256]),
        }
    }
}

struct Whirlpool {
    params: Arc<WhirlpoolParams>,
    visuals: Arc<Mutex<Visuals>>,
    
    // DSP State
    planner: FftPlanner<f32>,
    in_buf: Vec<f32>,   
    out_buf: VecDeque<f32>, 
    window: Vec<f32>,
    scratch_in: Vec<Complex<f32>>,
    scratch_out: Vec<Complex<f32>>,
    
    // Random seed for blur
    seed: u32,
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
        // Hann Window
        let window = (0..WINDOW_SIZE).map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (WINDOW_SIZE as f32 - 1.0)).cos())).collect();
        
        Self {
            params: Arc::new(WhirlpoolParams::default()),
            visuals: Arc::new(Mutex::new(Visuals::default())),
            planner,
            in_buf: Vec::with_capacity(FFT_SIZE),
            out_buf: VecDeque::from(vec![0.0; FFT_SIZE]),
            window,
            scratch_in: vec![Complex::zero(); FFT_SIZE],
            scratch_out: vec![Complex::zero(); FFT_SIZE],
            seed: 0,
        }
    }
}

impl Default for WhirlpoolParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(600, 450), // Slightly larger default

            harmonics: FloatParam::new("Harmonics", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 }),
            shift: FloatParam::new("Shift", 1.0, FloatRange::Linear { min: 0.5, max: 2.0 }), // Tuned range
            blur: FloatParam::new("Blur", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            mix: FloatParam::new("Dry/Wet", 0.8, FloatRange::Linear { min: 0.0, max: 1.0 }),
            out_gain: FloatParam::new("Volume", 1.0, FloatRange::Linear { min: 0.0, max: 2.0 }),
        }
    }
}

impl Plugin for Whirlpool {
    const NAME: &'static str = "Whirlpool Spectral";
    const VENDOR: &'static str = "Antigravity";
    const URL: &'static str = "https://example.com";
    const EMAIL: &'static str = "info@example.com";
    const VERSION: &'static str = "2.1.0"; // Version bump

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
                // Style Polish
                let mut style = (*ctx.style()).clone();
                style.spacing.item_spacing = egui::vec2(10.0, 15.0); // More breathing room
                style.spacing.slider_width = 300.0; // Wider sliders
                ctx.set_style(style);

                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label(egui::RichText::new("WHIRLPOOL SPECTRAL").heading().strong().color(egui::Color32::CYAN));
                        ui.separator();
                        
                        // --- SPLIT VISUALIZER ---
                        ui.columns(2, |cols| {
                            if let Ok(vis) = visuals.try_lock() {
                                // Panel 1: Input
                                cols[0].group(|ui| {
                                    ui.label("Input Signal");
                                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 80.0), egui::Sense::hover());
                                    ui.painter().rect_filled(rect, 3.0, egui::Color32::from_rgb(20, 20, 25));
                                    
                                    if vis.input_history.len() > 1 {
                                        let points: Vec<egui::Pos2> = vis.input_history.iter().enumerate().map(|(i, &v)| {
                                            let x = rect.min.x + (i as f32 / vis.input_history.len() as f32) * rect.width();
                                            let y = rect.center().y - v * 30.0; 
                                            egui::pos2(x, y.clamp(rect.min.y, rect.max.y)) 
                                        }).collect();
                                        ui.painter().add(egui::Shape::line(points, egui::Stroke::new(1.0, egui::Color32::GRAY)));
                                    }
                                });

                                // Panel 2: Output
                                cols[1].group(|ui| {
                                    ui.label("Harmonized Output");
                                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 80.0), egui::Sense::hover());
                                    ui.painter().rect_filled(rect, 3.0, egui::Color32::from_rgb(20, 20, 25));

                                    if vis.output_history.len() > 1 {
                                        let points: Vec<egui::Pos2> = vis.output_history.iter().enumerate().map(|(i, &v)| {
                                            let x = rect.min.x + (i as f32 / vis.output_history.len() as f32) * rect.width();
                                            let y = rect.center().y - v * 30.0;
                                            egui::pos2(x, y.clamp(rect.min.y, rect.max.y))
                                        }).collect();
                                        ui.painter().add(egui::Shape::line(points, egui::Stroke::new(1.5, egui::Color32::CYAN)));
                                    }
                                });
                            }
                        });
                        
                        ui.ctx().request_repaint(); 

                        ui.add_space(20.0);
                        ui.separator();
                        ui.add_space(20.0);

                        // --- CONTROLS (Grid Layout) ---
                        ui.group(|ui| {
                             ui.heading("Parameters");
                             ui.add_space(10.0);
                             
                             egui::Grid::new("my_grid")
                                 .num_columns(2)
                                 .spacing([40.0, 20.0])
                                 .striped(true)
                                 .show(ui, |ui| {
                                     // Row 1: Harmonics
                                     ui.label("Harmonics");
                                     ui.add(widgets::ParamSlider::for_param(&params.harmonics, setter));
                                     ui.end_row();

                                     // Row 2: Shift
                                     ui.label("Shift");
                                     ui.add(widgets::ParamSlider::for_param(&params.shift, setter));
                                     ui.end_row();

                                     // Row 3: Blur
                                     ui.label("Blur");
                                     ui.add(widgets::ParamSlider::for_param(&params.blur, setter));
                                     ui.end_row();
                                     
                                     // Row 4: Mix
                                     ui.label("Dry/Wet");
                                     ui.add(widgets::ParamSlider::for_param(&params.mix, setter));
                                     ui.end_row();

                                     // Row 5: Gain
                                     ui.label("Volume");
                                     ui.add(widgets::ParamSlider::for_param(&params.out_gain, setter));
                                     ui.end_row();
                                 });
                        });
                    });
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

        // Update seed once per block or keep rolling
        self.seed = self.seed.wrapping_add(1);

        for channel_samples in buffer.iter_samples() {
            let mut samples: Vec<&mut f32> = channel_samples.into_iter().collect();
            if samples.is_empty() { continue; }

            // Mono Input
            let mut input_mono = 0.0;
            for s in samples.iter() { input_mono += **s; }
            input_mono /= samples.len() as f32;

            self.in_buf.push(input_mono);

            if self.in_buf.len() >= FFT_SIZE {
                 // 1. Window + Prepare
                 for i in 0..FFT_SIZE {
                     self.scratch_in[i] = Complex::new(self.in_buf[i] * self.window[i], 0.0);
                 }
                 
                 // 2. FFT
                 self.planner.plan_fft_forward(FFT_SIZE).process(&mut self.scratch_in);

                 // 3. Spectral Processing
                 for x in self.scratch_out.iter_mut() { *x = Complex::zero(); }
                 let half = FFT_SIZE / 2;

                 for i in 0..half {
                     let bin = self.scratch_in[i];
                     if bin.norm_sqr() < 1e-6 { continue; } // Gate

                     // Base
                     self.scratch_out[i] += bin;

                     // Harmonics
                     if harmonics > 0.01 {
                         let target = (i as f32 * (1.0 + shift)).round() as usize; // e.g. 100hz -> 200hz (Octave UP) if Shift=1.0
                         if target < half {
                             let mag = bin.norm();
                             let phase = bin.arg();
                             
                             // BLUR LOGIC: RANDOMIZED PHASE
                             // If blur > 0, we scramble phase. 
                             // Magnitude is preserved (so tone is there), but phase coherency is lost = Diffuse/Reverb sound.
                             let new_phase = if blur > 0.0 {
                                 let r = fast_rand(target + self.seed as usize, self.seed);
                                 // Blend between original phase and random phase based on blur amount
                                 // Linear interpolation of angle is tricky, but let's just add random noise
                                 phase + (r * 2.0 * PI * blur) 
                             } else { phase };
                             
                             self.scratch_out[target] += Complex::from_polar(mag * harmonics, new_phase);
                         }
                     }
                 }
                 
                 // Symmetry
                 for i in 1..half {
                     self.scratch_out[FFT_SIZE - i] = self.scratch_out[i].conj();
                 }

                 // 4. IFFT
                 self.planner.plan_fft_inverse(FFT_SIZE).process(&mut self.scratch_out);

                 // 5. Output
                 let norm = 1.0 / FFT_SIZE as f32;
                 for i in 0..FFT_SIZE {
                     self.out_buf.push_back(self.scratch_out[i].re * norm);
                 }
                 self.in_buf.clear(); 
            }
            
            // Output
            let wet_sig = self.out_buf.pop_front().unwrap_or(0.0);
            let final_wet = (wet_sig * 2.0).tanh(); 

            // Visuals
            if let Ok(mut vis) = self.visuals.try_lock() {
                 if vis.input_history.len() >= 256 { vis.input_history.pop_front(); }
                 vis.input_history.push_back(input_mono);

                 if vis.output_history.len() >= 256 { vis.output_history.pop_front(); }
                 vis.output_history.push_back(final_wet);
            }

            let output_sig = input_mono * (1.0 - mix) + final_wet * mix;
            
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
    const VST3_CLASS_ID: [u8; 16] = *b"Whirlpool_______"; 
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Modulation];
}

nih_export_clap!(Whirlpool);
nih_export_vst3!(Whirlpool);
