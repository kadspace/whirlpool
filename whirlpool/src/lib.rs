
use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, EguiState, widgets};
use rustfft::{FftPlanner, num_complex::Complex};
use rustfft::num_traits::Zero;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use std::f32::consts::PI;

const FFT_SIZE: usize = 1024;
const WINDOW_SIZE: usize = 1024;

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
            input_history: VecDeque::from(vec![0.0; 256]), 
            output_history: VecDeque::from(vec![0.0; 256]),
        }
    }
}

// GUI State for Toggle
struct GuiState {
    show_settings: bool,
}

struct Whirlpool {
    params: Arc<WhirlpoolParams>,
    visuals: Arc<Mutex<Visuals>>,
    planner: FftPlanner<f32>,
    in_buf: Vec<f32>,   
    out_buf: VecDeque<f32>, 
    window: Vec<f32>,
    scratch_in: Vec<Complex<f32>>,
    scratch_out: Vec<Complex<f32>>,
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
            editor_state: EguiState::from_size(800, 600), // Increased height to prevent cutoff

            harmonics: FloatParam::new("Harmonics", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 }),
            shift: FloatParam::new("Shift", 1.0, FloatRange::Linear { min: 0.5, max: 2.0 }),
            blur: FloatParam::new("Blur", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            mix: FloatParam::new("Dry/Wet", 0.8, FloatRange::Linear { min: 0.0, max: 1.0 }),
            out_gain: FloatParam::new("Volume", 1.0, FloatRange::Linear { min: 0.0, max: 2.0 }),
        }
    }
}

// Custom Knob Helper
fn knob(ui: &mut egui::Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, diameter: f32) -> egui::Response {
    let desired_size = egui::vec2(diameter, diameter);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::drag());

    if response.dragged() {
        let delta = response.drag_delta().y * -0.0025; // Slower, tighter feel
        *value = (*value + delta).clamp(*range.start(), *range.end());
        response.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let center = rect.center();
        let radius = diameter / 2.0;

        // Draw Background
        ui.painter().circle(center, radius, egui::Color32::from_gray(30), egui::Stroke::new(1.0, egui::Color32::GRAY));

        // Draw Indicator
        let angle_rot = ((*value - *range.start()) / (*range.end() - *range.start())) * 2.0 * PI * 0.8 + (PI * 0.6); // 270 deg range
        let angle = PI / 2.0 + angle_rot; 
        // Fixed mapping: 0 = 7AM, 1 = 5PM
        
        // Simple 0-1 map to angle
        let t = (*value - *range.start()) / (*range.end() - *range.start());
        let angle = PI * 0.75 + (t * 1.5 * PI); // Start at bottom left, go to bottom right
        
        let end_pos = center + egui::vec2(angle.cos(), angle.sin()) * (radius * 0.8);
        ui.painter().line_segment([center, end_pos], egui::Stroke::new(3.0, egui::Color32::CYAN));
    }

    response
}

impl Plugin for Whirlpool {
    const NAME: &'static str = "Whirlpool Spectral";
    const VENDOR: &'static str = "Antigravity";
    const URL: &'static str = "https://example.com";
    const EMAIL: &'static str = "info@example.com";
    const VERSION: &'static str = "2.2.0";

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
            GuiState { show_settings: false },
            |_, _| {},
            move |ctx: &egui::Context, setter: &ParamSetter, state: &mut GuiState| {
                // Style Polish
                let mut style = (*ctx.style()).clone();
                style.spacing.item_spacing = egui::vec2(10.0, 10.0);
                style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(15, 15, 18); // Dark BG
                ctx.set_style(style);

                egui::CentralPanel::default().show(ctx, |ui| {
                    
                    // HEADER (Visuals + Title)
                    // We want Visuals at VERY TOP, then Title.
                    
                    // --- SPLIT VISUALIZER ---
                    ui.columns(2, |cols| {
                        if let Ok(vis) = visuals.try_lock() {
                            // Panel 1: IN
                            cols[0].vertical_centered(|ui| {
                                ui.label(egui::RichText::new("IN").heading());
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 120.0), egui::Sense::hover()); // Taller
                                ui.painter().rect_filled(rect, 5.0, egui::Color32::from_rgb(10, 10, 12));
                                if vis.input_history.len() > 1 {
                                    let points: Vec<egui::Pos2> = vis.input_history.iter().enumerate().map(|(i, &v)| {
                                        let x = rect.min.x + (i as f32 / vis.input_history.len() as f32) * rect.width();
                                        let y = rect.center().y - v * 40.0; 
                                        egui::pos2(x, y.clamp(rect.min.y, rect.max.y)) 
                                    }).collect();
                                    ui.painter().add(egui::Shape::line(points, egui::Stroke::new(1.5, egui::Color32::GRAY)));
                                }
                            });

                            // Panel 2: OUT
                            cols[1].vertical_centered(|ui| {
                                ui.label(egui::RichText::new("OUT").heading());
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 120.0), egui::Sense::hover());
                                ui.painter().rect_filled(rect, 5.0, egui::Color32::from_rgb(10, 10, 12));
                                if vis.output_history.len() > 1 {
                                    let points: Vec<egui::Pos2> = vis.output_history.iter().enumerate().map(|(i, &v)| {
                                        let x = rect.min.x + (i as f32 / vis.output_history.len() as f32) * rect.width();
                                        let y = rect.center().y - v * 40.0;
                                        egui::pos2(x, y.clamp(rect.min.y, rect.max.y))
                                    }).collect();
                                    ui.painter().add(egui::Shape::line(points, egui::Stroke::new(2.0, egui::Color32::CYAN)));
                                }
                            });
                        }
                    });
                    
                    // BRANDING separator
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                         ui.label(egui::RichText::new("WHIRLPOOL").size(40.0).strong().family(egui::FontFamily::Proportional));
                         ui.label("SPECTRAL HARMONIZER");
                    });
                    ui.add_space(20.0);

                    // --- KNOBS (MAIN) ---
                    // 2x2 Grid + Mix/Vol
                    if !state.show_settings {
                        ui.columns(2, |cols| {
                            // Column 1
                            cols[0].vertical_centered(|ui| {
                                // Harmonics
                                ui.label("HARMONICS");
                                
                                // We can't use generic knob easily with setter without implementing Param interaction logic manually.
                                // Widgets::ParamSlider handles automation. 
                                // DragValue is easier?
                                // Let's use `widgets::ParamSlider` but hide it and draw knob over? Complex.
                                // Better: Use Nih-Plug-Egui's `util` or just use standard Slider for now if Knob is too hard?
                                // No, user insisted on Knobs.
                                // Let's use a standard `egui::DragValue` logic but updating the parameter.
                                // setter.begin_set_parameter(&params.harmonics); setter.set_parameter(...); setter.end_set_parameter(...);
                                // This requires tracking drag state.
                                
                                // Alternative: `ui.add(widgets::ParamKnob::for_param(...))` DOES NOT EXIST in this version?
                                // I'll assume simple knob function I wrote above.
                                // But mapping to parameter:
                                let mut temp_val = params.harmonics.value();
                                let resp = knob(ui, &mut temp_val, 0.0..=1.0, 80.0);
                                if resp.changed() {
                                    setter.begin_set_parameter(&params.harmonics);
                                    setter.set_parameter(&params.harmonics, temp_val);
                                    setter.end_set_parameter(&params.harmonics);
                                }
                                
                                ui.add_space(20.0);
                                
                                // Blur
                                ui.label("BLUR");
                                let mut temp_blur = params.blur.value();
                                let resp = knob(ui, &mut temp_blur, 0.0..=1.0, 80.0);
                                if resp.changed() {
                                    setter.begin_set_parameter(&params.blur);
                                    setter.set_parameter(&params.blur, temp_blur);
                                    setter.end_set_parameter(&params.blur);
                                }
                            });
                            
                            // Column 2
                            cols[1].vertical_centered(|ui| {
                                // Shift
                                ui.label("SHIFT");
                                let mut temp_shift = params.shift.value();
                                let resp = knob(ui, &mut temp_shift, 0.5..=2.0, 80.0);
                                if resp.changed() {
                                    setter.begin_set_parameter(&params.shift);
                                    setter.set_parameter(&params.shift, temp_shift);
                                    setter.end_set_parameter(&params.shift);
                                }
                                
                                ui.add_space(20.0);

                                // Mix
                                ui.label("MIX");
                                let mut temp_mix = params.mix.value();
                                let resp = knob(ui, &mut temp_mix, 0.0..=1.0, 80.0);
                                if resp.changed() {
                                    setter.begin_set_parameter(&params.mix);
                                    setter.set_parameter(&params.mix, temp_mix);
                                    setter.end_set_parameter(&params.mix);
                                }
                            });
                        });
                    } else {
                        // SETTINGS PAGE
                        egui::ScrollArea::vertical().show(ui, |ui| {
                             ui.heading("Settings");
                             ui.label("Output Volume");
                             ui.add(widgets::ParamSlider::for_param(&params.out_gain, setter)); // Slider for vol is fine
                             
                             ui.add_space(20.0);
                             if ui.button("Back").clicked() {
                                 state.show_settings = false;
                             }
                        });
                    }



                    ui.ctx().request_repaint();
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
                 for i in 0..FFT_SIZE {
                     self.scratch_in[i] = Complex::new(self.in_buf[i] * self.window[i], 0.0);
                 }
                 
                 self.planner.plan_fft_forward(FFT_SIZE).process(&mut self.scratch_in);

                 for x in self.scratch_out.iter_mut() { *x = Complex::zero(); }
                 let half = FFT_SIZE / 2;

                 for i in 0..half {
                     let bin = self.scratch_in[i];
                     if bin.norm_sqr() < 1e-6 { continue; }

                     // FUNDAMENTAL (Apply Blur to Dry signal too if requested)
                     // If blur > 0, we can randomize phase of fundamental to wash it out.
                     if blur > 0.0 {
                         let mag = bin.norm();
                         let phase = bin.arg();
                         // Phase Randomization for "Reverb" feel
                         let r = fast_rand(i + self.seed as usize, self.seed);
                         let new_phase = phase + (r * 2.0 * PI * blur);
                         
                         self.scratch_out[i] += Complex::from_polar(mag, new_phase);
                     } else {
                         self.scratch_out[i] += bin;
                     }

                     // HARMONICS (Always Blurred if global blur is on)
                     if harmonics > 0.01 {
                         let target = (i as f32 * (1.0 + shift)).round() as usize; 
                         if target < half {
                             let mag = bin.norm();
                             let phase = bin.arg();
                             
                             let new_phase = if blur > 0.0 {
                                 let r = fast_rand(target + self.seed as usize, self.seed * 2); // Diff seed
                                 phase + (r * 2.0 * PI * blur) 
                             } else { phase };
                             
                             self.scratch_out[target] += Complex::from_polar(mag * harmonics, new_phase);
                         }
                     }
                 }
                 
                 for i in 1..half {
                     self.scratch_out[FFT_SIZE - i] = self.scratch_out[i].conj();
                 }

                 self.planner.plan_fft_inverse(FFT_SIZE).process(&mut self.scratch_out);

                 let norm = 1.0 / FFT_SIZE as f32;
                 for i in 0..FFT_SIZE {
                     self.out_buf.push_back(self.scratch_out[i].re * norm);
                 }
                 self.in_buf.clear(); 
            }
            
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
